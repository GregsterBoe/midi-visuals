use crate::midi::MidiState;
use crate::sketches::{Param, Sketch};
use nannou::color::LinSrgba;
use nannou::prelude::*;
use nannou::rand::{thread_rng, Rng};
use std::cell::Cell;

// ── simulation ────────────────────────────────────────────────────────────────
const CELL_SIZE: f32 = 8.0;
const MAX_DROPLETS: usize = 400;
const SPAWN_MARGIN: f32 = 10.0;
const CLAMP_PAD: f32 = 4.0;

const DROPLET_SPEED: f32 = 120.0;   // px / s
const SPEED_VARIATION: f32 = 0.3;   // ± fraction of base speed
const DROPLET_RADIUS: f32 = 2.0;
const DROPLET_ALPHA: f32 = 0.78;
const MAX_DROPLET_AGE: f32 = 6.0;   // seconds

const ENTRY_CENTER_Y: f32 = 0.5;    // 0 = top, 1 = bottom of window
const ENTRY_SPREAD: f32 = 0.8;      // fraction of window height

const PATH_PROBE_AHEAD: f32 = 18.0; // px ahead to probe
const PATH_PROBE_STEP: f32 = 10.0;  // px up/down for steering probe
const BRANCH_CHANCE: f32 = 0.12;

const INFLUENCE_DEPOSIT: f32 = 0.4;
const INFLUENCE_RADIUS: f32 = 8.0;  // px
const INFLUENCE_THRESHOLD: f32 = 0.05;
const INFLUENCE_RENDER_GAIN: f32 = 0.25; // intensity → alpha
const INFLUENCE_RENDER_MAX: f32 = 95.0 / 255.0;

// Trail geometry is rebuilt every N frames. Droplet geometry rebuilds every frame.
// At 60 fps this yields ~20 trail rebuilds/s; the trail changes slowly enough
// that the 3-frame lag is invisible.
const TRAIL_REBUILD_INTERVAL: u8 = 3;

// ── MIDI params ───────────────────────────────────────────────────────────────
const PARAMS: &[Param] = &[
    Param::new(24, "base hue",   0.0,  360.0),
    Param::new(25, "spawn rate", 0.0, 300.0),
    Param::new(26, "attraction", 0.0,  3.0),
    Param::new(27, "jitter",     0.0,  120.0),
    Param::new(28, "decay",      0.05, 2.0),
];

// ── color ─────────────────────────────────────────────────────────────────────
const BASE_HUE: f32 = 210.0;   // degrees — also used as default before MIDI connects
const BASE_SAT: f32 = 0.75;
const BASE_LIGHT: f32 = 0.55;
const MIN_LUMA_SPAWN: f32 = 0.32;
const MIN_LUMA_TRAIL: f32 = 0.12;
const MIN_LUMA_MIXED: f32 = 0.26;

// ── types ─────────────────────────────────────────────────────────────────────

struct Droplet {
    pos: Vec2,
    prev_pos: Vec2,
    vel_y: f32,
    radius: f32,
    alpha: f32,
    age: f32,
    max_age: f32,
    color: [f32; 3],
    deposit_scale: f32,
    alive: bool,
}

pub struct Droplets {
    droplets: Vec<Droplet>,
    influence: Vec<f32>,
    trail_rgb: Vec<[f32; 3]>,
    cols: usize,
    rows: usize,
    spawn_accum: f32,
    base_hue: f32,
    win: Cell<Rect>,
    deposits: Vec<(Vec2, f32, [f32; 3])>,
    // Trail mesh rebuilt every TRAIL_REBUILD_INTERVAL frames (slow-changing geometry).
    // Vertices are at z=-1 so a separate draw call always sorts behind the drop mesh.
    trail_verts: Vec<(Vec3, LinSrgba<f32>)>,
    trail_idx: Vec<usize>,
    trail_frame: u8,
    // Drop mesh rebuilt every frame (fast-moving geometry).
    drop_verts: Vec<(Vec3, LinSrgba<f32>)>,
    drop_idx: Vec<usize>,
}

// ── impl ──────────────────────────────────────────────────────────────────────

