use crate::midi::{MidiState, NoteState};
use crate::sketches::{flag_suffix, Envelope, Param, Sketch, Smoothed, Stream, StreamParams, LAYER_HUE_SHIFT};
use bytemuck::{Pod, Zeroable};
use nannou::prelude::*;
use nannou::rand::{thread_rng, Rng};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;

// hue, circles, dot radius, speed, ratio and orbit radius are kept per stream
// and follow the tune target; alignment (between the auto collections) and
// trail length (one shared trail texture) are not.
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
const P_HUE: usize = 0;
const P_CIRCLES: usize = 1;
const P_DOT_R: usize = 2;
const P_SPEED: usize = 3;
const P_RATIO: usize = 4;
const P_ORBIT_R: usize = 5;
const P_ALIGN: usize = 6;
const P_TRAIL: usize = 7;

const N_SIDES: usize = 8;
const MAX_BOUNDS: usize = 16;
const MAX_COLLECTIONS: usize = 4;
const SPRING_K: f32 = 6.0;
const SPRING_DAMP: f32 = 3.5;
const IMPULSE_SCALE: f32 = 250.0;
const KICK_STRENGTH: f32 = 0.9;   // a layer kick hits like a note at this velocity
const KICK_GLOW_SECS: f32 = 1.5;  // how long a kick shows the layer while it's off
const LAYER_ALPHA: f32 = 0.9;
const GLIDE_RATE: f32 = 4.0;      // circles glide to new spacing at this rate (1/s)
const FADE_RATE: f32 = 3.0;       // circles and collections fade in and out at this rate
const CLEAR_SECS: f32 = 0.5;      // Clear trails fades them out over this long
const CLEAR_FADE: f32 = 0.12;     // trail fade per frame while clearing
const BEAT_SWELL: f32 = 0.3;      // a full beat swells the dots by this much

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

// The auto collections plus the layer's one.
const MAX_DOT_VERTS: usize = (MAX_COLLECTIONS + 1) * MAX_BOUNDS * (N_SIDES + 1);
const MAX_DOT_IDX:   usize = (MAX_COLLECTIONS + 1) * MAX_BOUNDS * N_SIDES * 3;

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

struct Circle {
    /// Angle from the collection's phase. It glides to `target` when the
    /// count changes, so the circles spread out instead of jumping.
    offset: f32,
    target: f32,
    lerp: f32,
    deflection: Deflection,
    alpha: f32,
    dying: bool,
}

struct Collection {
    phase: f32,
    circles: Vec<Circle>,
    /// Fades the whole collection in and out as collections come and go.
    alpha: f32,
    dying: bool,
}

impl Collection {
    /// `n` evenly spaced circles, already in place and visible.
    fn new(n: usize, phase: f32, rng: &mut impl Rng) -> Self {
        let mut c = Self { phase, circles: Vec::new(), alpha: 1.0, dying: false };
        c.set_count(n, rng);
        for circle in &mut c.circles {
            circle.offset = circle.target;
            circle.alpha = 1.0;
        }
        c
    }

    fn live(&self) -> usize {
        self.circles.iter().filter(|c| !c.dying).count()
    }

    /// Heads for `n` evenly spaced circles: new ones split off the first
    /// circle and glide into place, extra ones fade out where they are.
    fn set_count(&mut self, n: usize, rng: &mut impl Rng) {
        let n = n.clamp(1, MAX_BOUNDS);
        let mut live = self.live();
        for c in self.circles.iter_mut().filter(|c| c.dying) {
            if live >= n { break; }
            c.dying = false;
            live += 1;
        }
        // Appended circles start a full turn round from the first one, so
        // the circles stay in angle order and each glides only a little.
        let start = self.circles.first().map_or(0.0, |c| c.offset) + TAU;
        while live < n {
            self.circles.push(Circle {
                offset: start,
                target: start,
                lerp: rng.gen_range(0.0f32..1.0),
                deflection: Deflection::default(),
                alpha: 0.0,
                dying: false,
            });
            live += 1;
        }
        for c in self.circles.iter_mut().filter(|c| !c.dying).skip(n) {
            c.dying = true;
        }
        let step = TAU / n as f32;
        for (k, c) in self.circles.iter_mut().filter(|c| !c.dying).enumerate() {
            c.target = k as f32 * step;
        }
    }

