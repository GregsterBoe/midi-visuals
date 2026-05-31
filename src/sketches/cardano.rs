use crate::midi::{MidiState, NoteState};
use crate::sketches::{Param, Sketch};
use bytemuck::{Pod, Zeroable};
use nannou::prelude::*;
use nannou::rand::{thread_rng, Rng};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;

const PARAMS: &[Param] = &[
    Param::new(1,  "hue",     0.0,    1.0),
    Param::new(24, "circles", 1.0,   16.0),
    Param::new(25, "dot_r",   1.0,   80.0),
    Param::new(26, "speed",   0.05,   6.0),
    Param::new(27, "ratio",   0.05,   8.0),
    Param::new(28, "orbit_r", 10.0, 700.0),
    Param::new(29, "align",   0.0,    1.0),
    Param::new(30, "trail",   1.0, 1000.0),
];

const N_SIDES: usize = 8;
const MAX_BOUNDS: usize = 16;
const MAX_COLLECTIONS: usize = 4;
const SPRING_K: f32 = 6.0;
const SPRING_DAMP: f32 = 3.5;
const IMPULSE_SCALE: f32 = 250.0;

const INDEX_OFFSETS: [usize; N_SIDES * 3] = [
    0, 1, 2,   0, 2, 3,   0, 3, 4,   0, 4, 5,
    0, 5, 6,   0, 6, 7,   0, 7, 8,   0, 8, 1,
];

// --- Music theory ---------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
enum ChordQuality { Major, Minor, Diminished, Augmented, Dom7th, Other }

impl ChordQuality {
    fn name(self) -> &'static str {
        match self {
            Self::Major      => "maj",
            Self::Minor      => "min",
            Self::Diminished => "dim",
            Self::Augmented  => "aug",
            Self::Dom7th     => "dom7",
            Self::Other      => "",
        }
    }
}

fn detect_chord(notes: &[NoteState; 128]) -> ChordQuality {
    let mut classes: Vec<u8> = (0u8..128)
        .filter(|&n| notes[n as usize].on)
        .map(|n| n % 12)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    classes.sort_unstable();
    if classes.len() < 3 { return ChordQuality::Other; }
    let root = classes[0];
    let intervals: Vec<u8> = classes.iter().map(|&c| (c + 12 - root) % 12).collect();
    let has = |n: u8| intervals.contains(&n);
    if has(4) && has(7) && has(10) { return ChordQuality::Dom7th; }
    if has(3) && has(6)            { return ChordQuality::Diminished; }
    if has(4) && has(8)            { return ChordQuality::Augmented; }
    if has(4) && has(7)            { return ChordQuality::Major; }
    if has(3) && has(7)            { return ChordQuality::Minor; }
    ChordQuality::Other
}

fn consonance_score(semitones: u8) -> f32 {
    match semitones % 12 {
        0  => 1.00,
        7  => 0.85,
        5  => 0.80,
        4  => 0.70,
        3  => 0.65,
        9  => 0.60,
        8  => 0.55,
        2  => 0.35,
        10 => 0.30,
        11 => 0.15,
        1  => 0.10,
        6  => 0.00,
        _  => 0.50,
    }
}

fn tension_from_notes(notes: &[NoteState; 128]) -> f32 {
    let held: Vec<u8> = (0..128u8).filter(|&n| notes[n as usize].on).collect();
    if held.len() < 2 { return 0.0; }
    let min_consonance = held.iter().enumerate()
        .flat_map(|(i, &a)| held[i + 1..].iter().map(move |&b| consonance_score(b.wrapping_sub(a))))
        .fold(1.0f32, f32::min);
    1.0 - min_consonance
}

fn chord_hue(q: ChordQuality) -> Option<f32> {
    match q {
        ChordQuality::Major      => Some(0.10),
        ChordQuality::Minor      => Some(0.65),
        ChordQuality::Diminished => Some(0.80),
        ChordQuality::Augmented  => Some(0.48),
        ChordQuality::Dom7th     => Some(0.98),
        ChordQuality::Other      => None,
    }
}

// --- GPU accumulation buffer ----------------------------------------------
//
// Instead of a CPU-side trail VecDeque, we keep a persistent GPU texture.
// Each frame: (1) draw a semi-transparent black quad to fade old content,
// (2) draw the current dots on top. The trail "lives" as pixels — zero CPU
// trail cost regardless of trail length.
//
// view()      → display the texture (1-frame lag, imperceptible at 60 fps)
// raw_render() → fade + dot update for the next frame

