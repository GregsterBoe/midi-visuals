# midi-visuals — Development Roadmap

## Architecture

Three layers. **`main.rs`** owns the Nannou window and the active sketch. **`midi.rs`** runs a background thread, listens for MIDI, and writes into `Arc<Mutex<MidiState>>`. **Sketches** implement a `Sketch` trait — they read `MidiState` and draw; switching is just swapping a `Box<dyn Sketch>`.

---

## Phases

| Phase | Description | Status |
|-------|-------------|--------|
| 0 | Project skeleton — folder layout, empty stubs, `cargo check` passes | `[DONE]` |
| 1 | MIDI input layer — `MidiState`, `midir` connection, CC changes logged to console | `[DONE]` |
| 2 | `Sketch` trait + `aurora` — trait abstraction, MIDI-reactive circle (CC 24–27) | `[DONE]` |
| 3 | Registry + sketch switching — `Tab` cycles sketches, CLI arg selects start sketch | `[DONE]` |
| 4 | `grid` sketch + HUD — 8×8 note grid, FPS/sketch-name overlay, `H` toggle | `[DONE]` |
| 5 | `particles` sketch — note-on spawns particles, physics via CC knobs, particle count in HUD | `[DONE]` |
| 6 | CC/note mapping system — `Param` struct with range mapping, declared per sketch, shown live in HUD | `[DONE]` |
| 7 | Performance scaling — batch geometry; further optimisation tracked in `optimizations.md` | `[DONE]` |
| 8 | `droplets` sketch core — influence grid, path-following rain droplets, trail color field | `[DONE]` |
| 9 | `droplets` MIDI integration — CC knobs for rate/speed/attraction/jitter/hue; note-on spawns burst at pitched Y | `[DONE]` |
| 10 | `droplets` play refinements — velocity scales trail deposit; spawn rate goes to 0; high notes → top, low → bottom | `[DONE]` |

---

## Phase 8 — `droplets` sketch: design notes

Port of the OpenFrameworks "Rain Droplet Path Field" simulation. Droplets spawn from the right edge, travel left, leave an influence trail in a 2D grid, and new droplets prefer to follow existing trails — producing a rain-on-glass branching effect.

### Influence field

A flat `Vec<f32>` of size `cols × rows` (cell size ~8 px). Each frame every cell is multiplied by `(1 - decay * dt)`. Droplets deposit influence in a circular radius around their position using a linear falloff. A parallel `Vec<[f32; 3]>` stores the RGB trail color per cell, blended via lerp at each deposit (`color_cell = lerp(color_cell, droplet_color, clamp(deposit * 0.15, 0, 1))`). On draw, cells above a threshold are rendered as colored rectangles with alpha proportional to intensity.

### Droplet struct

```rust
struct Droplet {
    pos: Vec2,
    prev_pos: Vec2,
    vel_y: f32,
    radius: f32,
    alpha: f32,
    age: f32,
    max_age: f32,
    color: Rgb,
}
```

Drawn as a `draw.line(prev_pos, pos)` plus a slightly brighter `draw.ellipse(pos)` head. Alpha fades linearly with `age / max_age`.

### Path-following logic (per frame)

1. Probe influence at `(pos.x - probe_ahead, pos.y)`, `pos.y ± probe_step`.
2. If `random() < branch_chance`: pick a random vertical offset.
3. Otherwise: steer toward the highest of the three samples.
4. `vel_y = lerp(vel_y, attraction * target_offset * 12 + jitter_noise, 0.12)`.
5. Move: `pos.x -= speed * dt`, `pos.y += vel_y * dt`, clamp Y to window.
6. Deposit influence + color at new position.
7. Kill if `pos.x < -margin` or `age >= max_age`.

### Color

Droplets pick a random HSV color near a configurable base hue (±12° hue jitter, ±saturation/brightness jitter). Minimum luma is enforced: if `0.2126r + 0.7152g + 0.0722b < 0.32`, all channels are scaled up proportionally. When a droplet spawns on an existing trail, its color is mixed toward the trail cell color (80–28% trail weight depending on trail luma).

### Nannou translation notes

| OpenFrameworks | Nannou / Rust |
|---|---|
| `ofVec2f` | `Vec2` (glam, re-exported by nannou) |
| `ofRandomf(-a, a)` | `(random::<f32>() * 2.0 - 1.0) * a` |
| `ofRandomuf()` | `random::<f32>()` |
| `ofLerp(a, b, t)` | `a + (b - a) * t` |
| `ofClamp(x, lo, hi)` | `x.clamp(lo, hi)` |
| `color.setHsb(h, s, b)` (0–255 scale) | `hsv(h/255.0 * 360.0, s/255.0, b/255.0)` then convert |
| `ofDrawLine`, `ofDrawCircle` | `draw.line().start().end()`, `draw.ellipse()` |
| `ofDrawRectangle(x, y, w, h)` | `draw.rect().x_y(x + w/2, y + h/2).w_h(w, h)` |

---

## Phase 9 — `droplets` MIDI integration: design notes

### CC knob mapping (Preset 1, CC 24–28)

| CC | Param | Range |
|---|---|---|
| 24 | base hue (degrees) | 0 – 360 |
| 25 | spawn rate (droplets/s) | 10 – 300 |
| 26 | path attraction | 0.0 – 3.0 |
| 27 | vertical jitter | 0 – 120 |
| 28 | influence decay | 0.05 – 2.0 |

Speed, radius, and other params stay hardcoded at sensible defaults for phase 9; expose more knobs in a later pass if desired.

### Note-on behavior

Map MIDI note number to a Y position: `y = win.top() - (note / 127.0) * win.h()`. On note-on, spawn 4–8 droplets clustered around that Y (gaussian spread ±20 px). Velocity scales initial alpha and radius. This creates a melodic "rain curtain" effect where played notes anchor new trail columns.

### `hud_info`

Return the active droplet count and field saturation (fraction of cells above threshold) as the extra HUD line.
