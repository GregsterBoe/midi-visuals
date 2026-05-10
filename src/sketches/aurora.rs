use crate::midi::MidiState;
use crate::sketches::Sketch;
use nannou::prelude::*;

// CC 24 = hue | CC 25 = radius | CC 26 = saturation | CC 27 = lightness
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
        self.hue = midi.ccs[24];
        self.radius = 40.0 + midi.ccs[25] * 260.0;
        self.saturation = 0.5 + midi.ccs[26] * 0.5;
        self.lightness = 0.3 + midi.ccs[27] * 0.4;
    }

    fn view(&self, draw: &Draw, _win: Rect) {
        draw.ellipse()
            .x_y(0.0, 0.0)
            .radius(self.radius)
            .color(hsl(self.hue, self.saturation, self.lightness));
    }

    fn name(&self) -> &'static str {
        "aurora"
    }
}
