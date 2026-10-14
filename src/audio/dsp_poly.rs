//! Hand-rolled polyphonic detector, no model files.
//!
//! Per 16 ms hop: Hann-windowed FFT → spectral whitening (each bin divided by its
//! local average, so partials stand out regardless of timbre) → harmonic-sum
//! salience per note → iterative pick-and-cancel (Klapuri 2006): take the most
//! salient note, subtract its partials, repeat. Cancelling stops a strong note's
//! upper partials from reading as extra notes. Spectral flux on the log
//! spectrum gives onsets, used to re-strike held notes.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

use super::tracker::TrackerParams;
use super::{env_f32, level_db, Frame, PitchEngine};

const SR: u32 = 16_000;
const N: usize = 2048;
const HOP: usize = 256;
const BINS: usize = N / 2 + 1;
const BIN_HZ: f32 = SR as f32 / N as f32;
const LO: usize = 36; // C2
const HI: usize = 96; // C7
const MAX_HARM: usize = 20;
const HARM_MAX_HZ: f32 = 6000.0;
const MAX_POLY: usize = 6;

struct Partial {
    lo: usize,
    hi: usize,
    w: f32,
}

pub struct DspPoly {
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    amp_scale: f32,
    buf: Vec<f32>,
    pending: Vec<f32>,
    input: Vec<f32>,
    spec: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    mag: Vec<f32>,
    prefix: Vec<f32>,
    resid: Vec<f32>,
    prev_log: Vec<f32>,
    flux_hist: [f32; 12],
    flux_above: bool,
    onset_window: u8,
    sal_hist: [[f32; 128]; 4],
    partials: Vec<Vec<Partial>>,
    /// Whitened level a partial must exceed to count (noise sits around 1).
    noise: f32,
    /// Salience that maps to full activation.
    act_scale: f32,
    /// Smallest salience that counts as a note at all.
    min_sal: f32,
    /// Later notes must reach this fraction of the strongest one.
    rel_stop: f32,
    /// Odd-partial evidence (vs. the upper note's salience) that moves a pick down an octave.
    odd_ratio: f32,
}