    /// Circles that are visible, with their absolute inner angle.
    fn angles(&self) -> impl Iterator<Item = (f32, &Circle)> {
        self.circles.iter().map(move |c| (self.phase + c.offset, c))
    }

    /// Advances the inner angles by `step`, glides the spacing and fades,
    /// and lets the springs settle.
    fn advance(&mut self, step: f32, dt: f32) {
        self.phase += step;
        let glide = 1.0 - (-GLIDE_RATE * dt).exp();
        let fade = 1.0 - (-FADE_RATE * dt).exp();
        for c in &mut self.circles {
            c.offset += (c.target - c.offset) * glide;
            c.alpha += (if c.dying { 0.0 } else { 1.0 } - c.alpha) * fade;
            let d = &mut c.deflection;
            d.velocity += (-SPRING_K * d.offset - SPRING_DAMP * d.velocity) * dt;
            d.offset   += d.velocity * dt;
        }
        self.circles.retain(|c| !(c.dying && c.alpha < 0.01));
        self.alpha += (if self.dying { 0.0 } else { 1.0 } - self.alpha) * fade;
    }

    /// Pushes every circle outward from its orbit, bent towards `bias`.
    fn kick(&mut self, angle_outer: f32, bias: Vec2, strength: f32) {
        let phase = self.phase;
        for c in &mut self.circles {
            let orbit_angle = angle_outer + phase + c.offset;
            let radial      = vec2(orbit_angle.cos(), orbit_angle.sin());
            let dir_raw     = radial + bias;
            let impulse_dir = if dir_raw.length_squared() > 0.001 {
                dir_raw.normalize()
            } else {
                radial
            };
            c.deflection.velocity += impulse_dir * strength;
        }
    }
}

// --- Cardano sketch -------------------------------------------------------

pub struct Cardano {
    angle_outer: f32,
    collections: Vec<Collection>,
    num_collections: usize,
    bounds: usize,
    /// Brightness: a beat brightens the dots, then they settle back.
    s_alpha: Smoothed,
    s_hue: Smoothed,
    s_tension: Smoothed,
    s_alignment: Smoothed,
    time: f32,
    /// Counts down from 1 while Clear trails fades them out.
    clearing: f32,
    tunes: StreamParams,
    /// The layer: one more collection, orbiting the other way in the
    /// opposite hue. It ignores the notes; it's switched on and kicked by hand.
    layer_on: bool,
    layer: Collection,
    layer_angle_outer: f32,
    kick: bool,
    s_layer: Smoothed,
    kick_glow: Envelope,
    win: Cell<Rect>,
    // Per-frame dot state passed from update() to raw_render(): position, colour, radius
    current_dots: Vec<(Vec2, LinSrgba, f32)>,
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
        Self {
            angle_outer: 0.0,
            collections: vec![Collection::new(bounds, 0.0, &mut rng)],
            num_collections: 1,
            bounds,
            s_alpha: Smoothed::new(0.75, 6.0),
            s_hue: Smoothed::new(0.0, 2.0),
            s_tension: Smoothed::new(0.0, 2.0),
            s_alignment: Smoothed::new(0.0, 3.0),
            time: 0.0,
            clearing: 0.0,
            tunes: StreamParams::new(PARAMS.len()),
            layer_on: false,
            layer: Collection::new(bounds, 0.0, &mut rng),
            layer_angle_outer: PI,
            kick: false,
            s_layer: Smoothed::new(0.0, 3.0),
            kick_glow: Envelope::new(14.0, 1.0 / KICK_GLOW_SECS),
            win: Cell::new(Rect::from_w_h(800.0, 600.0)),
            current_dots: Vec::new(),
            fade_alpha: 0.02,
            gpu: RefCell::new(None),
            current_chord: ChordQuality::Other,
            current_tension: 0.0,
        }
    }

    fn set_bounds(&mut self, n: usize) {
        self.bounds = n.clamp(1, MAX_BOUNDS);
        let mut rng = thread_rng();
        for c in &mut self.collections {
            c.set_count(self.bounds, &mut rng);
        }
    }

    /// New collections fade in at the first one's angles, for immediate
    /// symmetry; removed ones fade out.
    fn set_num_collections(&mut self, n: usize) {
        self.num_collections = n.clamp(1, MAX_COLLECTIONS);
        let mut live = self.collections.iter().filter(|c| !c.dying).count();
        for c in self.collections.iter_mut().filter(|c| c.dying) {
            if live >= self.num_collections { break; }
            c.dying = false;
            live += 1;
        }
        let phase = self.collections.first().map_or(0.0, |c| c.phase);
        let mut rng = thread_rng();
        while live < self.num_collections {
            let mut c = Collection::new(self.bounds, phase, &mut rng);
            c.alpha = 0.0;
            self.collections.push(c);
            live += 1;
        }
        let keep = self.num_collections;
        for c in self.collections.iter_mut().filter(|c| !c.dying).skip(keep) {
            c.dying = true;
        }
    }
}

