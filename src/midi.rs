use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use midir::{MidiInput, MidiInputConnection};

#[derive(Clone, Copy, Default)]
pub struct NoteState {
    #[allow(dead_code)]
    pub velocity: f32,
    pub on: bool,
}

#[derive(Clone, Copy)]
pub struct NoteEvent {
    pub note: u8,
    pub velocity: f32,
    pub on: bool,
}

#[derive(Clone)]
pub struct MidiState {
    pub ccs: [f32; 128],
    pub notes: [NoteState; 128],
    pub recent_events: VecDeque<NoteEvent>,
}

impl Default for MidiState {
    fn default() -> Self {
        Self {
            ccs: [0.0; 128],
            notes: [NoteState::default(); 128],
            recent_events: VecDeque::with_capacity(64),
        }
    }
}

pub fn start() -> (Arc<Mutex<MidiState>>, Option<MidiInputConnection<()>>) {
    let state = Arc::new(Mutex::new(MidiState::default()));

    let midi_in = match MidiInput::new("midi-visuals") {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Failed to create MIDI input: {e}. Continuing without MIDI.");
            return (state, None);
        }
    };

    let ports = midi_in.ports();
    if ports.is_empty() {
        println!("No MIDI ports found. Continuing without MIDI.");
        return (state, None);
    }

    // Prefer a real hardware port over the Linux "Midi Through" virtual loopback.
    let port = ports
        .iter()
        .find(|p| {
            midi_in
                .port_name(p)
                .map(|n| !n.contains("Midi Through"))
                .unwrap_or(false)
        })
        .unwrap_or(&ports[0]);

    let port_name = midi_in
        .port_name(port)
        .unwrap_or_else(|_| "unknown".to_string());
    println!("Connecting to MIDI port: {port_name}");

    let state_clone = Arc::clone(&state);
    let conn = midi_in.connect(
        port,
        "midi-visuals-input",
        move |_stamp, message, _| {
            if message.len() < 3 {
                return;
            }
            let status = message[0];
            let b1 = message[1] as usize;
            let b2 = message[2];
            let mut s = state_clone.lock().unwrap();
            match status & 0xF0 {
                0xB0 => {
                    s.ccs[b1] = b2 as f32 / 127.0;
                }
                0x90 => {
                    let on = b2 > 0;
                    s.notes[b1] = NoteState { velocity: b2 as f32 / 127.0, on };
                    push_event(
                        &mut s.recent_events,
                        NoteEvent { note: b1 as u8, velocity: b2 as f32 / 127.0, on },
                    );
                }
                0x80 => {
                    s.notes[b1].on = false;
                    push_event(
                        &mut s.recent_events,
                        NoteEvent { note: b1 as u8, velocity: 0.0, on: false },
                    );
                }
                _ => {}
            }
        },
        (),
    );

    match conn {
        Ok(c) => (state, Some(c)),
        Err(e) => {
            eprintln!("Failed to connect to MIDI port: {e}. Continuing without MIDI.");
            (state, None)
        }
    }
}

impl MidiState {
    pub fn note_on_events(&self) -> impl Iterator<Item = &NoteEvent> {
        self.recent_events.iter().filter(|e| e.on)
    }
}

fn push_event(queue: &mut VecDeque<NoteEvent>, event: NoteEvent) {
    if queue.len() >= 64 {
        queue.pop_front();
    }
    queue.push_back(event);
}