impl Droplets {
    pub fn new() -> Self {
        let (win_w, win_h) = (800.0f32, 600.0f32);
        let cols = (win_w / CELL_SIZE).ceil() as usize;
        let rows = (win_h / CELL_SIZE).ceil() as usize;
        let max_cells = cols * rows;
        let drop_cap = MAX_DROPLETS * 2; // tail + head quads per droplet
        Self {
            droplets: Vec::new(),
            influence: vec![0.0; max_cells],
            trail_rgb: vec![[0.0; 3]; max_cells],
            cols,
            rows,
            spawn_accum: 0.0,
            base_hue: BASE_HUE,
            win: Cell::new(Rect::from_w_h(win_w, win_h)),
            deposits: Vec::new(),
            trail_verts: Vec::with_capacity(max_cells * 4),
            trail_idx: Vec::with_capacity(max_cells * 6),
            trail_frame: 0,
            drop_verts: Vec::with_capacity(drop_cap * 4),
            drop_idx: Vec::with_capacity(drop_cap * 6),
        }
    }

    fn cell_of(&self, pos: Vec2) -> (i32, i32) {
        let win = self.win.get();
        (
            ((pos.x - win.left()) / CELL_SIZE) as i32,
            ((win.top() - pos.y) / CELL_SIZE) as i32,
        )
    }

    fn cell_idx(&self, gx: i32, gy: i32) -> Option<usize> {
        if gx >= 0 && gy >= 0 && (gx as usize) < self.cols && (gy as usize) < self.rows {
            Some(gy as usize * self.cols + gx as usize)
        } else {
            None
        }
    }

    fn trail_rgb_at(&self, pos: Vec2) -> [f32; 3] {
        let (gx, gy) = self.cell_of(pos);
        self.cell_idx(gx, gy)
            .map_or([0.1, 0.45, 0.9], |i| enforce_min_luma(self.trail_rgb[i], MIN_LUMA_TRAIL))
    }

    fn deposit(&mut self, pos: Vec2, amount: f32, color: [f32; 3]) {
        let win = self.win.get();
        let cx = ((pos.x - win.left()) / CELL_SIZE) as i32;
        let cy = ((win.top() - pos.y) / CELL_SIZE) as i32;
        let radius_cells = INFLUENCE_RADIUS / CELL_SIZE;
        let reach = radius_cells.ceil() as i32;

        for oy in -reach..=reach {
            for ox in -reach..=reach {
                let dist = ((ox * ox + oy * oy) as f32).sqrt();
                if dist > radius_cells { continue; }
                if let Some(i) = self.cell_idx(cx + ox, cy + oy) {
                    let w = amount * (1.0 - dist / radius_cells);
                    self.influence[i] = (self.influence[i] + w).min(20.0);
                    let blend = (w * 0.15).clamp(0.0, 1.0);
                    for c in 0..3 {
                        self.trail_rgb[i][c] += (color[c] - self.trail_rgb[i][c]) * blend;
                    }
                }
            }
        }
    }

    fn pick_color(&self, rng: &mut impl Rng, probe: Vec2) -> [f32; 3] {
        let base = random_blue(rng, self.base_hue);
        let (gx, gy) = self.cell_of(probe);
        let influence = self.cell_idx(gx, gy).map_or(0.0, |i| self.influence[i]);
        if influence < INFLUENCE_THRESHOLD { return base; }

        let trail = self.trail_rgb_at(probe);
        let tl = luma(trail);
        if tl < 0.08 { return base; }

        let t = ((tl - 0.08) / 0.32).clamp(0.0, 1.0);
        let mix = 0.80 - 0.52 * t; // 0.80 at low luma, 0.28 at high
        let mixed = [
            trail[0] * (1.0 - mix) + base[0] * mix,
            trail[1] * (1.0 - mix) + base[1] * mix,
            trail[2] * (1.0 - mix) + base[2] * mix,
        ];
        enforce_min_luma(mixed, MIN_LUMA_MIXED)
    }

    fn spawn_one(&mut self, rng: &mut impl Rng, win: Rect, jitter_range: f32) {
        let x = win.right() + SPAWN_MARGIN;
        let center_y = win.top() - ENTRY_CENTER_Y * win.h();
        let half = ENTRY_SPREAD * win.h() * 0.5;
        let y = (center_y + rng.gen_range(-half..half)).clamp(win.bottom(), win.top());
        let color = self.pick_color(rng, vec2(x - PATH_PROBE_AHEAD, y));
        self.droplets.push(Droplet {
            pos: vec2(x, y),
            prev_pos: vec2(x, y),
            vel_y: rng.gen_range(-1.0f32..1.0) * jitter_range * 0.2,
            radius: (DROPLET_RADIUS + rng.gen_range(-0.7f32..0.9f32)).max(0.4),
            alpha: (DROPLET_ALPHA + rng.gen_range(-40.0f32 / 255.0..25.0f32 / 255.0))
                .clamp(40.0 / 255.0, 1.0),
            age: 0.0,
            max_age: (MAX_DROPLET_AGE + rng.gen_range(-2.5f32..3.0f32)).max(1.0),
            color,
            deposit_scale: 1.0,
            alive: true,
        });
    }

