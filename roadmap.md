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
