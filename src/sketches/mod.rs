pub mod aurora;
pub mod droplets;
pub mod grid;
pub mod particles;

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

pub trait Sketch {
    fn update(&mut self, midi: &MidiState, dt: f32);
    fn view(&self, draw: &Draw, win: Rect);
    fn name(&self) -> &'static str;
    fn params(&self) -> &[Param] { &[] }
    fn hud_info(&self) -> Option<String> { None }
    fn key_pressed(&mut self, _key: Key) {}
}

pub type SketchFactory = fn() -> Box<dyn Sketch>;

pub fn registry() -> Vec<(&'static str, SketchFactory)> {
    vec![
        ("aurora",    || Box::new(aurora::Aurora::new())),
        ("droplets",  || Box::new(droplets::Droplets::new())),
        ("grid",      || Box::new(grid::Grid::new())),
        ("particles", || Box::new(particles::Particles::new())),
    ]
}
