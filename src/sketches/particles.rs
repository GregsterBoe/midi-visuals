use crate::midi::MidiState;
use crate::sketches::{Param, Sketch};
use nannou::prelude::*;
use nannou::rand::{thread_rng, Rng};

const PARAMS: &[Param] = &[
    Param::new(24, "hue",       0.0,   1.0),
    Param::new(25, "hue_spread", 0.0,  0.5),
    Param::new(26, "gravity",   0.0, 500.0),
    Param::new(27, "drag",      0.0,   4.0),
    Param::new(28, "spawn",     1.0,  50.0),
];

pub struct Particles {
    particles: Vec<Particle>,
}

struct Particle {
    pos: Vec2,
    vel: Vec2,
    lifetime: f32,
    max_lifetime: f32,
    hue: f32,
}

impl Particles {
    pub fn new() -> Self {
        Self { particles: Vec::new() }
    }

    fn spawn(&mut self, count: usize, base_hue: f32, hue_spread: f32) {
        let mut rng = thread_rng();
        for _ in 0..count {
            let angle = rng.gen_range(0.0f32..TAU);
            let speed = rng.gen_range(50.0f32..350.0f32);
            let hue_offset = if hue_spread > 0.0 { rng.gen_range(-hue_spread..hue_spread) } else { 0.0 };
            let hue = (base_hue + hue_offset).rem_euclid(1.0);
            let lifetime = rng.gen_range(1.5f32..3.5f32);
            self.particles.push(Particle {
                pos: vec2(
                    rng.gen_range(-250.0f32..250.0f32),
                    rng.gen_range(-180.0f32..180.0f32),
                ),
                vel: vec2(angle.cos() * speed, angle.sin() * speed),
                lifetime,
                max_lifetime: lifetime,
                hue,
            });
        }
    }
}

impl Sketch for Particles {
    fn update(&mut self, midi: &MidiState, dt: f32) {
        let gravity     = PARAMS[2].read(midi);
        let drag        = PARAMS[3].read(midi);
        let spawn_count = PARAMS[4].read(midi) as usize;
        let base_hue    = PARAMS[0].read(midi);
        let hue_spread  = PARAMS[1].read(midi);

        for p in &mut self.particles {
            p.vel.y -= gravity * dt;
            p.vel *= (1.0 - drag * dt).max(0.0);
            p.pos += p.vel * dt;
            p.lifetime -= dt;
        }
        self.particles.retain(|p| p.lifetime > 0.0);

        for _ in midi.note_on_events() {
            let remaining = 10_000usize.saturating_sub(self.particles.len());
            if remaining > 0 {
                self.spawn(spawn_count.min(remaining), base_hue, hue_spread);
            }
        }
    }

    fn view(&self, draw: &Draw, _win: Rect) {
        for p in &self.particles {
            let alpha = (p.lifetime / p.max_lifetime).powi(2);
            draw.ellipse()
                .xy(p.pos)
                .radius(3.0_f32)
                .color(hsla(p.hue, 0.9, 0.6, alpha));
        }
    }

    fn name(&self) -> &'static str { "particles" }

    fn params(&self) -> &[Param] { PARAMS }

    fn hud_info(&self) -> Option<String> {
        Some(format!("{} particles", self.particles.len()))
    }
}
