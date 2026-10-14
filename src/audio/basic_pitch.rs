//! Basic Pitch (Spotify, ICASSP 2022): polyphonic, instrument-agnostic note model.
//! https://github.com/spotify/basic-pitch — Apache-2.0, model vendored in assets/models.
//!
//! It is a batch model: a fixed 2 s window in, 172 frames (86 fps) of note and
//! onset posteriors out, and its CQT front end plus ~120 ms CNN lookahead mean
//! the newest frames are unreliable. We slide the window every `run_hop` samples
//! and read the frame `lookahead` frames back from the end. The official note
//! decoder runs backwards in time, so the tracker thresholds frames instead.

use ort::session::Session;
use ort::value::TensorRef;

use super::tracker::TrackerParams;
use super::{env_usize, level_db, ort_session, Frame, PitchEngine};

const SR: u32 = 22_050;
const FFT_HOP: usize = 256;
const N_SAMPLES: usize = 43_844; // AUDIO_SAMPLE_RATE * 2 - FFT_HOP
const N_FRAMES: usize = 172;
const N_KEYS: usize = 88;
const LOWEST_KEY: usize = 21; // A0

pub struct BasicPitch {
    session: Session,
    lookahead: usize,
    /// Model frames advanced per inference (each run emits this many frames).
    run_frames: usize,
    buf: Vec<f32>,
    pending: Vec<f32>,
    note: Vec<f32>,
    onset: Vec<f32>,
}

impl BasicPitch {
    pub fn new() -> Result<Self, String> {
        // A 2 s window per run is heavy: 4 threads keep it ~5x faster than real time.
        let session = ort_session(include_bytes!("../../assets/models/basic_pitch_nmp.onnx"), 4)?;
        Ok(Self {
            session,
            lookahead: env_usize("BASIC_PITCH_LOOKAHEAD", 6).clamp(1, 60),
            run_frames: env_usize("BASIC_PITCH_RUN_FRAMES", 4).clamp(1, 16),
            buf: vec![0.0; N_SAMPLES],
            pending: Vec::new(),
            note: Vec::new(),
            onset: Vec::new(),
        })
    }

    fn run(&mut self) -> Result<(), ort::Error> {
        let input = TensorRef::from_array_view(([1usize, N_SAMPLES, 1], &self.buf[..]))?;
        let outputs = self.session.run(ort::inputs!["serving_default_input_2:0" => input])?;
        let (_, note) = outputs["StatefulPartitionedCall:1"].try_extract_tensor::<f32>()?;
        let (_, onset) = outputs["StatefulPartitionedCall:2"].try_extract_tensor::<f32>()?;
        self.note.clear();
        self.note.extend_from_slice(note);
        self.onset.clear();
        self.onset.extend_from_slice(onset);
        Ok(())
    }
}

impl PitchEngine for BasicPitch {
    fn name(&self) -> &'static str { "basic-pitch" }
    fn sample_rate(&self) -> u32 { SR }
    fn latency_ms(&self) -> f32 {
        (self.lookahead + self.run_frames) as f32 * FFT_HOP as f32 / SR as f32 * 1000.0
    }

    fn tracker(&self) -> TrackerParams {
        // Upstream defaults: onset 0.5 starts a note, frame 0.3 sustains it.
        TrackerParams { on: 0.6, off: 0.3, onset: 0.5, on_frames: 1, off_frames: 2, max_poly: 8, min_retrigger_frames: 8 }
    }

    fn push(&mut self, samples: &[f32], out: &mut Vec<Frame>) {
        self.pending.extend_from_slice(samples);
        let run_len = self.run_frames * FFT_HOP;
        while self.pending.len() >= run_len {
            self.buf.drain(..run_len);
            self.buf.extend(self.pending.drain(..run_len));
            if let Err(e) = self.run() {
                eprintln!("basic-pitch inference failed: {e}");
                return;
            }
            let last = N_FRAMES - 1 - self.lookahead;
            for fi in (last + 1 - self.run_frames)..=last {
                let s = (fi * FFT_HOP).min(N_SAMPLES - FFT_HOP);
                let mut frame = Frame::silent(level_db(&self.buf[s..s + FFT_HOP]));
                let row = fi * N_KEYS;
                for k in 0..N_KEYS {
                    frame.act[LOWEST_KEY + k] = self.note[row + k];
                    frame.onset[LOWEST_KEY + k] = self.onset[row + k];
                }
                out.push(frame);
            }
        }
    }
}
