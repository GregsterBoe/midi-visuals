use crate::midi::MidiState;
use crate::sketches::{flag_suffix, Param, Sketch, Stream, StreamParams};
use nannou::prelude::*;
use nannou::rand::{thread_rng, Rng};
use std::cell::{Cell, RefCell};

// ── simulation ────────────────────────────────────────────────────────────────
const CELL_SIZE: f32 = 8.0;
const MAX_DROPLETS: usize = 5000;
const SPAWN_MARGIN: f32 = 10.0;
const CLAMP_PAD: f32 = 4.0;

const DROPLET_SPEED: f32 = 100.0;   // px / s base (decay scales this up)
const DECAY_SPEED_GAIN: f32 = 150.0; // added speed per unit of decay_rate
const SPEED_VARIATION: f32 = 0.25;  // ± fraction of base speed
const DROPLET_RADIUS: f32 = 2.0;
const DROPLET_ALPHA: f32 = 0.78;

const PATH_PROBE_AHEAD: f32 = 18.0; // px ahead to probe
const PATH_PROBE_STEP: f32 = 10.0;  // px up/down for steering probe
const BRANCH_CHANCE: f32 = 0.12;

const INFLUENCE_DEPOSIT: f32 = 0.4;
const INFLUENCE_RADIUS: f32 = 8.0;  // px
const INFLUENCE_THRESHOLD: f32 = 0.05;
const INFLUENCE_RENDER_GAIN: f32 = 0.25; // intensity → alpha
const INFLUENCE_RENDER_MAX: f32 = 95.0 / 255.0;

// Trail geometry is rebuilt every N frames. Drop geometry rebuilds every frame.
const TRAIL_REBUILD_INTERVAL: u8 = 3;

// ── instanced rendering ───────────────────────────────────────────────────────
// Each instance is 10 f32s: center(2) + axes(4) + color(4).
// axes = [right.x, right.y, up.x, up.y] — encodes orientation and half-extents.
// For axis-aligned quads:   right=(hs,0), up=(0,hs)
// For oriented tail quads:  right=half_dir, up=perp*half_width
const INSTANCE_FLOATS: usize = 10;
const INSTANCE_STRIDE: u64 = (INSTANCE_FLOATS * 4) as u64; // bytes

// WGSL shader — one pipeline handles all quad types via the axes transform.
const SHADER: &str = r#"
struct Globals {
    win_size: vec2<f32>,
    _pad: vec2<f32>,
}
@group(0) @binding(0) var<uniform> globals: Globals;

