use crate::midi::MidiState;
use crate::sketches::{flag_suffix, smoothstep, Param, Sketch, Smoothed, Stream, StreamParams, LAYER_HUE_SHIFT};
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

const FADE_IN_SECS: f32 = 0.12;
const CLEAR_SECS: f32 = 0.45;      // Clear fades the rings out over this long

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
    // Gravity and its direction ease in rather than switching.
    s_gravity: Smoothed,
    s_push_pull: Smoothed,
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
    age: f32,
    /// Seconds to reach `max_radius`; the ring slows as it gets there.
    life: f32,
    max_radius: f32,
    hue: f32,
    base_alpha: f32,
    stream: Stream,
    /// 1, falling to 0 once the ring is cleared.
    fade: f32,
    dying: bool,
}

impl Ring {
    fn progress(&self) -> f32 { (self.age / self.life).min(1.0) }

    fn radius(&self) -> f32 {
        let u = self.progress();
        self.max_radius * (1.0 - (1.0 - u) * (1.0 - u))
    }

    fn alpha(&self) -> f32 {
        let tail = (1.0 - self.progress()).powf(1.6);
        self.base_alpha * smoothstep(self.age / FADE_IN_SECS) * tail * self.fade
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
            s_gravity: Smoothed::new(0.0, 2.0),
            s_push_pull: Smoothed::new(1.0, 2.5),
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
        // Eased growth starts at twice the average speed: at `speed`.
        let life = (2.0 * max_radius / self.tunes.get(stream, P_SPEED).max(1.0)).max(0.3);
        self.rings.push(Ring {
            center: pos,
            offset: Vec2::ZERO,
            age: 0.0,
            life,
            max_radius,
            hue: hue.rem_euclid(1.0),
            base_alpha,
            stream,
            fade: 1.0,
            dying: false,
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
            let radius = r.radius();
            let base = self.mesh_verts.len();
            self.mesh_verts.push((vec3(pos.x, pos.y, 0.0), color));
            for i in 0..N_SIDES {
                let angle = i as f32 * TAU / N_SIDES as f32;
                self.mesh_verts.push((
                    vec3(pos.x + angle.cos() * radius, pos.y + angle.sin() * radius, 0.0),
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

        let gravity_mix = self.s_gravity.drive(if self.gravity_on { 1.0 } else { 0.0 }, dt);
        let push_pull = self.s_push_pull.drive(self.push_pull, dt);
        for r in &mut self.rings {
            r.age += dt;
            if r.dying { r.fade -= dt / CLEAR_SECS; }
            let gravity = self.tunes.get(r.stream, P_GRAVITY) * gravity_mix;
            if gravity > 0.0 {
                let diff = (r.center + r.offset) - gravity_center;
                let dist = diff.length();
                if dist > 1.0 {
                    r.offset += push_pull * gravity * (diff / dist) * dt;
                }
            }
        }
        self.rings.retain(|r| r.age < r.life && r.fade > 0.0);

        let mut rng = thread_rng();

        // Auto stream: a ring per hit, pitch → x.
        let hue   = self.tunes.get(Stream::Auto, P_HUE);
        let max_r = self.tunes.get(Stream::Auto, P_MAX_R);
        for ev in &midi.music.hits {
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
            Key::R => self.rings.iter_mut().for_each(|r| r.dying = true),
            Key::O => self.gravity_on = !self.gravity_on,
            Key::K => self.push_pull = -self.push_pull,
            _ => {}
        }
    }
}
