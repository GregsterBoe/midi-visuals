use crate::midi::MidiState;
use crate::sketches::{Param, Sketch};
use nannou::prelude::*;
use nannou::rand::{thread_rng, Rng};
use std::cell::Cell;
use std::collections::VecDeque;

const PARAMS: &[Param] = &[
    Param::new(1,  "hue",     0.0,    1.0),  // mod wheel; pitch bend controls hue2
    Param::new(24, "circles", 1.0,   16.0),
    Param::new(25, "dot_r",   1.0,   80.0),
    Param::new(26, "speed",   0.05,   6.0),
    Param::new(27, "ratio",   0.05,   8.0),
    Param::new(28, "orbit_r", 10.0, 700.0),
    Param::new(29, "align",   0.0,    1.0),
    Param::new(30, "trail",   1.0,  500.0),
];

const N_SIDES: usize = 8;
const MAX_BOUNDS: usize = 16;
const MAX_TRAIL: usize = 500;
const MAX_COLLECTIONS: usize = 4;
const SPRING_K: f32 = 6.0;
const SPRING_DAMP: f32 = 3.5;
const IMPULSE_SCALE: f32 = 250.0;

// Triangle fan: center at index 0, perimeter verts 1..=N_SIDES.
// Hardcoded for N_SIDES=8 to avoid a per-dot loop with modulo.
const INDEX_OFFSETS: [usize; N_SIDES * 3] = [
    0, 1, 2,   0, 2, 3,   0, 3, 4,   0, 4, 5,
    0, 5, 6,   0, 6, 7,   0, 7, 8,   0, 8, 1,
];

#[derive(Clone, Default)]
struct Deflection {
    offset: Vec2,
    velocity: Vec2,
}

struct Collection {
    angles_inner: Vec<f32>,
    lerp_factors: Vec<f32>,
    deflections: Vec<Deflection>,
}

impl Collection {
    fn new(bounds: usize, angles_inner: Vec<f32>, rng: &mut impl Rng) -> Self {
        Self {
            lerp_factors: (0..bounds).map(|_| rng.gen_range(0.0f32..1.0)).collect(),
            deflections: vec![Deflection::default(); bounds],
            angles_inner,
        }
    }
}

pub struct Cardano {
    angle_outer: f32,
    collections: Vec<Collection>,
    num_collections: usize,
    bounds: usize,
    base_alpha: f32,
    // trail[frame] = flat [c0d0, c0d1, ..., c1d0, ...] — one Vec<Vec2> per frame,
    // indexed as [ci * bounds + j]. Flat layout improves cache locality vs nested Vecs.
    trail: VecDeque<Vec<Vec2>>,
    win: Cell<Rect>,
    // LinSrgba instead of Hsla: convert once per circle here rather than
    // once per vertex inside Nannou's draw path (~9× fewer conversions).
    mesh_verts: Vec<(Vec3, LinSrgba)>,
    mesh_idx: Vec<usize>,
}

/// HSL (hue in [0,1]) → linear-sRGB RGBA, computed once per circle.
/// Skips the sRGB→linear gamma step for speed (colors are slightly brighter
/// but imperceptible in a dark visualizer context).
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

impl Cardano {
    pub fn new() -> Self {
        let mut rng = thread_rng();
        let bounds = 3;
        let angles = Self::evenly_spaced(bounds);
        Self {
            angle_outer: 0.0,
            collections: vec![Collection::new(bounds, angles, &mut rng)],
            num_collections: 1,
            bounds,
            base_alpha: 0.9,
            trail: VecDeque::with_capacity(MAX_TRAIL + 1),
            win: Cell::new(Rect::from_w_h(800.0, 600.0)),
            mesh_verts: Vec::new(),
            mesh_idx: Vec::new(),
        }
    }

    fn evenly_spaced(bounds: usize) -> Vec<f32> {
        let step = TAU / bounds as f32;
        (0..bounds).map(|i| i as f32 * step).collect()
    }

    fn set_bounds(&mut self, n: usize) {
        self.bounds = n.max(1).min(MAX_BOUNDS);
        let angles = Self::evenly_spaced(self.bounds);
        let mut rng = thread_rng();
        for c in &mut self.collections {
            *c = Collection::new(self.bounds, angles.clone(), &mut rng);
        }
        self.trail.clear();
    }

    fn set_num_collections(&mut self, n: usize) {
        self.num_collections = n.max(1).min(MAX_COLLECTIONS);
        let current_angles: Vec<f32> = self.collections.first()
            .map(|c| c.angles_inner.clone())
            .unwrap_or_else(|| Self::evenly_spaced(self.bounds));
        let mut rng = thread_rng();
        while self.collections.len() < self.num_collections {
            self.collections.push(Collection::new(self.bounds, current_angles.clone(), &mut rng));
        }
        self.collections.truncate(self.num_collections);
        self.trail.clear();
    }

