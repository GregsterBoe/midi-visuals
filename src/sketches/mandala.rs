use crate::midi::{MidiState, NoteState};
use crate::sketches::{flag_suffix, smoothstep, Envelope, Param, Sketch, Smoothed, Stream, StreamParams, LAYER_HUE_SHIFT};
use nannou::prelude::*;
use std::cell::Cell;
use std::collections::HashSet;

// All four are kept per stream and follow the tune target.
const PARAMS: &[Param] = &[
    Param::new(1,  "hue",   0.0, 1.0),
    Param::new(24, "speed", 0.1, 3.0),
    Param::new(25, "size",  0.3, 0.95),
    Param::new(26, "sat",   0.2, 1.0),
];
const P_HUE: usize = 0;
const P_SPEED: usize = 1;
const P_SIZE: usize = 2;
const P_SAT: usize = 3;

const BEZ_SEGS:      usize = 18;
const MORPH_SECS:    f32 = 0.9;  // a chord's form cross-fades in over this long
const LIS_SECS:      f32 = 0.7;  // the core figure cross-fades over this long
const BEAT_TURN:     f32 = 0.12; // a full beat turns the rings this far (radians × spin)
const LAYER_ROT:     usize = 7;  // ring_rot slot of the layer's halo
const SURGE_SECS:    f32 = 1.2;  // how long a surge stretches the halo

// ── Petal forms ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
enum PetalForm {
    Ellipse,
    Lotus,
    Flame,
    Spike,
    Rose { k: f32 },
    Hypotrochoid { ratio: f32, pen: f32, loops: u32 },
}

// ── Ring specification ────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct RingSpec {
    r_frac:   f32,
    n_mult:   f32,
    form:     PetalForm,
    spin:     f32,
    len_mult: f32,
    wid_mult: f32,
    hue_off:  f32,
    lum:      f32,
    alpha:    f32,
    dot:      bool,
}

fn rs(r: f32, nm: f32, form: PetalForm, spin: f32,
      lm: f32, wm: f32, ho: f32, lu: f32, al: f32, dot: bool) -> RingSpec {
    RingSpec { r_frac: r, n_mult: nm, form, spin,
               len_mult: lm, wid_mult: wm, hue_off: ho, lum: lu, alpha: al, dot }
}

// ── Symmetry morph ────────────────────────────────────────────────────────────

/// A chord's form fading out after the next one took over.
/// It keeps turning its own way while it fades.
struct Ghost {
    specs: Vec<RingSpec>,
    rot:   Vec<f32>,
    n:     usize,
    alpha: f32,  // falls to 0
}

// ── Music theory ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
enum ChordKind { Major, Minor, Diminished, Augmented, Dom7th, Other }

impl ChordKind {
    fn symmetry(self) -> usize {
        match self {
            Self::Major      =>  8,
            Self::Minor      =>  6,
            Self::Diminished => 12,
            Self::Augmented  =>  4,
            Self::Dom7th     => 16,
            Self::Other      =>  5,
        }
    }

    fn hue(self, base: f32) -> f32 {
        match self {
            Self::Major      => 0.10,
            Self::Minor      => 0.65,
            Self::Diminished => 0.80,
            Self::Augmented  => 0.48,
            Self::Dom7th     => 0.98,
            Self::Other      => base,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Major      => "Maj  8×",
            Self::Minor      => "Min  6×",
            Self::Diminished => "Dim 12×",
            Self::Augmented  => "Aug  4×",
            Self::Dom7th     => "Dom7 16×",
            Self::Other      => "—",
        }
    }

