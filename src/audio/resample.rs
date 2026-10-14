/// Streaming windowed-sinc resampler for mono audio.
///
/// Quality is aimed at pitch analysis, not playback: a 48-tap Blackman-windowed
/// sinc whose cutoff sits just below the lower Nyquist, so downsampling 48 kHz
/// to 16 kHz doesn't fold upper partials back into the analysis band.
pub struct Resampler {
    /// Input samples advanced per output sample (in_rate / out_rate).
    step: f64,
    /// Read position of the next output sample, as an index into `hist`.
    pos: f64,
    hist: Vec<f32>,
    /// Normalised cutoff (fraction of the input Nyquist).
    cutoff: f64,
    passthrough: bool,
}

const HALF: usize = 24;

impl Resampler {
    pub fn new(in_rate: u32, out_rate: u32) -> Self {
        let step = in_rate as f64 / out_rate as f64;
        Self {
            step,
            pos: HALF as f64,
            // Leading zeros give the filter history before the first real sample.
            hist: vec![0.0; HALF],
            cutoff: (1.0 / step).min(1.0) * 0.92,
            passthrough: in_rate == out_rate,
        }
    }

    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.passthrough {
            out.extend_from_slice(input);
            return;
        }
        self.hist.extend_from_slice(input);

        while self.pos + HALF as f64 + 1.0 < self.hist.len() as f64 {
            let i0 = self.pos.floor() as usize;
            let frac = self.pos - i0 as f64;
            let mut acc = 0.0f64;
            let mut norm = 0.0f64;
            for k in (1 - HALF as isize)..=(HALF as isize) {
                let t = k as f64 - frac;
                let w = self.kernel(t);
                acc += self.hist[(i0 as isize + k) as usize] as f64 * w;
                norm += w;
            }
            out.push((acc / norm) as f32);
            self.pos += self.step;
        }

        // Drop history the filter no longer reaches.
        let keep_from = (self.pos.floor() as usize).saturating_sub(HALF);
        if keep_from > 0 {
            self.hist.drain(..keep_from);
            self.pos -= keep_from as f64;
        }
    }

    fn kernel(&self, t: f64) -> f64 {
        let x = t * self.cutoff;
        let sinc = if x.abs() < 1e-9 { 1.0 } else { (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x) };
        // Blackman window over [-HALF, HALF].
        let u = (t / HALF as f64 + 1.0) * 0.5;
        let w = 0.42 - 0.5 * (std::f64::consts::TAU * u).cos() + 0.08 * (2.0 * std::f64::consts::TAU * u).cos();
        sinc * w.max(0.0)
    }
}

/// One-pole high-pass at ~20 Hz. Some inputs (an empty mic jack, cheap
/// interfaces) carry a DC offset or slow drift that reads as a loud level,
/// holding the gate open while containing no pitch at all.
pub struct DcBlocker {
    r: f32,
    x1: f32,
    y1: f32,
}

impl DcBlocker {
    pub fn new(rate: u32) -> Self {
        Self { r: 1.0 - std::f32::consts::TAU * 20.0 / rate as f32, x1: 0.0, y1: 0.0 }
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            let y = *x - self.x1 + self.r * self.y1;
            self.x1 = *x;
            self.y1 = y;
            *x = y;
        }
    }
}
