use crate::midi::{MidiState, NoteState};
use crate::sketches::{zodiac_points, Param, Sketch};
use nannou::prelude::*;
use nannou::rand::{thread_rng, Rng};
use std::cell::Cell;
use std::collections::HashSet;

const PARAMS: &[Param] = &[
    Param::new(24, "hue",        0.0,   1.0),
    Param::new(25, "shell spd",  150.0, 800.0),
    Param::new(26, "gravity",    50.0,  600.0),
    Param::new(27, "particles",  20.0,  300.0),
    Param::new(28, "lifetime",   0.5,   4.0),
    Param::new(29, "hue spread", 0.0,   0.5),
];

const MAX_SHELLS: usize = 32;
const MAX_PARTICLES: usize = 12_000;
const SHELL_GRAV_SCALE: f32 = 0.35;
const SHELL_R: f32 = 3.0;
const PARTICLE_R: f32 = 2.5;
const GOLDEN_ANGLE: f32 = 2.399_963_2; // 137.5° — sunflower packing

fn hsl_to_lin(h: f32, s: f32, l: f32, a: f32) -> LinSrgba {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h6 = h * 6.0;
    let x = c * (1.0 - (h6.rem_euclid(2.0) - 1.0).abs());
    let m = l - c * 0.5;
    let (r, g, b) = match h6 as u32 {
        0 => (c + m, x + m, m),
        1 => (x + m, c + m, m),
        2 => (m, c + m, x + m),
        3 => (m, x + m, c + m),
        4 => (x + m, m, c + m),
        _ => (c + m, m, x + m),
    };
    LinSrgba::new(r, g, b, a)
}

// ── Music theory ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
enum ChordQuality {
    Major,
    Minor,
    Diminished,
    Augmented,
    Dom7th,
    Other,
}

impl ChordQuality {
    fn name(self) -> &'static str {
        match self {
            Self::Major      => "major",
            Self::Minor      => "minor",
            Self::Diminished => "dim",
            Self::Augmented  => "aug",
            Self::Dom7th     => "dom7",
            Self::Other      => "",
        }
    }
}

fn detect_chord(notes: &[NoteState; 128]) -> ChordQuality {
    let mut classes: Vec<u8> = (0u8..128)
        .filter(|&n| notes[n as usize].on)
        .map(|n| n % 12)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    classes.sort_unstable();
    if classes.len() < 3 {
        return ChordQuality::Other;
    }
    let root = classes[0];
    let intervals: Vec<u8> = classes.iter().map(|&c| (c + 12 - root) % 12).collect();
    let has = |n: u8| intervals.contains(&n);
    // Check most-specific patterns first
    if has(4) && has(7) && has(10) { return ChordQuality::Dom7th; }
    if has(3) && has(6)            { return ChordQuality::Diminished; }
    if has(4) && has(8)            { return ChordQuality::Augmented; }
    if has(4) && has(7)            { return ChordQuality::Major; }
    if has(3) && has(7)            { return ChordQuality::Minor; }
    ChordQuality::Other
}

fn consonance_score(semitones: u8) -> f32 {
    match semitones % 12 {
        0  => 1.00,   // unison / octave
        7  => 0.85,   // perfect 5th
        5  => 0.80,   // perfect 4th
        4  => 0.70,   // major 3rd
        3  => 0.65,   // minor 3rd
        9  => 0.60,   // major 6th
        8  => 0.55,   // minor 6th
        2  => 0.35,   // major 2nd
        10 => 0.30,   // minor 7th
        11 => 0.15,   // major 7th
        1  => 0.10,   // minor 2nd
        6  => 0.00,   // tritone
        _  => 0.50,
    }
}

fn tension_from_notes(notes: &[NoteState; 128]) -> f32 {
    let held: Vec<u8> = (0u8..128).filter(|&n| notes[n as usize].on).collect();
    if held.len() < 2 {
        return 0.0;
    }
    let min_cons = held
        .iter()
        .enumerate()
        .flat_map(|(i, &a)| {
            held[i + 1..].iter().map(move |&b| consonance_score(b.wrapping_sub(a)))
        })
        .fold(1.0f32, f32::min);
    1.0 - min_cons
}