    fn ring_specs(self) -> Vec<RingSpec> {
        let hy = |ratio: f32, pen: f32, loops: u32| PetalForm::Hypotrochoid { ratio, pen, loops };
        let ro = |k: f32| PetalForm::Rose { k };
        match self {
            Self::Major => vec![
                rs(0.13, 1.0, PetalForm::Spike,  -1.0, 1.8, 0.45, 0.00, 0.60, 0.80, false),
                rs(0.27, 1.0, PetalForm::Lotus,   1.0, 2.0, 0.65, 0.03, 0.52, 0.82, true ),
                rs(0.39, 2.0, PetalForm::Ellipse,-1.0, 0.75,0.35, 0.06, 0.62, 0.60, false),
                rs(0.49, 1.0, PetalForm::Lotus,   1.0, 1.4, 0.45, 0.09, 0.50, 0.72, false),
                rs(0.57, 1.0, ro(4.0),           -0.5, 1.0, 1.0,  0.14, 0.68, 0.55, false),
            ],
            Self::Minor => vec![
                rs(0.12, 1.0, PetalForm::Lotus,  -1.0, 1.6, 0.60, 0.00, 0.48, 0.80, true ),
                rs(0.25, 2.0, PetalForm::Ellipse, 1.0, 0.8, 0.40, 0.04, 0.55, 0.65, false),
                rs(0.37, 1.0, PetalForm::Flame,  -1.0, 1.5, 0.50, 0.07, 0.45, 0.75, false),
                rs(0.48, 1.0, PetalForm::Ellipse, 1.0, 1.2, 0.38, 0.10, 0.52, 0.68, false),
                rs(0.57, 3.0, PetalForm::Spike,  -0.6, 0.5, 0.20, 0.15, 0.65, 0.45, false),
            ],
            Self::Diminished => vec![
                rs(0.11, 1.0, PetalForm::Spike,   1.0, 2.0, 0.35, 0.00, 0.45, 0.85, false),
                rs(0.22, 1.0, PetalForm::Flame,  -1.0, 1.3, 0.40, 0.05, 0.42, 0.78, false),
                rs(0.34, 1.0, hy(5.0/12.0, 0.70, 12), 1.0, 1.0, 1.0, 0.08, 0.55, 0.65, false),
                rs(0.45, 1.0, PetalForm::Spike,  -1.0, 1.5, 0.30, 0.12, 0.40, 0.70, false),
                rs(0.56, 1.0, ro(6.0),            0.7, 1.0, 1.0,  0.18, 0.58, 0.60, false),
            ],
            Self::Augmented => vec![
                rs(0.14, 1.0, PetalForm::Spike,   1.0, 2.5, 0.40, 0.00, 0.55, 0.85, false),
                rs(0.32, 1.0, hy(1.0/4.0,  0.80,  4),-0.6, 1.0, 1.0, 0.06, 0.55, 0.70, false),
                rs(0.46, 2.0, PetalForm::Spike,   1.0, 1.2, 0.28, 0.10, 0.62, 0.60, false),
                rs(0.57, 1.0, ro(2.0),           -0.4, 1.0, 1.0,  0.15, 0.62, 0.55, false),
            ],
            Self::Dom7th => vec![
                rs(0.10, 1.0, PetalForm::Spike,  -1.0, 1.5, 0.30, 0.00, 0.55, 0.82, false),
                rs(0.20, 1.0, PetalForm::Lotus,   1.0, 1.6, 0.55, 0.03, 0.50, 0.78, true ),
                rs(0.30, 2.0, PetalForm::Ellipse,-1.0, 0.7, 0.30, 0.06, 0.60, 0.60, false),
                rs(0.40, 1.0, PetalForm::Flame,   1.0, 1.4, 0.45, 0.09, 0.48, 0.72, false),
                rs(0.50, 1.0, hy(7.0/16.0, 0.65, 16),-0.8, 1.0, 1.0, 0.12, 0.55, 0.65, false),
                rs(0.58, 1.0, ro(8.0),            0.5, 1.0, 1.0,  0.16, 0.62, 0.55, false),
            ],
            Self::Other => vec![
                rs(0.14, 1.0, PetalForm::Lotus,  -1.0, 1.8, 0.60, 0.00, 0.55, 0.78, false),
                rs(0.30, 1.0, PetalForm::Ellipse, 1.0, 1.3, 0.45, 0.05, 0.58, 0.68, true ),
                rs(0.44, 1.0, ro(2.5),           -0.7, 1.0, 1.0,  0.10, 0.62, 0.58, false),
                rs(0.56, 2.0, PetalForm::Spike,   0.4, 0.6, 0.22, 0.15, 0.68, 0.45, false),
            ],
        }
    }
}

fn detect_chord(notes: &[NoteState; 128]) -> ChordKind {
    let mut classes: Vec<u8> = (0u8..128)
        .filter(|&n| notes[n as usize].on)
        .map(|n| n % 12)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    classes.sort_unstable();
    if classes.len() < 3 { return ChordKind::Other; }
    let root = classes[0];
    let ivs: Vec<u8> = classes.iter().map(|&c| (c + 12 - root) % 12).collect();
    let has = |n: u8| ivs.contains(&n);
    if has(4) && has(7) && has(10) { return ChordKind::Dom7th; }
    if has(3) && has(6)            { return ChordKind::Diminished; }
    if has(4) && has(8)            { return ChordKind::Augmented; }
    if has(4) && has(7)            { return ChordKind::Major; }
    if has(3) && has(7)            { return ChordKind::Minor; }
    ChordKind::Other
}