const FADE_WGSL: &str = r#"
struct FU { alpha: f32, }
@group(0) @binding(0) var<uniform> u: FU;

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    var xy = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    return vec4<f32>(xy[vi], 0.0, 1.0);
}

@fragment
fn fs() -> @location(0) vec4<f32> {
    // Premultiplied black with fade alpha — blended with OVER dims existing RGB
    return vec4<f32>(0.0, 0.0, 0.0, u.alpha);
}
"#;

// Fullscreen triangle that subtracts 1/255 per channel — snaps near-zero values to
// exact black so the trail doesn't linger as visible grey.
const SUBTRACT_WGSL: &str = r#"
@vertex
fn vs(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    var xy = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    return vec4<f32>(xy[vi], 0.0, 1.0);
}
@fragment
fn fs() -> @location(0) vec4<f32> {
    return vec4<f32>(1.0 / 255.0, 1.0 / 255.0, 1.0 / 255.0, 0.0);
}
"#;

const DOT_WGSL: &str = r#"
struct VIn {
    @location(0) pos: vec2<f32>,
    @location(1) col: vec4<f32>,
}
struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) col: vec4<f32>,
}

@vertex
fn vs(in: VIn) -> VOut {
    return VOut(vec4<f32>(in.pos, 0.0, 1.0), in.col);
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
    // Premultiply alpha for correct OVER compositing
    return vec4<f32>(in.col.rgb * in.col.a, in.col.a);
}
"#;