    // Rebuilds trail geometry. Called every TRAIL_REBUILD_INTERVAL frames.
    // Trail vertices are placed at z=-1 so a separate draw call sorts behind drop_verts.
    fn build_trail_mesh(&mut self) {
        let win = self.win.get();
        self.trail_verts.clear();
        self.trail_idx.clear();

        for gy in 0..self.rows {
            for gx in 0..self.cols {
                let i = gy * self.cols + gx;
                let intensity = self.influence[i];
                if intensity < INFLUENCE_THRESHOLD { continue; }
                let alpha = (intensity * INFLUENCE_RENDER_GAIN).min(INFLUENCE_RENDER_MAX);
                let [r, g, b] = self.trail_rgb[i];
                let x = win.left() + (gx as f32 + 0.5) * CELL_SIZE;
                let y = win.top()  - (gy as f32 + 0.5) * CELL_SIZE;
                let h = CELL_SIZE * 0.5;
                let c = LinSrgba::new(r, g, b, alpha);
                let base = self.trail_verts.len();
                self.trail_verts.extend_from_slice(&[
                    (vec3(x - h, y - h, -1.0), c),
                    (vec3(x + h, y - h, -1.0), c),
                    (vec3(x + h, y + h, -1.0), c),
                    (vec3(x - h, y + h, -1.0), c),
                ]);
                self.trail_idx.extend_from_slice(&[base, base+1, base+2, base, base+2, base+3]);
            }
        }
    }

    // Rebuilds droplet geometry (tails + heads). Called every frame.
    fn build_drop_mesh(&mut self) {
        self.drop_verts.clear();
        self.drop_idx.clear();

        for d in &self.droplets {
            let fade = (1.0 - d.age / d.max_age).clamp(0.0, 1.0);
            let [r, g, b] = d.color;
            let a = d.alpha * fade;

            // Tail: thin quad along the movement direction
            let dir = d.pos - d.prev_pos;
            if dir.length_squared() > 0.001 {
                let perp = vec2(-dir.y, dir.x).normalize() * 0.5;
                let c = LinSrgba::new(r, g, b, a);
                let p0 = d.prev_pos - perp;
                let p1 = d.prev_pos + perp;
                let p2 = d.pos + perp;
                let p3 = d.pos - perp;
                let base = self.drop_verts.len();
                self.drop_verts.extend_from_slice(&[
                    (vec3(p0.x, p0.y, 0.0), c),
                    (vec3(p1.x, p1.y, 0.0), c),
                    (vec3(p2.x, p2.y, 0.0), c),
                    (vec3(p3.x, p3.y, 0.0), c),
                ]);
                self.drop_idx.extend_from_slice(&[base, base+1, base+2, base, base+2, base+3]);
            }

            // Head: bright quad
            let hr = d.radius;
            let ch = LinSrgba::new(
                (r + 0.10).min(1.0),
                (g + 0.10).min(1.0),
                (b + 0.10).min(1.0),
                (a + 0.10).min(1.0),
            );
            let x = d.pos.x;
            let y = d.pos.y;
            let base = self.drop_verts.len();
            self.drop_verts.extend_from_slice(&[
                (vec3(x - hr, y - hr, 0.0), ch),
                (vec3(x + hr, y - hr, 0.0), ch),
                (vec3(x + hr, y + hr, 0.0), ch),
                (vec3(x - hr, y + hr, 0.0), ch),
            ]);
            self.drop_idx.extend_from_slice(&[base, base+1, base+2, base, base+2, base+3]);
        }
    }
}

