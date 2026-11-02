//! A few calm, decisive musical features for the sketches, from either note source.
//!
//! Raw input is busy. A pitch engine on a full mix reports ten or more notes
//! a second and its held notes flicker in and out. Sketches that answer all
//! of it twitch, so they read `Music` instead:
//!
//! - `beat`: a beat landed this frame. From audio, beats come from a tempo
//!   tracker on the onset envelope, so they keep time and skip off-beat
//!   noise; from MIDI, every played chord is a beat.
//! - `hits`: the notes to answer this frame. From audio they come only on
//!   beats, one or two at a time.
//! - `pulse`: swells quickly but smoothly on each beat, then dies away.
//! - `energy`: how loud and busy the music is, slow.
//! - `held`: a steady chord. From audio it changes only on a beat, once a
//!   new chord has settled and the last one has had its time.

use std::collections::VecDeque;

use crate::audio::onset::HOP_SECS;
use crate::midi::{NoteEvent, NoteState};

#[derive(Clone)]
pub struct Music {
    /// Strength in (0, 1] on the frame a beat lands, else 0.
    pub beat: f32,
    pub hits: Vec<NoteEvent>,
    /// 0–1, rises over ~50 ms on a beat, falls over ~400 ms.
    pub pulse: f32,
    /// 0–1, follows over a second or two.
    pub energy: f32,
    /// Beats per minute while the tracker is locked, else 0.
    pub bpm: f32,
    /// The steady chord: one note per pitch class from audio, the keys as
    /// played from MIDI.
    pub held: [NoteState; 128],
    /// `held` changed this frame.
    pub chord_changed: bool,
}

impl Default for Music {
    fn default() -> Self {
        Self {
            beat: 0.0,
            hits: Vec::new(),
            pulse: 0.0,
            energy: 0.0,
            bpm: 0.0,
            held: [NoteState::default(); 128],
            chord_changed: false,
        }
    }
}

impl Music {
    pub fn on_beat(&self) -> bool { self.beat > 0.0 }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Source {
    Midi,
    Audio,
}

// ── Tuning ──────────────────────────────────────────────────────────────────

/// MIDI: notes closer than this count as one chord, and so one beat.
const MIDI_BEAT_GAP: f32 = 0.08;
/// MIDI: at most this many notes answered per frame.
const MIDI_MAX_HITS: usize = 4;
/// Audio: notes answered per beat (one more on a strong beat).
const AUDIO_HITS: usize = 1;
const STRONG_BEAT: f32 = 0.7;
/// Audio: note-ons older than this are forgotten instead of waiting for a beat.
const PENDING_SECS: f32 = 0.8;

const PULSE_ATTACK: f32 = 22.0;
const PULSE_DECAY: f32 = 3.0;
const ENERGY_RISE: f32 = 1.2;
const ENERGY_FALL: f32 = 0.5;

/// How long a new chord must hold before it's taken, and how long a chord
/// stays at least, per source.
const SETTLE_SECS: [f32; 2] = [0.05, 0.3];
const MIN_CHORD_SECS: [f32; 2] = [0.25, 1.6];
/// Silence after which the chord is let go.
const RELEASE_SECS: [f32; 2] = [0.4, 2.0];
/// Audio: pitch-class weights fade with this time constant.
const CHROMA_SECS: f32 = 1.2;
const CHORD_MAX_CLASSES: usize = 4;

pub struct Analyzer {
    t: f32,
    tracker: BeatTracker,
    last_beat: f32,
    /// Audio note-ons waiting for a beat: (time, event).
    pending: Vec<(f32, NoteEvent)>,
    pulse_target: f32,
    pulse: f32,
    energy: f32,
    note_rate: f32,
    chroma: [f32; 12],
    chord: u16,
    held: [NoteState; 128],
    candidate: u16,
    candidate_since: f32,
    chord_since: f32,
    last_sound: f32,
}

impl Analyzer {
    pub fn new() -> Self {
        Self {
            t: 0.0,
            tracker: BeatTracker::new(),
            last_beat: -1.0,
            pending: Vec::new(),
            pulse_target: 0.0,
            pulse: 0.0,
            energy: 0.0,
            note_rate: 0.0,
            chroma: [0.0; 12],
            chord: 0,
            held: [NoteState::default(); 128],
            candidate: 0,
            candidate_since: 0.0,
            chord_since: -10.0,
            last_sound: -10.0,
        }
    }

