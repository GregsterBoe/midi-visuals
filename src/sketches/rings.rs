use crate::midi::MidiState;
use crate::sketches::{Param, Sketch};
use nannou::prelude::*;
use nannou::rand::{thread_rng, Rng};
use std::cell::Cell;

const PARAMS: &[Param] = &[
    Param::new(24, "hue",      0.0,    1.0),
    Param::new(25, "max_r",    5.0, 1200.0),
    Param::new(26, "speed",    5.0,  800.0),
    Param::new(27, "gravity",  0.0,  600.0),
    Param::new(28, "rain/s",   0.1,   30.0),
    Param::new(29, "r-spread", 0.0,  200.0),
];

const RAIN_COLOR_SPLIT: f32 = 0.1;
const RAIN_CYCLE_SECS: f32 = 10.0;

const N_SIDES: usize = 16;
const MAX_RINGS: usize = 2_000;

pub struct Rings {
    rings: Vec<Ring>,
    rain: bool,
    gravity_on: bool,
    push_pull: f32,
    rain_accum: f32,
    rain_angle: f32,
    rain_flip: bool,
    win: Cell<Rect>,
    mesh_verts: Vec<(Vec3, Hsla)>,
    mesh_idx: Vec<usize>,
}

struct Ring {
    center: Vec2,
    offset: Vec2,
    radius: f32,
    max_radius: f32,
    grow_rate: f32,
    hue: f32,
    base_alpha: f32,
}

impl Ring {
    fn alpha(&self) -> f32 {
        (1.0 - self.radius / self.max_radius).max(0.0) * self.base_alpha
    }
}

impl Rings {
    pub fn new() -> Self {
        Self {
            rings: Vec::new(),
            rain: false,
            gravity_on: false,
            push_pull: 1.0,
            rain_accum: 0.0,
            rain_angle: 0.0,
            rain_flip: false,
            win: Cell::new(Rect::from_w_h(800.0, 600.0)),
            mesh_verts: Vec::with_capacity(MAX_RINGS * (N_SIDES + 1)),
            mesh_idx: Vec::with_capacity(MAX_RINGS * N_SIDES * 3),
        }
    }

    fn spawn_at(&mut self, pos: Vec2, max_radius: f32, grow_rate: f32, hue: f32, base_alpha: f32) {
        if self.rings.len() >= MAX_RINGS { return; }
        self.rings.push(Ring {
            center: pos,
            offset: Vec2::ZERO,
            radius: 2.0,
            max_radius,
            grow_rate,
            hue: hue.rem_euclid(1.0),
            base_alpha,
        });
    }

    fn build_mesh(&mut self) {
        self.mesh_verts.clear();
        self.mesh_idx.clear();
        for r in &self.rings {
            let alpha = r.alpha();
            if alpha < 0.01 { continue; }
            let color = hsla(r.hue, 0.8, 0.55, alpha);
            let pos = r.center + r.offset;
            let base = self.mesh_verts.len();
            self.mesh_verts.push((vec3(pos.x, pos.y, 0.0), color));
            for i in 0..N_SIDES {
                let angle = i as f32 * TAU / N_SIDES as f32;
                self.mesh_verts.push((
                    vec3(pos.x + angle.cos() * r.radius, pos.y + angle.sin() * r.radius, 0.0),
                    color,
                ));
            }
            for i in 0..N_SIDES {
                self.mesh_idx.extend_from_slice(&[base, base + 1 + i, base + 1 + (i + 1) % N_SIDES]);
            }
        }
    }
}

impl Sketch for Rings {
    fn update(&mut self, midi: &MidiState, dt: f32) {
        let hue       = PARAMS[0].read(midi);
        let max_r     = PARAMS[1].read(midi);
        let speed     = PARAMS[2].read(midi);
        let gravity   = PARAMS[3].read(midi);
        let rain_rate = PARAMS[4].read(midi);
        let r_spread  = PARAMS[5].read(midi);

        let win = self.win.get();
        let gravity_center = win.xy();
        let gravity_on = self.gravity_on;
        let push_pull = self.push_pull;

        self.rain_angle = (self.rain_angle + dt * TAU / RAIN_CYCLE_SECS).rem_euclid(TAU);

        for r in &mut self.rings {
            r.radius += r.grow_rate * dt;
            if gravity_on && gravity > 0.0 {
                let diff = (r.center + r.offset) - gravity_center;
                let dist = diff.length();
                if dist > 1.0 {
                    r.offset += push_pull * gravity * (diff / dist) * dt;
                }
            }
        }
        self.rings.retain(|r| r.radius < r.max_radius);

        let mut rng = thread_rng();

        for ev in midi.note_on_events() {
            let x = win.left() + (ev.note as f32 / 127.0) * win.w();
            let y = rng.gen_range(win.bottom() * 0.8..win.top() * 0.8);
            let r_max = max_r * (0.5 + ev.velocity * 0.5);
            let note_hue = (hue + ev.note as f32 / 127.0 * 0.3).rem_euclid(1.0);
            self.spawn_at(vec2(x, y), r_max, speed, note_hue, ev.velocity.max(0.4));
        }

        if self.rain && rain_rate > 0.0 {
            self.rain_accum += dt;
            let interval = 1.0 / rain_rate;
            while self.rain_accum >= interval {
                self.rain_accum -= interval;
                let pos = win.xy()
                    + vec2(self.rain_angle.cos(), self.rain_angle.sin()) * r_spread;
                let offset = if self.rain_flip { RAIN_COLOR_SPLIT } else { -RAIN_COLOR_SPLIT };
                let rain_hue = (hue + 0.5 + offset).rem_euclid(1.0);
                self.rain_flip = !self.rain_flip;
                self.spawn_at(pos, max_r, speed, rain_hue, 0.8);
            }
        }

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

    fn name(&self) -> &'static str { "rings" }

    fn params(&self) -> &[Param] { PARAMS }

    fn hud_info(&self) -> Option<String> {
        let mut flags = Vec::new();
        if self.rain { flags.push("rain"); }
        if self.gravity_on {
            flags.push(if self.push_pull > 0.0 { "push" } else { "pull" });
        }
        let suffix = if flags.is_empty() {
            String::new()
        } else {
            format!("  [{}]", flags.join("+"))
        };
        Some(format!("{} rings{}", self.rings.len(), suffix))
    }

    fn key_pressed(&mut self, key: Key) {
        match key {
            Key::G => { self.rain = !self.rain; self.rain_accum = 0.0; }
            Key::O => self.gravity_on = !self.gravity_on,
            Key::K => self.push_pull = -self.push_pull,
            Key::R => self.rings.clear(),
            _ => {}
        }
    }
}