struct VertOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(
    @location(0) local: vec2<f32>,
    @location(1) center: vec2<f32>,
    @location(2) axes: vec4<f32>,
    @location(3) color: vec4<f32>,
) -> VertOut {
    let world = center + local.x * axes.xy + local.y * axes.zw;
    let ndc = world / (globals.win_size * 0.5);
    var out: VertOut;
    out.clip_pos = vec4<f32>(ndc.x, ndc.y, 0.0, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_main(@location(0) color: vec4<f32>) -> @location(0) vec4<f32> {
    return color;
}
"#;

struct DropletWgpu {
    pipeline: wgpu::RenderPipeline,
    quad_vbuf: wgpu::Buffer,
    quad_ibuf: wgpu::Buffer,
    instance_buf: wgpu::Buffer,
    globals_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    instance_capacity: usize,
}

// ── MIDI params ───────────────────────────────────────────────────────────────
// hue, attraction and jitter are kept per stream and follow the tune target.
// decay is shared (both streams draw into one trail field); the flow params
// only drive the layer.
const PARAMS: &[Param] = &[
    Param::new(24, "base hue",   0.0,  360.0),
    Param::new(25, "flow rate",  0.0,  300.0),
    Param::new(26, "attraction", 0.0,  3.0),
    Param::new(27, "jitter",     0.0,  120.0),
    Param::new(28, "decay",      0.05, 2.0),
    Param::new(29, "flow y",     0.0,  1.0),
    Param::new(30, "flow width", 0.05, 1.0),
];
const P_HUE: usize = 0;
const P_FLOW_RATE: usize = 1;
const P_ATTRACTION: usize = 2;
const P_JITTER: usize = 3;
const P_DECAY: usize = 4;
const P_FLOW_Y: usize = 5;
const P_FLOW_WIDTH: usize = 6;

// ── layer ─────────────────────────────────────────────────────────────────────
const SURGE_COUNT: usize = 60;      // droplets in one surge, spread over the flow band

// ── color ─────────────────────────────────────────────────────────────────────
const LAYER_HUE_OFFSET: f32 = 180.0; // the layer is drawn in the opposite hue
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
    color: [f32; 3],
    stream: Stream,
    alive: bool,
}

pub struct Droplets {
    droplets: Vec<Droplet>,
    influence: Vec<f32>,
    trail_rgb: Vec<[f32; 3]>,
    /// How much of each cell's trail the layer laid down: 0 = auto, 1 = layer.
    trail_layer: Vec<f32>,
    cols: usize,
    rows: usize,
    tunes: StreamParams,
    /// The layer's continuous flow, toggled by hand.
    flow: bool,
    surge: bool,
    spawn_accum: f32,
    win: Cell<Rect>,
    deposits: Vec<(Vec2, f32, [f32; 3], Stream)>,
    trail_instances: Vec<f32>,   // 10 floats per quad instance, rebuilt every TRAIL_REBUILD_INTERVAL frames
    drop_instances: Vec<f32>,    // 10 floats per quad instance, rebuilt every frame
    trail_frame: u8,
    wgpu: RefCell<Option<DropletWgpu>>,
}

// ── impl ──────────────────────────────────────────────────────────────────────

impl Droplets {
    pub fn new() -> Self {
        let (win_w, win_h) = (800.0f32, 600.0f32);
        let cols = (win_w / CELL_SIZE).ceil() as usize;
        let rows = (win_h / CELL_SIZE).ceil() as usize;
        let max_cells = cols * rows;
        Self {
            droplets: Vec::new(),
            influence: vec![0.0; max_cells],
            trail_rgb: vec![[0.0; 3]; max_cells],
            trail_layer: vec![0.0; max_cells],
            cols,
            rows,
            tunes: StreamParams::new(PARAMS.len()),
            flow: false,
            surge: false,
            spawn_accum: 0.0,
            win: Cell::new(Rect::from_w_h(win_w, win_h)),
            deposits: Vec::new(),
            trail_instances: Vec::with_capacity(max_cells * INSTANCE_FLOATS),
            drop_instances: Vec::with_capacity(MAX_DROPLETS * 2 * INSTANCE_FLOATS),
            trail_frame: 0,
            wgpu: RefCell::new(None),
        }
    }

    fn create_wgpu(device: &wgpu::Device, capacity: usize) -> DropletWgpu {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("droplets"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("droplets_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("droplets_globals"),
            size: 16, // 4 × f32
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("droplets_bg"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buf.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("droplets_pl"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });

        // Template unit quad: vertices in [-1,1]², indices form two triangles
        let quad_verts: [f32; 8] = [-1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0];
        let quad_vbuf = device.create_buffer_init(&wgpu::BufferInitDescriptor {
            label: Some("droplets_qv"),
            contents: bytemuck::cast_slice(&quad_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let quad_idx: [u16; 6] = [0, 1, 2, 0, 2, 3];
        let quad_ibuf = device.create_buffer_init(&wgpu::BufferInitDescriptor {
            label: Some("droplets_qi"),
            contents: bytemuck::cast_slice(&quad_idx),
            usage: wgpu::BufferUsages::INDEX,
        });

        let instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("droplets_inst"),
            size: (capacity * INSTANCE_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("droplets"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[
                    // Slot 0: per-vertex — the unit quad template
                    wgpu::VertexBufferLayout {
                        array_stride: 8,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        }],
                    },
                    // Slot 1: per-instance — center, axes, color
                    wgpu::VertexBufferLayout {
                        array_stride: INSTANCE_STRIDE,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &[
                            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0,  shader_location: 1 },
                            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 8,  shader_location: 2 },
                            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 24, shader_location: 3 },
                        ],
                    },
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: Frame::TEXTURE_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            // Must match Nannou's default 4x MSAA intermediary texture
            multisample: wgpu::MultisampleState {
                count: Frame::DEFAULT_MSAA_SAMPLES,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
        });

        DropletWgpu {
            pipeline,
            quad_vbuf,
            quad_ibuf,
            instance_buf,
            globals_buf,
            bind_group,
            instance_capacity: capacity,
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

    fn deposit(&mut self, pos: Vec2, amount: f32, color: [f32; 3], stream: Stream) {
        let layer = if stream == Stream::Layer { 1.0 } else { 0.0 };
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
                    self.trail_layer[i] += (layer - self.trail_layer[i]) * blend;
                }
            }
        }
    }

    fn stream_hue(&self, stream: Stream) -> f32 {
        let hue = self.tunes.get(stream, P_HUE);
        if stream == Stream::Layer { hue + LAYER_HUE_OFFSET } else { hue }
    }

    fn pick_color(&self, rng: &mut impl Rng, probe: Vec2, stream: Stream) -> [f32; 3] {
        let base = random_tint(rng, self.stream_hue(stream));
        let (gx, gy) = self.cell_of(probe);
        let Some(i) = self.cell_idx(gx, gy) else { return base };
        if self.influence[i] < INFLUENCE_THRESHOLD { return base; }

        let trail = enforce_min_luma(self.trail_rgb[i], MIN_LUMA_TRAIL);
        let tl = luma(trail);
        if tl < 0.08 { return base; }

        // Only trail laid by the droplet's own stream tints it, so the two
        // streams keep their opposite colours where they cross.
        let own = if stream == Stream::Layer { self.trail_layer[i] } else { 1.0 - self.trail_layer[i] };
        let t = ((tl - 0.08) / 0.32).clamp(0.0, 1.0);
        let mix = 1.0 - (0.20 + 0.52 * t) * own;
        let mixed = [
            trail[0] * (1.0 - mix) + base[0] * mix,
            trail[1] * (1.0 - mix) + base[1] * mix,
            trail[2] * (1.0 - mix) + base[2] * mix,
        ];
        enforce_min_luma(mixed, MIN_LUMA_MIXED)
    }

    fn spawn(&mut self, rng: &mut impl Rng, y: f32, stream: Stream, radius: f32, alpha: f32) {
        if self.droplets.len() >= MAX_DROPLETS { return; }
        let win = self.win.get();
        let x = win.right() + SPAWN_MARGIN;
        let y = y.clamp(win.bottom() + CLAMP_PAD, win.top() - CLAMP_PAD);
        let color = self.pick_color(rng, vec2(x - PATH_PROBE_AHEAD, y), stream);
        self.droplets.push(Droplet {
            pos: vec2(x, y),
            prev_pos: vec2(x, y),
            vel_y: rng.gen_range(-1.0f32..1.0) * self.tunes.get(stream, P_JITTER) * 0.2,
            radius: radius.max(0.4),
            alpha: alpha.clamp(40.0 / 255.0, 1.0),
            color,
            stream,
            alive: true,
        });
    }

    /// One droplet of the layer's flow, entering somewhere in the flow band.
    fn spawn_flow(&mut self, rng: &mut impl Rng, band: (f32, f32)) {
        let (center, half) = band;
        let y = center + rng.gen_range(-half..=half);
        let radius = DROPLET_RADIUS + rng.gen_range(-0.7f32..0.9f32);
        let alpha = DROPLET_ALPHA + rng.gen_range(-40.0f32 / 255.0..25.0f32 / 255.0);
        self.spawn(rng, y, Stream::Layer, radius, alpha);
    }

    // Rebuilds trail instance data. Called every TRAIL_REBUILD_INTERVAL frames.
    fn build_trail_instances(&mut self) {
        let win = self.win.get();
        self.trail_instances.clear();
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
                // Axis-aligned quad: right=(h,0), up=(0,h)
                self.trail_instances.extend_from_slice(&[x, y, h, 0.0, 0.0, h, r, g, b, alpha]);
            }
        }
    }

    // Rebuilds drop instance data (tails + heads). Called every frame.
    fn build_drop_instances(&mut self) {
        let win = self.win.get();
        self.drop_instances.clear();
        for d in &self.droplets {
            // Fade out only in the last 15% of the screen so droplets are bright the whole crossing.
            let progress = ((d.pos.x - win.left()) / win.w()).clamp(0.0, 1.0);
            let [r, g, b] = d.color;
            let a = d.alpha * (progress / 0.15).min(1.0);

            // Tail: oriented quad spanning prev_pos → pos
            let dir = d.pos - d.prev_pos;
            if dir.length_squared() > 0.001 {
                let half_dir = dir * 0.5;
                let perp = vec2(-dir.y, dir.x).normalize() * 0.5;
                let cx = (d.pos.x + d.prev_pos.x) * 0.5;
                let cy = (d.pos.y + d.prev_pos.y) * 0.5;
                self.drop_instances.extend_from_slice(&[
                    cx, cy, half_dir.x, half_dir.y, perp.x, perp.y, r, g, b, a,
                ]);
            }

            // Head: bright axis-aligned quad
            let hr = d.radius;
            let (rh, gh, bh) = ((r + 0.10).min(1.0), (g + 0.10).min(1.0), (b + 0.10).min(1.0));
            let ah = (a + 0.10).min(1.0);
            self.drop_instances.extend_from_slice(&[
                d.pos.x, d.pos.y, hr, 0.0, 0.0, hr, rh, gh, bh, ah,
            ]);
        }
    }
}