    /// One frame. `onsets` is the audio onset envelope since the last frame
    /// (empty on MIDI); `loudness` is 0–1 above the gate, from audio.
    pub fn step(
        &mut self,
        source: Source,
        events: &VecDeque<NoteEvent>,
        notes: &[NoteState; 128],
        onsets: &[f32],
        loudness: Option<f32>,
        dt: f32,
    ) -> Music {
        self.t += dt;
        let si = source as usize;
        let mut music = Music::default();
        let ons = events.iter().filter(|e| e.on);

        match source {
            Source::Midi => {
                let mut hits: Vec<NoteEvent> = ons.copied().collect();
                hits.sort_by(|a, b| b.velocity.total_cmp(&a.velocity));
                hits.truncate(MIDI_MAX_HITS);
                if let Some(top) = hits.first() {
                    if self.t - self.last_beat >= MIDI_BEAT_GAP {
                        music.beat = top.velocity.max(0.2);
                        self.last_beat = self.t;
                    }
                }
                music.hits = hits;
            }
            Source::Audio => {
                for e in ons {
                    self.pending.push((self.t, *e));
                }
                let t = self.t;
                self.pending.retain(|(at, _)| t - at < PENDING_SECS);
                let gate_open = loudness.is_some_and(|l| l > 0.0);
                if let Some(strength) = self.tracker.feed(onsets, gate_open) {
                    music.beat = strength;
                    self.last_beat = self.t;
                    music.hits = self.beat_hits(strength, notes);
                }
                music.bpm = self.tracker.bpm();
            }
        }

        // Pulse: a beat raises the target, the pulse chases it fast, and
        // the target dies away, so the rise is eased but still sharp.
        self.pulse_target = (self.pulse_target * (-PULSE_DECAY * dt).exp()).max(music.beat);
        self.pulse += (self.pulse_target - self.pulse) * (1.0 - (-PULSE_ATTACK * dt).exp());
        music.pulse = self.pulse.clamp(0.0, 1.0);

        // Energy: loudness from audio, note rate from MIDI.
        let energy_target = match source {
            Source::Audio => loudness.unwrap_or(0.0),
            Source::Midi => {
                let played: f32 = events.iter().filter(|e| e.on).map(|e| e.velocity).sum();
                self.note_rate = self.note_rate * (-dt / 2.0).exp() + played / 2.0;
                1.0 - (-self.note_rate / 4.0).exp()
            }
        };
        let rate = if energy_target > self.energy { ENERGY_RISE } else { ENERGY_FALL };
        self.energy += (energy_target - self.energy) * (1.0 - (-rate * dt).exp());
        music.energy = self.energy.clamp(0.0, 1.0);

        music.chord_changed = self.track_chord(source, si, notes, music.on_beat(), dt);
        music.held = self.held;
        music
    }

    /// The notes to answer on an audio beat: the strongest that started
    /// since the last one, or the strongest held note.
    fn beat_hits(&mut self, strength: f32, notes: &[NoteState; 128]) -> Vec<NoteEvent> {
        let n = AUDIO_HITS + (strength >= STRONG_BEAT) as usize;
        let mut cands: Vec<NoteEvent> = self.pending.drain(..).map(|(_, e)| e).collect();
        if cands.is_empty() {
            cands = (0..128u8)
                .filter(|&k| notes[k as usize].on)
                .map(|k| NoteEvent { note: k, velocity: notes[k as usize].velocity, on: true })
                .collect();
        }
        cands.sort_by(|a, b| b.velocity.total_cmp(&a.velocity));
        let mut hits: Vec<NoteEvent> = Vec::with_capacity(n);
        for c in cands {
            if hits.len() >= n { break; }
            if hits.iter().any(|h| h.note % 12 == c.note % 12) { continue; }
            hits.push(NoteEvent { velocity: 0.7 * strength + 0.3 * c.velocity, ..c });
        }
        hits
    }

