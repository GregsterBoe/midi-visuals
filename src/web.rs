//! Local control panel: a small HTTP server on 127.0.0.1 serving one page.
//!
//! The render loop owns all app state, so the two sides only meet through
//! messages: the page POSTs a `Command`, which `update` drains each frame, and
//! `update` publishes a `Status` snapshot that the page polls ~10 times a
//! second. Messages written with `log_info!`/`log_warn!` go to the terminal
//! as before and also to the page's log.

macro_rules! log_info {
    ($($t:tt)*) => { $crate::web::log(false, format!($($t)*)) };
}
macro_rules! log_warn {
    ($($t:tt)*) => { $crate::web::log(true, format!($($t)*)) };
}

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tiny_http::{Header, Method, Request, Response, Server};

use crate::audio::pulse::{self, Source};

const PAGE: &str = include_str!("../assets/panel.html");
pub const DEFAULT_PORT: u16 = 7878;
/// Further ports tried when the default is taken (e.g. a second instance).
const PORT_TRIES: u16 = 10;
const LOG_LINES: usize = 300;

// ── Log ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
struct LogLine {
    seq: u64,
    /// Unix time in ms, so the page can show local wall-clock times.
    ms: u64,
    warn: bool,
    text: String,
}

static LOG: Mutex<(u64, VecDeque<LogLine>)> = Mutex::new((0, VecDeque::new()));

pub fn log(warn: bool, text: String) {
    if warn {
        eprintln!("{text}");
    } else {
        println!("{text}");
    }
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let mut log = LOG.lock().unwrap();
    log.0 += 1;
    let seq = log.0;
    if log.1.len() >= LOG_LINES {
        log.1.pop_front();
    }
    log.1.push_back(LogLine { seq, ms, warn, text });
}

fn log_since(seq: u64) -> Vec<LogLine> {
    LOG.lock().unwrap().1.iter().filter(|l| l.seq > seq).cloned().collect()
}

// ── Messages ────────────────────────────────────────────────────────────────

/// Something the page asks the app to do.
#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    Sketch { idx: usize },
    /// One of the active sketch's `actions()`.
    Action { idx: usize },
    Input { audio: bool },
    Engine { idx: usize },
    Gate { db: f32 },
    /// A PipeWire/Pulse source name, or `None` for the system default.
    Source { name: Option<String> },
    /// A knob value in [0, 1], as a MIDI controller would send it.
    Cc { cc: u8, value: f32 },
    LoadPreset { idx: usize },
    SavePreset { name: String },
    DeletePreset { idx: usize },
    Hud { on: bool },
}

#[derive(Serialize, Default)]
pub struct Status {
    pub sketches: Vec<&'static str>,
    pub sketch: usize,
    pub info: Option<String>,
    pub actions: Vec<&'static str>,
    pub params: Vec<ParamStatus>,
    pub presets: Vec<String>,
    pub preset: usize,
    pub hud: bool,
    pub fps: f32,
    pub audio_on: bool,
    pub midi_port: Option<String>,
    pub midi_active: bool,
    /// Notes held on the active source.
    pub notes: Vec<u8>,
    pub engines: Vec<&'static str>,
    pub engine: usize,
    pub gate_db: f32,
    pub source: Option<String>,
    /// Present while audio capture runs.
    pub audio: Option<AudioStatus>,
    pub audio_error: Option<String>,
}

#[derive(Serialize)]
pub struct ParamStatus {
    pub cc: u8,
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    /// Knob position in [0, 1].
    pub raw: f32,
}

#[derive(Serialize)]
pub struct AudioStatus {
    pub device: String,
    pub level_db: f32,
    pub load: f32,
    pub latency_ms: f32,
    pub engine_error: Option<String>,
}

// ── Server ──────────────────────────────────────────────────────────────────

pub struct Panel {
    pub url: String,
    pub status: Arc<Mutex<Status>>,
    commands: Receiver<Command>,
}