impl Sketch for Droplets {
    fn update(&mut self, midi: &MidiState, dt: f32) {
        let mut rng = thread_rng();
        let win = self.win.get();

        // Read MIDI params (CC at 0 → min, CC at 127 → max)
        self.base_hue    = PARAMS[0].read(midi);
        let spawn_rate   = PARAMS[1].read(midi);
        let attraction   = PARAMS[2].read(midi);
        let jitter_range = PARAMS[3].read(midi);
        let decay_rate   = PARAMS[4].read(midi);

        // Rebuild grid when the window is resized.
        // view() updates self.win each frame, so update() sees the new size one frame later.
        let new_cols = (win.w() / CELL_SIZE).ceil() as usize;
        let new_rows = (win.h() / CELL_SIZE).ceil() as usize;
        if new_cols != self.cols || new_rows != self.rows {
            self.cols = new_cols;
            self.rows = new_rows;
            self.influence = vec![0.0; new_cols * new_rows];
            self.trail_rgb = vec![[0.0; 3]; new_cols * new_rows];
            self.droplets.clear();
            self.spawn_accum = 0.0;
            self.trail_frame = 0; // force trail rebuild on next frame
        }

        // Decay the influence field
        let decay = (1.0 - decay_rate * dt).max(0.0);
        for v in &mut self.influence { *v *= decay; }

        // Physics pass — reads influence immutably, collects deposits separately
        // so we can apply them after without conflicting borrows.
        let mut deposits = std::mem::take(&mut self.deposits);
        deposits.clear();
        {
            let influence = &self.influence;
            let (cols, rows) = (self.cols, self.rows);

            for d in &mut self.droplets {
                d.prev_pos = d.pos;
                d.age += dt;

                let probe_x = d.pos.x - PATH_PROBE_AHEAD;
                let c  = cell_sample(influence, cols, rows, win, vec2(probe_x, d.pos.y));
                let up = cell_sample(influence, cols, rows, win, vec2(probe_x, d.pos.y + PATH_PROBE_STEP));
                let dn = cell_sample(influence, cols, rows, win, vec2(probe_x, d.pos.y - PATH_PROBE_STEP));

                let target_vy = if rng.gen_range(0.0f32..1.0) < BRANCH_CHANCE {
                    rng.gen_range(-PATH_PROBE_STEP..PATH_PROBE_STEP)
                } else {
                    let mut best = c;
                    let mut offset = 0.0f32;
                    if up > best { best = up; offset = PATH_PROBE_STEP; }
                    if dn > best { offset = -PATH_PROBE_STEP; }
                    attraction * offset * 12.0
                };

                let jitter = rng.gen_range(-1.0f32..1.0) * jitter_range;
                d.vel_y += (target_vy + jitter - d.vel_y) * 0.12;

                let speed = (DROPLET_SPEED
                    * (1.0 + rng.gen_range(-SPEED_VARIATION..SPEED_VARIATION)))
                    .max(10.0);
                d.pos.x -= speed * dt;
                d.pos.y = (d.pos.y + d.vel_y * dt)
                    .clamp(win.bottom() + CLAMP_PAD, win.top() - CLAMP_PAD);

                deposits.push((d.pos, INFLUENCE_DEPOSIT * dt * 60.0 * d.deposit_scale, d.color));

                if d.pos.x < win.left() - SPAWN_MARGIN || d.age >= d.max_age {
                    d.alive = false;
                }
            }
        }

        // Apply deposits (influence borrow released)
        for (pos, amount, color) in deposits.drain(..) {
            self.deposit(pos, amount, color);
        }
        self.deposits = deposits;

        self.droplets.retain(|d| d.alive);

        // Note-on burst: spawn 4–8 droplets anchored to the note's Y position.
        // Velocity scales alpha and radius so harder hits produce brighter, larger drops.
        for event in midi.note_on_events() {
            let remaining = MAX_DROPLETS.saturating_sub(self.droplets.len());
            if remaining == 0 { break; }
            let note_y = win.bottom() + (event.note as f32 / 127.0) * win.h();
            let count = rng.gen_range(4..=8_usize).min(remaining);
            let velocity = event.velocity;
            // deposit_scale: soft hits leave a thin trace; hard hits leave 2× the trail
            let deposit_scale = (0.5 + 1.5 * velocity).clamp(0.5, 2.0);
            for _ in 0..count {
                let y = (note_y + rng.gen_range(-20.0f32..20.0f32))
                    .clamp(win.bottom() + CLAMP_PAD, win.top() - CLAMP_PAD);
                let x = win.right() + SPAWN_MARGIN;
                let color = self.pick_color(&mut rng, vec2(x - PATH_PROBE_AHEAD, y));
                self.droplets.push(Droplet {
                    pos: vec2(x, y),
                    prev_pos: vec2(x, y),
                    vel_y: rng.gen_range(-1.0f32..1.0) * jitter_range * 0.2,
                    radius: (DROPLET_RADIUS * (0.7 + 0.6 * velocity)).max(0.4),
                    alpha: (DROPLET_ALPHA * (0.5 + 0.5 * velocity)).clamp(40.0 / 255.0, 1.0),
                    age: 0.0,
                    max_age: (MAX_DROPLET_AGE + rng.gen_range(-2.5f32..3.0f32)).max(1.0),
                    color,
                    deposit_scale,
                    alive: true,
                });
            }
        }

        // Continuous spawn from right edge
        self.spawn_accum += spawn_rate * dt;
        while self.spawn_accum >= 1.0 && self.droplets.len() < MAX_DROPLETS {
            self.spawn_accum -= 1.0;
            self.spawn_one(&mut rng, win, jitter_range);
        }

        // Rebuild drop geometry every frame; trail only every TRAIL_REBUILD_INTERVAL frames.
        if self.trail_frame == 0 {
            self.build_trail_mesh();
        }
        self.trail_frame = (self.trail_frame + 1) % TRAIL_REBUILD_INTERVAL;
        self.build_drop_mesh();
    }

