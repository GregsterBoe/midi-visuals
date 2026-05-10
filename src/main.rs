mod midi;
mod sketches;

use midi::MidiState;
use midir::MidiInputConnection;
use nannou::prelude::*;
use sketches::{registry, Sketch, SketchFactory};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

fn main() {
    nannou::app(model).update(update).run();
}

struct Model {
    midi_state: Arc<Mutex<MidiState>>,
    _midi_conn: Option<MidiInputConnection<()>>,
    active: Box<dyn Sketch>,
    prev_ccs: [f32; 128],
    sketch_idx: usize,
    registry: Vec<(&'static str, SketchFactory)>,
    show_hud: bool,
    last_midi_t: Instant,
}

fn model(app: &App) -> Model {
    app.new_window()
        .size(800, 600)
        .view(view)
        .key_pressed(key_pressed)
        .build()
        .unwrap();

    let (midi_state, midi_conn) = midi::start();
    let reg = registry();

    let idx = match std::env::args().nth(1) {
        Some(name) => match reg.iter().position(|(n, _)| *n == name.as_str()) {
            Some(i) => i,
            None => {
                eprintln!("Unknown sketch '{name}', falling back to '{}'", reg[0].0);
                0
            }
        },
        None => 0,
    };

    let active = reg[idx].1();
    println!("Sketch: {}", reg[idx].0);

    Model {
        midi_state,
        _midi_conn: midi_conn,
        active,
        prev_ccs: [0.0; 128],
        sketch_idx: idx,
        registry: reg,
        show_hud: true,
        last_midi_t: Instant::now(),
    }
}

fn key_pressed(_app: &App, model: &mut Model, key: Key) {
    match key {
        Key::Tab => {
            model.sketch_idx = (model.sketch_idx + 1) % model.registry.len();
            model.active = model.registry[model.sketch_idx].1();
            println!("Sketch: {}", model.registry[model.sketch_idx].0);
        }
        Key::H => model.show_hud = !model.show_hud,
        _ => {}
    }
}

fn update(_app: &App, model: &mut Model, update: Update) {
    let dt = update.since_last.secs() as f32;
    let (midi_snapshot, events) = {
        let mut s = model.midi_state.lock().unwrap();
        let events: Vec<_> = s.recent_events.drain(..).collect();
        let recent_events: VecDeque<_> = events.iter().copied().collect();
        (MidiState { ccs: s.ccs, notes: s.notes, recent_events }, events)
    };

    let changed: Vec<String> = midi_snapshot
        .ccs
        .iter()
        .enumerate()
        .filter(|&(i, &val)| (val - model.prev_ccs[i]).abs() > f32::EPSILON)
        .map(|(i, &val)| format!("CC{i}={val:.2}"))
        .collect();
    if !changed.is_empty() {
        println!("MIDI: {}", changed.join(" "));
        model.last_midi_t = Instant::now();
    }
    model.prev_ccs = midi_snapshot.ccs;

    for e in &events {
        if e.on {
            println!("Note {} on  vel={:.3}", e.note, e.velocity);
        } else {
            println!("Note {} off", e.note);
        }
    }
    if !events.is_empty() {
        model.last_midi_t = Instant::now();
    }

    model.active.update(&midi_snapshot, dt);
}

fn view(app: &App, model: &Model, frame: Frame) {
    let draw = app.draw();
    let win = app.window_rect();

    draw.background().color(BLACK);
    model.active.view(&draw, win);

    if model.show_hud {
        let fps = app.fps();
        let name = model.active.name();
        let midi_active = model.last_midi_t.elapsed().as_millis() < 500;

        let dot_color = if midi_active { GREEN } else { DIMGRAY };
        let hud_x = win.left() + 10.0;
        let hud_y = win.top() - 14.0;

        draw.text(&format!("{name}   {fps:.0} fps"))
            .color(WHITE)
            .font_size(14)
            .x_y(hud_x + 90.0, hud_y)
            .w_h(180.0, 20.0);

        draw.ellipse()
            .x_y(hud_x + 185.0, hud_y)
            .radius(5.0)
            .color(dot_color);
    }

    draw.to_frame(app, &frame).unwrap();
}
