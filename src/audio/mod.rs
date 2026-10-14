//! Live audio input as an alternative note source to MIDI.
//!
//! cpal captures the input device; a worker thread resamples to the active
//! engine's rate, runs pitch detection, and a `Tracker` turns per-frame note
//! activations into on/off events written into `AudioState` with the same
//! shape as `MidiState`'s notes, so sketches can't tell the sources apart.

mod basic_pitch;
mod dsp_poly;
pub mod probe;
mod resample;
mod swift_f0;
mod tracker;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};

use crate::midi::{push_event, NoteEvent, NoteState};
use resample::{DcBlocker, Resampler};
use tracker::Tracker;

pub const ENGINES: [&str; 3] = ["basic-pitch", "swift-f0", "dsp-poly"];
pub const DEFAULT_GATE_DB: f32 = -50.0;

/// One analysis frame from an engine.
pub struct Frame {
    /// Per-MIDI-note activation in [0, 1].
    pub act: [f32; 128],
    /// Per-MIDI-note onset strength in [0, 1].
    pub onset: [f32; 128],
    /// Input level of the frame's audio, dBFS.
    pub level_db: f32,
}

impl Frame {
    pub fn silent(level_db: f32) -> Self {
        Self { act: [0.0; 128], onset: [0.0; 128], level_db }
    }
}

pub trait PitchEngine: Send {
    fn name(&self) -> &'static str;
    fn sample_rate(&self) -> u32;
    /// Rough delay from sound to frame (window and lookahead), for the HUD.
    fn latency_ms(&self) -> f32;
    fn tracker(&self) -> tracker::TrackerParams;
    /// Feed mono samples at `sample_rate()`, appending completed frames to `out`.
    fn push(&mut self, samples: &[f32], out: &mut Vec<Frame>);
}

pub fn build_engine(idx: usize) -> Result<Box<dyn PitchEngine>, String> {
    match idx {
        0 => Ok(Box::new(basic_pitch::BasicPitch::new()?)),
        1 => Ok(Box::new(swift_f0::SwiftF0::new()?)),
        _ => Ok(Box::new(dsp_poly::DspPoly::new())),
    }
}

pub fn engine_index(name: &str) -> Option<usize> {
    ENGINES.iter().position(|e| *e == name)
}

pub struct AudioState {
    pub notes: [NoteState; 128],
    pub recent_events: VecDeque<NoteEvent>,
    pub level_db: f32,
    pub engine: usize,
    pub engine_error: Option<String>,
    pub latency_ms: f32,
    /// Fraction of real time spent in the engine (1.0 = can't keep up).
    pub load: f32,
}

impl Default for AudioState {
    fn default() -> Self {
        Self {
            notes: [NoteState::default(); 128],
            recent_events: VecDeque::with_capacity(64),
            level_db: -120.0,
            engine: 0,
            engine_error: None,
            latency_ms: 0.0,
            load: 0.0,
        }
    }
}

struct Control {
    engine: AtomicUsize,
    gate_db_bits: AtomicU32,
}

impl Control {
    fn gate_db(&self) -> f32 { f32::from_bits(self.gate_db_bits.load(Ordering::Relaxed)) }
}

/// Handle to a running capture stream. Dropping it stops capture and the worker.
pub struct AudioInput {
    pub state: Arc<Mutex<AudioState>>,
    control: Arc<Control>,
    _stream: cpal::Stream,
}