    fn view(&self, draw: &Draw, win: Rect) {
        self.win.set(win);

        // Trail is at z=-1, drops at z=0 — separate draw calls are safe because
        // the z difference gives Nannou a deterministic sort order.
        if !self.trail_verts.is_empty() {
            draw.mesh().indexed_colored(
                self.trail_verts.iter().copied(),
                self.trail_idx.iter().copied(),
            );
        }
        if !self.drop_verts.is_empty() {
            draw.mesh().indexed_colored(
                self.drop_verts.iter().copied(),
                self.drop_idx.iter().copied(),
            );
        }
    }

    fn name(&self) -> &'static str { "droplets" }

    fn params(&self) -> &[Param] { PARAMS }

    fn hud_info(&self) -> Option<String> {
        let active = self.influence.iter().filter(|&&v| v >= INFLUENCE_THRESHOLD).count();
        let sat = active as f32 / (self.cols * self.rows) as f32;
        Some(format!("{} droplets  sat {:.0}%", self.droplets.len(), sat * 100.0))
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn cell_sample(influence: &[f32], cols: usize, rows: usize, win: Rect, pos: Vec2) -> f32 {
    let gx = ((pos.x - win.left()) / CELL_SIZE) as i32;
    let gy = ((win.top()  - pos.y) / CELL_SIZE) as i32;
    if gx >= 0 && gy >= 0 && (gx as usize) < cols && (gy as usize) < rows {
        influence[gy as usize * cols + gx as usize]
    } else {
        0.0
    }
}

fn random_blue(rng: &mut impl Rng, base_hue: f32) -> [f32; 3] {
    let h = (base_hue + rng.gen_range(-12.0f32..12.0f32)).rem_euclid(360.0);
    let s = (BASE_SAT   + rng.gen_range(-0.07f32..0.07f32)).clamp(0.0, 1.0);
    let l = (BASE_LIGHT + rng.gen_range(-0.07f32..0.05f32)).clamp(0.0, 1.0);
    enforce_min_luma(hsl_to_rgb(h / 360.0, s, l), MIN_LUMA_SPAWN)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    if s == 0.0 { return [l, l, l]; }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    [hue_ch(p, q, h + 1.0 / 3.0), hue_ch(p, q, h), hue_ch(p, q, h - 1.0 / 3.0)]
}

fn hue_ch(p: f32, q: f32, t: f32) -> f32 {
    let t = t.rem_euclid(1.0);
    if t < 1.0 / 6.0 { return p + (q - p) * 6.0 * t; }
    if t < 1.0 / 2.0 { return q; }
    if t < 2.0 / 3.0 { return p + (q - p) * (2.0 / 3.0 - t) * 6.0; }
    p
}

fn luma(rgb: [f32; 3]) -> f32 {
    0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
}

fn enforce_min_luma(rgb: [f32; 3], min: f32) -> [f32; 3] {
    let l = luma(rgb);
    if l >= min || l <= 0.00001 { return rgb; }
    let gain = min / l;
    [(rgb[0] * gain).clamp(0.0, 1.0), (rgb[1] * gain).clamp(0.0, 1.0), (rgb[2] * gain).clamp(0.0, 1.0)]
}
