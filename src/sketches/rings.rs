use crate::midi::MidiState;
use crate::sketches::{flag_suffix, Param, Sketch, Stream, StreamParams, LAYER_HUE_SHIFT};
use nannou::prelude::*;
use nannou::rand::{thread_rng, Rng};
use std::cell::Cell;

// hue, max radius, speed and gravity are kept per stream and follow the tune
// target; rain rate and spread only drive the layer.
const PARAMS: &[Param] = &[
    Param::new(24, "hue",      0.0,    1.0),
    Param::new(25, "max_r",    5.0, 1200.0),
    Param::new(26, "speed",    5.0,  800.0),
    Param::new(27, "gravity",  0.0,  600.0),
    Param::new(28, "rain/s",   0.1,   30.0),
    Param::new(29, "r-spread", 0.0,  200.0),
];
const P_HUE: usize = 0;
const P_MAX_R: usize = 1;
const P_SPEED: usize = 2;
const P_GRAVITY: usize = 3;
const P_RAIN_RATE: usize = 4;
const P_SPREAD: usize = 5;

const RAIN_COLOR_SPLIT: f32 = 0.1;
const RAIN_CYCLE_SECS: f32 = 10.0;
const HALO_RINGS: usize = 12;     // rings in one halo, evenly round the rain circle
const HALO_MIN_SPREAD: f32 = 60.0;

const N_SIDES: usize = 16;
const MAX_RINGS: usize = 2_000;

pub struct Rings {
    rings: Vec<Ring>,
    tunes: StreamParams,
    /// The layer's rain, toggled by hand.
    rain: bool,
    halo: bool,
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
    stream: Stream,
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
            tunes: StreamParams::new(PARAMS.len()),
            rain: false,
            halo: false,
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

    fn spawn_at(&mut self, pos: Vec2, stream: Stream, max_radius: f32, hue: f32, base_alpha: f32) {
        if self.rings.len() >= MAX_RINGS { return; }
        self.rings.push(Ring {
            center: pos,
            offset: Vec2::ZERO,
            radius: 2.0,
            max_radius,
            grow_rate: self.tunes.get(stream, P_SPEED),
            hue: hue.rem_euclid(1.0),
            base_alpha,
            stream,
        });
    }

    /// One layer ring at `angle` on the rain circle, alternating either side
    /// of the layer's hue.
    fn spawn_layer(&mut self, angle: f32, spread: f32) {
        let pos = self.win.get().xy() + vec2(angle.cos(), angle.sin()) * spread;
        let split = if self.rain_flip { RAIN_COLOR_SPLIT } else { -RAIN_COLOR_SPLIT };
        self.rain_flip = !self.rain_flip;
        let hue = self.tunes.get(Stream::Layer, P_HUE) + LAYER_HUE_SHIFT + split;
        self.spawn_at(pos, Stream::Layer, self.tunes.get(Stream::Layer, P_MAX_R), hue, 0.8);
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
        self.tunes.update(PARAMS, midi);
        let rain_rate = PARAMS[P_RAIN_RATE].read(midi);
        let r_spread  = PARAMS[P_SPREAD].read(midi);

        let win = self.win.get();
        let gravity_center = win.xy();

        self.rain_angle = (self.rain_angle + dt * TAU / RAIN_CYCLE_SECS).rem_euclid(TAU);

        for r in &mut self.rings {
            r.radius += r.grow_rate * dt;
            let gravity = self.tunes.get(r.stream, P_GRAVITY);
            if self.gravity_on && gravity > 0.0 {
                let diff = (r.center + r.offset) - gravity_center;
                let dist = diff.length();
                if dist > 1.0 {
                    r.offset += self.push_pull * gravity * (diff / dist) * dt;
                }
            }
        }
        self.rings.retain(|r| r.radius < r.max_radius);

        let mut rng = thread_rng();

        // Auto stream: a ring per note, pitch → x.
        let hue   = self.tunes.get(Stream::Auto, P_HUE);
        let max_r = self.tunes.get(Stream::Auto, P_MAX_R);
        for ev in midi.note_on_events() {
            let x = win.left() + (ev.note as f32 / 127.0) * win.w();
            let y = rng.gen_range(win.bottom() * 0.8..win.top() * 0.8);
            let r_max = max_r * (0.5 + ev.velocity * 0.5);
            let note_hue = hue + ev.note as f32 / 127.0 * 0.3;
            self.spawn_at(vec2(x, y), Stream::Auto, r_max, note_hue, ev.velocity.max(0.4));
        }

        // Layer: rain from a slowly turning point on the rain circle while
        // it's on, plus a whole halo round that circle when asked for.
        if self.rain && rain_rate > 0.0 {
            self.rain_accum += dt;
            let interval = 1.0 / rain_rate;
            while self.rain_accum >= interval {
                self.rain_accum -= interval;
                self.spawn_layer(self.rain_angle, r_spread);
            }
        }
        if std::mem::take(&mut self.halo) {
            for i in 0..HALO_RINGS {
                let angle = self.rain_angle + i as f32 * TAU / HALO_RINGS as f32;
                self.spawn_layer(angle, r_spread.max(HALO_MIN_SPREAD));
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
        flags.extend(self.tunes.hud_flag());
        Some(format!("{} rings{}", self.rings.len(), flag_suffix(&flags)))
    }

    fn actions(&self) -> &[(Key, &'static str)] {
        &[
            (Key::G, "Rain"),
            (Key::B, "Halo"),
            (Key::T, "Tune: both / auto / layer"),
            (Key::R, "Clear"),
            (Key::O, "Gravity"),
            (Key::K, "Push / pull"),
        ]
    }

    fn key_pressed(&mut self, key: Key) {
        match key {
            Key::G => { self.rain = !self.rain; self.rain_accum = 0.0; }
            Key::B => self.halo = true,
            Key::T => self.tunes.target = self.tunes.target.next(),
            Key::R => self.rings.clear(),
            Key::O => self.gravity_on = !self.gravity_on,
            Key::K => self.push_pull = -self.push_pull,
            _ => {}
        }
    }
}