fn tension(notes: &[NoteState; 128]) -> f32 {
    let held: Vec<u8> = (0..128u8).filter(|&n| notes[n as usize].on).collect();
    if held.len() < 2 { return 0.0; }
    let min_c = held
        .iter()
        .enumerate()
        .flat_map(|(i, &a)| held[i + 1..].iter().map(move |&b| consonance(b.wrapping_sub(a))))
        .fold(1.0f32, f32::min);
    1.0 - min_c
}

fn consonance(s: u8) -> f32 {
    match s % 12 {
        0  => 1.00, 7  => 0.85, 5  => 0.80,
        4  => 0.70, 3  => 0.65, 9  => 0.60,
        8  => 0.55, 2  => 0.35, 10 => 0.30,
        11 => 0.15, 1  => 0.10, 6  => 0.00,
        _  => 0.50,
    }
}

// ── Lissajous ratio ───────────────────────────────────────────────────────────

// Maps the dominant held interval to a Lissajous a:b ratio.
// Consonant intervals → simple ratios → closed stable figures.
// Dissonant intervals → complex ratios → drifting knot-like figures.
fn lissajous_ratio(notes: &[NoteState; 128]) -> (u32, u32) {
    let held: Vec<u8> = (0..128u8).filter(|&n| notes[n as usize].on).collect();
    if held.len() < 2 { return (1, 1); }
    let root = held[0];
    let ivs: Vec<u8> = held.iter().map(|&n| (n + 12 - root) % 12).collect();
    let has = |s: u8| ivs.iter().any(|&i| i == s);
    // Priority: most consonant → simplest ratio → most beautiful closed figure
    if has(7)  { return (3, 2); }   // P5  → trefoil
    if has(4)  { return (5, 4); }   // M3  → bow-tie variant
    if has(3)  { return (6, 5); }   // m3  → 6:5 knot
    if has(5)  { return (4, 3); }   // P4  → 4:3 figure
    if has(9)  { return (5, 3); }   // M6  → 5:3 figure
    if has(8)  { return (8, 5); }   // m6  → complex stable
    if has(10) { return (7, 4); }   // m7  → 7:4 knot
    if has(2)  { return (9, 8); }   // M2  → near-unison drift
    if has(11) { return (15, 8); }  // M7  → very tight knot
    if has(6)  { return (7, 5); }   // TT  → irrational feel, 24 crossings
    if has(1)  { return (16, 15); } // m2  → maximum complexity
    (1, 1)
}


// ── Bezier helpers ────────────────────────────────────────────────────────────

fn cbez(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f32) -> Vec2 {
    let mt = 1.0 - t;
    p0 * (mt * mt * mt) + p1 * (3.0 * mt * mt * t)
        + p2 * (3.0 * mt * t * t) + p3 * (t * t * t)
}

fn ring_transform(local: Vec2, rx: f32, ry: f32, cos_a: f32, sin_a: f32) -> Vec2 {
    vec2(rx + local.x * cos_a - local.y * sin_a,
         ry + local.x * sin_a + local.y * cos_a)
}

// Lotus petal: concave sides (wide "shoulders" near center, pinching to both tips).
// Local space: inner tip at -half_len, outer tip at +half_len, along x-axis.
fn lotus_pts(half_len: f32, half_w: f32) -> Vec<Vec2> {
    let p0 = vec2(-half_len, 0.0);
    let p3 = vec2( half_len, 0.0);
    let p1 = vec2(-half_len * 0.10, half_w * 1.40);
    let p2 = vec2( half_len * 0.10, half_w * 1.40);
    let mut pts = Vec::with_capacity(BEZ_SEGS * 2 + 2);
    for i in 0..=BEZ_SEGS {
        pts.push(cbez(p0, p1, p2, p3, i as f32 / BEZ_SEGS as f32));
    }
    for i in 1..=BEZ_SEGS {
        pts.push(cbez(p3, vec2(p2.x, -p2.y), vec2(p1.x, -p1.y), p0,
                      i as f32 / BEZ_SEGS as f32));
    }
    pts
}

// Flame petal: asymmetric, tip swept in the `sweep` direction (+y = CCW tangent).
fn flame_pts(half_len: f32, half_w: f32, sweep: f32) -> Vec<Vec2> {
    let p0  = vec2(-half_len, 0.0);
    let tip = vec2( half_len, sweep * half_w * 0.42);
    let c1  = vec2(-half_len * 0.08, half_w * 1.15 + sweep * half_w * 0.55);
    let c2  = vec2( half_len * 0.28, half_w * 0.72 + sweep * half_w * 0.82);
    let c3  = vec2( half_len * 0.22, -half_w * 0.62 + sweep * half_w * 0.18);
    let c4  = vec2(-half_len * 0.12, -half_w * 0.88 + sweep * half_w * 0.08);
    let mut pts = Vec::with_capacity(BEZ_SEGS * 2 + 2);
    for i in 0..=BEZ_SEGS {
        pts.push(cbez(p0, c1, c2, tip, i as f32 / BEZ_SEGS as f32));
    }
    for i in 1..=BEZ_SEGS {
        pts.push(cbez(tip, c3, c4, p0, i as f32 / BEZ_SEGS as f32));
    }
    pts
}