impl Sketch for Cardano {
    fn update(&mut self, midi: &MidiState, dt: f32) {
        self.tunes.update(PARAMS, midi);
        self.time += dt;
        let music      = &midi.music;
        let hue1       = self.tunes.get(Stream::Auto, P_HUE);
        let hue2       = midi.pitch_bend;
        let new_bounds = self.tunes.get(Stream::Auto, P_CIRCLES).round() as usize;
        let swell      = 1.0 + BEAT_SWELL * music.pulse;
        let dot_r      = self.tunes.get(Stream::Auto, P_DOT_R) * swell;
        let speed      = self.tunes.get(Stream::Auto, P_SPEED);
        let ratio      = self.tunes.get(Stream::Auto, P_RATIO);
        let orbit_r    = self.tunes.get(Stream::Auto, P_ORBIT_R);
        let align_t    = PARAMS[P_ALIGN].read(midi);
        let trail_len  = PARAMS[P_TRAIL].read(midi);

        // Fade alpha: chosen so the trail is near-invisible after trail_len frames.
        // 1 - 0.01^(1/trail_len) gives exact "1% brightness after trail_len frames".
        self.fade_alpha = 1.0 - (0.01_f32).powf(1.0 / trail_len);
        if self.clearing > 0.0 {
            self.fade_alpha = self.fade_alpha.max(CLEAR_FADE);
            self.clearing = (self.clearing - dt / CLEAR_SECS).max(0.0);
        }

        if new_bounds != self.bounds {
            self.set_bounds(new_bounds);
        }

        // Music theory, from the steady chord: its hue and tension glide.
        self.current_chord   = detect_chord(&music.held);
        self.current_tension = self.s_tension.drive(tension_from_notes(&music.held), dt);
        let chord_hue1       = self.s_hue.drive_hue(chord_hue(self.current_chord).unwrap_or(hue1), dt);
        let alpha            = self.s_alpha.drive(0.75 + 0.25 * music.pulse, dt);

        // Dissonance makes the orbits breathe, each circle at its own pace.
        let wobble_amp = (self.current_tension - 0.2).max(0.0) * orbit_r * 0.20;

        let target_alignment = align_t * TAU / self.num_collections.max(2) as f32;
        let alignment = self.s_alignment.drive(target_alignment, dt);

        self.angle_outer += speed * dt;

        for coll in &mut self.collections {
            coll.advance(-speed * ratio * dt, dt);
        }
        self.collections.retain(|c| !(c.dying && c.alpha < 0.01));

        let mut rng = thread_rng();

        // Layer: turns the other way round, so it reads as its own figure.
        let l_speed  = self.tunes.get(Stream::Layer, P_SPEED);
        let l_bounds = (self.tunes.get(Stream::Layer, P_CIRCLES).round() as usize).clamp(1, MAX_BOUNDS);
        if l_bounds != self.layer.live() {
            self.layer.set_count(l_bounds, &mut rng);
        }
        self.layer_angle_outer -= l_speed * dt;
        self.layer.advance(l_speed * self.tunes.get(Stream::Layer, P_RATIO) * dt, dt);
        if std::mem::take(&mut self.kick) {
            self.layer.kick(self.layer_angle_outer, Vec2::ZERO, KICK_STRENGTH * IMPULSE_SCALE);
            self.kick_glow.trigger(1.0);
        }
        let glow = self.kick_glow.step(dt);
        let layer_on = self.s_layer.drive(if self.layer_on { 1.0 } else { 0.0 }, dt);

        // Compute current dot positions and colours (no trail storage)
        self.current_dots.clear();
        for (ci, coll) in self.collections.iter().enumerate() {
            let coll_shift  = ci as f32 / MAX_COLLECTIONS as f32;
            let outer_angle = self.angle_outer + ci as f32 * alignment;
            let ox = orbit_r * outer_angle.cos();
            let oy = orbit_r * outer_angle.sin();
            for (j, (a, c)) in coll.angles().enumerate() {
                let wobble = (self.time * 3.1 + j as f32 * 1.7 + ci as f32 * 2.3).sin();
                let r   = orbit_r + wobble_amp * wobble;
                let pos = vec2(ox + r * a.cos(), oy + r * a.sin()) + c.deflection.offset;
                let hue = (chord_hue1 + c.lerp * (hue2 - chord_hue1) + coll_shift).rem_euclid(1.0);
                let a   = alpha * c.alpha * coll.alpha;
                self.current_dots.push((pos, hsl_to_lin(hue, 0.85, 0.55, a), dot_r));
            }
        }

        // A kick shows the layer for a moment even while it's off.
        let layer_alpha = LAYER_ALPHA * layer_on.max(glow);
        if layer_alpha > 0.01 {
            let l_hue1  = self.tunes.get(Stream::Layer, P_HUE);
            let l_orbit = self.tunes.get(Stream::Layer, P_ORBIT_R);
            let l_dot_r = self.tunes.get(Stream::Layer, P_DOT_R);
            let ox = l_orbit * self.layer_angle_outer.cos();
            let oy = l_orbit * self.layer_angle_outer.sin();
            for (a, c) in self.layer.angles() {
                let pos = vec2(ox + l_orbit * a.cos(), oy + l_orbit * a.sin()) + c.deflection.offset;
                let hue = (l_hue1 + c.lerp * (hue2 - l_hue1) + LAYER_HUE_SHIFT).rem_euclid(1.0);
                self.current_dots.push((pos, hsl_to_lin(hue, 0.85, 0.55, layer_alpha * c.alpha), l_dot_r));
            }
        }

        // Auto stream: every hit kicks the collections, bent up or down by pitch.
        for ev in &music.hits {
            let pitch_bias = vec2(0.0, ev.note as f32 / 127.0 * 2.0 - 1.0);
            for coll in &mut self.collections {
                coll.kick(self.angle_outer, pitch_bias, ev.velocity * IMPULSE_SCALE);
            }
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
        // Build dot vertex/index data for this frame's positions only
        let mut verts: Vec<DotVertex>  = Vec::with_capacity(self.current_dots.len() * (N_SIDES + 1));
        let mut indices: Vec<u32>      = Vec::with_capacity(self.current_dots.len() * N_SIDES * 3);

        for &(pos, col, dot_r) in &self.current_dots {
            let rx  = dot_r / half_w;  // radius in NDC x
            let ry  = dot_r / half_h;  // radius in NDC y (separate to keep circles round)
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
        let mut flags = Vec::new();
        if self.layer_on { flags.push("layer"); }
        flags.extend(self.tunes.hud_flag());
        Some(format!(
            "{}x{} circles  {}{}",
            self.num_collections, self.bounds, theory, flag_suffix(&flags)
        ))
    }

    fn actions(&self) -> &[(Key, &'static str)] {
        &[
            (Key::G, "Layer"),
            (Key::B, "Kick"),
            (Key::T, "Tune: both / auto / layer"),
            (Key::R, "Clear trails"),
            (Key::C, "Collections"),
        ]
    }

    fn key_pressed(&mut self, key: Key) {
        match key {
            Key::G => self.layer_on = !self.layer_on,
            Key::B => self.kick = true,
            Key::T => self.tunes.target = self.tunes.target.next(),
            Key::C => {
                let next = self.num_collections % MAX_COLLECTIONS + 1;
                self.set_num_collections(next);
            }
            Key::R => self.clearing = 1.0,
            _ => {}
        }
    }
}
