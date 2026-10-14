mod audio;
mod midi;
mod presets;
mod sketches;

use audio::AudioInput;
use midi::{MidiState, NoteEvent};
use midir::MidiInputConnection;
use nannou::prelude::*;
use presets::PresetStore;
use sketches::{registry, Sketch, SketchFactory};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone, Copy, PartialEq)]
enum InputSource {
    Midi,
    Audio,
}

enum PresetMode {
    Normal,
    Naming { buf: String, skip: bool },
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let tool = match args.get(1).map(String::as_str) {
        Some("--probe") => Some(audio::probe::run as fn(&[String]) -> Result<(), String>),
        Some("--listen") => Some(audio::probe::listen as fn(&[String]) -> Result<(), String>),
        _ => None,
    };
    if let Some(tool) = tool {
        if let Err(e) = tool(&args[2..]) {
            eprintln!("{}: {e}", args[1]);
            std::process::exit(1);
        }
        return;
    }
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
    /// Where notes come from. CC knobs always come from MIDI.
    input: InputSource,
    /// Running only while audio is the note source.
    audio: Option<AudioInput>,
    audio_engine: usize,
    audio_gate_db: f32,
    audio_error: Option<String>,
    last_input: InputSource,
    last_notes_on: [bool; 128],
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

    // Positional arg picks the sketch; `--audio` starts on audio input and
    // `--engine=<name>` picks the pitch engine.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let audio_engine = args
        .iter()
        .find_map(|a| a.strip_prefix("--engine="))
        .map(|name| audio::engine_index(name).unwrap_or_else(|| {
            eprintln!("Unknown engine '{name}', expected one of {:?}", audio::ENGINES);
            0
        }))
        .unwrap_or(0);
    let idx = match args.iter().find(|a| !a.starts_with("--")).cloned() {
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

    let mut model = Model {
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
        input: InputSource::Midi,
        audio: None,
        audio_engine,
        audio_gate_db: audio::DEFAULT_GATE_DB,
        audio_error: None,
        last_input: InputSource::Midi,
        last_notes_on: [false; 128],
    };
    if args.iter().any(|a| a == "--audio") {
        model.set_input(InputSource::Audio);
    }
    model
}

impl Model {
    fn set_input(&mut self, input: InputSource) {
        if input == InputSource::Audio && self.audio.is_none() {
            match AudioInput::start(self.audio_engine, self.audio_gate_db) {
                Ok(a) => {
                    self.audio = Some(a);
                    self.audio_error = None;
                }
                Err(e) => {
                    eprintln!("Audio input unavailable: {e}. Staying on MIDI.");
                    self.audio_error = Some(e);
                    return;
                }
            }
        }
        if input == InputSource::Midi {
            // Stops capture and the engine, so MIDI mode costs no CPU.
            self.audio = None;
        }
        self.input = input;
        println!("Note input: {}", if input == InputSource::Audio { "audio" } else { "MIDI" });
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
        Key::A => {
            let next = if model.input == InputSource::Midi { InputSource::Audio } else { InputSource::Midi };
            model.set_input(next);
        }
        Key::E => {
            model.audio_engine = (model.audio_engine + 1) % audio::ENGINES.len();
            match &model.audio {
                Some(a) => a.set_engine(model.audio_engine),
                None => println!("Audio engine: {} (used when audio input starts)", audio::ENGINES[model.audio_engine]),
            }
        }
        Key::Minus | Key::Equals => {
            if let Some(a) = &model.audio {
                a.adjust_gate(if key == Key::Minus { -3.0 } else { 3.0 });
                model.audio_gate_db = a.gate_db();
                println!("Audio gate: {:.0} dB", model.audio_gate_db);
            }
        }
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
    let mut midi_snapshot = {
        let mut s = model.midi_state.lock().unwrap();
        let snap = s.clone();
        s.recent_events.clear();
        snap
    };

    // In audio mode the detected notes replace MIDI's; knobs stay on MIDI.
    if model.input == InputSource::Audio {
        if let Some(a) = &model.audio {
            let mut s = a.state.lock().unwrap();
            midi_snapshot.notes = s.notes;
            midi_snapshot.recent_events = std::mem::take(&mut s.recent_events);
        }
    }
    // On a source switch, release notes the old source still held so
    // sketches tracking held notes (chords, tension) don't get stuck.
    if model.input != model.last_input {
        for n in 0..128 {
            if model.last_notes_on[n] && !midi_snapshot.notes[n].on {
                midi_snapshot.recent_events.push_back(NoteEvent { note: n as u8, velocity: 0.0, on: false });
            }
        }
        model.last_input = model.input;
    }
    for n in 0..128 {
        model.last_notes_on[n] = midi_snapshot.notes[n].on;
    }

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

/// HUD lines describing the note source: [A] toggles, [E] cycles engines, [-]/[=] gate.
fn input_hud(model: &Model) -> Vec<(String, Srgb<u8>)> {
    let mut lines = Vec::new();
    match (&model.audio, model.input) {
        (Some(a), InputSource::Audio) => {
            let s = a.state.lock().unwrap();
            let engine = audio::ENGINES[s.engine];
            match &s.engine_error {
                Some(_) => lines.push((format!("AUDIO {engine} unavailable"), RED)),
                None => lines.push((format!("AUDIO {engine} ~{:.0}ms", s.latency_ms), CYAN)),
            }
            let busy = if s.load > 0.8 { " BUSY" } else { "" };
            lines.push((format!("{:.0}dB gate {:.0} cpu {:.0}%{busy}", s.level_db, a.gate_db(), s.load * 100.0), DIMGRAY));
            let held: Vec<String> = (0..128u8).filter(|&n| s.notes[n as usize].on).map(audio::probe::note_name).collect();
            if !held.is_empty() {
                lines.push((held.join(" "), WHITE));
            }
        }
        _ => {
            let err = if model.audio_error.is_some() { "  (audio failed)" } else { "" };
            lines.push((format!("MIDI in{err}"), if err.is_empty() { DIMGRAY } else { RED }));
        }
    }
    lines
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

        for (line, color) in input_hud(model) {
            draw.text(&line)
                .color(color)
                .font_size(12)
                .x_y(hud_x + 95.0, next_y)
                .w_h(200.0, 16.0);
            next_y -= 16.0;
        }

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
