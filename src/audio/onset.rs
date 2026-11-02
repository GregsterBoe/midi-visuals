//! Onset envelope of the raw input, for beat tracking: the spectral flux
//! (how much louder each frequency got since the last hop), one value per
//! 10 ms. It runs on every input chunk next to the pitch engine, so beats
//! don't depend on which notes the engine finds.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

pub const HOP_SECS: f32 = 0.01;
/// Kick drum and bass: their flux counts extra, since they carry the beat.
const LOW_HZ: f32 = 180.0;
const HIGH_HZ: f32 = 8000.0;
const LOW_WEIGHT: f32 = 1.5;
/// Log compression, so quiet hits still register next to loud ones.
const COMPRESS: f32 = 1000.0;

pub struct OnsetDetector {
    n: usize,
    hop: usize,
    pending: Vec<f32>,
    window: Vec<f32>,
    input: Vec<f32>,
    spec: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    fft: Arc<dyn RealToComplex<f32>>,
    prev: Vec<f32>,
    low_bins: usize,
    high_bins: usize,
}

impl OnsetDetector {
    pub fn new(rate: u32) -> Self {
        // About 21 ms of audio per window: short enough to place a hit
        // within a hop, long enough to resolve the bass.
        let n = ((rate as f32 * 0.021) as usize).next_power_of_two();
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(n);
        let bin_hz = rate as f32 / n as f32;
        let window = (0..n).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos()).collect();
        Self {
            n,
            hop: ((rate as f32 * HOP_SECS) as usize).max(1),
            pending: Vec::with_capacity(n * 2),
            window,
            input: vec![0.0; n],
            scratch: fft.make_scratch_vec(),
            spec: fft.make_output_vec(),
            fft,
            prev: vec![0.0; n / 2 + 1],
            low_bins: ((LOW_HZ / bin_hz).ceil() as usize).max(2),
            high_bins: ((HIGH_HZ / bin_hz) as usize).min(n / 2),
        }
    }

    /// Feeds mono samples at the rate given to `new`, appending one onset
    /// value per completed hop to `out`.
    pub fn push(&mut self, samples: &[f32], out: &mut Vec<f32>) {
        self.pending.extend_from_slice(samples);
        while self.pending.len() >= self.n {
            for i in 0..self.n {
                self.input[i] = self.pending[i] * self.window[i];
            }
            self.pending.drain(..self.hop);
            if self.fft.process_with_scratch(&mut self.input, &mut self.spec, &mut self.scratch).is_err() {
                continue;
            }
            let norm = 4.0 / self.n as f32;
            let (mut low, mut full) = (0.0, 0.0);
            for k in 1..self.high_bins {
                let mag = (COMPRESS * self.spec[k].norm() * norm).ln_1p();
                let rise = (mag - self.prev[k]).max(0.0);
                self.prev[k] = mag;
                if k < self.low_bins {
                    low += rise;
                }
                full += rise;
            }
            let low = low / (self.low_bins - 1) as f32;
            let full = full / (self.high_bins - 1) as f32;
            out.push(LOW_WEIGHT * low + full);
        }
    }
}
