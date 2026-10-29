use std::fs;
use std::path::PathBuf;

use crate::midi::Control;

/// CC number for the knob that scrolls through presets. Change this to match
/// your controller's last/scroll knob.
pub const SCROLL_CC: u8 = 31;

pub struct Preset {
    pub name: String,
    pub ccs: [f32; 128],
}

pub struct PresetStore {
    pub list: Vec<Preset>,
    path: PathBuf,
}

impl PresetStore {
    pub fn load(sketch_name: &str) -> Self {
        let path = PathBuf::from("presets").join(format!("{}.txt", sketch_name));
        let mut store = Self { list: Vec::new(), path };
        if let Ok(text) = fs::read_to_string(&store.path) {
            let mut lines = text.lines();
            loop {
                let name = match lines.next() {
                    Some(n) => n.trim().to_string(),
                    None => break,
                };
                if name.is_empty() { continue; }
                let vals_line = match lines.next() {
                    Some(v) => v,
                    None => break,
                };
                let vals: Vec<f32> = vals_line
                    .split_whitespace()
                    .filter_map(|v| v.parse().ok())
                    .collect();
                if vals.len() == 128 {
                    let mut ccs = [0.0f32; 128];
                    ccs.copy_from_slice(&vals);
                    store.list.push(Preset { name, ccs });
                }
            }
        }
        store
    }

    pub fn save(&self) {
        let _ = fs::create_dir_all(self.path.parent().unwrap());
        let mut out = String::new();
        for p in &self.list {
            out.push_str(&p.name);
            out.push('\n');
            let vals: Vec<String> = p.ccs.iter().map(|v| format!("{:.6}", v)).collect();
            out.push_str(&vals.join(" "));
            out.push('\n');
        }
        let _ = fs::write(&self.path, out);
    }

    /// Adds a new preset or overwrites an existing one with the same name.
    pub fn add(&mut self, name: String, ccs: [f32; 128]) {
        if let Some(p) = self.list.iter_mut().find(|p| p.name == name) {
            p.ccs = ccs;
        } else {
            self.list.push(Preset { name, ccs });
        }
    }
}

/// Audio input choices kept between runs, so a source picked once in the
/// control panel is used again next time.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    pub engine: usize,
    pub gate_db: f32,
    pub source: Option<String>,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self { engine: 0, gate_db: crate::audio::DEFAULT_GATE_DB, source: None }
    }
}

const AUDIO_SETTINGS: &str = "presets/audio.json";

impl AudioSettings {
    pub fn load() -> Self {
        fs::read_to_string(AUDIO_SETTINGS)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = fs::create_dir_all("presets");
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = fs::write(AUDIO_SETTINGS, text);
        }
    }
}

/// Which MIDI control sits where on the controller, so the panel can draw
/// it. Defaults to the MPK mini: two wheels, two rows of four knobs, and two
/// banks of eight pads (A then B, each numbered from the bottom left). Slots
/// are re-learned from the panel when they don't match.
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ControllerLayout {
    pub wheels: Vec<Control>,
    pub knobs: Vec<Control>,
    pub pads: Vec<Control>,
}

impl Default for ControllerLayout {
    fn default() -> Self {
        Self {
            wheels: vec![Control::PitchBend, Control::Cc(1)],
            knobs: (24..=31).map(Control::Cc).collect(),
            pads: (36..=51).map(Control::Pad).collect(),
        }
    }
}

const CONTROLLER_LAYOUT: &str = "presets/controller.json";

impl ControllerLayout {
    pub fn load() -> Self {
        fs::read_to_string(CONTROLLER_LAYOUT)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = fs::create_dir_all("presets");
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = fs::write(CONTROLLER_LAYOUT, text);
        }
    }

    /// Puts `control` in a slot (wheels first, then knobs, then pads). If
    /// another slot had it, that slot takes this one's old control, so
    /// nothing is lost.
    pub fn assign(&mut self, slot: usize, control: Control) {
        let mut slots: Vec<&mut Control> =
            self.wheels.iter_mut().chain(self.knobs.iter_mut()).chain(self.pads.iter_mut()).collect();
        let Some(old) = slots.get(slot).map(|c| **c) else { return };
        if let Some(other) = slots.iter_mut().find(|c| ***c == control) {
            **other = old;
        }
        *slots[slot] = control;
    }
}
