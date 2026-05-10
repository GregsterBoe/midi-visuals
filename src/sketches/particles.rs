use crate::midi::MidiState;
use crate::sketches::Sketch;
use nannou::prelude::*;
use nannou::rand::{thread_rng, Rng};

// CC 24 = base hue | CC 25 = hue spread | CC 26 = gravity
// CC 27 = drag     | CC 28 = spawn count (1–50 per note)
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
        let gravity = midi.ccs[26] * 500.0;
        let drag = midi.ccs[27] * 4.0;
        let spawn_count = (1.0 + midi.ccs[28] * 49.0) as usize;
        let base_hue = midi.ccs[24];
        let hue_spread = midi.ccs[25] * 0.5;

        for p in &mut self.particles {
            p.vel.y -= gravity * dt;
            p.vel *= (1.0 - drag * dt).max(0.0);
            p.pos += p.vel * dt;
            p.lifetime -= dt;
        }
        self.particles.retain(|p| p.lifetime > 0.0);

        for event in &midi.recent_events {
            if event.on && self.particles.len() < 10_000 {
                self.spawn(spawn_count, base_hue, hue_spread);
            }
        }
    }

    fn view(&self, draw: &Draw, _win: Rect) {
        for p in &self.particles {
            let alpha = (p.lifetime / p.max_lifetime).powi(2);
            draw.ellipse()
                .xy(p.pos)
                .radius(3.0)
                .color(hsla(p.hue, 0.9, 0.6, alpha));
        }
    }

    fn name(&self) -> &'static str {
        "particles"
    }

    fn hud_info(&self) -> Option<String> {
        Some(format!("{} particles", self.particles.len()))
    }
}