const ACCUM_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DotVertex {
    pos: [f32; 2],
    col: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FadeUniforms {
    alpha: f32,
    _pad: [f32; 3],
}

const MAX_DOT_VERTS: usize = MAX_COLLECTIONS * MAX_BOUNDS * (N_SIDES + 1);
const MAX_DOT_IDX:   usize = MAX_COLLECTIONS * MAX_BOUNDS * N_SIDES * 3;

struct GpuAccum {
    accum_texture:     wgpu::Texture,         // nannou's Texture — used by draw.texture()
    accum_view:        wgpu::TextureView,     // nannou's TextureView — derefs for render passes
    fade_pipeline:     wgpu::RenderPipeline,
    subtract_pipeline: wgpu::RenderPipeline,
    dot_pipeline:      wgpu::RenderPipeline,
    fade_uniform_buf: wgpu::Buffer,
    fade_bind_group:  wgpu::BindGroup,
    dot_vtx_buf: wgpu::Buffer,
    dot_idx_buf: wgpu::Buffer,
    cleared: bool,
    win_size: [u32; 2],
}

impl GpuAccum {
    fn new(device: &wgpu::Device, w: u32, h: u32) -> Self {
        // Accumulation texture: persistent across frames
        let accum_texture = wgpu::TextureBuilder::new()
            .size([w, h])
            .format(ACCUM_FORMAT)
            .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING)
            .build(device);
        let accum_view = accum_texture.view().build();

        // Shared blend: premultiplied OVER (dims existing content, draws dots on top)
        let over_blend = wgpu::BlendState {
            color: wgpu::BlendComponent::OVER,
            alpha: wgpu::BlendComponent::OVER,
        };
        let color_target = wgpu::ColorTargetState {
            format: ACCUM_FORMAT,
            blend: Some(over_blend),
            write_mask: wgpu::ColorWrites::ALL,
        };

        // Fade pipeline — fullscreen triangle, no vertex buffer
        let fade_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cardano_fade"),
            source: wgpu::ShaderSource::Wgsl(FADE_WGSL.into()),
        });
        let fade_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(
                        std::mem::size_of::<FadeUniforms>() as u64,
                    ),
                },
                count: None,
            }],
        });
        let fade_uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fade_uniform"),
            size: std::mem::size_of::<FadeUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let fade_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &fade_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: fade_uniform_buf.as_entire_binding(),
            }],
        });
        let fade_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&fade_bgl],
            push_constant_ranges: &[],
        });
        let fade_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cardano_fade"),
            layout: Some(&fade_pl),
            vertex: wgpu::VertexState {
                module: &fade_shader,
                entry_point: "vs",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &fade_shader,
                entry_point: "fs",
                targets: &[Some(color_target.clone())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        // Subtract pipeline — fullscreen triangle, ReverseSubtract blend, no uniforms
        let subtract_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cardano_subtract"),
            source: wgpu::ShaderSource::Wgsl(SUBTRACT_WGSL.into()),
        });
        let subtract_color_target = wgpu::ColorTargetState {
            format: ACCUM_FORMAT,
            blend: Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation:  wgpu::BlendOperation::ReverseSubtract,
                },
                // Preserve the alpha channel unchanged
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Zero,
                    dst_factor: wgpu::BlendFactor::One,
                    operation:  wgpu::BlendOperation::Add,
                },
            }),
            write_mask: wgpu::ColorWrites::ALL,
        };
        let subtract_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });
        let subtract_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cardano_subtract"),
            layout: Some(&subtract_pl),
            vertex: wgpu::VertexState {
                module: &subtract_shader,
                entry_point: "vs",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &subtract_shader,
                entry_point: "fs",
                targets: &[Some(subtract_color_target)],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        // Dot pipeline — vertex buffer with position + colour
        let dot_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cardano_dot"),
            source: wgpu::ShaderSource::Wgsl(DOT_WGSL.into()),
        });
        let dot_vtx_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<DotVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x2 },
                wgpu::VertexAttribute { offset: 8, shader_location: 1, format: wgpu::VertexFormat::Float32x4 },
            ],
        };
        let dot_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });
        let dot_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cardano_dot"),
            layout: Some(&dot_pl),
            vertex: wgpu::VertexState {
                module: &dot_shader,
                entry_point: "vs",
                buffers: &[dot_vtx_layout],
            },
            fragment: Some(wgpu::FragmentState {
                module: &dot_shader,
                entry_point: "fs",
                targets: &[Some(color_target)],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        // Static-capacity vertex/index buffers — dots never exceed MAX_DOT_VERTS
        let dot_vtx_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dot_vtx"),
            size: (MAX_DOT_VERTS * std::mem::size_of::<DotVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let dot_idx_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dot_idx"),
            size: (MAX_DOT_IDX * std::mem::size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        GpuAccum {
            accum_texture,
            accum_view,
            fade_pipeline,
            subtract_pipeline,
            dot_pipeline,
            fade_uniform_buf,
            fade_bind_group,
            dot_vtx_buf,
            dot_idx_buf,
            cleared: false,
            win_size: [w, h],
        }
    }
}

// --- Spring / collection data --------------------------------------------

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

// --- Cardano sketch -------------------------------------------------------

pub struct Cardano {
    angle_outer: f32,
    collections: Vec<Collection>,
    num_collections: usize,
    bounds: usize,
    base_alpha: f32,
    win: Cell<Rect>,
    // Per-frame dot state passed from update() to raw_render()
    current_dots: Vec<(Vec2, LinSrgba)>,
    dot_r: f32,
    fade_alpha: f32,
    // GPU accumulation buffer — initialised lazily on first raw_render call
    gpu: RefCell<Option<GpuAccum>>,
    current_chord: ChordQuality,
    current_tension: f32,
}

/// HSL (hue in [0,1]) → linear-sRGB RGBA, computed once per circle.
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
            win: Cell::new(Rect::from_w_h(800.0, 600.0)),
            current_dots: Vec::new(),
            dot_r: 10.0,
            fade_alpha: 0.02,
            gpu: RefCell::new(None),
            current_chord: ChordQuality::Other,
            current_tension: 0.0,
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
        // Clear the GPU texture so the old trail doesn't linger
        if let Ok(mut g) = self.gpu.try_borrow_mut() {
            if let Some(ref mut gpu) = *g {
                gpu.cleared = false;
            }
        }
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
        if let Ok(mut g) = self.gpu.try_borrow_mut() {
            if let Some(ref mut gpu) = *g {
                gpu.cleared = false;
            }
        }
    }
}

