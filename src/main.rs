mod midi;
mod presets;
mod sketches;

use midi::MidiState;
use midir::MidiInputConnection;
use nannou::prelude::*;
use presets::PresetStore;
use sketches::{registry, Sketch, SketchFactory};
use std::sync::{Arc, Mutex};
use std::time::Instant;

enum PresetMode {
    Normal,
    Naming { buf: String, skip: bool },
}

fn main() {
    nannou::app(model).update(update).run();
}

fn print_sketch_info(sketch: &dyn Sketch) {
    let params = sketch.params();
    if params.is_empty() {
        println!("Sketch: {}", sketch.name());
    } else {
        let mapping: Vec<String> = params.iter()
            .map(|p| format!("CC{}={}", p.cc, p.name))
            .collect();
        println!("Sketch: {}  [{}]", sketch.name(), mapping.join("  "));
    }
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
    midi_cc_seen: bool,
    midi_note_seen: bool,
    presets: PresetStore,
    preset_idx: usize,
    preset_mode: PresetMode,
    preset_scroll_accum: f32,
}

fn model(app: &App) -> Model {
    app.new_window()
        .size(800, 600)
        .view(view)
        .key_pressed(key_pressed)
        .received_character(received_character)
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
    print_sketch_info(&*active);
    let presets = PresetStore::load(active.name());

    Model {
        midi_state,
        _midi_conn: midi_conn,
        active,
        prev_ccs: [0.0; 128],
        sketch_idx: idx,
        registry: reg,
        show_hud: true,
        last_midi_t: Instant::now(),
        midi_cc_seen: false,
        midi_note_seen: false,
        presets,
        preset_idx: 0,
        preset_mode: PresetMode::Normal,
        preset_scroll_accum: 0.0,
    }
}

fn received_character(_app: &App, model: &mut Model, ch: char) {
    if let PresetMode::Naming { ref mut buf, ref mut skip } = model.preset_mode {
        if *skip { *skip = false; return; }
        match ch {
            '\x08' => { buf.pop(); }
            c if !c.is_control() => buf.push(c),
            _ => {}
        }
    }
}

fn key_pressed(_app: &App, model: &mut Model, key: Key) {
    // In naming mode only control keys are handled; characters come via received_character.
    if matches!(model.preset_mode, PresetMode::Naming { .. }) {
        match key {
            Key::Return => {
                let name = if let PresetMode::Naming { ref buf, .. } = model.preset_mode {
                    buf.trim().to_string()
                } else {
                    String::new()
                };
                if !name.is_empty() {
                    model.presets.add(name, model.prev_ccs);
                    model.preset_idx = model.presets.list.len().saturating_sub(1);
                    model.presets.save();
                }
                model.preset_mode = PresetMode::Normal;
            }
            Key::Escape => model.preset_mode = PresetMode::Normal,
            _ => {}
        }
        return;
    }

    match key {
        Key::Tab => {
            model.sketch_idx = (model.sketch_idx + 1) % model.registry.len();
            model.active = model.registry[model.sketch_idx].1();
            print_sketch_info(&*model.active);
            model.presets = PresetStore::load(model.active.name());
            model.preset_idx = 0;
            model.preset_scroll_accum = 0.0;
        }
        Key::H => model.show_hud = !model.show_hud,
        Key::S => model.preset_mode = PresetMode::Naming { buf: String::new(), skip: true },
        Key::Return => {
            if !model.presets.list.is_empty() {
                let ccs = model.presets.list[model.preset_idx].ccs;
                model.midi_state.lock().unwrap().ccs = ccs;
                model.prev_ccs = ccs;
            }
        }
        _ => model.active.key_pressed(key),
    }
}

fn update(_app: &App, model: &mut Model, update: Update) {
    let dt = update.since_last.secs() as f32;
    let midi_snapshot = {
        let mut s = model.midi_state.lock().unwrap();
        let snap = s.clone();
        s.recent_events.clear();
        snap
    };

    let cc_changed = midi_snapshot.ccs.iter().enumerate()
        .any(|(i, &val)| (val - model.prev_ccs[i]).abs() > f32::EPSILON);
    if cc_changed {
        model.last_midi_t = Instant::now();
        if !model.midi_cc_seen {
            println!("MIDI knobs: OK");
            model.midi_cc_seen = true;
        }
    }

    // Preset scroll: accumulate CC deltas; one step per 10 MIDI ticks (~1/12 turn).
    if !model.presets.list.is_empty() {
        let cur   = midi_snapshot.ccs[presets::SCROLL_CC as usize];
        let prev  = model.prev_ccs[presets::SCROLL_CC as usize];
        model.preset_scroll_accum += cur - prev;
        let step = 10.0 / 127.0;
        let n = model.presets.list.len();
        while model.preset_scroll_accum >= step {
            model.preset_scroll_accum -= step;
            model.preset_idx = (model.preset_idx + 1) % n;
        }
        while model.preset_scroll_accum <= -step {
            model.preset_scroll_accum += step;
            model.preset_idx = (model.preset_idx + n - 1) % n;
        }
    }

    model.prev_ccs = midi_snapshot.ccs;

    if !midi_snapshot.recent_events.is_empty() {
        model.last_midi_t = Instant::now();
        if !model.midi_note_seen {
            println!("MIDI notes: OK");
            model.midi_note_seen = true;
        }
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

        let mut next_y = hud_y - 20.0;

        if let Some(info) = model.active.hud_info() {
            draw.text(&info)
                .color(GRAY)
                .font_size(13)
                .x_y(hud_x + 90.0, next_y)
                .w_h(180.0, 20.0);
            next_y -= 18.0;
        }

        for param in model.active.params() {
            let val = param.read_from(&model.prev_ccs);
            draw.text(&format!("CC{:02} {:<10} {:.2}", param.cc, param.name, val))
                .color(DIMGRAY)
                .font_size(12)
                .x_y(hud_x + 95.0, next_y)
                .w_h(200.0, 16.0);
            next_y -= 16.0;
        }

        // Preset browser: show the currently highlighted preset.
        if !model.presets.list.is_empty() {
            let n = model.presets.list.len();
            let name = &model.presets.list[model.preset_idx].name;
            draw.text(&format!("> {} ({}/{})", name, model.preset_idx + 1, n))
                .color(YELLOW)
                .font_size(12)
                .x_y(hud_x + 95.0, next_y)
                .w_h(200.0, 16.0);
            next_y -= 16.0;
        }

        // Naming prompt (shown at bottom of HUD when saving).
        if let PresetMode::Naming { ref buf, .. } = model.preset_mode {
            draw.text(&format!("Save as: {}_", buf))
                .color(CYAN)
                .font_size(12)
                .x_y(hud_x + 95.0, next_y)
                .w_h(200.0, 16.0);
        }
    } else if let PresetMode::Naming { ref buf, .. } = model.preset_mode {
        // Show naming prompt even when the HUD is hidden.
        let hud_x = win.left() + 10.0;
        let hud_y = win.top() - 14.0;
        draw.text(&format!("Save as: {}_", buf))
            .color(CYAN)
            .font_size(14)
            .x_y(hud_x + 95.0, hud_y)
            .w_h(200.0, 20.0);
    }

    draw.to_frame(app, &frame).unwrap();

    // Sketches that implement raw_render() draw here via their own wgpu pipeline.
    let dqp = frame.device_queue_pair();
    let mut encoder = frame.command_encoder();
    let target = frame.texture_view();
    model.active.raw_render(dqp.device(), dqp.queue(), &mut *encoder, &**target, win);
}
