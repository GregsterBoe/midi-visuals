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
| 7 | Performance scaling — batch geometry, instanced rendering, or GPU compute (only when FPS wall is hit) | `[TODO]` |

---

## Phase 7 — Performance scaling `[TODO]`

Only worth doing once you've actually hit the FPS wall with particles.

1. **Batch geometry** — build a single mesh of all particles per frame, one draw call. Big win, stays inside Nannou's API.
2. **Instanced rendering** — custom `wgpu` pipeline with a single quad mesh and a per-particle instance buffer. GPU does the work; CPU just writes positions and colours. Order-of-magnitude improvement over per-particle draw calls.

GPU compute (physics in a shader) is deferred until actually needed — likely only above ~100k particles.
