use crate::midi::MidiState;
use crate::sketches::Sketch;
use nannou::prelude::*;

const COLS: usize = 8;
const ROWS: usize = 8;
const CELLS: usize = COLS * ROWS;

pub struct Grid {
    brightness: [f32; CELLS],
}

impl Grid {
    pub fn new() -> Self {
        Self { brightness: [0.0; CELLS] }
    }
}

impl Sketch for Grid {
    fn update(&mut self, midi: &MidiState, dt: f32) {
        for b in &mut self.brightness {
            *b = (*b - dt * 1.5).max(0.0);
        }
        for event in &midi.recent_events {
            if event.on {
                self.brightness[event.note as usize % CELLS] = event.velocity;
            }
        }
    }

    fn view(&self, draw: &Draw, win: Rect) {
        let cell_w = win.w() / COLS as f32;
        let cell_h = win.h() / ROWS as f32;
        let pad = 4.0;

        for row in 0..ROWS {
            for col in 0..COLS {
                let b = self.brightness[row * COLS + col];
                let x = win.left() + col as f32 * cell_w + cell_w * 0.5;
                let y = win.top() - row as f32 * cell_h - cell_h * 0.5;
                draw.rect()
                    .x_y(x, y)
                    .w_h(cell_w - pad, cell_h - pad)
                    .color(hsl(0.65, 0.8, 0.08 + b * 0.6));
            }
        }
    }

    fn name(&self) -> &'static str {
        "grid"
    }
}
