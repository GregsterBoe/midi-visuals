# midi-visuals — Technical Setup

A multi-sketch MIDI-driven visualization project built with [Nannou](https://nannou.cc) and Rust.

---

## Concept

Each visualization is an independent sketch that shares a common MIDI input layer.
The connection between all sketches is that they process MIDI signals — the visual logic is freely defined per sketch.
Switching between sketches happens on the fly without bloating a single project.

---

## Stack

| Layer | Tool |
|---|---|
| Language | Rust |
| Creative framework | Nannou 0.19 |
| MIDI input | midir |
| GPU backend | wgpu (via Nannou) |
| Editor | VS Code + rust-analyzer |

---

## System Requirements

### Ubuntu / Linux

```bash
# Core build tools
sudo apt-get install curl build-essential cmake pkg-config python3

# Audio / MIDI (ALSA)
sudo apt-get install libasound2-dev

# XCB windowing
sudo apt-get install libxcb-shape0-dev libxcb-xfixes0-dev libx11-dev

# Vulkan — NVIDIA
sudo apt-get install nvidia-driver vulkan-tools

# Vulkan — AMD / Intel (open source)
sudo apt-get install mesa-vulkan-drivers vulkan-tools

# Verify Vulkan
vulkaninfo
```

### Windows

- Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
  - Select **C++ build tools** under "Other Tools and Frameworks"
- Install Rust via [rustup.rs](https://rustup.rs)
- Vulkan is supported out of the box with any recent GPU driver

---

## Rust Installation

```bash
# Install Rust (Linux/macOS)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Add useful components
rustup component add rust-src rustfmt
```

---

## Project Setup

```bash
cargo new midi-visuals
cd midi-visuals
```

### Cargo.toml

```toml
[package]
name = "midi-visuals"
version = "0.1.0"
edition = "2021"

[dependencies]
nannou = "0.19"
midir = "0.9"
```

> **Note:** The first `cargo build` or `cargo run` will take several minutes as Nannou and all
> its dependencies compile from scratch. Subsequent builds are much faster.

---

## VS Code Extensions

| Extension | Purpose |
|---|---|
| `rust-analyzer` | Core — autocomplete, type hints, inline errors, go-to-definition |
| `Even Better TOML` | Syntax highlighting for `Cargo.toml` |
| `crates` | Shows outdated dependency versions inline in `Cargo.toml` |
| `CodeLLDB` | Debugger support for tracing panics |

---

## Verify the Setup

Replace `src/main.rs` with this minimal Nannou sketch and run `cargo run`.
A black window with a blue circle confirms everything is working.

```rust
use nannou::prelude::*;

fn main() {
    nannou::app(model).update(update).run();
}

struct Model {}

fn model(app: &App) -> Model {
    app.new_window().size(800, 600).view(view).build().unwrap();
    Model {}
}

fn update(_app: &App, _model: &mut Model, _update: Update) {}

fn view(app: &App, _model: &Model, frame: Frame) {
    let draw = app.draw();
    draw.background().color(BLACK);
    draw.ellipse()
        .x_y(0.0, 0.0)
        .radius(100.0)
        .color(STEELBLUE);
    draw.to_frame(app, &frame).unwrap();
}
```

```bash
cargo run
# Expected: black window with a blue circle appears
```

---

## MIDI Device

Currently using **Akai MPK mini IV**. Knob CC assignments depend on the active preset —
**always use Preset 1** (the default). Switching presets changes the CC numbers and
breaks the sketch param mappings.

Sketches use CC 24–28 by convention. If a new sketch needs different knobs, define
its `PARAMS` to match the preset 1 CC numbers for those physical knobs.

---

## Architecture

```
midi-visuals/
  src/
    main.rs          ← launcher, sketch switcher (Tab), HUD (H)
    midi.rs          ← shared MIDI input layer (midir)
    sketches/
      mod.rs         ← Sketch trait, Param struct, registry
      aurora.rs      ← MIDI-reactive circle
      grid.rs        ← 8×8 note grid
      particles.rs   ← particle system
```

Each sketch implements the `Sketch` trait:

```rust
pub trait Sketch {
    fn update(&mut self, midi: &MidiState, dt: f32);
    fn view(&self, draw: &Draw, win: Rect);
    fn name(&self) -> &'static str;
    fn params(&self) -> &[Param] { &[] }          // CC knob declarations
    fn hud_info(&self) -> Option<String> { None }  // extra HUD line
    fn key_pressed(&mut self, key: Key) {}         // sketch-local keys
}
```

CC knobs are declared once per sketch as a `const PARAMS` array and read via
`PARAMS[i].read(midi)`. The HUD shows live values automatically.

---

## Known Warnings

```
warning: the following packages contain code that will be rejected by a future version of Rust: noise v0.7.0
```

This comes from a Nannou dependency (`noise` crate) and is harmless. No action needed.

---

## References

- [Nannou Guide](https://guide.nannou.cc)
- [Nannou GitHub](https://github.com/nannou-org/nannou)
- [midir crate](https://crates.io/crates/midir)
- [wgpu](https://wgpu.rs)
- [Rust Book](https://doc.rust-lang.org/book/)