    /// Updates the steady chord; true when it changed.
    fn track_chord(&mut self, source: Source, si: usize, notes: &[NoteState; 128], beat: bool, dt: f32) -> bool {
        let any = notes.iter().any(|n| n.on);
        if any {
            self.last_sound = self.t;
        }
        let candidate = match source {
            Source::Midi => (0..128).filter(|&n| notes[n].on).fold(0u16, |m, n| m | 1 << (n % 12)),
            Source::Audio => {
                let fade = (-dt / CHROMA_SECS).exp();
                for (pc, w) in self.chroma.iter_mut().enumerate() {
                    *w *= fade;
                    for oct in 0..11 {
                        let n = oct * 12 + pc;
                        if n < 128 && notes[n].on {
                            *w += notes[n].velocity.max(0.2) * dt;
                        }
                    }
                }
                strongest_classes(&self.chroma)
            }
        };
        if candidate != self.candidate {
            self.candidate = candidate;
            self.candidate_since = self.t;
        }

        let settled = self.t - self.candidate_since >= SETTLE_SECS[si];
        let aged = self.t - self.chord_since >= MIN_CHORD_SECS[si];
        // From audio a chord change waits for a beat, unless the beats stopped.
        let on_time = source == Source::Midi || beat || self.t - self.last_beat > 1.0;
        let silent = self.t - self.last_sound >= RELEASE_SECS[si];

        let next = if silent { 0 } else { self.candidate };
        if next == self.chord || !(silent || (next != 0 && settled && aged && on_time)) {
            return false;
        }
        self.chord = next;
        self.chord_since = self.t;
        self.held = [NoteState::default(); 128];
        match source {
            Source::Midi if !silent => self.held = *notes,
            _ => {
                for pc in 0..12 {
                    if next & 1 << pc != 0 {
                        self.held[48 + pc] = NoteState { velocity: 0.8, on: true };
                    }
                }
            }
        }
        true
    }
}

/// The pitch classes that dominate the weights, at most `CHORD_MAX_CLASSES`.
fn strongest_classes(w: &[f32; 12]) -> u16 {
    let max = w.iter().copied().fold(0.0f32, f32::max);
    if max < 0.08 {
        return 0;
    }
    let mut order: Vec<usize> = (0..12).collect();
    order.sort_by(|&a, &b| w[b].total_cmp(&w[a]));
    order.iter()
        .take(CHORD_MAX_CLASSES)
        .filter(|&&pc| w[pc] >= 0.4 * max)
        .fold(0u16, |m, &pc| m | 1 << pc)
}

// ── Beat tracking ───────────────────────────────────────────────────────────

/// Onset history for the tempo estimate, in hops (8 s).
const HISTORY: usize = 800;
// Beats faster than 150 BPM are tracked at half time: calmer to watch.
const MIN_LAG: usize = 40;   // 150 BPM
const MAX_LAG: usize = 100;  // 60 BPM
/// Tempo prior: centred on 100 BPM, about an octave wide.
const PRIOR_LAG: f32 = 60.0;
const PRIOR_WIDTH: f32 = 1.0;
const TEMPO_EVERY: usize = 25;
/// Each estimate moves the averaged autocorrelation this far, so the
/// tempo follows the last few seconds rather than the last moment.
const ACF_BLEND: f32 = 0.12;
/// A rival tempo must score this much better, this many estimates in a
/// row, to take over.
const RIVAL_MARGIN: f32 = 1.25;
const RIVAL_COUNT: u8 = 8;
/// Onset threshold, in mean deviations above the running mean.
const PEAK_K: f32 = 1.6;
/// An onset this close to a predicted beat (fraction of a period) steers it.
const MATCH_WINDOW: f32 = 0.2;
const PHASE_GAIN: f32 = 0.25;
/// Below this tempo confidence the tracker follows raw onsets instead.
const MIN_CONFIDENCE: f32 = 0.12;
const FREE_STRENGTH: f32 = 0.6;
const FREE_GAP: f32 = 0.5;

struct BeatTracker {
    hist: VecDeque<f32>,
    /// Hops seen.
    n: usize,
    mean: f32,
    dev: f32,
    prev: [f32; 2],
    /// Beat period in hops, 0 while unknown.
    period: f32,
    confidence: f32,
    acf: Vec<f32>,
    /// A different tempo that has scored better on the last `rival_count` estimates.
    rival: f32,
    rival_count: u8,
    next_beat: f32,
    last_beat: f32,
    /// Strongest onset since the last beat that fell near it, and its strength.
    near: f32,
    /// How well onsets have been landing on the predicted beats, 0–1.
    support: f32,
    level: f32,
    silent_hops: usize,
}

impl BeatTracker {
    fn new() -> Self {
        Self {
            hist: VecDeque::with_capacity(HISTORY),
            n: 0,
            mean: 0.0,
            dev: 0.0,
            prev: [0.0; 2],
            period: 0.0,
            confidence: 0.0,
            acf: vec![0.0; MAX_LAG * 2 + 3],
            rival: 0.0,
            rival_count: 0,
            next_beat: 0.0,
            last_beat: -100.0,
            near: 0.0,
            support: 0.0,
            level: 0.5,
            silent_hops: 0,
        }
    }

