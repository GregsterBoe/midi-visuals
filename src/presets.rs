use std::fs;
use std::path::PathBuf;

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