impl Sketch for Cardano {
    fn update(&mut self, midi: &MidiState, dt: f32) {
        let hue1      = PARAMS[0].read(midi);
        let hue2      = midi.pitch_bend;
        let new_bounds = PARAMS[1].read(midi).round() as usize;
        self.dot_r    = PARAMS[2].read(midi);
        let speed     = PARAMS[3].read(midi);
        let ratio     = PARAMS[4].read(midi);
        let orbit_r   = PARAMS[5].read(midi);
        let align_t   = PARAMS[6].read(midi);
        let trail_len = PARAMS[7].read(midi);

        // Fade alpha: chosen so the trail is near-invisible after trail_len frames.
        // 1 - 0.01^(1/trail_len) gives exact "1% brightness after trail_len frames".
        self.fade_alpha = 1.0 - (0.01_f32).powf(1.0 / trail_len);

        if new_bounds != self.bounds {
            self.set_bounds(new_bounds);
        }

        // Music theory
        self.current_chord   = detect_chord(&midi.notes);
        self.current_tension = tension_from_notes(&midi.notes);
        let effective_hue1   = chord_hue(self.current_chord).unwrap_or(hue1);

        let wobble_amp = if self.current_tension > 0.2 {
            (self.current_tension - 0.2) * orbit_r * 0.20
        } else {
            0.0
        };

        let alignment = align_t * TAU / self.num_collections.max(2) as f32;

        self.angle_outer += speed * dt;

        for coll in &mut self.collections {
            for a in &mut coll.angles_inner {
                *a -= speed * ratio * dt;
            }
            for d in &mut coll.deflections {
                d.velocity += (-SPRING_K * d.offset - SPRING_DAMP * d.velocity) * dt;
                d.offset   += d.velocity * dt;
            }
        }

        let mut rng = thread_rng();

        // Compute current dot positions and colours (no trail storage)
        self.current_dots.clear();
        for (ci, coll) in self.collections.iter().enumerate() {
            let coll_shift  = ci as f32 / MAX_COLLECTIONS as f32;
            let outer_angle = self.angle_outer + ci as f32 * alignment;
            let ox = orbit_r * outer_angle.cos();
            let oy = orbit_r * outer_angle.sin();
            for (j, (&a, d)) in coll.angles_inner.iter().zip(&coll.deflections).enumerate() {
                let r = if wobble_amp > 0.0 {
                    orbit_r + rng.gen_range(-wobble_amp..wobble_amp)
                } else {
                    orbit_r
                };
                let pos = vec2(ox + r * a.cos(), oy + r * a.sin()) + d.offset;
                let lf  = coll.lerp_factors[j];
                let hue = (effective_hue1 + lf * (hue2 - effective_hue1) + coll_shift).rem_euclid(1.0);
                self.current_dots.push((pos, hsl_to_lin(hue, 0.85, 0.55, self.base_alpha)));
            }
        }

        for ev in midi.note_on_events() {
            let strength = ev.velocity * IMPULSE_SCALE;
            let angle_outer = self.angle_outer;
            let pitch_bias  = vec2(0.0, ev.note as f32 / 127.0 * 2.0 - 1.0);
            for coll in &mut self.collections {
                for i in 0..coll.deflections.len() {
                    let orbit_angle = angle_outer + coll.angles_inner[i];
                    let radial      = vec2(orbit_angle.cos(), orbit_angle.sin());
                    let dir_raw     = radial + pitch_bias;
                    let impulse_dir = if dir_raw.length_squared() > 0.001 {
                        dir_raw.normalize()
                    } else {
                        radial
                    };
                    coll.deflections[i].velocity += impulse_dir * strength;
                }
            }
            self.base_alpha = ev.velocity.max(0.4);
        }
    }

    fn view(&self, draw: &Draw, win: Rect) {
        self.win.set(win);
        // Display the accumulation texture from the previous raw_render.
        // The HUD (drawn after this by main.rs) naturally layers on top.
        let gpu = self.gpu.borrow();
        if let Some(ref g) = *gpu {
            draw.texture(&g.accum_texture).w_h(win.w(), win.h());
        }
    }