    fn bpm(&self) -> f32 {
        if self.locked() { 60.0 / (self.period * HOP_SECS) } else { 0.0 }
    }

    fn locked(&self) -> bool {
        self.period > 0.0 && self.confidence >= MIN_CONFIDENCE && self.support >= 0.3
    }

    /// Feeds a frame's onset values; returns the strength of a beat that
    /// landed among them, if any.
    fn feed(&mut self, onsets: &[f32], gate_open: bool) -> Option<f32> {
        let mut beat = None;
        for &o in onsets {
            if let Some(s) = self.hop(o, gate_open) {
                beat = Some(beat.map_or(s, |b: f32| b.max(s)));
            }
        }
        beat
    }

    fn hop(&mut self, o: f32, gate_open: bool) -> Option<f32> {
        self.n += 1;
        let t = self.n as f32;
        if !gate_open {
            self.silent_hops += 1;
            if self.silent_hops > 200 {
                self.support = 0.0;
            }
            return None;
        }
        self.silent_hops = 0;

        let a = HOP_SECS / 2.0;
        self.mean += a * (o - self.mean);
        self.dev += a * ((o - self.mean).abs() - self.dev);
        let above = (o - self.mean).max(0.0);
        if self.hist.len() == HISTORY {
            self.hist.pop_front();
        }
        self.hist.push_back(above);

        // An onset peaked one hop ago.
        let peak = self.prev[1];
        let onset = (peak > self.prev[0] && peak >= o && peak > self.mean + PEAK_K * self.dev.max(1e-4))
            .then(|| ((peak - self.mean) / (5.0 * self.dev.max(1e-4))).clamp(0.0, 1.0));
        self.prev = [self.prev[1], o];

        if self.n % TEMPO_EVERY == 0 && self.hist.len() >= HISTORY / 2 {
            self.estimate_tempo();
        }

        if self.period <= 0.0 || self.confidence < MIN_CONFIDENCE {
            // No tempo yet: strong onsets are beats, spaced out.
            let s = onset?;
            if s < FREE_STRENGTH || (t - self.last_beat) * HOP_SECS < FREE_GAP {
                return None;
            }
            self.last_beat = t;
            self.next_beat = t + self.period.max(MIN_LAG as f32);
            return Some(s);
        }

        if let Some(s) = onset {
            // Steer the beat grid towards onsets that fall near it.
            let at = t - 1.0;
            let ahead = self.next_beat - at;
            let behind = at - self.last_beat;
            let win = MATCH_WINDOW * self.period;
            if ahead.abs() <= win {
                self.next_beat -= PHASE_GAIN * ahead;
                self.near = self.near.max(s);
            } else if behind.abs() <= win {
                self.next_beat += PHASE_GAIN * behind;
                self.near = self.near.max(s);
            } else if self.support < 0.3 && s >= FREE_STRENGTH {
                // Lost: restart the grid on this strong onset.
                self.next_beat = at + self.period;
                self.last_beat = at;
                self.near = s;
                self.support = 0.3;
                return Some(s);
            }
        }

        if t < self.next_beat {
            return None;
        }
        // A predicted beat: as strong as the onset that met it, or a bit
        // weaker than usual if none did.
        let matched = self.near > 0.0;
        self.support = (self.support + if matched { 0.2 } else { -0.15 }).clamp(0.0, 1.0);
        let strength = if matched { self.near } else { 0.6 * self.level };
        self.level += 0.3 * (strength - self.level);
        self.near = 0.0;
        self.last_beat = self.next_beat;
        self.next_beat += self.period;
        if self.next_beat <= t {
            self.next_beat = t + self.period;
        }
        (self.support >= 0.3).then_some(strength.max(0.15))
    }

