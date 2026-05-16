# midi-visuals

A MIDI-reactive visual synthesizer written in Rust using [Nannou](https://nannou.cc). Connect a MIDI controller and watch knobs and keys drive real-time visuals.

## Run

```
cargo run                # start on the default sketch (aurora)
cargo run -- aurora      # start on a specific sketch by name
cargo run -- cardano
cargo run -- rings
```

## Keyboard shortcuts

| Key   | Action                          |
|-------|---------------------------------|
| `Tab` | Cycle to the next sketch        |
| `H`   | Toggle the HUD overlay          |
| `S`   | Save current knob state as preset (then type a name + Enter) |
| `Enter` | Load selected preset          |

The HUD (top-left) shows the active sketch name, current FPS, and a green dot that flashes on any MIDI input.
Presets are saved per-sketch in `presets/<sketch_name>.txt`. CC 31 scrolls through saved presets.

## Sketches

### aurora
A glowing circle driven by four knobs.

| CC | Parameter  | Range         |
|----|------------|---------------|
| 24 | Hue        | full spectrum |
| 25 | Radius     | 40 – 300 px   |
| 26 | Saturation | 0.5 – 1.0     |
| 27 | Lightness  | 0.3 – 0.7     |

---

### grid
An 8×8 grid of cells. Each note-on event lights up the cell at `note % 64` with the note's velocity, then fades out over ~0.7 s. No knobs.

---

### particles
Note-on events spawn bursts of particles that fly outward, fade, and die.

| CC | Parameter   | Range                         |
|----|-------------|-------------------------------|
| 24 | Base hue    | full spectrum                 |
| 25 | Hue spread  | 0 = monochrome, 0.5 = wide mix |
| 26 | Gravity     | 0 – 500                       |
| 27 | Drag        | 0 – 4                         |
| 28 | Spawn count | 1 – 50 per note               |

HUD shows live particle count.

---

### droplets
Simulated rain droplets follow each other's trails, leaving a glowing field. Note-on spawns droplets anchored to the played note's Y position.

| CC | Parameter   | Range       |
|----|-------------|-------------|
| 24 | Base hue    | 0 – 360°    |
| 25 | Spawn rate  | 0 – 300 /s  |
| 26 | Attraction  | 0 – 3       |
| 27 | Jitter      | 0 – 120     |
| 28 | Decay       | 0.05 – 2.0  |

HUD shows live droplet count.

---

### rings
Note-on events spawn expanding rings positioned by note pitch. Optional gravity and rain mode.

| CC | Parameter  | Range        |
|----|------------|--------------|
| 24 | Hue        | full spectrum |
| 25 | Max radius | 5 – 1200 px  |
| 26 | Speed      | 5 – 800 px/s |
| 27 | Gravity    | 0 – 600      |
| 28 | Rain rate  | 0.1 – 30 /s  |
| 29 | Rain spread| 0 – 200 px   |

Key bindings:

| Key | Action                              |
|-----|-------------------------------------|
| `G` | Toggle rain mode                    |
| `O` | Toggle gravity                      |
| `K` | Flip gravity between push and pull  |
| `R` | Clear all rings                     |

HUD shows ring count and active modes.

---

### cardano
Spirograph-style epicycloid circles with spring deflection on note-on events.
Multiple independent collections can be layered with angular alignment control.

| Controller   | Parameter   | Range                      |
|--------------|-------------|----------------------------|
| CC 1 (mod wheel) | Hue (base) | full spectrum          |
| Pitch bend   | Hue (target)| full spectrum (0.5 = center) |
| CC 24        | Circles     | 1 – 16 inner circles       |
| CC 25        | Dot radius  | 1 – 80 px                  |
| CC 26        | Speed       | 0.05 – 6                   |
| CC 27        | Ratio       | 0.05 – 8 (inner/outer speed)|
| CC 28        | Orbit radius| 10 – 700 px                |
| CC 29        | Alignment   | 0 – 1 (spread between collections) |
| CC 30        | Trail length| 1 – 500 frames             |

Note-on events send a radial spring impulse — circles deflect outward then snap back.

Key bindings:

| Key | Action                              |
|-----|-------------------------------------|
| `C` | Cycle collections 1 → 2 → 3 → 4 → 1 |
| `R` | Clear trail                         |

HUD shows `NxM circles  F frames` (collections × circles per collection, trail frames).

---

## Requirements

- Rust (stable)
- A connected MIDI controller
- Linux: `libasound2-dev` for MIDI (`sudo apt install libasound2-dev`)

### Monitoring raw MIDI input

```bash
aconnect -l            # list ports
aseqdump -p <port>     # print all MIDI messages from that port
```