// ── Visual elements ───────────────────────────────────────────────────────────

struct Pulse { radius: f32, max_r: f32, alpha: f32, hue: f32, age: f32 }
struct Blip  { x: f32, y: f32, life: f32, size: f32, hue: f32 }

/// What a ring is drawn with, beyond its spec: the stream's hue and
/// saturation, how far its petals stretch, and an alpha multiplier.
#[derive(Clone, Copy)]
struct RingStyle { hue: f32, sat: f32, stretch: f32, alpha: f32 }

// ── Sketch ────────────────────────────────────────────────────────────────────

pub struct Mandala {
    chord:           ChordKind,
    symmetry:        usize,
    current_specs:   Vec<RingSpec>,
    /// The current form fades in from 0 after a chord change.
    form_alpha:      f32,
    ghosts:          Vec<Ghost>,
    // ring_rot[0..5] = ring slots, [6] = border dots, [7] = layer halo
    ring_rot:        [f32; 8],
    flash:           Envelope,
    flash_hue:       f32,
    s_hue:           Smoothed,
    s_tension:       Smoothed,
    energy:          f32,
    pulse:           f32,
    /// Beats turn the rings: the target steps, the turn chases it.
    kick_target:     f32,
    s_kick:          Smoothed,
    lis_ratio:       (u32, u32),
    lis_prev:        (u32, u32),
    lis_mix:         f32,  // 0 → 1 while fading from lis_prev
    time:            f32,
    pulses:          Vec<Pulse>,
    blips:           Vec<Blip>,
    // The auto stream's knob values
    p_hue: f32, p_speed: f32, p_size: f32, p_sat: f32,
    tunes:           StreamParams,
    // Layer: a halo of petals round the mandala, played by hand
    layer_on:        bool,
    layer_spec:      RingSpec,
    s_layer_alpha:   Smoothed,  // fades the halo in and out
    surge:           Envelope,
    win:             Cell<Rect>,
}

impl Mandala {
    pub fn new() -> Self {
        let chord = ChordKind::Other;
        Self {
            chord,
            symmetry:        chord.symmetry(),
            current_specs:   chord.ring_specs(),
            form_alpha:      1.0,
            ghosts:          Vec::new(),
            ring_rot:        [0.0; 8],
            flash:           Envelope::new(10.0, 1.6),
            flash_hue:       0.5,
            s_hue:           Smoothed::new(0.5, 2.0),
            s_tension:       Smoothed::new(0.0, 2.0),
            energy:          0.0,
            pulse:           0.0,
            kick_target:     0.0,
            s_kick:          Smoothed::new(0.0, 6.0),
            lis_ratio:       (1, 1),
            lis_prev:        (1, 1),
            lis_mix:         1.0,
            time:            0.0,
            pulses:          Vec::new(),
            blips:           Vec::new(),
            p_hue: 0.5, p_speed: 1.0, p_size: 0.7, p_sat: 0.8,
            tunes:           StreamParams::new(PARAMS.len()),
            layer_on:        false,
            layer_spec:      rs(0.78, 2.0, PetalForm::Lotus, -1.0, 1.3, 0.50, 0.0, 0.58, 0.85, true),
            s_layer_alpha:   Smoothed::new(0.0, 3.0),
            surge:           Envelope::new(12.0, 1.0 / SURGE_SECS),
            win: Cell::new(Rect::from_w_h(800.0, 600.0)),
        }
    }

    /// The auto stream's hue: the chord's, gliding when the chord changes.
    fn hue(&self) -> f32 { self.s_hue.value.rem_euclid(1.0) }

    /// Takes on `chord`'s form: the current one fades out as a ghost.
    fn set_chord(&mut self, chord: ChordKind) {
        if self.form_alpha > 0.01 {
            let specs = std::mem::take(&mut self.current_specs);
            self.ghosts.push(Ghost {
                rot:   self.ring_rot[..specs.len()].to_vec(),
                specs,
                n:     self.symmetry,
                alpha: self.form_alpha,
            });
        }
        self.chord = chord;
        self.symmetry = chord.symmetry();
        self.current_specs = chord.ring_specs();
        self.form_alpha = 0.0;
    }

