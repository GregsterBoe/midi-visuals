# midi-visuals — Development Roadmap

A phased plan for building the project. Each phase is sized to fit one focused
session with an AI assistant: small enough to review carefully, large enough to
end with something visibly working.

---

## Architecture in one paragraph

Three layers, each with a single job. **The launcher** (`main.rs`) owns the
Nannou app, the window, and the currently active sketch — nothing else.
**The MIDI layer** (`midi.rs`) runs on a background thread, listens to incoming
messages, and writes them into a shared `MidiState` struct guarded by a `Mutex`.
**Sketches** are independent modules that implement a common `Sketch` trait;
they read whatever they want from `MidiState` and ignore the rest. Switching
between sketches means swapping a `Box<dyn Sketch>` in the launcher — the rest
of the app doesn't care which sketch is active.

```
┌──────────────────────────────────────────────────────┐
│ main.rs                                              │
│  ┌──────────┐    ┌────────────────────┐              │
│  │ Nannou   │    │ active: Box<dyn    │              │
│  │ window   │    │   Sketch>          │              │
│  └──────────┘    └────────────────────┘              │
│         ▲                  ▲                         │
│         │ view()           │ update()                │
│         │                  │                         │
│  ┌──────┴──────────────────┴────────┐                │
│  │ Arc<Mutex<MidiState>>            │ ◀─ midir       │
│  └──────────────────────────────────┘    callback    │
│                                          (bg thread) │
└──────────────────────────────────────────────────────┘
```

---

## Locked-in design decisions

These are decided up front so we don't relitigate them mid-build:

- **Generic MIDI state.** `MidiState` exposes raw CCs (0–127), note states, and
  a queue of recent note events. Sketches map what they care about themselves —
  no global "this knob is hue" mapping.
- **Trait-object dispatch.** Sketches behind `Box<dyn Sketch>`. Cost is one
  vtable lookup per `update`/`view` call, which is negligible. The win is that
  live switching is just an assignment.
- **Per-sketch state, recreated on switch.** When you swap to a sketch, you get
  a fresh instance via a factory closure. No leftover state from previous runs.
  Simpler mental model; we can add "pause and resume" later if it ever matters.
- **`Mutex<MidiState>` for v1.** Lock contention is a non-issue at MIDI rates
  (a busy controller sends a few hundred messages a second; we lock for
  microseconds). If profiling later shows a problem, we swap to `crossbeam`
  channels or atomics — but not before.
- **Nannou's `draw` API for now.** Skip custom `wgpu` pipelines until a sketch
  actually needs them. Particle systems can ride on `draw.ellipse()` for the
  first few thousand particles; instanced rendering is a Phase 6 concern.

---

## Phase 0 — Project skeleton `[DONE]`

**Goal:** the directory structure from SETUP.md exists, all modules compile,
the blue-circle window from the verify step still renders.

What gets built:

- Create the folder layout (`src/main.rs`, `src/midi.rs`, `src/sketches/mod.rs`,
  `src/sketches/aurora.rs`).
- `midi.rs` and `sketches/aurora.rs` are empty stubs (just a `pub fn placeholder() {}`
  so they compile).
- `mod.rs` declares the `aurora` submodule.
- `main.rs` is the verify-step sketch from SETUP.md, unchanged.

**Done when:** `cargo run` opens the blue-circle window and `cargo check`
passes with no warnings other than dead-code stubs.

> **Why bother with this phase?** Because committing the layout before there's
> any logic in it means every later phase has a clear home for its code. It's
> 15 minutes of work that prevents an hour of "where does this go?" later.

---

## Phase 1 — MIDI input layer `[DONE]`

**Goal:** turning a knob on a connected MIDI device prints the CC number and
value to the console. No visuals yet.

What gets built:

- `MidiState` struct in `midi.rs`:
  ```rust
  pub struct MidiState {
      pub ccs: [f32; 128],          // normalised 0.0–1.0
      pub notes: [NoteState; 128],  // velocity + on/off + last_change
      pub recent_events: VecDeque<NoteEvent>,  // ringbuffer, cap ~64
  }
  ```