impl DspPoly {
    pub fn new() -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(N);
        let window: Vec<f32> = (0..N)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos())
            .collect();
        let amp_scale = 2.0 / window.iter().sum::<f32>();

        let inharm = env_f32("DSP_INHARM", 0.0004);
        let mut partials = Vec::new();
        for m in LO..=HI {
            let f0 = midi_hz(m);
            let mut ps = Vec::new();
            for h in 1..=MAX_HARM {
                // Stiff strings (piano most of all) stretch partials sharp:
                // f_h = h·f0·√(1 + B·h²). The band spans the ideal and the
                // stretched position, ±0.4 semitone, at least the nearest bin.
                let ideal = f0 * h as f32;
                let f = ideal * (1.0 + inharm * (h * h) as f32).sqrt();
                if f > HARM_MAX_HZ {
                    break;
                }
                let centre = (f / BIN_HZ).round() as usize;
                let lo = ((ideal * 0.977 / BIN_HZ).ceil() as usize).min(centre);
                let hi = ((f * 1.0234 / BIN_HZ).floor() as usize).max(centre);
                // Decaying weights independent of f0: a high note's lone partial
                // can't outweigh a lower note that explains several.
                let w = (h as f32).powf(-self_weight_exp());
                ps.push(Partial { lo, hi: hi.min(BINS - 1), w });
            }
            partials.push(ps);
        }

        Self {
            scratch: fft.make_scratch_vec(),
            spec: fft.make_output_vec(),
            fft,
            window,
            amp_scale,
            buf: vec![0.0; N],
            pending: Vec::new(),
            input: vec![0.0; N],
            mag: vec![0.0; BINS],
            prefix: vec![0.0; BINS + 1],
            resid: vec![0.0; BINS],
            prev_log: vec![0.0; BINS],
            flux_hist: [0.0; 12],
            flux_above: false,
            onset_window: 0,
            sal_hist: [[0.0; 128]; 4],
            partials,
            noise: env_f32("DSP_NOISE", 1.5),
            act_scale: env_f32("DSP_ACT_SCALE", 4.0),
            min_sal: env_f32("DSP_MIN_SAL", 1.0),
            rel_stop: env_f32("DSP_REL_STOP", 0.2),
            odd_ratio: env_f32("DSP_ODD_RATIO", 0.25),
        }
    }

    /// Salience from partials 1, 3 and 5 of `m` alone.
    fn odd_evidence(&self, m: usize) -> f32 {
        let mut s = 0.0;
        for (_, p) in self.partials[m - LO].iter().enumerate().filter(|(h, _)| h % 2 == 0).take(3) {
            let peak = self.resid[p.lo..=p.hi].iter().copied().fold(0.0, f32::max);
            s += p.w * (peak - self.noise).max(0.0);
        }
        s
    }

    fn salience(&self, m: usize, spec: &[f32]) -> f32 {
        let mut s = 0.0;
        for p in &self.partials[m - LO] {
            let peak = spec[p.lo..=p.hi].iter().copied().fold(0.0, f32::max);
            s += p.w * (peak - self.noise).max(0.0);
        }
        s
    }

    fn analyze(&mut self, frame: &mut Frame) {
        for i in 0..N {
            self.input[i] = self.buf[i] * self.window[i];
        }
        if self.fft.process_with_scratch(&mut self.input, &mut self.spec, &mut self.scratch).is_err() {
            return;
        }
        for k in 0..BINS {
            self.mag[k] = self.spec[k].norm() * self.amp_scale;
        }

        // Onsets: half-wave rectified flux of the log spectrum against a running
        // mean. Only the rising edge counts; it opens a short window in which notes
        // whose salience has jumped get re-struck (their salience lags the flux).
        let mut flux = 0.0;
        for k in 1..BINS {
            let l = (1.0 + 1000.0 * self.mag[k]).ln();
            flux += (l - self.prev_log[k]).max(0.0);
            self.prev_log[k] = l;
        }
        let mean = self.flux_hist.iter().sum::<f32>() / self.flux_hist.len() as f32;
        let above = flux > mean * 1.6 + 25.0;
        if above && !self.flux_above {
            self.onset_window = 4;
        }
        self.flux_above = above;
        self.flux_hist.rotate_left(1);
        self.flux_hist[11] = flux;

        // Partial whitening (Klapuri's ν = 0.33): each bin over its local mean
        // magnitude, times a third-power of that mean's level so louder regions
        // still count for more. Floored 40 dB under the loudest region.
        self.prefix[0] = 0.0;
        for k in 0..BINS {
            self.prefix[k + 1] = self.prefix[k] + self.mag[k];
        }
        let local_mean = |k: usize, prefix: &[f32]| {
            let w = (k / 8).max(8);
            let a = k.saturating_sub(w);
            let b = (k + w + 1).min(BINS);
            (prefix[b] - prefix[a]) / (b - a) as f32
        };
        let mut env_max: f32 = 0.0;
        for k in 0..BINS {
            env_max = env_max.max(local_mean(k, &self.prefix));
        }
        let floor = env_max * 0.01 + 1e-7;
        for k in 0..BINS {
            let env = local_mean(k, &self.prefix).max(floor);
            self.resid[k] = self.mag[k] / env * (env / (env_max + 1e-9)).powf(0.33);
        }

        let mut sal = [0.0f32; 128];
        for m in LO..=HI {
            sal[m] = self.salience(m, &self.resid);
        }

        // Iterative pick-and-cancel, stopping once the best remaining note is
        // weak in absolute terms or next to the strongest one.
        let mut taken = [false; 128];
        let mut first = 0.0;
        for i in 0..MAX_POLY {
            let mut best = (0, 0.0f32);
            for m in LO..=HI {
                if taken[m] {
                    continue;
                }
                let c = if i == 0 { sal[m] } else { self.salience(m, &self.resid) };
                if c > best.1 {
                    best = (m, c);
                }
            }
            let (mut m, mut c) = best;
            // Octave check: prefer the octave below when its odd partials (which
            // the upper note can't explain) carry their own energy.
            while m >= LO + 12 && !taken[m - 12] {
                let odd = self.odd_evidence(m - 12);
                if odd < c * self.odd_ratio {
                    break;
                }
                m -= 12;
                c = self.salience(m, &self.resid);
            }
            if i == 0 {
                first = c;
            }
            let act = (c / self.act_scale).min(1.0);
            if c < self.min_sal || c < first * self.rel_stop {
                break;
            }
            taken[m] = true;
            frame.act[m] = act;

            // Cancel the note's partials, smoothed so a partial shared with another
            // note is only removed as far as this note's spectral envelope implies.
            let ps = &self.partials[m - LO];
            let peaks: Vec<(usize, f32)> = ps
                .iter()
                .map(|p| {
                    (p.lo..=p.hi)
                        .map(|k| (k, self.resid[k]))
                        .fold((p.lo, 0.0), |a, b| if b.1 > a.1 { b } else { a })
                })
                .collect();
            for (i, &(kp, a)) in peaks.iter().enumerate() {
                if a <= 0.0 {
                    continue;
                }
                let lo = peaks[i.saturating_sub(1)].1;
                let hi = peaks.get(i + 1).map_or(a, |p| p.1);
                let smooth = a.min((lo + a + hi) / 3.0).max(a * 0.5);
                let keep = 1.0 - smooth / a;
                for k in kp.saturating_sub(2)..=(kp + 2).min(BINS - 1) {
                    self.resid[k] *= keep;
                }
            }
        }

        if self.onset_window > 0 {
            self.onset_window -= 1;
            let old = &self.sal_hist[0];
            for m in LO..=HI {
                if frame.act[m] > 0.0 && sal[m] > old[m] * 1.5 + self.min_sal * 0.5 {
                    frame.onset[m] = 1.0;
                }
            }
        }
        self.sal_hist.rotate_left(1);
        self.sal_hist[3] = sal;
    }
}

impl PitchEngine for DspPoly {
    fn name(&self) -> &'static str { "dsp-poly" }
    fn sample_rate(&self) -> u32 { SR }
    fn latency_ms(&self) -> f32 { (N / 4 + HOP) as f32 / SR as f32 * 1000.0 }

    fn tracker(&self) -> TrackerParams {
        TrackerParams { on: 0.35, off: 0.12, onset: 0.5, on_frames: 3, off_frames: 6, max_poly: MAX_POLY, min_retrigger_frames: 8 }
    }

    fn push(&mut self, samples: &[f32], out: &mut Vec<Frame>) {
        self.pending.extend_from_slice(samples);
        while self.pending.len() >= HOP {
            self.buf.drain(..HOP);
            self.buf.extend(self.pending.drain(..HOP));
            let mut frame = Frame::silent(level_db(&self.buf[N - HOP..]));
            self.analyze(&mut frame);
            out.push(frame);
        }
    }
}

fn self_weight_exp() -> f32 { env_f32("DSP_WEIGHT_EXP", 0.7) }

fn midi_hz(m: usize) -> f32 { 440.0 * 2f32.powf((m as f32 - 69.0) / 12.0) }
