use crate::midi::{MidiState, NoteState};
use crate::sketches::{flag_suffix, Param, Sketch, Smoothed, Stream, StreamParams, LAYER_HUE_SHIFT};
use bytemuck::{Pod, Zeroable};
use nannou::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;

// hue, speed, freq and warp are kept per stream and follow the tune target;
// smooth shapes both.
const PARAMS: &[Param] = &[
    Param::new(1,  "hue",    0.0,  1.0),
    Param::new(24, "speed",  0.1,  3.0),
    Param::new(25, "freq",   5.0, 35.0),
    Param::new(26, "warp",   0.0,  1.4),
    Param::new(27, "smooth", 0.0,  1.0),  // 0=punchy, 1=dreamy
];
const P_HUE: usize = 0;
const P_SPEED: usize = 1;
const P_FREQ: usize = 2;
const P_WARP: usize = 3;
const P_SMOOTH: usize = 4;

const ENERGY_WINDOW: f32 = 4.0;
const ENERGY_PEAK:   f32 = 20.0;
const TEX_FMT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

// ── WGSL shader ───────────────────────────────────────────────────────────────

const KAL_WGSL: &str = r#"
struct Uniforms {
    resolution:   vec2<f32>,
    time:         f32,
    level:        f32,
    freq:         f32,
    warp_str:     f32,
    n_sym:        f32,
    speed:        f32,
    onset:        f32,
    hue:          f32,
    sat:          f32,
    color_mode:   f32,
    moire_mix:    f32,
    moire_delta:  f32,
    pattern_mode: f32,
    threshold:    f32,
    layer_mix:    f32,
    layer_freq:   f32,
    layer_warp:   f32,
    layer_speed:  f32,
    layer_onset:  f32,
    layer_hue:    f32,
    _pad0:        f32,
    _pad1:        f32,
}
@group(0) @binding(0) var<uniform> u: Uniforms;

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    var xy = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    return vec4<f32>(xy[vi], 0.0, 1.0);
}

fn hash22(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let s = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(hash22(i),                    hash22(i + vec2<f32>(1.0, 0.0)), s.x),
        mix(hash22(i + vec2<f32>(0.0, 1.0)), hash22(i + vec2<f32>(1.0, 1.0)), s.x),
        s.y
    );
}

fn fbm(p_in: vec2<f32>) -> f32 {
    var p = p_in;
    var a = 0.5;
    var s = 0.0;
    for (var i = 0u; i < 5u; i++) {
        s += a * noise(p);
        p  = p * 2.02;
        a *= 0.5;
    }
    return s;
}

// Fold the plane into a single kaleidoscope sector.
// ang → [0, TAU/n), then mirror about the sector midline.
fn kfold(uv_in: vec2<f32>, n: f32) -> vec2<f32> {
    let ang    = atan2(uv_in.y, uv_in.x);
    let rad    = length(uv_in);
    let sector = 6.28318530 / n;
    let a_mod  = ((ang % sector) + sector) % sector;  // always in [0, sector)
    let a      = abs(a_mod - sector * 0.5);            // mirror about midline
    return vec2<f32>(cos(a), sin(a)) * rad;
}

