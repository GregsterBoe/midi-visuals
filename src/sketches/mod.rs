pub mod cardano;
pub mod droplets;
pub mod fireworks;
pub mod kaleidoscope;
pub mod mandala;
pub mod rings;
pub mod zodiac_points;

use crate::midi::MidiState;
use nannou::prelude::*;

pub struct Param {
    pub cc: u8,
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
}

impl Param {
    pub const fn new(cc: u8, name: &'static str, min: f32, max: f32) -> Self {
        Self { cc, name, min, max }
    }

    pub fn read(&self, midi: &MidiState) -> f32 {
        self.read_from(&midi.ccs)
    }

    pub fn read_from(&self, ccs: &[f32]) -> f32 {
        let t = ccs[self.cc as usize];
        self.min + t * (self.max - self.min)
    }
}

/// First-order IIR low-pass filter for smooth audio/MIDI reactivity.
///
/// `rate` = 1/τ, so:
///   rate 1.0 → 63% convergence per second  (very slow, buttery)
///   rate 3.0 → 63% in ~333ms               (slow, good for chord hue/n_sym)
///   rate 5.0 → 63% in 200ms                (snappy, good for CC knobs)
///   rate 10.0 → 63% in 100ms               (fast, near-instant)
///
/// Use `drive()` each frame; `snap()` to hard-set with no lag (e.g. on reset).
pub struct Smoothed {
    pub value: f32,
    pub rate:  f32,
}

impl Smoothed {
    pub const fn new(value: f32, rate: f32) -> Self { Self { value, rate } }

    /// Advance toward `target` by one frame and return the new value.
    pub fn drive(&mut self, target: f32, dt: f32) -> f32 {
        let alpha = 1.0 - (-self.rate * dt).exp();
        self.value += alpha * (target - self.value);
        self.value
    }

    /// Teleport to `value` with no smoothing lag.
    #[allow(dead_code)]
    pub fn snap(&mut self, value: f32) { self.value = value; }
}

/// The two streams of a layered sketch: `Auto` is drawn from the notes
/// played (MIDI or audio), `Layer` is played by hand on top of it through
/// keys, panel buttons and knobs, in the opposite colour.
#[derive(Clone, Copy, PartialEq)]
pub enum Stream {
    Auto,
    Layer,
}

/// Which stream the knobs tune.
#[derive(Clone, Copy, PartialEq)]
pub enum TuneTarget {
    Both,
    Auto,
    Layer,
}

impl TuneTarget {
    pub fn next(self) -> Self {
        match self {
            TuneTarget::Both => TuneTarget::Auto,
            TuneTarget::Auto => TuneTarget::Layer,
            TuneTarget::Layer => TuneTarget::Both,
        }
    }
}

/// A value per stream for each of a sketch's params. A knob that moves sets
/// the streams `target` picks; the other stream keeps the value it had.
pub struct StreamParams {
    pub target: TuneTarget,
    auto: Vec<f32>,
    layer: Vec<f32>,
    /// Knob positions last frame; NaN until the first frame, so both
    /// streams start from the knobs.
    seen: Vec<f32>,
}

impl StreamParams {
    pub fn new(n: usize) -> Self {
        Self { target: TuneTarget::Both, auto: vec![0.0; n], layer: vec![0.0; n], seen: vec![f32::NAN; n] }
    }

    pub fn update(&mut self, params: &[Param], midi: &MidiState) {
        for (i, p) in params.iter().enumerate() {
            let raw = midi.ccs[p.cc as usize];
            if raw == self.seen[i] { continue; }
            let first = self.seen[i].is_nan();
            let v = p.read(midi);
            if first || self.target != TuneTarget::Layer { self.auto[i] = v; }
            if first || self.target != TuneTarget::Auto { self.layer[i] = v; }
            self.seen[i] = raw;
        }
    }

    pub fn get(&self, stream: Stream, i: usize) -> f32 {
        match stream {
            Stream::Auto => self.auto[i],
            Stream::Layer => self.layer[i],
        }
    }

    /// HUD flag while the knobs tune only one stream.
    pub fn hud_flag(&self) -> Option<&'static str> {
        match self.target {
            TuneTarget::Both => None,
            TuneTarget::Auto => Some("tune auto"),
            TuneTarget::Layer => Some("tune layer"),
        }
    }
}

/// How far round the colour wheel (in turns) the layer sits from its hue
/// knob, so it is drawn opposite the auto stream while both share a hue.
pub const LAYER_HUE_SHIFT: f32 = 0.5;

/// "  [rain+tune auto]"-style suffix for a HUD line; empty without flags.
pub fn flag_suffix(flags: &[&str]) -> String {
    if flags.is_empty() { String::new() } else { format!("  [{}]", flags.join("+")) }
}

pub trait Sketch {
    fn update(&mut self, midi: &MidiState, dt: f32);
    fn view(&self, draw: &Draw, win: Rect);
    fn name(&self) -> &'static str;
    fn params(&self) -> &[Param] { &[] }
    fn hud_info(&self) -> Option<String> { None }
    fn key_pressed(&mut self, _key: Key) {}
    /// Keys `key_pressed` handles, with labels for the control panel's buttons.
    fn actions(&self) -> &[(Key, &'static str)] { &[] }
    /// Optional raw wgpu render pass called after draw.to_frame(). Default is a no-op.
    fn raw_render(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _encoder: &mut wgpu::CommandEncoder,
        _target: &wgpu::TextureViewHandle,
        _win: Rect,
    ) {}
}

pub type SketchFactory = fn() -> Box<dyn Sketch>;

pub fn registry() -> Vec<(&'static str, SketchFactory)> {
    vec![
        ("cardano",       || Box::new(cardano::Cardano::new())),
        ("droplets",      || Box::new(droplets::Droplets::new())),
        ("fireworks",     || Box::new(fireworks::Fireworks::new())),
        ("kaleidoscope",  || Box::new(kaleidoscope::Kaleidoscope::new())),
        ("mandala",       || Box::new(mandala::Mandala::new())),
        ("rings",         || Box::new(rings::Rings::new())),
    ]
}