- `start()` function that opens a midir input connection, spawns the
  background callback, and returns `Arc<Mutex<MidiState>>` plus the
  `MidiInputConnection` (which must be kept alive — store it on the model
  or it gets dropped and the connection closes).
- Port selection: list available ports, pick the first one for now, print
  which one was chosen. We'll make this configurable in a later phase.
- `main.rs` calls `midi::start()` in `model()`, stores the result on the
  Model, and in `update()` locks the state and prints any CC that changed
  since last frame.

**Done when:** running with a controller connected, twiddling a knob produces
output like `CC 21 = 0.347` on stdout. Closing the window cleanly disconnects.

**Gotchas to flag for the AI assistant:**

- `midir`'s callback runs on a thread it owns — `Send + 'static` bounds apply
  to the closure.
- Don't hold the mutex across the print loop; lock, copy what you need, drop,
  then print.
- If no MIDI port is available, print a helpful message and continue with a
  dummy `MidiState` rather than panicking. Useful for developing visuals on
  the train.

---

## Phase 2 — Sketch trait and first real sketch `[DONE]`

**Goal:** the trait abstraction exists, and `aurora` is a real sketch reactive
to MIDI input.

What gets built:

- The trait, in `sketches/mod.rs`:
  ```rust
  pub trait Sketch {
      fn update(&mut self, midi: &MidiState, dt: f32);
      fn view(&self, draw: &Draw, win: Rect);
      fn name(&self) -> &'static str;
  }
  ```
  Object-safe (no generics on methods, no `Self` in return types). `dt` is
  delta-time in seconds since the last update — sketches that animate over
  time will want it.
- `aurora` implementation: a simple sketch where, say, CC 21 drives the hue
  of a glowing circle and CC 22 drives its radius. Pick any two CCs — the
  point is to prove MIDI → visual works.
- `main.rs` holds an `active: Box<dyn Sketch>` field, calls `active.update()`
  in `update()` and `active.view()` in `view()`. Aurora is hardcoded for now.

**Done when:** turning the two mapped knobs visibly changes the circle's hue
and size in real time, with no perceptible lag.

**Note on the trait shape:** by passing `&Draw` rather than the full `Frame`,
you keep sketches simple and consistent. If a future sketch needs raw `wgpu`
access, we can add a second method like `fn render_custom(&self, frame: &Frame)`
with a default empty implementation.

---

## Phase 3 — Sketch registry and switching `[DONE]`

**Goal:** launch with `cargo run -- aurora` to pick a starting sketch, and
press a key during runtime to cycle to the next one.

What gets built:

- A registry in `sketches/mod.rs`:
  ```rust
  type SketchFactory = fn() -> Box<dyn Sketch>;
  pub fn registry() -> Vec<(&'static str, SketchFactory)> {
      vec![
          ("aurora", || Box::new(aurora::Aurora::new())),
          // more added here as sketches are written
      ]
  }
  ```
  A `Vec` (not a `HashMap`) so the cycle order is stable and predictable.
- CLI parsing in `main.rs`: read `std::env::args().nth(1)`, look it up in the
  registry, fall back to the first entry if missing or unknown. No need to
  pull in `clap` for this — one positional arg is fine.
- Keyboard handling: in Nannou's `event` callback (or by storing the key state
  on Model), swap `active` to the next factory in the registry when (say)
  `Tab` is pressed.

**Done when:** `cargo run -- aurora` starts on aurora, `cargo run` (no arg)
also starts on aurora, `cargo run -- nonexistent` falls back to aurora with
a warning, and pressing `Tab` cycles through everything in the registry.

> Right now there's still only one sketch, so cycling is a no-op. That's fine —
> Phase 4 adds the second one and proves switching actually works.

---

## Phase 4 — Second sketch and a debug HUD `[CURRENT]`

**Goal:** validate the abstraction by adding a second, deliberately different
sketch, and add an on-screen overlay so you can see what's happening at a
glance.

What gets built:

- `sketches/grid.rs` — a sketch with totally different visual logic. Suggestion:
  an N×M grid of squares where note-on events light up the cell at
  `note_number % grid_width`. Different enough from aurora that any leaked
  state would be obvious.
- Register it in the registry.
- A small HUD drawn in `main.rs` (after the active sketch's view, so it
  overlays): current sketch name, FPS (Nannou exposes this on `App`), and a
  simple "MIDI active" dot that lights when any CC changed in the last
  500ms. Top-left corner, small font, semi-transparent.
- A toggle key (e.g. `H`) to hide the HUD for clean visuals.

**Done when:** Tab switches cleanly between aurora and grid, no flicker or
state bleed, HUD shows the right sketch name, FPS is steady around your
monitor's refresh rate.

This is also the right phase to commit a `git tag v0.1` — you have a working
multi-sketch MIDI visualizer. Everything after this is making it richer.

---

## Phase 5 — First particle sketch `[TODO]`

**Goal:** prove the architecture handles many objects, establish a baseline
for the performance work that comes later.

What gets built:

- `sketches/particles.rs` — `Vec<Particle>` where each particle has position,
  velocity, lifetime, colour. Note-on events spawn N particles at random
  positions; CCs control gravity, drag, spawn count, hue range.
- Use `draw.ellipse()` per particle for now. Don't optimise yet — the point
  is to see where the wall is.
- Add a particle count to the HUD.

**Done when:** particles spawn on note-on, animate smoothly, FPS stays at
target with at least a few thousand particles. Note the particle count where
FPS starts dropping — that number is the input for Phase 6.

**What you'll likely learn:** `draw.ellipse()` is fine up to a few thousand
particles, then it becomes the bottleneck because each call is a separate
draw command. That's the cue for Phase 6.

---

## Phase 6 — Performance scaling (deferred) `[TODO]`

**Goal:** raise the particle ceiling. Only worth doing when you've actually
hit the wall in Phase 5.

Options, roughly in order of effort:

- **Batch geometry.** Build a single mesh of all particles per frame and submit
  it as one draw call. Big win, moderate effort, stays inside Nannou's API.
- **Instanced rendering.** Custom `wgpu` pipeline with a single quad mesh and
  a per-particle instance buffer (position, colour, size). The GPU does the
  work; CPU just writes the instance buffer. Order-of-magnitude improvement
  over per-particle draw calls.
- **GPU compute for particle update.** Move the per-frame physics into a
  compute shader. Only worth it for >100k particles.

I deliberately don't sketch the code for this phase — by the time you're
here, you'll know which option you need based on what's slow. Premature
shader work is the easiest way to lose two weeks on this kind of project.

---

## How to work through this with an AI assistant

A pattern that tends to work well:

- **One phase per session.** Start each session by pasting the relevant phase
  description plus a brief "current state" summary (what files exist, what
  works, any deviations from the plan). The AI doesn't need the whole
  conversation history — it needs the contract for this phase.
- **Treat "Done when" as the acceptance test.** If the AI hands you code that
  doesn't satisfy the bullet, it isn't done. Resist the urge to push forward
  until the previous phase's criterion is met — it's much harder to debug a
  broken Phase 4 if Phase 2's foundation is wobbly.
- **Commit after every phase.** Even a messy commit. Phase boundaries are
  natural rollback points if a later phase reveals an earlier mistake.
- **Keep this file open.** When the AI suggests something that contradicts the
  "Locked-in decisions" section, push back — those decisions exist so they
  don't get re-debated every session.

---

## Open questions for later (don't worry about now)

- Do you want sketch parameters (the CC mappings) configurable via a TOML file,
  or hardcoded in each sketch? Hardcoded is simpler; TOML is friendlier once
  you have ten sketches. Decide around Phase 4 or 5.
- Audio output? The current plan is MIDI-in only, no sound. If you want
  sketches to also drive synthesis, that's a whole separate layer (probably
  `cpal` + `fundsp`) — worth scoping as a separate roadmap.
- Recording output to video? Nannou can capture frames; ffmpeg can stitch them.
  Easy to bolt on later when you want a portfolio reel.