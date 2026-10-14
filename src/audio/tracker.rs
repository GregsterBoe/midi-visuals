use super::Frame;

/// Thresholds that turn per-frame note activations into on/off events.
/// Each engine ships its own, since their activations mean different things
/// (a calibrated probability for the models, a harmonic contrast for DSP).
#[derive(Clone, Copy, Default)]
pub struct TrackerParams {
    /// Activation needed to start a note.
    pub on: f32,
    /// Activation below which a held note starts counting toward release.
    pub off: f32,
    /// Onset strength that starts a note early (if `act >= off`) or re-strikes a held one.
    pub onset: f32,
    /// Consecutive frames a new note must qualify before it starts
    /// (filters one-frame blips such as the last note's reverb tail).
    pub on_frames: u8,
    /// Consecutive frames below `off` before release (debounces flicker).
    pub off_frames: u8,
    pub max_poly: usize,
    /// A held note can only be re-struck after this many frames.
    pub min_retrigger_frames: u16,
}

pub struct Tracker {
    p: TrackerParams,
    on: [bool; 128],
    act_on: [f32; 128],
    below: [u8; 128],
    above: [u8; 128],
    age: [u16; 128],
    cands: Vec<(usize, f32)>,
}

/// (note, velocity, on)
pub type Event = (u8, f32, bool);

impl Tracker {
    /// `TRACK_ON`, `TRACK_OFF`, `TRACK_ON_FRAMES`, `TRACK_OFF_FRAMES` override
    /// the engine's defaults, for tuning against a real instrument.
    pub fn new(mut p: TrackerParams) -> Self {
        let get = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<f32>().ok());
        if let Some(v) = get("TRACK_ON") { p.on = v; }
        if let Some(v) = get("TRACK_OFF") { p.off = v; }
        if let Some(v) = get("TRACK_ON_FRAMES") { p.on_frames = v as u8; }
        if let Some(v) = get("TRACK_OFF_FRAMES") { p.off_frames = v as u8; }
        Self {
            p,
            on: [false; 128],
            act_on: [0.0; 128],
            below: [0; 128],
            above: [0; 128],
            age: [0; 128],
            cands: Vec::with_capacity(16),
        }
    }

    /// Release every held note (engine switch, input loss).
    pub fn release_all(&mut self, out: &mut Vec<Event>) {
        for n in 0..128 {
            if self.on[n] {
                self.on[n] = false;
                out.push((n as u8, 0.0, false));
            }
        }
    }

    pub fn step(&mut self, f: &Frame, gate_db: f32, out: &mut Vec<Event>) {
        let p = self.p;
        if f.level_db < gate_db {
            self.above = [0; 128];
            self.release_all(out);
            return;
        }
        // Louder than the gate by 36 dB or more is full velocity.
        let vel = ((f.level_db - gate_db) / 36.0).clamp(0.1, 1.0);

        // Releases and re-strikes of held notes.
        let mut held = 0;
        for n in 0..128 {
            if !self.on[n] {
                continue;
            }
            self.age[n] = self.age[n].saturating_add(1);
            if f.act[n] < p.off {
                self.below[n] += 1;
                if self.below[n] >= p.off_frames {
                    self.on[n] = false;
                    out.push((n as u8, 0.0, false));
                    continue;
                }
            } else {
                self.below[n] = 0;
                self.act_on[n] = f.act[n];
            }
            if f.onset[n] >= p.onset && self.age[n] >= p.min_retrigger_frames {
                out.push((n as u8, 0.0, false));
                out.push((n as u8, vel, true));
                self.age[n] = 0;
            }
            held += 1;
        }

        // New notes, strongest first, up to the polyphony limit.
        self.cands.clear();
        for n in 0..128 {
            let a = f.act[n];
            if self.on[n] {
                continue;
            }
            if a >= p.on || (f.onset[n] >= p.onset && a >= p.off) {
                self.above[n] = self.above[n].saturating_add(1);
                if self.above[n] >= p.on_frames {
                    self.cands.push((n, a));
                }
            } else {
                self.above[n] = 0;
            }
        }
        self.cands.sort_by(|a, b| b.1.total_cmp(&a.1));
        for i in 0..self.cands.len() {
            let (n, a) = self.cands[i];
            if held >= p.max_poly {
                // Steal the weakest held note if the newcomer is clearly stronger
                // (lets a monophonic engine follow legato lines without a gap).
                let weakest = (0..128).filter(|&m| self.on[m]).min_by(|&x, &y| self.act_on[x].total_cmp(&self.act_on[y]));
                match weakest {
                    Some(w) if a > self.act_on[w] => {
                        self.on[w] = false;
                        out.push((w as u8, 0.0, false));
                        held -= 1;
                    }
                    _ => break,
                }
            }
            self.on[n] = true;
            self.act_on[n] = a;
            self.below[n] = 0;
            self.age[n] = 0;
            out.push((n as u8, vel, true));
            held += 1;
        }
    }
}