    fn set_lissajous(&mut self, ratio: (u32, u32)) {
        if ratio == self.lis_ratio { return; }
        self.lis_prev = if self.lis_mix < 0.5 { self.lis_prev } else { self.lis_ratio };
        self.lis_ratio = ratio;
        self.lis_mix = 0.0;
    }

    fn layer_hue(&self) -> f32 {
        (self.tunes.get(Stream::Layer, P_HUE) + LAYER_HUE_SHIFT).rem_euclid(1.0)
    }

    /// Radius of the layer's halo.
    fn layer_r(&self, win: Rect) -> f32 {
        self.layer_spec.r_frac * Self::max_r(win, self.tunes.get(Stream::Layer, P_SIZE))
    }

    fn max_r(win: Rect, size: f32) -> f32 {
        win.w().min(win.h()) * 0.5 * size
    }

    fn draw_ring(&self, draw: &Draw, cx: f32, cy: f32, max_r: f32,
                 spec: &RingSpec, rot: f32, n_base: usize, style: RingStyle) {
        let n        = ((n_base as f32 * spec.n_mult).round() as usize).max(1);
        let ring_r   = spec.r_frac * max_r;
        let spacing  = max_r * 0.10;
        let p_len    = spec.len_mult * spacing * style.stretch;
        let p_wid    = spec.wid_mult * spacing;
        let base_hue = (style.hue + spec.hue_off).rem_euclid(1.0);
        let sat      = style.sat;
        let ea       = spec.alpha * style.alpha;  // effective alpha
        let stroke_c = hsla(base_hue, sat, spec.lum, ea);

        match &spec.form {
            PetalForm::Ellipse => {
                for i in 0..n {
                    let angle = rot + i as f32 * TAU / n as f32;
                    let ph = (base_hue + i as f32 / n as f32 * 0.04
                              + self.time * 0.004).rem_euclid(1.0);
                    let px = cx + ring_r * angle.cos();
                    let py = cy + ring_r * angle.sin();
                    draw.ellipse().x_y(px, py).w_h(p_len, p_wid).rotate(angle)
                        .color(hsla(ph, sat, spec.lum, ea));
                    if spec.dot {
                        draw.ellipse().x_y(px, py).radius(p_wid * 0.38)
                            .color(hsla(ph, sat, 0.85, ea * 0.65));
                    }
                }
            }
            PetalForm::Lotus => {
                let local = lotus_pts(p_len * 0.5, p_wid * 0.5);
                for i in 0..n {
                    let angle = rot + i as f32 * TAU / n as f32;
                    let (ca, sa) = (angle.cos(), angle.sin());
                    let px = cx + ring_r * ca;
                    let py = cy + ring_r * sa;
                    let ph = (base_hue + i as f32 / n as f32 * 0.03
                              + self.time * 0.004).rem_euclid(1.0);
                    let world: Vec<Vec2> = local.iter()
                        .map(|&p| ring_transform(p, px, py, ca, sa)).collect();
                    draw.polygon().points(world).color(hsla(ph, sat, spec.lum, ea));
                    if spec.dot {
                        draw.ellipse().x_y(px, py).radius(p_wid * 0.35)
                            .color(hsla(ph, sat, 0.88, ea * 0.60));
                    }
                }
            }
            PetalForm::Flame => {
                // Sweep direction follows ring spin so flames lean into motion
                let local = flame_pts(p_len * 0.5, p_wid * 0.5, -spec.spin * 0.68);
                for i in 0..n {
                    let angle = rot + i as f32 * TAU / n as f32;
                    let (ca, sa) = (angle.cos(), angle.sin());
                    let px = cx + ring_r * ca;
                    let py = cy + ring_r * sa;
                    let ph = (base_hue + i as f32 / n as f32 * 0.03
                              + self.time * 0.005).rem_euclid(1.0);
                    let world: Vec<Vec2> = local.iter()
                        .map(|&p| ring_transform(p, px, py, ca, sa)).collect();
                    draw.polygon().points(world).color(hsla(ph, sat, spec.lum, ea));
                }
            }
            PetalForm::Spike => {
                for i in 0..n {
                    let angle = rot + i as f32 * TAU / n as f32;
                    let (ca, sa) = (angle.cos(), angle.sin());
                    let (px, py) = (-sa, ca); // perpendicular unit vector
                    let hw     = p_wid * 0.5;
                    let base_r = ring_r - p_len * 0.28;
                    let tip_r  = ring_r + p_len * 0.72;
                    let tip = vec2(cx + tip_r  * ca,           cy + tip_r  * sa);
                    let bl  = vec2(cx + base_r * ca + px * hw, cy + base_r * sa + py * hw);
                    let br  = vec2(cx + base_r * ca - px * hw, cy + base_r * sa - py * hw);
                    let ph  = (base_hue + i as f32 / n as f32 * 0.03
                               + self.time * 0.003).rem_euclid(1.0);
                    draw.polygon().points([tip, bl, br])
                        .color(hsla(ph, sat, spec.lum, ea));
                }
            }
            PetalForm::Rose { k } => {
                let k = *k;
                let (cr, sr) = (rot.cos(), rot.sin());
                let pts: Vec<Vec2> = (0..=512).map(|i| {
                    let t  = i as f32 / 512.0 * TAU * 2.0;
                    let r  = ring_r * (k * t).cos();
                    let lx = r * t.cos();
                    let ly = r * t.sin();
                    vec2(cx + lx * cr - ly * sr, cy + lx * sr + ly * cr)
                }).collect();
                draw.polyline().stroke_weight(1.4).points(pts).color(stroke_c);
            }
            PetalForm::Hypotrochoid { ratio, pen, loops } => {
                let small_r     = ring_r * ratio;
                let d           = small_r * pen;
                let diff        = ring_r - small_r;
                let inner_ratio = diff / small_r;
                let steps       = (*loops as usize * 256).min(4096);
                let (cr, sr)    = (rot.cos(), rot.sin());
                let pts: Vec<Vec2> = (0..=steps).map(|i| {
                    let t  = i as f32 / steps as f32 * TAU * *loops as f32;
                    let lx = diff * t.cos() + d * (inner_ratio * t).cos();
                    let ly = diff * t.sin() - d * (inner_ratio * t).sin();
                    vec2(cx + lx * cr - ly * sr, cy + lx * sr + ly * cr)
                }).collect();
                draw.polyline().stroke_weight(1.4).points(pts).color(stroke_c);
            }
        }
    }