impl Panel {
    /// Serves the panel on `port`, or the next free port above it.
    pub fn start(port: u16) -> Result<Self, String> {
        let (server, port) = (port..port.saturating_add(PORT_TRIES))
            .find_map(|p| Server::http(("127.0.0.1", p)).ok().map(|s| (s, p)))
            .ok_or(format!("ports {port}-{} are all taken", port.saturating_add(PORT_TRIES - 1)))?;
        let status = Arc::new(Mutex::new(Status::default()));
        let (tx, commands) = mpsc::channel();
        let st = Arc::clone(&status);
        std::thread::Builder::new()
            .name("control-panel".into())
            .spawn(move || serve(server, port, st, tx))
            .map_err(|e| e.to_string())?;
        Ok(Self { url: format!("http://127.0.0.1:{port}"), status, commands })
    }

    pub fn commands(&self) -> Vec<Command> {
        self.commands.try_iter().collect()
    }

    pub fn open_in_browser(&self) {
        if let Err(e) = std::process::Command::new("xdg-open").arg(&self.url).spawn() {
            log_warn!("Couldn't open a browser ({e}). The panel is at {}", self.url);
        }
    }
}

fn serve(server: Server, port: u16, status: Arc<Mutex<Status>>, tx: Sender<Command>) {
    let hosts = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    let mut sources: Vec<Source> = Vec::new();
    let mut sources_at: Option<Instant> = None;
    // Lets an open page notice the app restarted and reset its log.
    let boot = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);

    for mut req in server.incoming_requests() {
        // Only pages served from this address may talk to it. Any other site
        // open in the browser could otherwise reach localhost too.
        let host_ok = header(&req, "Host").is_some_and(|h| hosts.contains(&h));
        if !host_ok {
            let _ = req.respond(Response::empty(403));
            continue;
        }
        let url = req.url().to_string();
        let (path, query) = url.split_once('?').unwrap_or((&url, ""));
        let res = match (req.method(), path) {
            (Method::Get, "/") => req.respond(
                Response::from_string(PAGE).with_header(content_type("text/html; charset=utf-8")),
            ),
            (Method::Get, "/api/state") => {
                let since = query
                    .split('&')
                    .find_map(|kv| kv.strip_prefix("since="))
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                // pactl takes a few ms, so the source list is refreshed every few seconds.
                if sources_at.is_none_or(|t| t.elapsed() > Duration::from_secs(3)) {
                    sources = pulse::sources();
                    sources_at = Some(Instant::now());
                }
                let body = {
                    let st = status.lock().unwrap();
                    serde_json::json!({ "boot": boot, "status": &*st, "sources": &sources, "log": log_since(since) })
                };
                req.respond(Response::from_string(body.to_string()).with_header(content_type("application/json")))
            }
            (Method::Post, "/api/cmd") => {
                // Requiring JSON makes browsers preflight cross-site requests, which fail here.
                let json = header(&req, "Content-Type").is_some_and(|c| c.starts_with("application/json"));
                let mut body = String::new();
                let parsed = req.as_reader().read_to_string(&mut body).ok().and_then(|_| serde_json::from_str(&body).ok());
                match (json, parsed) {
                    (true, Some(cmd)) => {
                        let _ = tx.send(cmd);
                        req.respond(Response::empty(204))
                    }
                    (false, _) => req.respond(Response::empty(415)),
                    (true, None) => req.respond(Response::from_string(format!("bad command: {body}")).with_status_code(400)),
                }
            }
            _ => req.respond(Response::empty(404)),
        };
        if let Err(e) = res {
            eprintln!("Control panel: {e}");
        }
    }
}

fn header(req: &Request, name: &str) -> Option<String> {
    req.headers().iter().find(|h| h.field.to_string().eq_ignore_ascii_case(name)).map(|h| h.value.to_string())
}

fn content_type(value: &str) -> Header {
    Header::from_bytes("Content-Type", value).unwrap()
}
