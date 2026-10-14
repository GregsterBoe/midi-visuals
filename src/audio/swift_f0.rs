//! SwiftF0 (Nieradzik, 2025): a 14k-parameter monophonic F0 model.
//! https://github.com/lars76/swift-f0 — MIT, model vendored in assets/models.
//!
//! The model emits one pitch + confidence per 256-sample hop at 16 kHz. A frame
//! is only bit-exact with batch output after 10 frames (176 ms) of lookahead;
//! we read it after `lookahead` frames instead and accept a little drift.

use ort::session::Session;
use ort::value::TensorRef;

use super::tracker::TrackerParams;
use super::{env_usize, hz_to_midi, level_db, ort_session, Frame, PitchEngine};

const SR: u32 = 16_000;
const HOP: usize = 256;
const FMIN: f32 = 46.875;
const FMAX: f32 = 2093.75;
/// Frames of context fed per run: 11 left frames reproduce batch output.
const LEFT_FRAMES: usize = 11;
/// Hops whose peak is below this are digital silence: the model hallucinates there.
const SILENCE_PEAK: f32 = 1e-3;

pub struct SwiftF0 {
    session: Session,
    lookahead: usize,
    /// Last (LEFT_FRAMES + 1 + lookahead) hops of audio.
    buf: Vec<f32>,
    pending: Vec<f32>,
    levels: [f32; 4],
    prev_note: Option<usize>,
    /// Frames since the last loudness jump, while it may still re-strike.
    since_jump: Option<u8>,
    /// Consecutive confident frames of `prev_note` since that jump.
    stable: u8,
}

impl SwiftF0 {
    pub fn new() -> Result<Self, String> {
        let session = ort_session(include_bytes!("../../assets/models/swift_f0.onnx"), 1)?;
        let lookahead = env_usize("SWIFT_F0_LOOKAHEAD", 1).min(10);
        let n = (LEFT_FRAMES + 1 + lookahead) * HOP;
        Ok(Self { session, lookahead, buf: vec![0.0; n], pending: Vec::new(), levels: [-120.0; 4], prev_note: None, since_jump: None, stable: 0 })
    }

    fn run(&mut self) -> Result<(f32, f32), ort::Error> {
        let audio = TensorRef::from_array_view(([1usize, self.buf.len()], &self.buf[..]))?;
        let fmin = TensorRef::from_array_view(([0usize; 0], &[FMIN][..]))?;
        let fmax = TensorRef::from_array_view(([0usize; 0], &[FMAX][..]))?;
        let outputs = self.session.run(ort::inputs![
            "audio" => audio,
            "fmin" => fmin,
            "fmax" => fmax,
        ])?;
        let (_, pitch) = outputs["pitch"].try_extract_tensor::<f64>()?;
        let (_, conf) = outputs["confidence"].try_extract_tensor::<f32>()?;
        let i = (pitch.len() - 1).saturating_sub(self.lookahead).min(LEFT_FRAMES);
        Ok((pitch[i] as f32, conf[i]))
    }
}

impl PitchEngine for SwiftF0 {
    fn name(&self) -> &'static str { "swift-f0" }
    fn sample_rate(&self) -> u32 { SR }
    fn latency_ms(&self) -> f32 { (self.lookahead + 1) as f32 * HOP as f32 / SR as f32 * 1000.0 }

    fn tracker(&self) -> TrackerParams {
        TrackerParams { on: 0.5, off: 0.35, onset: 0.5, on_frames: 2, off_frames: 3, max_poly: 1, min_retrigger_frames: 6 }
    }

    fn push(&mut self, samples: &[f32], out: &mut Vec<Frame>) {
        self.pending.extend_from_slice(samples);
        while self.pending.len() >= HOP {
            self.buf.drain(..HOP);
            self.buf.extend(self.pending.drain(..HOP));

            // The frame we report is `lookahead` hops back from the newest audio.
            let s = LEFT_FRAMES * HOP;
            let hop = &self.buf[s..s + HOP];
            let db = level_db(hop);
            let silent = hop.iter().all(|x| x.abs() < SILENCE_PEAK);

            let mut frame = Frame::silent(db);
            // A loudness jump re-strikes the note, but only once the same pitch has
            // held for 3 frames after it: at a note change the first two frames
            // still report the old note.
            let floor = self.levels.iter().copied().fold(f32::MAX, f32::min);
            if db - floor > 6.0 {
                (self.since_jump, self.stable) = (Some(0), 0);
            }
            let mut note = None;
            if !silent {
                match self.run() {
                    Ok((hz, conf)) => {
                        note = hz_to_midi(hz);
                        if let Some(n) = note {
                            frame.act[n] = conf;
                            if self.since_jump.is_some() {
                                let same = self.prev_note == Some(n) && conf >= 0.5;
                                self.stable = if same { self.stable + 1 } else { 0 };
                                if self.stable >= 2 {
                                    frame.onset[n] = 1.0;
                                    self.since_jump = None;
                                }
                            }
                        }
                    }
                    Err(e) => eprintln!("swift-f0 inference failed: {e}"),
                }
            }
            self.since_jump = self.since_jump.map(|j| j + 1).filter(|&j| j < 8);
            self.prev_note = note;
            self.levels.rotate_left(1);
            self.levels[3] = db;
            out.push(frame);
        }
    }
}