    fn raw_render(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        _target: &wgpu::TextureViewHandle,
        _win: Rect,
    ) {
        let win    = self.win.get();
        let w      = win.w() as u32;
        let h      = win.h() as u32;
        let half_w = win.w() * 0.5;
        let half_h = win.h() * 0.5;

        let mut gpu_opt = self.gpu.borrow_mut();

        // Lazy init and window-resize handling
        let needs_init = gpu_opt.is_none()
            || gpu_opt.as_ref().map(|g| g.win_size != [w, h]).unwrap_or(false);
        if needs_init {
            *gpu_opt = Some(GpuAccum::new(device, w, h));
        }
        let gpu = gpu_opt.as_mut().unwrap();

        // Pre-compute polygon vertex offsets in NDC space
        let mut cos_t = [0.0f32; N_SIDES];
        let mut sin_t = [0.0f32; N_SIDES];
        for k in 0..N_SIDES {
            let a = k as f32 * TAU / N_SIDES as f32;
            cos_t[k] = a.cos();
            sin_t[k] = a.sin();
        }
        let rx = self.dot_r / half_w;  // radius in NDC x
        let ry = self.dot_r / half_h;  // radius in NDC y (separate to keep circles round)

        // Build dot vertex/index data for this frame's positions only
        let mut verts: Vec<DotVertex>  = Vec::with_capacity(self.current_dots.len() * (N_SIDES + 1));
        let mut indices: Vec<u32>      = Vec::with_capacity(self.current_dots.len() * N_SIDES * 3);

        for &(pos, col) in &self.current_dots {
            let cx  = pos.x / half_w;
            let cy  = pos.y / half_h;
            let c   = [col.red, col.green, col.blue, col.alpha];
            let base = verts.len() as u32;
            verts.push(DotVertex { pos: [cx, cy], col: c });
            for k in 0..N_SIDES {
                verts.push(DotVertex {
                    pos: [cx + cos_t[k] * rx, cy + sin_t[k] * ry],
                    col: c,
                });
            }
            for &o in &INDEX_OFFSETS {
                indices.push(base + o as u32);
            }
        }

        // Upload to GPU
        if !verts.is_empty() {
            queue.write_buffer(&gpu.dot_vtx_buf, 0, bytemuck::cast_slice(&verts));
            queue.write_buffer(&gpu.dot_idx_buf, 0, bytemuck::cast_slice(&indices));
        }
        queue.write_buffer(
            &gpu.fade_uniform_buf,
            0,
            bytemuck::bytes_of(&FadeUniforms { alpha: self.fade_alpha, _pad: [0.0; 3] }),
        );

        let view_ref: &wgpu::TextureViewHandle = &*gpu.accum_view;

        // Pass 1: clear (first frame only) or load
        if !gpu.cleared {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("accum_clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: view_ref,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: true,
                    },
                })],
                depth_stencil_attachment: None,
            });
            drop(pass);
            gpu.cleared = true;
        }

        // Pass 2: fade — dims existing content by (1 - fade_alpha) per frame
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("accum_fade"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: view_ref,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: true },
                })],
                depth_stencil_attachment: None,
            });
            pass.set_pipeline(&gpu.fade_pipeline);
            pass.set_bind_group(0, &gpu.fade_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        // Pass 3: subtract 1/255 per channel — forces near-zero values to exact black
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("accum_subtract"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: view_ref,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: true },
                })],
                depth_stencil_attachment: None,
            });
            pass.set_pipeline(&gpu.subtract_pipeline);
            pass.draw(0..3, 0..1);
        }

        // Pass 4: draw current dots
        if !verts.is_empty() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("accum_dots"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: view_ref,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: true },
                })],
                depth_stencil_attachment: None,
            });
            pass.set_pipeline(&gpu.dot_pipeline);
            pass.set_vertex_buffer(0, gpu.dot_vtx_buf.slice(..));
            pass.set_index_buffer(gpu.dot_idx_buf.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..indices.len() as u32, 0, 0..1);
        }
    }

    fn name(&self) -> &'static str { "cardano" }

    fn params(&self) -> &[Param] { PARAMS }

    fn hud_info(&self) -> Option<String> {
        let chord_str = self.current_chord.name();
        let theory = if chord_str.is_empty() {
            format!("tension:{:.0}%", self.current_tension * 100.0)
        } else {
            format!("{}  tension:{:.0}%", chord_str, self.current_tension * 100.0)
        };
        Some(format!(
            "{}x{} circles  {}",
            self.num_collections, self.bounds, theory
        ))
    }

    fn key_pressed(&mut self, key: Key) {
        match key {
            Key::C => {
                let next = self.num_collections % MAX_COLLECTIONS + 1;
                self.set_num_collections(next);
            }
            Key::R => {
                if let Ok(mut g) = self.gpu.try_borrow_mut() {
                    if let Some(ref mut gpu) = *g {
                        gpu.cleared = false;
                    }
                }
            }
            _ => {}
        }
    }
}