impl AudioInput {
    /// Opens the input named by `AUDIO_DEVICE` (substring match) or the default
    /// input. `AUDIO_CHANNEL` (1-based) picks one channel instead of the mix.
    pub fn start(engine: usize, gate_db: f32) -> Result<Self, String> {
        let host = cpal::default_host();
        let wanted = std::env::var("AUDIO_DEVICE").ok();
        let device = match &wanted {
            Some(w) => host
                .input_devices()
                .map_err(|e| e.to_string())?
                .find(|d| d.name().map(|n| n.contains(w.as_str())).unwrap_or(false))
                .ok_or_else(|| format!("no input device matching AUDIO_DEVICE='{w}'"))?,
            None => host.default_input_device().ok_or("no default audio input device")?,
        };
        if let Ok(devs) = host.input_devices() {
            let names: Vec<String> = devs.filter_map(|d| d.name().ok()).collect();
            println!("Audio inputs (pick with AUDIO_DEVICE=<substring>): {}", names.join(" | "));
        }
        let device_name = device.name().unwrap_or_else(|_| "unknown".into());
        let config = device.default_input_config().map_err(|e| e.to_string())?;
        let rate = config.sample_rate().0;
        let channels = config.channels() as usize;
        let channel = std::env::var("AUDIO_CHANNEL")
            .ok()
            .and_then(|c| c.parse::<usize>().ok())
            .map(|c| c.clamp(1, channels) - 1);
        println!(
            "Audio input: {device_name} ({rate} Hz, {channels} ch, {:?}){}",
            config.sample_format(),
            channel.map_or(String::new(), |c| format!(", using channel {}", c + 1))
        );

        let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(256);
        let stream_config: cpal::StreamConfig = config.clone().into();
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => build_stream::<f32>(&device, &stream_config, channels, channel, tx),
            cpal::SampleFormat::I16 => build_stream::<i16>(&device, &stream_config, channels, channel, tx),
            cpal::SampleFormat::I32 => build_stream::<i32>(&device, &stream_config, channels, channel, tx),
            cpal::SampleFormat::U16 => build_stream::<u16>(&device, &stream_config, channels, channel, tx),
            f => return Err(format!("unsupported sample format {f:?}")),
        }?;
        stream.play().map_err(|e| e.to_string())?;

        let state = Arc::new(Mutex::new(AudioState { engine, ..Default::default() }));
        let control = Arc::new(Control {
            engine: AtomicUsize::new(engine),
            gate_db_bits: AtomicU32::new(gate_db.to_bits()),
        });
        let (st, ctl) = (Arc::clone(&state), Arc::clone(&control));
        std::thread::Builder::new()
            .name("pitch-worker".into())
            .spawn(move || worker(rx, rate, st, ctl))
            .map_err(|e| e.to_string())?;

