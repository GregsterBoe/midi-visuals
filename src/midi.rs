use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use midir::{MidiInput, MidiInputConnection};
use serde::{Deserialize, Serialize};

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

/// A physical control on the controller, as it shows up in MIDI.
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Control {
    Cc(u8),
    PitchBend,
}

#[derive(Clone)]
pub struct MidiState {
    pub ccs: [f32; 128],
    pub notes: [NoteState; 128],
    pub recent_events: VecDeque<NoteEvent>,
    pub pitch_bend: f32,
    /// The control the hardware moved last, and a count of all moves so far,
    /// which lets the panel tell a new move from an old one.
    pub last_control: Option<Control>,
    pub control_moves: u64,
}

impl Default for MidiState {
    fn default() -> Self {
        Self {
            ccs: [0.0; 128],
            notes: [NoteState::default(); 128],
            recent_events: VecDeque::with_capacity(64),
            pitch_bend: 0.5,
            last_control: None,
            control_moves: 0,
        }
    }
}

/// Connects the first hardware MIDI input. Also returns the port's name.
pub fn start() -> (Arc<Mutex<MidiState>>, Option<MidiInputConnection<()>>, Option<String>) {
    let state = Arc::new(Mutex::new(MidiState::default()));

    let midi_in = match MidiInput::new("midi-visuals") {
        Ok(m) => m,
        Err(e) => {
            log_warn!("Failed to create MIDI input: {e}. Continuing without MIDI.");
            return (state, None, None);
        }
    };

    let ports = midi_in.ports();
    if ports.is_empty() {
        log_info!("No MIDI ports found. Continuing without MIDI.");
        return (state, None, None);
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
    log_info!("Connecting to MIDI port: {port_name}");

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
                    s.last_control = Some(Control::Cc(b1 as u8));
                    s.control_moves += 1;
                }
                0xE0 => {
                    // 14-bit pitch bend: message[1]=LSB, message[2]=MSB, range 0-16383
                    let raw = ((b2 as u16) << 7) | (b1 as u16);
                    s.pitch_bend = raw as f32 / 16383.0;
                    s.last_control = Some(Control::PitchBend);
                    s.control_moves += 1;
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
        Ok(c) => (state, Some(c), Some(port_name)),
        Err(e) => {
            log_warn!("Failed to connect to MIDI port: {e}. Continuing without MIDI.");
            (state, None, None)
        }
    }
}

impl MidiState {
    pub fn note_on_events(&self) -> impl Iterator<Item = &NoteEvent> {
        self.recent_events.iter().filter(|e| e.on)
    }
}

pub fn push_event(queue: &mut VecDeque<NoteEvent>, event: NoteEvent) {
    if queue.len() >= 64 {
        queue.pop_front();
    }
    queue.push_back(event);
}
