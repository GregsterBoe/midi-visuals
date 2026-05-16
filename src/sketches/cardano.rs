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
    // trail[frame][collection][circle]
    trail: VecDeque<Vec<Vec<Vec2>>>,
    win: Cell<Rect>,
    mesh_verts: Vec<(Vec3, Hsla)>,
    mesh_idx: Vec<usize>,
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
        // New collections inherit current inner angles so they're in sync
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

        // Snapshot lerp_factors before taking mut borrows on mesh vecs
        let lerp_snap: Vec<Vec<f32>> = self.collections.iter()
            .map(|c| c.lerp_factors.clone())
            .collect();

        self.mesh_verts.clear();
        self.mesh_idx.clear();

        for (fi, frame) in self.trail.iter().enumerate().skip(start) {
            let t = (fi - start + 1) as f32 / n as f32;
            let alpha = t.powi(2) * self.base_alpha;
            if alpha < 0.005 { continue; }
            let r = (dot_r * (0.4 + t * 0.6)).max(0.5);

            for (ci, positions) in frame.iter().enumerate() {
                // Shift hue slightly per collection for visual differentiation
                let coll_shift = ci as f32 / MAX_COLLECTIONS as f32;
                for (j, &pos) in positions.iter().enumerate() {
                    let lf = lerp_snap.get(ci).and_then(|v| v.get(j)).copied().unwrap_or(0.5);
                    let hue = (hue1 + lf * (hue2 - hue1) + coll_shift).rem_euclid(1.0);
                    let color = hsla(hue, 0.85, 0.55, alpha);
                    let base = self.mesh_verts.len();
                    self.mesh_verts.push((vec3(pos.x, pos.y, 0.0), color));
                    for k in 0..N_SIDES {
                        let angle = k as f32 * TAU / N_SIDES as f32;
                        self.mesh_verts.push((
                            vec3(pos.x + angle.cos() * r, pos.y + angle.sin() * r, 0.0),
                            color,
                        ));
                    }
                    for k in 0..N_SIDES {
                        self.mesh_idx.extend_from_slice(&[
                            base,
                            base + 1 + k,
                            base + 1 + (k + 1) % N_SIDES,
                        ]);
                    }
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

        // Alignment: fraction of the even-spread angle between consecutive collections
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

        // Compute current positions per collection
        let angle_outer = self.angle_outer;
        let frame: Vec<Vec<Vec2>> = self.collections.iter().enumerate()
            .map(|(ci, coll)| {
                let outer_angle = angle_outer + ci as f32 * alignment;
                let ox = orbit_r * outer_angle.cos();
                let oy = orbit_r * outer_angle.sin();
                coll.angles_inner.iter().zip(&coll.deflections)
                    .map(|(&a, d)| vec2(ox + orbit_r * a.cos(), oy + orbit_r * a.sin()) + d.offset)
                    .collect()
            })
            .collect();

        self.trail.push_back(frame);
        while self.trail.len() > MAX_TRAIL {
            self.trail.pop_front();
        }

        // Notes: outward radial spring impulse — circles deflect then spring back
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