    fn draw_border_dots(&self, draw: &Draw, cx: f32, cy: f32, max_r: f32, symmetry: usize, alpha: f32) {
        let border_r = max_r * 0.64;
        let n   = symmetry * 3;
        let rot = self.ring_rot[6];
        let hue = (self.hue() + 0.15).rem_euclid(1.0);
        for i in 0..n {
            let angle = rot + i as f32 * TAU / n as f32;
            draw.ellipse()
                .x_y(cx + border_r * angle.cos(), cy + border_r * angle.sin())
                .radius(max_r * 0.008)
                .color(hsla(hue, self.p_sat, 0.72, 0.45 * alpha));
        }
    }

    fn draw_core(&self, draw: &Draw, cx: f32, cy: f32, max_r: f32) {
        let hue = self.hue();
        let sat = self.p_sat;

        // Lissajous figure — shape determined by dominant held interval,
        // cross-fading from the last one when it changes
        let mix = smoothstep(self.lis_mix);
        let figures = [(self.lis_prev, 1.0 - mix), (self.lis_ratio, mix)];
        for ((a, b), alpha) in figures {
            if alpha < 0.01 { continue; }
            let scale    = max_r * 0.095;
            let delta    = self.time * 0.09; // slow phase drift keeps figure alive
            let n_steps  = (a.max(b) as usize * 64).min(1024);
            let lis_hue  = (hue + 0.06).rem_euclid(1.0);
            let pts: Vec<Vec2> = (0..=n_steps).map(|i| {
                let t = i as f32 / n_steps as f32 * TAU;
                vec2(cx + scale * (a as f32 * t + delta).sin(),
                     cy + scale * (b as f32 * t).sin())
            }).collect();
            draw.polyline().stroke_weight(2.0).points(pts)
                .color(hsla(lis_hue, sat, 0.84, 0.90 * alpha));
        }

        // Pulsing center dot on top
        let pulse  = (self.time * 2.5).sin() * 0.5 + 0.5;
        let core_r = max_r * (0.026 + pulse * 0.008);
        draw.ellipse().x_y(cx, cy).radius(core_r)
            .color(hsla(hue, sat, 0.95, 1.0));
    }

    fn draw_flash(&self, draw: &Draw, win: Rect) {
        let f = self.flash.value;
        if f < 0.01 { return; }
        draw.rect().wh(win.wh())
            .color(hsla(self.flash_hue, 0.95, 0.55, f * f * 0.30));
    }

    fn draw_pulses(&self, draw: &Draw, cx: f32, cy: f32) {
        for p in &self.pulses {
            // Fades in as it starts and out as it nears its last radius.
            let alpha = p.alpha * smoothstep(p.age / 0.12)
                * smoothstep((p.max_r + 20.0 - p.radius) / 40.0);
            if alpha < 0.01 { continue; }
            let n_dots = 48usize;
            for i in 0..n_dots {
                let angle = i as f32 * TAU / n_dots as f32;
                draw.ellipse()
                    .x_y(cx + p.radius * angle.cos(), cy + p.radius * angle.sin())
                    .radius(3.5)
                    .color(hsla(p.hue, 0.95, 0.70, alpha));
            }
        }
    }