// ── Burst forms ───────────────────────────────────────────────────────────────

/// Particle emission pattern for a shell burst.
/// Add SVG silhouettes to assets/ and re-run scripts/gen_zodiac.py to extend
/// the Animal library without touching this file.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Debug)]
enum BurstForm {
    Scatter,                                    // random radial — reserved
    Ring,                                       // evenly-spaced circle — reserved
    Star,                                       // 5-arm star — reserved
    Spiral,                                     // golden-angle Archimedean — reserved
    Cross,                                      // 4-arm cross — reserved
    Animal { name: &'static str, points: &'static [(f32, f32)] },
}

impl BurstForm {
    /// Cycle through the animal registry.
    fn cycle(index: usize) -> Self {
        let animals = zodiac_points::ANIMALS;
        let (name, points) = animals[index % animals.len()];
        Self::Animal { name, points }
    }

    /// Chord-quality override — currently disabled (animals always used).
    fn from_chord(_q: ChordQuality) -> Option<Self> {
        None
    }

    fn name(self) -> &'static str {
        match self {
            Self::Scatter              => "scatter",
            Self::Ring                 => "ring",
            Self::Star                 => "star",
            Self::Spiral               => "spiral",
            Self::Cross                => "cross",
            Self::Animal { name, .. }  => name,
        }
    }

    fn particle_angle(&self, i: usize, count: usize, rng: &mut impl Rng) -> f32 {
        match self {
            Self::Scatter => rng.gen_range(0.0f32..TAU),
            Self::Ring => {
                let base = i as f32 / count as f32 * TAU;
                base + rng.gen_range(-0.15f32..0.15)
            }
            Self::Star => {
                const ARMS: usize = 5;
                let base = (i % ARMS) as f32 / ARMS as f32 * TAU;
                base + rng.gen_range(-0.3f32..0.3)
            }
            Self::Spiral => (i as f32) * GOLDEN_ANGLE,
            Self::Cross => {
                let base = (i % 4) as f32 / 4.0 * TAU;
                base + rng.gen_range(-0.2f32..0.2)
            }
            Self::Animal { points, .. } => {
                let (px, py) = points[i % points.len()];
                py.atan2(px) + rng.gen_range(-0.12f32..0.12)
            }
        }
    }

    fn speed_scale(&self, i: usize, count: usize, rng: &mut impl Rng) -> f32 {
        let n = count.max(1);
        match self {
            Self::Scatter => rng.gen_range(0.6f32..1.0),
            Self::Ring    => rng.gen_range(0.88f32..1.0),
            Self::Star => {
                let arm_frac = (i as f32 * 5.0 / n as f32).fract();
                let arm_peak = 1.0 - (arm_frac * 2.0 - 1.0).abs();
                (0.6 + arm_peak * 0.4) * rng.gen_range(0.9f32..1.0)
            }
            Self::Spiral => {
                let t = i as f32 / n as f32;
                0.3 + t * 0.7
            }
            Self::Cross => rng.gen_range(0.7f32..1.0),
            Self::Animal { points, .. } => {
                let (px, py) = points[i % points.len()];
                let dist = (px * px + py * py).sqrt().clamp(0.2, 1.0);
                dist * rng.gen_range(0.9f32..1.0)
            }
        }
    }
}

fn chord_hue(quality: ChordQuality, base_hue: f32) -> f32 {
    match quality {
        ChordQuality::Major      => 0.10,  // warm gold
        ChordQuality::Minor      => 0.65,  // blue-violet
        ChordQuality::Diminished => 0.80,  // deep magenta
        ChordQuality::Augmented  => 0.48,  // alien cyan
        ChordQuality::Dom7th     => 0.98,  // blood red — craves resolution
        ChordQuality::Other      => base_hue,
    }
}

// ── Structs ───────────────────────────────────────────────────────────────────

struct Shell {
    pos: Vec2,
    vel: Vec2,
    hue: f32,
    burst_vel: f32,
    burst_count: usize,
    fuse: f32,
    age: f32,
    form: BurstForm,
}