        Ok(Self { state, control, _stream: stream })
    }

    pub fn set_engine(&self, idx: usize) { self.control.engine.store(idx % ENGINES.len(), Ordering::Relaxed); }

    pub fn gate_db(&self) -> f32 { self.control.gate_db() }

    pub fn adjust_gate(&self, delta_db: f32) {
        let g = (self.gate_db() + delta_db).clamp(-90.0, -6.0);
        self.control.gate_db_bits.store(g.to_bits(), Ordering::Relaxed);
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    channel: Option<usize>,
    tx: SyncSender<Vec<f32>>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let mono: Vec<f32> = data
                    .chunks(channels)
                    .map(|fr| match channel {
                        Some(c) => fr[c].to_sample::<f32>(),
                        None => fr.iter().map(|s| s.to_sample::<f32>()).sum::<f32>() / channels as f32,
                    })
                    .collect();
                // If the worker falls behind, drop audio rather than block capture.
                let _ = tx.try_send(mono);
            },
            |e| eprintln!("Audio input error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

/// Runs the active engine over captured audio until the stream is dropped.
fn worker(rx: Receiver<Vec<f32>>, in_rate: u32, state: Arc<Mutex<AudioState>>, ctl: Arc<Control>) {
    let mut active = usize::MAX;
    let mut engine: Option<Box<dyn PitchEngine>> = None;
    let mut dc = DcBlocker::new(in_rate);
    let mut resampler = Resampler::new(in_rate, in_rate);
    let mut tracker = Tracker::new(tracker::TrackerParams::default());
    let mut chunk = Vec::new();
    let mut resampled = Vec::new();
    let mut frames = Vec::new();
    let mut events = Vec::new();
    let (mut busy, mut audio_secs, mut window_start) = (0.0f32, 0.0f32, Instant::now());

    while let Ok(first) = rx.recv() {
        chunk.clear();
        chunk.extend_from_slice(&first);
        while let Ok(more) = rx.try_recv() {
            chunk.extend_from_slice(&more);
        }

        let want = ctl.engine.load(Ordering::Relaxed);
        if want != active {
            active = want;
            tracker.release_all(&mut events);
            let built = build_engine(want);
            let mut s = state.lock().unwrap();
            s.engine = want;
            match built {
                Ok(e) => {
                    println!("Audio engine: {}  (~{:.0} ms latency)", e.name(), e.latency_ms());
                    resampler = Resampler::new(in_rate, e.sample_rate());
                    tracker = Tracker::new(e.tracker());
                    s.latency_ms = e.latency_ms();
                    s.engine_error = None;
                    engine = Some(e);
                }
                Err(err) => {
                    eprintln!("Audio engine {} unavailable: {err}", ENGINES[want]);
                    s.engine_error = Some(err);
                    engine = None;
                }
            }
        }

        let Some(eng) = engine.as_mut() else {
            flush_events(&state, &mut events, None);
            continue;
        };

        dc.process(&mut chunk);
        resampled.clear();
        resampler.process(&chunk, &mut resampled);
        let t0 = Instant::now();
        eng.push(&resampled, &mut frames);
        busy += t0.elapsed().as_secs_f32();
        audio_secs += chunk.len() as f32 / in_rate as f32;

        let gate = ctl.gate_db();
        let mut level = None;
        for f in frames.drain(..) {
            tracker.step(&f, gate, &mut events);
            level = Some(f.level_db);
        }

        let load = (window_start.elapsed().as_secs_f32() > 1.0).then(|| {
            let l = busy / audio_secs.max(1e-3);
            (busy, audio_secs, window_start) = (0.0, 0.0, Instant::now());
            l
        });
        flush_events(&state, &mut events, level.map(|l| (l, load)));
    }
}

fn flush_events(state: &Mutex<AudioState>, events: &mut Vec<tracker::Event>, meter: Option<(f32, Option<f32>)>) {
    if events.is_empty() && meter.is_none() {
        return;
    }
    let mut s = state.lock().unwrap();
    for (note, velocity, on) in events.drain(..) {
        s.notes[note as usize] = NoteState { velocity, on };
        push_event(&mut s.recent_events, NoteEvent { note, velocity, on });
    }
    if let Some((level, load)) = meter {
        s.level_db = level;
        if let Some(l) = load {
            s.load = l;
        }
    }
}

// ── Shared helpers ──────────────────────────────────────────────────────────

fn ort_env() -> Result<&'static ort::environment::Environment, String> {
    static ENV: OnceLock<Result<ort::environment::Environment, String>> = OnceLock::new();
    ENV.get_or_init(|| ort::init().with_name("midi-visuals").build().map_err(|e| e.to_string()))
        .as_ref()
        .map_err(Clone::clone)
}

/// CPU session with `threads` intra-op threads (`ORT_THREADS` overrides).
fn ort_session(model: &[u8], threads: usize) -> Result<ort::session::Session, String> {
    let threads = env_usize("ORT_THREADS", threads);
    ort::session::Session::builder(ort_env()?)
        .and_then(|b| b.with_intra_threads(threads).map_err(Into::into))
        .and_then(|mut b| b.commit_from_memory(model))
        .map_err(|e| e.to_string())
}

pub fn level_db(samples: &[f32]) -> f32 {
    let ms = samples.iter().map(|x| x * x).sum::<f32>() / samples.len().max(1) as f32;
    10.0 * ms.max(1e-12).log10()
}

pub fn hz_to_midi(hz: f32) -> Option<usize> {
    if !(hz > 0.0) {
        return None;
    }
    let m = (69.0 + 12.0 * (hz / 440.0).log2()).round();
    (0.0..128.0).contains(&m).then_some(m as usize)
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn env_f32(key: &str, default: f32) -> f32 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}