    fn draw_blips(&self, draw: &Draw) {
        for b in &self.blips {
            // Grows in over 0.15 s, then shrinks and fades out.
            let t = smoothstep(b.life / 1.5) * smoothstep((1.5 - b.life) / 0.15);
            draw.ellipse().x_y(b.x, b.y).radius(b.size * t)
                .color(hsla(b.hue, 0.92, 0.75, t * 0.88));
        }
    }
}

impl Sketch for Mandala {
    fn name(&self) -> &'static str { "mandala" }
    fn params(&self) -> &[Param] { PARAMS }

    fn update(&mut self, midi: &MidiState, dt: f32) {
        let music = &midi.music;
        self.tunes.update(PARAMS, midi);
        self.p_hue   = self.tunes.get(Stream::Auto, P_HUE);
        self.p_speed = self.tunes.get(Stream::Auto, P_SPEED);
        self.p_size  = self.tunes.get(Stream::Auto, P_SIZE);
        self.p_sat   = self.tunes.get(Stream::Auto, P_SAT);
        self.time   += dt;

        // Layer: the halo turns against the mandala at a steady pace (it
        // doesn't speed up with the notes) and stretches after a surge.
        let layer_speed = self.tunes.get(Stream::Layer, P_SPEED);
        self.ring_rot[LAYER_ROT] += self.layer_spec.spin * 0.25 * layer_speed * dt;
        self.s_layer_alpha.drive(if self.layer_on { 1.0 } else { 0.0 }, dt);
        self.surge.step(dt);
        self.flash.step(dt);

        self.energy = music.energy;
        self.pulse  = music.pulse;
        self.s_tension.drive(tension(&music.held), dt);

        // Fade the current form in and old ones out.
        self.form_alpha = (self.form_alpha + dt / MORPH_SECS).min(1.0);
        self.ghosts.retain_mut(|g| { g.alpha -= dt / MORPH_SECS; g.alpha > 0.0 });
        self.lis_mix = (self.lis_mix + dt / LIS_SECS).min(1.0);

        // Rings turn with the energy, and each beat turns them a step more.
        self.kick_target += music.beat * BEAT_TURN;
        let kick_before = self.s_kick.value;
        let kick = self.s_kick.drive(self.kick_target, dt) - kick_before;
        let base_speed = 0.4 * self.p_speed * (0.20 + self.energy * 0.80);
        for (i, spec) in self.current_specs.iter().enumerate() {
            self.ring_rot[i] += spec.spin * (base_speed * dt + kick);
        }
        for g in &mut self.ghosts {
            for (rot, spec) in g.rot.iter_mut().zip(&g.specs) {
                *rot += spec.spin * (base_speed * dt + kick);
            }
        }
        self.ring_rot[6] += -0.28 * (base_speed * dt + kick); // border dots

        self.pulses.retain_mut(|p| {
            p.age    += dt;
            p.radius += 165.0 * dt;
            p.alpha  -= dt * 1.1;
            p.alpha > 0.01 && p.radius < p.max_r + 20.0
        });
        self.blips.retain_mut(|b| { b.life -= dt; b.life > 0.0 });

        if music.chord_changed {
            self.set_lissajous(lissajous_ratio(&music.held));
            let new_chord = detect_chord(&music.held);
            if new_chord != ChordKind::Other && new_chord != self.chord {
                let old_sym = self.symmetry;
                self.set_chord(new_chord);
                self.flash_hue = new_chord.hue(self.p_hue);
                self.flash.trigger(1.0);
                let win   = self.win.get();
                let max_r = Self::max_r(win, self.p_size);
                self.pulses.push(Pulse {
                    radius: 0.0, max_r: max_r * 0.62,
                    alpha: 1.0,  hue: self.flash_hue, age: 0.0,
                });
                if (self.symmetry as i32 - old_sym as i32).unsigned_abs() >= 4 {
                    self.pulses.push(Pulse {
                        radius: max_r * 0.08, max_r: max_r * 0.72,
                        alpha: 0.65, hue: (self.flash_hue + 0.5).rem_euclid(1.0), age: 0.0,
                    });
                }
            }
        }
        self.s_hue.drive_hue(self.chord.hue(self.p_hue), dt);

        for ev in &music.hits {
            let win   = self.win.get();
            let max_r = Self::max_r(win, self.p_size);
            let cx    = win.x();
            let cy    = win.y();
            let note_hue = (self.flash_hue + ev.note as f32 / 127.0 * 0.18).rem_euclid(1.0);
            self.pulses.push(Pulse {
                radius: 0.0, max_r: max_r * (0.10 + ev.velocity * 0.22),
                alpha: ev.velocity * 0.55, hue: note_hue, age: 0.0,
            });
            let ring_r_frac = self.current_specs.get(1)
                .map_or(0.24, |s| s.r_frac);
            let pitch_angle = (ev.note % 12) as f32 / 12.0 * TAU + self.ring_rot[1];
            let ring_r = ring_r_frac * max_r
                + (ev.note as f32 / 127.0 - 0.5) * 0.12 * max_r;
            self.blips.push(Blip {
                x: cx + ring_r * pitch_angle.cos(),
                y: cy + ring_r * pitch_angle.sin(),
                life: 1.5,
                size: ev.velocity * max_r * 0.042 + max_r * 0.010,
                hue:  note_hue,
            });
        }
    }

