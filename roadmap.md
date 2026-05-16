# midi-visuals — Development Roadmap

## Architecture

Three layers. **`main.rs`** owns the Nannou window and the active sketch. **`midi.rs`** runs a background thread, listens for MIDI, and writes into `Arc<Mutex<MidiState>>`. **Sketches** implement a `Sketch` trait — they read `MidiState` and draw; switching is just swapping a `Box<dyn Sketch>`.

---

## Phases

| Phase | Description | Status |
|-------|-------------|--------|
| 0  | Project skeleton — folder layout, empty stubs, `cargo check` passes | `[DONE]` |
| 1  | MIDI input layer — `MidiState`, `midir` connection, CC changes logged to console | `[DONE]` |
| 2  | `Sketch` trait + `aurora` — trait abstraction, MIDI-reactive circle (CC 24–27) | `[DONE]` |
| 3  | Registry + sketch switching — `Tab` cycles sketches, CLI arg selects start sketch | `[DONE]` |
| 4  | `grid` sketch + HUD — 8×8 note grid, FPS/sketch-name overlay, `H` toggle | `[DONE]` |
| 5  | `particles` sketch — note-on spawns particles, physics via CC knobs, particle count in HUD | `[DONE]` |
| 6  | CC/note mapping system — `Param` struct with range mapping, declared per sketch, shown live in HUD | `[DONE]` |
| 7  | Performance scaling — batch geometry; further optimisation tracked in `optimizations.md` | `[DONE]` |
| 8  | `droplets` sketch core — influence grid, path-following rain droplets, trail color field | `[DONE]` |
| 9  | `droplets` MIDI integration — CC knobs for rate/speed/attraction/jitter/hue; note-on spawns burst at pitched Y | `[DONE]` |
| 10 | `droplets` play refinements — velocity scales trail deposit; spawn rate goes to 0; high notes → top, low → bottom | `[DONE]` |
| 11 | Preset system — `S` key saves named CC snapshots per sketch; CC 31 scrolls; `Enter` loads | `[DONE]` |
| 12 | `rings` sketch — expanding ring polygons; note pitch → X position; rain mode; gravity push/pull | `[DONE]` |
| 13 | `rings` refinements — dual rain color split; CC 29 r-spread; expanded knob ranges | `[DONE]` |
| 14 | `cardano` sketch — Spirograph epicycloid circles with spring deflection on notes; multi-collection; alignment knob | `[DONE]` |
| 15 | `cardano` MIDI expansion — mod wheel + pitch bend for hue; CC 24 circle count knob; CC 25 dot radius | `[DONE]` |

---

## Phase 11 — Preset system

Named CC snapshots saved to `presets/<sketch_name>.txt`. Pressing `S` enters naming mode (HUD prompts for a name), `Enter` confirms and saves. CC 31 scrolls through saved presets (debounced to 10 ticks). `Enter` with a preset selected loads it immediately. Toggle states (rain, gravity, etc.) are not saved.

---

## Phase 12–13 — `rings` sketch

Expanding ring polygons (N=16-sided) spawned on note-on. Note number maps to X position across the window; Y is randomised. Velocity scales max radius and base alpha.

### CC knob mapping

| CC | Param      | Range        |
|----|------------|--------------|
| 24 | Hue        | full spectrum |
| 25 | Max radius | 5 – 1200 px  |
| 26 | Speed      | 5 – 800 px/s |
| 27 | Gravity    | 0 – 600      |
| 28 | Rain rate  | 0.1 – 30 /s  |
| 29 | Rain spread| 0 – 200 px   |

Rain mode (`G` key) spawns rings continuously from the center, slowly rotating the spawn angle over a 10-second cycle. Alternating rings are offset ±0.1 in hue from the complementary color. Gravity (`O` key) applies a radial push or pull (`K` toggles sign) to ring offsets each frame.

---

## Phase 14–15 — `cardano` sketch

Spirograph-style epicycloid: an outer point orbits the center, `bounds` inner circles orbit that point. Inner orbit ratio (CC 27) creates classic Spirograph patterns at integer values.

### MIDI sources

| Controller       | Parameter           |
|------------------|---------------------|
| CC 1 (mod wheel) | Base hue            |
| Pitch bend       | Target hue (hue2)   |
| CC 24            | Circle count (1–16) |
| CC 25            | Dot radius (1–80 px)|
| CC 26            | Speed               |
| CC 27            | Ratio (inner/outer) |
| CC 28            | Orbit radius        |
| CC 29            | Alignment           |
| CC 30            | Trail length        |

Note-on events send an outward radial spring impulse to all circles in all collections. Circles deflect then spring back to their orbital paths (spring K=6, damping=3.5). Velocity scales impulse strength and base alpha.

### Collections (`C` key)

Cycles 1 → 2 → 3 → 4 → 1 independent Cardano systems. New collections inherit the current inner angles for immediate rotational symmetry. CC 29 `align` 0→1 offsets each collection by a fraction of `TAU/N`, sweeping from clustered to evenly spread.

### Keys

| Key | Action                              |
|-----|-------------------------------------|
| `C` | Cycle collections (1–4)             |
| `R` | Clear trail                         |

---

## Ideas / backlog

- Waveform/oscilloscope sketch driven by CC values
- Global brightness / blackout via a master CC
- MIDI channel filtering (channel-per-sketch mode)
- BPM sync: detect tap tempo from note timing, quantise spawn events