    fn build_mesh(&mut self, hue1: f32, hue2: f32, dot_r: f32, trail_len: usize) {
        let n = self.trail.len().min(trail_len.max(1));
        let start = self.trail.len().saturating_sub(n);

        // Flat lerp_factors: index [ci * bounds + j]
        let lerp_snap: Vec<f32> = self.collections.iter()
            .flat_map(|c| c.lerp_factors.iter().copied())
            .collect();

        // Precompute polygon vertex offsets — 8 trig calls instead of 8 × n_dots
        let mut cos_table = [0.0f32; N_SIDES];
        let mut sin_table = [0.0f32; N_SIDES];
        for k in 0..N_SIDES {
            let a = k as f32 * TAU / N_SIDES as f32;
            cos_table[k] = a.cos();
            sin_table[k] = a.sin();
        }

        self.mesh_verts.clear();
        self.mesh_idx.clear();

        let bounds = self.bounds;
        let n_coll = self.num_collections;

        for (fi, frame) in self.trail.iter().enumerate().skip(start) {
            let t = (fi - start + 1) as f32 / n as f32;
            let alpha = t * t * self.base_alpha;
            if alpha < 0.01 { continue; }
            let r = (dot_r * (0.4 + t * 0.6)).max(0.5);

            for ci in 0..n_coll {
                let coll_shift = ci as f32 / MAX_COLLECTIONS as f32;
                for j in 0..bounds {
                    let pos = frame[ci * bounds + j];
                    let lf = lerp_snap[ci * bounds + j];
                    let hue = (hue1 + lf * (hue2 - hue1) + coll_shift).rem_euclid(1.0);
                    let color = hsl_to_lin(hue, 0.85, 0.55, alpha);
                    let base = self.mesh_verts.len();
                    self.mesh_verts.push((vec3(pos.x, pos.y, 0.0), color));
                    for k in 0..N_SIDES {
                        self.mesh_verts.push((
                            vec3(pos.x + cos_table[k] * r, pos.y + sin_table[k] * r, 0.0),
                            color,
                        ));
                    }
                    self.mesh_idx.extend(INDEX_OFFSETS.iter().map(|&o| base + o));
                }
            }
        }
    }
}

impl Sketch for Cardano {
    fn update(&mut self, midi: &MidiState, dt: f32) {
        let hue1       = PARAMS[0].read(midi);
        let hue2       = midi.pitch_bend;
        let new_bounds = PARAMS[1].read(midi).round() as usize;
        let dot_r      = PARAMS[2].read(midi);
        let speed      = PARAMS[3].read(midi);
        let ratio      = PARAMS[4].read(midi);
        let orbit_r    = PARAMS[5].read(midi);
        let align_t    = PARAMS[6].read(midi);
        let trail_len  = PARAMS[7].read(midi) as usize;

        if new_bounds != self.bounds {
            self.set_bounds(new_bounds);
        }

        let alignment = align_t * TAU / self.num_collections.max(2) as f32;

        self.angle_outer += speed * dt;

        for coll in &mut self.collections {
            for a in &mut coll.angles_inner {
                *a -= speed * ratio * dt;
            }
            for d in &mut coll.deflections {
                d.velocity += (-SPRING_K * d.offset - SPRING_DAMP * d.velocity) * dt;
                d.offset += d.velocity * dt;
            }
        }

        // Build flat frame: positions indexed as [ci * bounds + j]
        let angle_outer = self.angle_outer;
        let mut frame: Vec<Vec2> = Vec::with_capacity(self.num_collections * self.bounds);
        for (ci, coll) in self.collections.iter().enumerate() {
            let outer_angle = angle_outer + ci as f32 * alignment;
            let ox = orbit_r * outer_angle.cos();
            let oy = orbit_r * outer_angle.sin();
            for (&a, d) in coll.angles_inner.iter().zip(&coll.deflections) {
                frame.push(vec2(ox + orbit_r * a.cos(), oy + orbit_r * a.sin()) + d.offset);
            }
        }
        self.trail.push_back(frame);
        while self.trail.len() > MAX_TRAIL {
            self.trail.pop_front();
        }

        for ev in midi.note_on_events() {
            let strength = ev.velocity * IMPULSE_SCALE;
            let angle_outer = self.angle_outer;
            for coll in &mut self.collections {
                for i in 0..coll.deflections.len() {
                    let orbit_angle = angle_outer + coll.angles_inner[i];
                    let radial = vec2(orbit_angle.cos(), orbit_angle.sin());
                    coll.deflections[i].velocity += radial * strength;
                }
            }
            self.base_alpha = ev.velocity.max(0.4);
        }

        self.build_mesh(hue1, hue2, dot_r, trail_len);
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

    fn name(&self) -> &'static str { "cardano" }

    fn params(&self) -> &[Param] { PARAMS }

    fn hud_info(&self) -> Option<String> {
        Some(format!(
            "{}x{} circles  {} frames",
            self.num_collections, self.bounds, self.trail.len()
        ))
    }

    fn key_pressed(&mut self, key: Key) {
        match key {
            Key::C => {
                let next = self.num_collections % MAX_COLLECTIONS + 1;
                self.set_num_collections(next);
            }
            Key::R => self.trail.clear(),
            _ => {}
        }
    }
}