    fn view(&self, draw: &Draw, win: Rect) {
        self.win.set(win);
        let max_r = Self::max_r(win, self.p_size);
        let cx = win.x();
        let cy = win.y();

        self.draw_flash(draw, win);

        // Layer halo, outermost so drawn first. A surge shows it for a
        // moment even while it's off.
        let surge = self.surge.value;
        let layer_alpha = self.s_layer_alpha.value.max(surge);
        if layer_alpha > 0.01 {
            let style = RingStyle {
                hue:     self.layer_hue(),
                sat:     self.tunes.get(Stream::Layer, P_SAT),
                stretch: 1.0 + surge * 0.8,
                alpha:   layer_alpha,
            };
            let layer_max_r = Self::max_r(win, self.tunes.get(Stream::Layer, P_SIZE));
            self.draw_ring(draw, cx, cy, layer_max_r, &self.layer_spec,
                           self.ring_rot[LAYER_ROT], self.symmetry, style);
        }

        // Draw rings: outer → inner so inner renders on top. Old forms
        // fade out under the current one as it fades in. A beat stretches
        // the petals a little.
        let style = |alpha| RingStyle {
            hue:     self.hue(),
            sat:     self.p_sat,
            stretch: 1.0 + self.s_tension.value * 0.48 + self.pulse * 0.12,
            alpha,
        };
        for g in &self.ghosts {
            let a = smoothstep(g.alpha);
            for (i, spec) in g.specs.iter().enumerate().rev() {
                self.draw_ring(draw, cx, cy, max_r, spec, g.rot[i], g.n, style(a));
            }
            self.draw_border_dots(draw, cx, cy, max_r, g.n, a);
        }
        let a = smoothstep(self.form_alpha);
        for (i, spec) in self.current_specs.iter().enumerate().rev() {
            self.draw_ring(draw, cx, cy, max_r, spec, self.ring_rot[i], self.symmetry, style(a));
        }
        self.draw_border_dots(draw, cx, cy, max_r, self.symmetry, a);
        self.draw_core(draw, cx, cy, max_r);
        self.draw_pulses(draw, cx, cy);
        self.draw_blips(draw);
    }

    fn hud_info(&self) -> Option<String> {
        let (a, b) = self.lis_ratio;
        let mut flags = Vec::new();
        if self.layer_on { flags.push("layer"); }
        flags.extend(self.tunes.hud_flag());
        Some(format!("{} | e:{:.0}% t:{:.0}% | lis {}:{}{}",
            self.chord.label(), self.energy * 100.0, self.s_tension.value * 100.0, a, b,
            flag_suffix(&flags)))
    }

    fn actions(&self) -> &[(Key, &'static str)] {
        &[(Key::G, "Layer"), (Key::B, "Surge"), (Key::T, "Tune: both / auto / layer"), (Key::R, "Reset")]
    }

    fn key_pressed(&mut self, key: Key) {
        match key {
            Key::G => self.layer_on = !self.layer_on,
            Key::B => {
                // Stretch the halo and send a pulse outward from it.
                self.surge.trigger(1.0);
                let r = self.layer_r(self.win.get());
                self.pulses.push(Pulse {
                    radius: r, max_r: r * 1.6,
                    alpha: 0.9, hue: self.layer_hue(), age: 0.0,
                });
            }
            Key::T => self.tunes.target = self.tunes.target.next(),
            // Back to the plain form, fading rather than cutting.
            Key::R => {
                self.set_chord(ChordKind::Other);
                self.set_lissajous((1, 1));
            }
            _ => {}
        }
    }
}
