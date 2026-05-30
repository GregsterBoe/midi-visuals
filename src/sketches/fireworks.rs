use crate::midi::MidiState;
use crate::sketches::{Param, Sketch};
use nannou::prelude::*;
use nannou::rand::{thread_rng, Rng};
use std::cell::Cell;

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

struct Shell {
    pos: Vec2,
    vel: Vec2,
    hue: f32,
    burst_vel: f32,   // max initial speed of burst particles; scaled by note velocity
    burst_count: usize,
    fuse: f32,        // maximum time before forced burst
    age: f32,         // guard against immediate apex trigger on spawn frame
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
}

impl Fireworks {
    pub fn new() -> Self {
        Self {
            shells: Vec::new(),
            particles: Vec::new(),
            win: Cell::new(Rect::from_w_h(800.0, 600.0)),
            mesh_verts: Vec::with_capacity((MAX_PARTICLES + MAX_SHELLS) * 4),
            mesh_idx: Vec::with_capacity((MAX_PARTICLES + MAX_SHELLS) * 6),
        }
    }

    fn burst(&mut self, shell: &Shell, lifetime: f32, rng: &mut impl Rng) {
        let available = MAX_PARTICLES.saturating_sub(self.particles.len());
        let count = shell.burst_count.min(available);
        for _ in 0..count {
            let angle = rng.gen_range(0.0f32..TAU);
            let speed = rng.gen_range(0.6f32..1.0) * shell.burst_vel;
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

        // Particles first (rendered below shells)
        for p in &self.particles {
            let t = p.lifetime / p.max_lifetime;
            let color = hsl_to_lin(p.hue, 0.7 + t * 0.25, 0.4 + t * 0.35, t);
            push_quad(&mut self.mesh_verts, &mut self.mesh_idx, p.pos, PARTICLE_R, color);
        }

        // Shells on top
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

        // Spawn a shell for each note-on event
        for ev in midi.note_on_events() {
            if self.shells.len() >= MAX_SHELLS { break; }
            let x = (ev.note as f32 / 127.0 - 0.5) * win.w() * 0.88;
            let launch_y = -win.h() * 0.44;
            let hue_offset = if hue_spread > 0.0 { rng.gen_range(-hue_spread..hue_spread) } else { 0.0 };
            // Note velocity scales burst radius: soft press = small, hard press = large
            let burst_vel = 80.0 + ev.velocity * 520.0;
            self.shells.push(Shell {
                pos: vec2(x, launch_y),
                vel: vec2(rng.gen_range(-25.0f32..25.0), shell_speed),
                hue: (base_hue + hue_offset).rem_euclid(1.0),
                burst_vel,
                burst_count,
                fuse: 2.5,
                age: 0.0,
            });
        }

        // Advance shells and collect those that should burst
        for shell in &mut self.shells {
            shell.vel.y -= gravity * SHELL_GRAV_SCALE * dt;
            shell.pos += shell.vel * dt;
            shell.fuse -= dt;
            shell.age += dt;
        }

        let mut bursting: Vec<Shell> = Vec::new();
        let mut alive: Vec<Shell> = Vec::new();
        for shell in self.shells.drain(..) {
            let at_apex    = shell.vel.y <= 0.0 && shell.age > 0.15;
            let too_high   = shell.pos.y > height_cap;
            let fuse_out   = shell.fuse <= 0.0;
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

        // Advance and cull particles
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
        Some(format!("{} shells  {} particles", self.shells.len(), self.particles.len()))
    }
}