impl Sketch for Droplets {
    fn update(&mut self, midi: &MidiState, dt: f32) {
        let mut rng = thread_rng();
        let win = self.win.get();

        self.tunes.update(PARAMS, midi);
        let flow_rate  = PARAMS[P_FLOW_RATE].read(midi);
        let decay_rate = PARAMS[P_DECAY].read(midi);
        let band = flow_band(win, PARAMS[P_FLOW_Y].read(midi), PARAMS[P_FLOW_WIDTH].read(midi));
        // (attraction, jitter) per stream, indexed by `stream_slot`.
        let steer = [Stream::Auto, Stream::Layer]
            .map(|s| (self.tunes.get(s, P_ATTRACTION), self.tunes.get(s, P_JITTER)));

        let new_cols = (win.w() / CELL_SIZE).ceil() as usize;
        let new_rows = (win.h() / CELL_SIZE).ceil() as usize;
        if new_cols != self.cols || new_rows != self.rows {
            self.cols = new_cols;
            self.rows = new_rows;
            self.influence = vec![0.0; new_cols * new_rows];
            self.trail_rgb = vec![[0.0; 3]; new_cols * new_rows];
            self.trail_layer = vec![0.0; new_cols * new_rows];
            self.droplets.clear();
            self.spawn_accum = 0.0;
            self.trail_frame = 0;
        }

        let decay = (1.0 - decay_rate * dt).max(0.0);
        for v in &mut self.influence { *v *= decay; }

        let mut deposits = std::mem::take(&mut self.deposits);
        deposits.clear();
        {
            let influence = &self.influence;
            let (cols, rows) = (self.cols, self.rows);

            let base_speed = DROPLET_SPEED + decay_rate * DECAY_SPEED_GAIN;

            for d in &mut self.droplets {
                d.prev_pos = d.pos;
                let (attraction, jitter_range) = steer[stream_slot(d.stream)];

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

                let speed = (base_speed
                    * (1.0 + rng.gen_range(-SPEED_VARIATION..SPEED_VARIATION)))
                    .max(10.0);
                d.pos.x -= speed * dt;
                d.pos.y = (d.pos.y + d.vel_y * dt)
                    .clamp(win.bottom() + CLAMP_PAD, win.top() - CLAMP_PAD);

                deposits.push((d.pos, INFLUENCE_DEPOSIT * dt * 60.0, d.color, d.stream));

                if d.pos.x < win.left() - SPAWN_MARGIN {
                    d.alive = false;
                }
            }
        }

        for (pos, amount, color, stream) in deposits.drain(..) {
            self.deposit(pos, amount, color, stream);
        }
        self.deposits = deposits;

        self.droplets.retain(|d| d.alive);

        // Auto stream — note-on burst: high notes → top, low notes → bottom.
        // Velocity scales radius and alpha; all droplets cross the full screen.
        for event in midi.note_on_events() {
            if self.droplets.len() >= MAX_DROPLETS { break; }
            let note_y = win.bottom() + (event.note as f32 / 127.0) * win.h();
            let velocity = event.velocity;
            for _ in 0..rng.gen_range(4..=8_usize) {
                let y = note_y + rng.gen_range(-20.0f32..20.0f32);
                let radius = DROPLET_RADIUS * (0.7 + 0.6 * velocity);
                let alpha = DROPLET_ALPHA * (0.5 + 0.5 * velocity);
                self.spawn(&mut rng, y, Stream::Auto, radius, alpha);
            }
        }

        // Layer: the continuous flow while it's on, plus a surge when asked for.
        if self.flow {
            self.spawn_accum += flow_rate * dt;
            while self.spawn_accum >= 1.0 && self.droplets.len() < MAX_DROPLETS {
                self.spawn_accum -= 1.0;
                self.spawn_flow(&mut rng, band);
            }
        }
        if std::mem::take(&mut self.surge) {
            for _ in 0..SURGE_COUNT {
                self.spawn_flow(&mut rng, band);
            }
        }

        if self.trail_frame == 0 {
            self.build_trail_instances();
        }
        self.trail_frame = (self.trail_frame + 1) % TRAIL_REBUILD_INTERVAL;
        self.build_drop_instances();
    }

