use crate::midi::MidiState;
use crate::sketches::{Param, Sketch};
use nannou::prelude::*;

const PARAMS: &[Param] = &[
    Param::new(24, "hue",        0.0,   1.0),
    Param::new(25, "radius",    40.0, 300.0),
    Param::new(26, "saturation", 0.5,   1.0),
    Param::new(27, "lightness",  0.3,   0.7),
];

pub struct Aurora {
    hue: f32,
    radius: f32,
    saturation: f32,
    lightness: f32,
}

impl Aurora {
    pub fn new() -> Self {
        Self { hue: 0.5, radius: 100.0, saturation: 0.8, lightness: 0.5 }
    }
}

impl Sketch for Aurora {
    fn update(&mut self, midi: &MidiState, _dt: f32) {
        self.hue        = PARAMS[0].read(midi);
        self.radius     = PARAMS[1].read(midi);
        self.saturation = PARAMS[2].read(midi);
        self.lightness  = PARAMS[3].read(midi);
    }

    fn view(&self, draw: &Draw, _win: Rect) {
        draw.ellipse()
            .x_y(0.0, 0.0)
            .radius(self.radius)
            .color(hsl(self.hue, self.saturation, self.lightness));
    }

    fn name(&self) -> &'static str { "aurora" }

    fn params(&self) -> &[Param] { PARAMS }
}