struct Particle {
    pos: Vec2,
    vel: Vec2,
    hue: f32,
    lifetime: f32,
    max_lifetime: f32,
}

pub struct Fireworks {
    shells: Vec<Shell>,
    particles: Vec<Particle>,
    win: Cell<Rect>,
    mesh_verts: Vec<(Vec3, LinSrgba)>,
    mesh_idx: Vec<usize>,
    form_index: usize,           // cycles BurstForm for single-note bursts
    prev_chord: ChordQuality,    // previous frame — used for V7→I detection
    current_chord: ChordQuality, // for HUD
    current_tension: f32,        // for HUD and shell jitter
    last_form: BurstForm,        // for HUD
}

impl Fireworks {
    pub fn new() -> Self {
        Self {
            shells: Vec::new(),
            particles: Vec::new(),
            win: Cell::new(Rect::from_w_h(800.0, 600.0)),
            mesh_verts: Vec::with_capacity((MAX_PARTICLES + MAX_SHELLS) * 4),
            mesh_idx: Vec::with_capacity((MAX_PARTICLES + MAX_SHELLS) * 6),
            form_index: 0,
            prev_chord: ChordQuality::Other,
            current_chord: ChordQuality::Other,
            current_tension: 0.0,
            last_form: BurstForm::cycle(0),
        }
    }

    fn burst(&mut self, shell: &Shell, lifetime: f32, rng: &mut impl Rng) {
        let available = MAX_PARTICLES.saturating_sub(self.particles.len());
        let count = shell.burst_count.min(available);
        for i in 0..count {
            let angle = shell.form.particle_angle(i, count, rng);
            let speed = shell.form.speed_scale(i, count, rng) * shell.burst_vel;
            let hue_offset = rng.gen_range(-0.06f32..0.06);
            self.particles.push(Particle {
                pos: shell.pos,
                vel: vec2(angle.cos() * speed, angle.sin() * speed),
                hue: (shell.hue + hue_offset).rem_euclid(1.0),
                lifetime,
                max_lifetime: lifetime,
            });
        }
    }

    fn build_mesh(&mut self) {
        self.mesh_verts.clear();
        self.mesh_idx.clear();
        for p in &self.particles {
            let t = p.lifetime / p.max_lifetime;
            let color = hsl_to_lin(p.hue, 0.7 + t * 0.25, 0.4 + t * 0.35, t);
            push_quad(&mut self.mesh_verts, &mut self.mesh_idx, p.pos, PARTICLE_R, color);
        }
        for shell in &self.shells {
            let color = hsl_to_lin(shell.hue, 0.4, 0.95, 1.0);
            push_quad(&mut self.mesh_verts, &mut self.mesh_idx, shell.pos, SHELL_R, color);
        }
    }
}