    // view() only captures the window rect so update() sees the current size next frame.
    // All rendering is done in raw_render() via the instanced wgpu pipeline.
    fn view(&self, _draw: &Draw, win: Rect) {
        self.win.set(win);
    }

    fn raw_render(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureViewHandle,
        win: Rect,
    ) {
        let trail_count = (self.trail_instances.len() / INSTANCE_FLOATS) as u32;
        let drop_count  = (self.drop_instances.len()  / INSTANCE_FLOATS) as u32;
        let total = (trail_count + drop_count) as usize;
        if total == 0 { return; }

        // Lazy init or grow the instance buffer when capacity is exceeded
        let needs_reinit = {
            let b = self.wgpu.borrow();
            b.as_ref().map_or(true, |g| g.instance_capacity < total)
        };
        if needs_reinit {
            let capacity = (total * 2).max(8300);
            *self.wgpu.borrow_mut() = Some(Self::create_wgpu(device, capacity));
        }

        let wgpu_borrow = self.wgpu.borrow();
        let gpu = wgpu_borrow.as_ref().unwrap();

        // Update window size uniform
        let globals: [f32; 4] = [win.w(), win.h(), 0.0, 0.0];
        queue.write_buffer(&gpu.globals_buf, 0, bytemuck::cast_slice(&globals));

        // Upload trail instances followed by drop instances into one contiguous buffer.
        // Trail is drawn first (background), drops second (foreground).
        let mut all: Vec<f32> = Vec::with_capacity(
            self.trail_instances.len() + self.drop_instances.len()
        );
        all.extend_from_slice(&self.trail_instances);
        all.extend_from_slice(&self.drop_instances);
        queue.write_buffer(&gpu.instance_buf, 0, bytemuck::cast_slice(&all));

        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("droplets"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: true },
            })],
            depth_stencil_attachment: None,
        });

        rpass.set_pipeline(&gpu.pipeline);
        rpass.set_bind_group(0, &gpu.bind_group, &[]);
        rpass.set_vertex_buffer(0, gpu.quad_vbuf.slice(..));
        rpass.set_vertex_buffer(1, gpu.instance_buf.slice(..));
        rpass.set_index_buffer(gpu.quad_ibuf.slice(..), wgpu::IndexFormat::Uint16);

        if trail_count > 0 {
            rpass.draw_indexed(0..6, 0, 0..trail_count);
        }
        if drop_count > 0 {
            rpass.draw_indexed(0..6, 0, trail_count..trail_count + drop_count);
        }
    }

    fn name(&self) -> &'static str { "droplets" }

    fn params(&self) -> &[Param] { PARAMS }

    fn hud_info(&self) -> Option<String> {
        let active = self.influence.iter().filter(|&&v| v >= INFLUENCE_THRESHOLD).count();
        let sat = active as f32 / (self.cols * self.rows) as f32;
        let mut flags = Vec::new();
        if self.flow { flags.push("flow"); }
        flags.extend(self.tunes.hud_flag());
        Some(format!("{} droplets  sat {:.0}%{}", self.droplets.len(), sat * 100.0, flag_suffix(&flags)))
    }

    fn actions(&self) -> &[(Key, &'static str)] {
        &[(Key::G, "Flow"), (Key::B, "Surge"), (Key::T, "Tune: both / auto / layer"), (Key::R, "Clear")]
    }

    fn key_pressed(&mut self, key: Key) {
        match key {
            Key::G => { self.flow = !self.flow; self.spawn_accum = 0.0; }
            Key::B => self.surge = true,
            Key::T => self.tunes.target = self.tunes.target.next(),
            Key::R => {
                self.droplets.clear();
                self.influence.fill(0.0);
                self.trail_rgb.fill([0.0; 3]);
                self.trail_layer.fill(0.0);
            }
            _ => {}
        }
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

fn stream_slot(stream: Stream) -> usize {
    match stream {
        Stream::Auto => 0,
        Stream::Layer => 1,
    }
}

/// The band the layer's flow enters through, as (center y, half height).
/// `width` is a fraction of the window; `pos` slides the band from the
/// bottom edge to the top without letting it leave the window.
fn flow_band(win: Rect, pos: f32, width: f32) -> (f32, f32) {
    let half = width * win.h() * 0.5;
    (win.bottom() + half + pos * (win.h() - 2.0 * half), half)
}

fn random_tint(rng: &mut impl Rng, base_hue: f32) -> [f32; 3] {
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
