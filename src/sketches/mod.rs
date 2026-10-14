pub mod cardano;
pub mod chess;
pub mod droplets;
pub mod fireworks;
pub mod kaleidoscope;
pub mod mandala;
pub mod particles;
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

pub trait Sketch {
    fn update(&mut self, midi: &MidiState, dt: f32);
    fn view(&self, draw: &Draw, win: Rect);
    fn name(&self) -> &'static str;
    fn params(&self) -> &[Param] { &[] }
    fn hud_info(&self) -> Option<String> { None }
    fn key_pressed(&mut self, _key: Key) {}
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
        ("chess",         || Box::new(chess::Chess::new())),
        ("droplets",      || Box::new(droplets::Droplets::new())),
        ("fireworks",     || Box::new(fireworks::Fireworks::new())),
        ("kaleidoscope",  || Box::new(kaleidoscope::Kaleidoscope::new())),
        ("mandala",       || Box::new(mandala::Mandala::new())),
        ("particles",     || Box::new(particles::Particles::new())),
        ("rings",         || Box::new(rings::Rings::new())),
    ]
}