fn push_quad(
    verts: &mut Vec<(Vec3, LinSrgba)>,
    idx: &mut Vec<usize>,
    pos: Vec2,
    r: f32,
    color: LinSrgba,
) {
    let base = verts.len();
    let (x, y) = (pos.x, pos.y);
    verts.extend_from_slice(&[
        (vec3(x - r, y - r, 0.0), color),
        (vec3(x + r, y - r, 0.0), color),
        (vec3(x + r, y + r, 0.0), color),
        (vec3(x - r, y + r, 0.0), color),
    ]);
    idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

impl Sketch for Fireworks {
    fn update(&mut self, midi: &MidiState, dt: f32) {
        let base_hue    = PARAMS[0].read(midi);
        let shell_speed = PARAMS[1].read(midi);
        let gravity     = PARAMS[2].read(midi);
        let burst_count = PARAMS[3].read(midi) as usize;
        let lifetime    = PARAMS[4].read(midi);
        let hue_spread  = PARAMS[5].read(midi);

        let win = self.win.get();
        let height_cap = win.h() * 0.35;
        let mut rng = thread_rng();

        // Music theory — derived from held notes this frame
        let chord = detect_chord(&midi.notes);
        let tension = tension_from_notes(&midi.notes);
        // V7→I: dom7 resolving to major or minor triggers a grand finale
        let trigger_finale = self.prev_chord == ChordQuality::Dom7th
            && (chord == ChordQuality::Major || chord == ChordQuality::Minor);
        self.current_chord = chord;
        self.current_tension = tension;

        // Spawn a shell for each note-on event
        for ev in midi.note_on_events() {
            if self.shells.len() >= MAX_SHELLS { break; }
            let x = (ev.note as f32 / 127.0 - 0.5) * win.w() * 0.88;
            let launch_y = -win.h() * 0.44;
            let hue_offset = if hue_spread > 0.0 {
                rng.gen_range(-hue_spread..hue_spread)
            } else {
                0.0
            };
            let shell_hue = (chord_hue(chord, base_hue) + hue_offset).rem_euclid(1.0);
            let form = BurstForm::from_chord(chord).unwrap_or_else(|| {
                let f = BurstForm::cycle(self.form_index);
                self.form_index += 1;
                f
            });
            self.last_form = form;
            self.shells.push(Shell {
                pos: vec2(x, launch_y),
                vel: vec2(rng.gen_range(-25.0f32..25.0), shell_speed),
                hue: shell_hue,
                burst_vel: 80.0 + ev.velocity * 520.0,
                burst_count,
                fuse: 2.5,
                age: 0.0,
                form,
            });
        }

        // Advance shells; dissonant held notes jitter rising trajectories
        for shell in &mut self.shells {
            shell.vel.y -= gravity * SHELL_GRAV_SCALE * dt;
            shell.pos += shell.vel * dt;
            shell.fuse -= dt;
            shell.age += dt;
            if tension > 0.3 {
                shell.pos.x += rng.gen_range(-1.0f32..1.0) * tension * 15.0 * dt;
            }
        }

        if trigger_finale {
            // Burst every live shell instantly, then fire a centred mega-ring
            let shells: Vec<Shell> = self.shells.drain(..).collect();
            for shell in &shells {
                self.burst(shell, lifetime, &mut rng);
            }
            let mega_count = (burst_count * 3)
                .min(MAX_PARTICLES.saturating_sub(self.particles.len()));
            let mega = Shell {
                pos: Vec2::ZERO,
                vel: Vec2::ZERO,
                hue: chord_hue(chord, base_hue),
                burst_vel: 700.0,
                burst_count: mega_count,
                fuse: 0.0,
                age: 100.0,
                form: BurstForm::Ring,
            };
            self.burst(&mega, lifetime * 1.5, &mut rng);
        } else {
            let mut bursting: Vec<Shell> = Vec::new();
            let mut alive: Vec<Shell> = Vec::new();
            for shell in self.shells.drain(..) {
                let at_apex  = shell.vel.y <= 0.0 && shell.age > 0.15;
                let too_high = shell.pos.y > height_cap;
                let fuse_out = shell.fuse <= 0.0;
                if at_apex || too_high || fuse_out {
                    bursting.push(shell);
                } else {
                    alive.push(shell);
                }
            }
            self.shells = alive;
            for shell in &bursting {
                self.burst(shell, lifetime, &mut rng);
            }
        }

        self.prev_chord = chord;

        for p in &mut self.particles {
            p.vel.y -= gravity * dt;
            p.pos += p.vel * dt;
            p.lifetime -= dt;
        }
        self.particles.retain(|p| p.lifetime > 0.0);

        self.build_mesh();
    }

    fn view(&self, draw: &Draw, win: Rect) {
        self.win.set(win);
        if !self.mesh_verts.is_empty() {
            draw.mesh().indexed_colored(
                self.mesh_verts.iter().copied(),
                self.mesh_idx.iter().copied(),
            );
        }
    }

    fn name(&self) -> &'static str { "fireworks" }
    fn params(&self) -> &[Param] { PARAMS }

    fn hud_info(&self) -> Option<String> {
        let chord_str = self.current_chord.name();
        let theory = if chord_str.is_empty() {
            format!("form:{}", self.last_form.name())
        } else {
            format!(
                "{} {}  tension:{:.0}%",
                chord_str,
                self.last_form.name(),
                self.current_tension * 100.0
            )
        };
        Some(format!(
            "{} shells  {} particles  {}",
            self.shells.len(),
            self.particles.len(),
            theory
        ))
    }
}
