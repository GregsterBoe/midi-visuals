pub mod aurora;
pub mod grid;

use crate::midi::MidiState;
use nannou::prelude::*;

pub trait Sketch {
    fn update(&mut self, midi: &MidiState, dt: f32);
    fn view(&self, draw: &Draw, win: Rect);
    fn name(&self) -> &'static str;
}

pub type SketchFactory = fn() -> Box<dyn Sketch>;

pub fn registry() -> Vec<(&'static str, SketchFactory)> {
    vec![
        ("aurora", || Box::new(aurora::Aurora::new())),
        ("grid",   || Box::new(grid::Grid::new())),
    ]
}
