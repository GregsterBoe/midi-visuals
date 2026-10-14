//! Picking a PipeWire/PulseAudio source for the capture stream.
//!
//! cpal only sees ALSA devices, so a specific source (a mic, or the monitor of
//! the headphones to visualise whatever is playing) is reached by opening the
//! ALSA `pulse` device and moving its stream with `pactl`. Moving works while
//! the stream runs, so switching sources needs no restart.

use std::process::Command;

use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct Source {
    pub name: String,
    pub description: String,
    /// Monitors carry what an output is playing rather than a microphone.
    pub monitor: bool,
}

/// Sources known to the sound server, or empty when `pactl` isn't available.
pub fn sources() -> Vec<Source> {
    let Some(list) = pactl_json(&["list", "sources"]) else { return Vec::new() };
    list.iter()
        .filter_map(|s| {
            let name = s["name"].as_str()?.to_string();
            let description = s["description"].as_str().unwrap_or(&name).to_string();
            Some(Source { monitor: name.ends_with(".monitor"), name, description })
        })
        .collect()
}

/// Moves this process's capture stream to `source`, waiting up to ~2 s for
/// the stream to register with the sound server after it opens.
pub fn move_to(source: &str) -> Result<(), String> {
    let pid = std::process::id().to_string();
    for _ in 0..20 {
        let outputs = pactl_json(&["list", "source-outputs"]).ok_or("pactl unavailable")?;
        let ours = outputs
            .iter()
            .find(|o| o["properties"]["application.process.id"].as_str() == Some(pid.as_str()))
            .and_then(|o| o["index"].as_u64());
        if let Some(idx) = ours {
            let status = Command::new("pactl")
                .args(["move-source-output", &idx.to_string(), source])
                .status()
                .map_err(|e| e.to_string())?;
            return if status.success() { Ok(()) } else { Err(format!("pactl couldn't move to '{source}'")) };
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err("capture stream never appeared in pactl".into())
}

fn pactl_json(args: &[&str]) -> Option<Vec<serde_json::Value>> {
    let out = Command::new("pactl").arg("-f").arg("json").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}