    fn estimate_tempo(&mut self) {
        let x: Vec<f32> = self.hist.iter().copied().collect();
        let mean = x.iter().sum::<f32>() / x.len() as f32;
        let x: Vec<f32> = x.iter().map(|v| v - mean).collect();
        let r0: f32 = x.iter().map(|v| v * v).sum();
        if r0 <= 1e-9 {
            return;
        }
        for (lag, avg) in self.acf.iter_mut().enumerate() {
            let r = if lag < x.len() { x[lag..].iter().zip(&x).map(|(a, b)| a * b).sum::<f32>() / r0 } else { 0.0 };
            *avg += ACF_BLEND * (r - *avg);
        }
        let r = &self.acf;
        let score = |lag: usize| -> f32 {
            let prior = (-0.5 * ((lag as f32 / PRIOR_LAG).log2() / PRIOR_WIDTH).powi(2)).exp();
            // A beat's double period echoes it; half-beats don't.
            (r[lag] + 0.5 * r[lag * 2]).max(0.0) * prior
        };
        let (best, best_score) = (MIN_LAG..=MAX_LAG)
            .map(|l| (l, score(l)))
            .fold((0, f32::MIN), |acc, c| if c.1 > acc.1 { c } else { acc });
        if best == 0 || best_score <= 0.0 {
            return;
        }
        // Parabolic refinement between neighbouring lags.
        let (y0, y1, y2) = (r[best - 1], r[best], r[best + 1]);
        let denom = y0 - 2.0 * y1 + y2;
        let shift = if denom.abs() > 1e-9 { (0.5 * (y0 - y2) / denom).clamp(-0.5, 0.5) } else { 0.0 };
        let lag = best as f32 + shift;

        if self.period <= 0.0 {
            self.period = lag;
        } else if (lag - self.period).abs() / self.period < 0.06 {
            self.period += 0.2 * (lag - self.period);
            self.rival_count = 0;
        } else {
            let current = score((self.period.round() as usize).clamp(MIN_LAG, MAX_LAG));
            let same_rival = (lag - self.rival).abs() / self.rival.max(1.0) < 0.06;
            self.rival_count = if best_score > RIVAL_MARGIN * current {
                if same_rival { self.rival_count + 1 } else { 1 }
            } else {
                0
            };
            self.rival = lag;
            if self.rival_count >= RIVAL_COUNT {
                self.period = lag;
                self.rival_count = 0;
            }
        }
        self.confidence = r[(self.period.round() as usize).clamp(MIN_LAG, MAX_LAG)].max(0.0);
    }
}