// Returns a value in [-1, 1] representing the pattern at warped uv.
fn base_pattern(uv: vec2<f32>, freq: f32, t: f32, warp: f32, mode: u32, speed: f32) -> f32 {
    // Two-pass domain warp (Quilez style)
    let q = vec2<f32>(
        fbm(uv * 2.0 + vec2<f32>(0.0,  t * 0.05)),
        fbm(uv * 2.0 + vec2<f32>(5.2, -t * 0.04))
    );
    let w = uv + warp * q;

    // Pattern shape: switch one line to change the whole character
    var r: f32;
    switch mode {
        case 1u: { r = w.x; }                                   // stripes
        case 2u: { r = (w.x + w.y) * 0.7071; }                  // diagonal bars
        case 3u: { r = sin(freq * 0.5 * w.x) * sin(freq * 0.5 * w.y); } // checker
        default: { r = length(w); }                              // rings
    }
    return sin(freq * r - t * speed + 6.0 * fbm(w * 1.5));
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> vec3<f32> {
    let c  = (1.0 - abs(2.0 * l - 1.0)) * s;
    let hp = h * 6.0;
    let x  = c * (1.0 - abs(hp % 2.0 - 1.0));
    let m  = l - c * 0.5;
    var rgb: vec3<f32>;
    if      hp < 1.0 { rgb = vec3<f32>(c, x, 0.0); }
    else if hp < 2.0 { rgb = vec3<f32>(x, c, 0.0); }
    else if hp < 3.0 { rgb = vec3<f32>(0.0, c, x); }
    else if hp < 4.0 { rgb = vec3<f32>(0.0, x, c); }
    else if hp < 5.0 { rgb = vec3<f32>(x, 0.0, c); }
    else             { rgb = vec3<f32>(c, 0.0, x); }
    return rgb + m;
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv_raw = (pos.xy - 0.5 * u.resolution) / u.resolution.y;
    let uv     = kfold(uv_raw, u.n_sym);

    // Audio modulates the two main levers
    let freq = u.freq    + u.onset * 4.0;
    let warp = u.warp_str + u.onset * 0.5 + u.level * 0.3;
    let mode = u32(u.pattern_mode);

    // Primary pattern
    let f0 = base_pattern(uv, freq, u.time, warp, mode, u.speed);

    // Moiré: second pass at a slightly different scale and frequency
    let uv1  = kfold(uv_raw * 1.004, u.n_sym);
    let f1   = base_pattern(uv1, freq + u.moire_delta, u.time, warp, mode, u.speed);
    let moir = abs(f0 - f1);

    let raw  = mix(f0, moir, u.moire_mix);
    let mask = smoothstep(-u.threshold, u.threshold, raw);

    // Color modes
    let bw  = vec3<f32>(mask);
    let col = hsl_to_rgb(u.hue + mask * 0.15, u.sat, 0.3 + mask * 0.5);
    let inv = vec3<f32>(1.0 - mask);

    var out: vec3<f32>;
    switch u32(u.color_mode) {
        case 1u: { out = col; }
        case 2u: { out = inv; }
        default: { out = bw; }
    }

    // Layer: the contour lines of a second pattern with its own knobs,
    // drawn over the first in the opposite hue. A surge thickens and
    // brightens them.
    if (u.layer_mix > 0.001) {
        let lf    = u.layer_freq + u.layer_onset * 6.0;
        let lw    = u.layer_warp + u.layer_onset * 0.6;
        let fl    = base_pattern(uv, lf, u.time, lw, mode, u.layer_speed);
        let width = 0.10 + u.layer_onset * 0.25;
        let line  = 1.0 - smoothstep(0.0, width, abs(fl));
        let lcol  = hsl_to_rgb(fract(u.layer_hue), u.sat, 0.55 + u.layer_onset * 0.3);
        out = mix(out, lcol, line * u.layer_mix);
    }
    return vec4<f32>(out, 1.0);
}
"#;

// ── Uniform buffer (must match WGSL struct byte-for-byte) ─────────────────────

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct KalUniforms {
    resolution:   [f32; 2],  // vec2<f32>: 8 bytes at offset 0
    time:         f32,
    level:        f32,
    freq:         f32,
    warp_str:     f32,
    n_sym:        f32,
    speed:        f32,
    onset:        f32,
    hue:          f32,
    sat:          f32,
    color_mode:   f32,
    moire_mix:    f32,
    moire_delta:  f32,
    pattern_mode: f32,
    threshold:    f32,
    layer_mix:    f32,
    layer_freq:   f32,
    layer_warp:   f32,
    layer_speed:  f32,
    layer_onset:  f32,
    layer_hue:    f32,
    _pad:         [f32; 2],  // keeps the size a multiple of 16
}
const _: () = assert!(std::mem::size_of::<KalUniforms>() == 96);

// ── Chord detection ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum ChordKind { Major, Minor, Diminished, Augmented, Dom7th, Other }

impl ChordKind {
    fn n_sym(self) -> f32 {
        match self {
            Self::Major      =>  8.0,
            Self::Minor      =>  6.0,
            Self::Diminished => 12.0,
            Self::Augmented  =>  4.0,
            Self::Dom7th     => 16.0,
            Self::Other      =>  5.0,
        }
    }

    fn hue(self, base: f32) -> f32 {
        match self {
            Self::Major      => 0.10,
            Self::Minor      => 0.65,
            Self::Diminished => 0.80,
            Self::Augmented  => 0.48,
            Self::Dom7th     => 0.98,
            Self::Other      => base,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Major      => "Maj  8×",
            Self::Minor      => "Min  6×",
            Self::Diminished => "Dim 12×",
            Self::Augmented  => "Aug  4×",
            Self::Dom7th     => "Dom7 16×",
            Self::Other      => "—",
        }
    }
}

fn detect_chord(notes: &[NoteState; 128]) -> ChordKind {
    let mut classes: Vec<u8> = (0u8..128)
        .filter(|&n| notes[n as usize].on)
        .map(|n| n % 12)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    classes.sort_unstable();
    if classes.len() < 3 { return ChordKind::Other; }
    let root = classes[0];
    let ivs: Vec<u8> = classes.iter().map(|&c| (c + 12 - root) % 12).collect();
    let has = |n: u8| ivs.contains(&n);
    if has(4) && has(7) && has(10) { return ChordKind::Dom7th; }
    if has(3) && has(6)            { return ChordKind::Diminished; }
    if has(4) && has(8)            { return ChordKind::Augmented; }
    if has(4) && has(7)            { return ChordKind::Major; }
    if has(3) && has(7)            { return ChordKind::Minor; }
    ChordKind::Other
}

fn tension(notes: &[NoteState; 128]) -> f32 {
    let held: Vec<u8> = (0..128u8).filter(|&n| notes[n as usize].on).collect();
    if held.len() < 2 { return 0.0; }
    let min_c = held
        .iter()
        .enumerate()
        .flat_map(|(i, &a)| held[i + 1..].iter().map(move |&b| consonance(b.wrapping_sub(a))))
        .fold(1.0f32, f32::min);
    1.0 - min_c
}

fn consonance(s: u8) -> f32 {
    match s % 12 {
        0  => 1.00, 7  => 0.85, 5  => 0.80,
        4  => 0.70, 3  => 0.65, 9  => 0.60,
        8  => 0.55, 2  => 0.35, 10 => 0.30,
        11 => 0.15, 1  => 0.10, 6  => 0.00,
        _  => 0.50,
    }
}

// ── GPU state ─────────────────────────────────────────────────────────────────

struct GpuKal {
    pipeline:    wgpu::RenderPipeline,
    uniform_buf: wgpu::Buffer,
    bind_group:  wgpu::BindGroup,
    tex:         wgpu::Texture,
    tex_view:    wgpu::TextureView,
    win_size:    [u32; 2],
}

impl GpuKal {
    fn new(device: &wgpu::Device, w: u32, h: u32) -> Self {
        let tex = wgpu::TextureBuilder::new()
            .size([w, h])
            .format(TEX_FMT)
            .usage(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING)
            .build(device);
        let tex_view = tex.view().build();

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label:  Some("kaleidoscope"),
            source: wgpu::ShaderSource::Wgsl(KAL_WGSL.into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label:   None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding:    0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty:         wgpu::BindingType::Buffer {
                    ty:                 wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size:   wgpu::BufferSize::new(
                        std::mem::size_of::<KalUniforms>() as u64,
                    ),
                },
                count: None,
            }],
        });

        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label:              Some("kal_uniforms"),
            size:               std::mem::size_of::<KalUniforms>() as u64,
            usage:              wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label:   None,
            layout:  &bgl,
            entries: &[wgpu::BindGroupEntry {
                binding:  0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });

        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label:                None,
            bind_group_layouts:   &[&bgl],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label:  Some("kaleidoscope"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module:      &shader,
                entry_point: "vs_main",
                buffers:     &[],
            },
            fragment: Some(wgpu::FragmentState {
                module:      &shader,
                entry_point: "fs_main",
                targets:     &[Some(wgpu::ColorTargetState {
                    format:     TEX_FMT,
                    blend:      None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive:     wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample:   wgpu::MultisampleState::default(),
            multiview:     None,
        });

        Self { pipeline, uniform_buf, bind_group, tex, tex_view, win_size: [w, h] }
    }
}

// ── Sketch ────────────────────────────────────────────────────────────────────

pub struct Kaleidoscope {
    chord:           ChordKind,
    energy:          f32,
    onset_raw:       f32,  // raw additive impulse accumulator
    note_timestamps: Vec<f32>,
    last_pitch_mask: u16,
    time:            f32,

    // Smoothed values — all driven toward targets each frame via IIR filter
    s_hue:     Smoothed,  // chord hue      (rate 2.0 → graceful color fade)
    s_n_sym:   Smoothed,  // fold count     (rate 2.5 → visible geometry morph)
    s_tension: Smoothed,  // tension        (rate 2.0 → smooth chord transitions)
    s_freq:    Smoothed,  // CC freq knob   (rate 6.0 → snappy, no 7-bit staircase)
    s_speed:   Smoothed,  // CC speed knob
    s_warp:    Smoothed,  // CC warp knob
    s_onset:   Smoothed,  // smoothed onset — rate set dynamically by smooth knob
    s_smooth:  Smoothed,  // CC smooth knob: 0=punchy, 1=dreamy

    // Layer: a second pattern played by hand, with its own knob values
    tunes:           StreamParams,
    layer_on:        bool,
    layer_onset_raw: f32,       // surge impulse, decays like the note onset
    s_layer_mix:     Smoothed,  // fades the layer in and out
    s_layer_onset:   Smoothed,
    s_layer_hue:     Smoothed,
    s_layer_freq:    Smoothed,
    s_layer_speed:   Smoothed,
    s_layer_warp:    Smoothed,

    color_mode:   u32,  // 0=B&W  1=color  2=inverted
    moire_on:     bool,
    pattern_mode: u32,  // 0=rings  1=stripes  2=diagonal  3=checker

    gpu: RefCell<Option<GpuKal>>,
    win: Cell<Rect>,
}

impl Kaleidoscope {
    pub fn new() -> Self {
        Self {
            chord:           ChordKind::Other,
            energy:          0.0,
            onset_raw:       0.0,
            note_timestamps: Vec::new(),
            last_pitch_mask: 0,
            time:            0.0,

            s_hue:     Smoothed::new(0.0,  2.0),
            s_n_sym:   Smoothed::new(5.0,  2.5),
            s_tension: Smoothed::new(0.0,  2.0),
            s_freq:    Smoothed::new(18.0, 6.0),
            s_speed:   Smoothed::new(0.8,  6.0),
            s_warp:    Smoothed::new(0.6,  6.0),
            s_onset:   Smoothed::new(0.0,  4.0),
            s_smooth:  Smoothed::new(0.0,  6.0),

            tunes:           StreamParams::new(PARAMS.len()),
            layer_on:        false,
            layer_onset_raw: 0.0,
            s_layer_mix:     Smoothed::new(0.0,  3.0),
            s_layer_onset:   Smoothed::new(0.0,  4.0),
            s_layer_hue:     Smoothed::new(0.5,  2.0),
            s_layer_freq:    Smoothed::new(18.0, 6.0),
            s_layer_speed:   Smoothed::new(0.8,  6.0),
            s_layer_warp:    Smoothed::new(0.6,  6.0),

            color_mode:   0,
            moire_on:     false,
            pattern_mode: 0,

            gpu: RefCell::new(None),
            win: Cell::new(Rect::from_w_h(800.0, 600.0)),
        }
    }

    fn pitch_mask(notes: &[NoteState; 128]) -> u16 {
        (0u8..128)
            .filter(|&n| notes[n as usize].on)
            .fold(0u16, |m, n| m | (1 << (n % 12)))
    }
}

impl Sketch for Kaleidoscope {
    fn name(&self) -> &'static str { "kaleidoscope" }
    fn params(&self) -> &[Param] { PARAMS }

    fn update(&mut self, midi: &MidiState, dt: f32) {
        self.time += dt;

        self.tunes.update(PARAMS, midi);

        // Read smooth knob first so its value informs this frame's rates.
        self.s_smooth.drive(PARAMS[P_SMOOTH].read(midi), dt);
        let smooth = self.s_smooth.value; // 0=punchy, 1=dreamy

        // CC knobs — IIR filter removes 7-bit MIDI staircase artifacts
        let (auto, layer) = (Stream::Auto, Stream::Layer);
        self.s_speed.drive(self.tunes.get(auto, P_SPEED), dt);
        self.s_freq.drive(self.tunes.get(auto, P_FREQ),   dt);
        self.s_warp.drive(self.tunes.get(auto, P_WARP),   dt);
        self.s_layer_speed.drive(self.tunes.get(layer, P_SPEED), dt);
        self.s_layer_freq.drive(self.tunes.get(layer, P_FREQ),   dt);
        self.s_layer_warp.drive(self.tunes.get(layer, P_WARP),   dt);
        self.s_layer_hue.drive(self.tunes.get(layer, P_HUE) + LAYER_HUE_SHIFT, dt);
        self.s_layer_mix.drive(if self.layer_on { 1.0 } else { 0.0 }, dt);

        // Onset: additive impulse on each note, then exponential decay.
        // The raw value is then routed through s_onset so the RISE is also
        // smooth — no more instant visual spike on each keypress.
        for _ in midi.note_on_events() {
            self.note_timestamps.push(self.time);
            self.onset_raw = (self.onset_raw + 0.8).min(1.0);
        }
        // smooth knob controls decay rate: 5.0 (quick, punchy) → 1.5 (slow swell)
        let onset_decay = 5.0 - smooth * 3.5;
        self.onset_raw *= (-dt * onset_decay).exp();

        // s_onset chases onset_raw — this smooths the rise, not just the fall.
        // Rate also scaled by smooth knob: 4.0 (quick) → 1.5 (dreamy)
        self.s_onset.rate = 4.0 - smooth * 2.5;
        self.s_onset.drive(self.onset_raw, dt);

        // The layer's surge rises and falls the same way.
        self.layer_onset_raw *= (-dt * onset_decay).exp();
        self.s_layer_onset.rate = self.s_onset.rate;
        self.s_layer_onset.drive(self.layer_onset_raw, dt);

        // Energy: note density over ENERGY_WINDOW seconds, tracked more lazily
        // than before (rate 2.0 vs 3.0 — softer peaks when playing fast).
        self.note_timestamps.retain(|&t| self.time - t < ENERGY_WINDOW);
        let target_e = (self.note_timestamps.len() as f32 / ENERGY_PEAK).min(1.0);
        self.energy += (target_e - self.energy) * (1.0 - (-2.0_f32 * dt).exp());

        // Tension at rate 2.0 — smooth enough not to flicker between notes
        self.s_tension.drive(tension(&midi.notes), dt);

        let cur_mask = Self::pitch_mask(&midi.notes);
        if cur_mask != self.last_pitch_mask {
            self.last_pitch_mask = cur_mask;
            let new_chord = detect_chord(&midi.notes);
            if new_chord != ChordKind::Other { self.chord = new_chord; }
        }

        // n_sym and hue smoothly chase the chord target each frame.
        // Because n_sym is a float in kfold(), values like 6.3 or 7.7 produce
        // real intermediate geometries — the morph is a visible transition.
        self.s_n_sym.drive(self.chord.n_sym(), dt);
        self.s_hue.drive(self.chord.hue(self.tunes.get(auto, P_HUE)), dt);
    }

    fn view(&self, draw: &Draw, win: Rect) {
        self.win.set(win);
        match self.gpu.borrow().as_ref() {
            Some(gpu) => { draw.texture(&gpu.tex).w_h(win.w(), win.h()); }
            None      => { draw.background().color(BLACK); }
        }
    }

    fn hud_info(&self) -> Option<String> {
        let mode_str = ["B&W", "color", "inv"][self.color_mode as usize];
        let pat_str  = ["rings", "stripes", "diag", "checker"][self.pattern_mode as usize];
        let mut flags = Vec::new();
        if self.layer_on { flags.push("layer"); }
        flags.extend(self.tunes.hud_flag());
        Some(format!(
            "{} | e:{:.0}% t:{:.0}% smooth:{:.0}% | {} {} {}{}",
            self.chord.label(),
            self.energy * 100.0,
            self.s_tension.value * 100.0,
            self.s_smooth.value * 100.0,
            mode_str,
            pat_str,
            if self.moire_on { "moiré" } else { "" },
            flag_suffix(&flags),
        ))
    }

    fn actions(&self) -> &[(Key, &'static str)] {
        &[
            (Key::G, "Layer"),
            (Key::B, "Surge"),
            (Key::T, "Tune: both / auto / layer"),
            (Key::C, "Colour mode"),
            (Key::M, "Moiré"),
            (Key::P, "Pattern"),
        ]
    }

    fn key_pressed(&mut self, key: Key) {
        match key {
            Key::G => self.layer_on = !self.layer_on,
            Key::B => self.layer_onset_raw = 1.0,
            Key::T => self.tunes.target = self.tunes.target.next(),
            Key::C => self.color_mode   = (self.color_mode   + 1) % 3,
            Key::M => self.moire_on     = !self.moire_on,
            Key::P => self.pattern_mode = (self.pattern_mode + 1) % 4,
            _ => {}
        }
    }

    fn raw_render(
        &self,
        device: &wgpu::Device,
        queue:  &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        _target: &wgpu::TextureViewHandle,
        win: Rect,
    ) {
        let w = win.w() as u32;
        let h = win.h() as u32;

        let mut gpu_opt = self.gpu.borrow_mut();
        if gpu_opt.is_none() || gpu_opt.as_ref().map_or(false, |g| g.win_size != [w, h]) {
            *gpu_opt = Some(GpuKal::new(device, w, h));
        }
        let gpu = gpu_opt.as_mut().unwrap();

        queue.write_buffer(
            &gpu.uniform_buf,
            0,
            bytemuck::bytes_of(&KalUniforms {
                resolution:   [win.w(), win.h()],
                time:         self.time,
                level:        self.energy,
                freq:         self.s_freq.value + self.s_tension.value * 6.0,
                warp_str:     self.s_warp.value,
                n_sym:        self.s_n_sym.value,
                speed:        self.s_speed.value,
                onset:        self.s_onset.value,
                hue:          self.s_hue.value,
                sat:          0.85,
                color_mode:   self.color_mode as f32,
                moire_mix:    if self.moire_on { 0.7 } else { 0.0 },
                moire_delta:  2.0,
                pattern_mode: self.pattern_mode as f32,
                threshold:    0.015,
                // A surge shows the layer for a moment even while it's off.
                layer_mix:    self.s_layer_mix.value.max(self.s_layer_onset.value),
                layer_freq:   self.s_layer_freq.value,
                layer_warp:   self.s_layer_warp.value,
                layer_speed:  self.s_layer_speed.value,
                layer_onset:  self.s_layer_onset.value,
                layer_hue:    self.s_layer_hue.value.rem_euclid(1.0),
                _pad:         [0.0; 2],
            }),
        );

        let view_ref: &wgpu::TextureViewHandle = &*gpu.tex_view;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("kaleidoscope"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view:           view_ref,
                resolve_target: None,
                ops:            wgpu::Operations {
                    load:  wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: true,
                },
            })],
            depth_stencil_attachment: None,
        });
        pass.set_pipeline(&gpu.pipeline);
        pass.set_bind_group(0, &gpu.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
