# Optimization Notes

Reference for future sketches. Written after a deep dive into Nannou 0.19 internals
while optimizing the particles sketch (Phase 7, step 1).

---

## Nannou Draw API — What It Actually Does

Understanding the cost model of `draw.*` calls is the foundation for all
rendering decisions.

- Every `draw.ellipse()`, `draw.rect()`, etc. **tessellates on the CPU** each
  frame. A circle is tessellated into roughly 64 triangles by default. For a
  single shape this is negligible; for thousands it dominates.
- Nannou's draw system is **retained-mode**: primitives accumulate in an
  intermediary mesh and are flushed to the GPU when `draw.to_frame()` is called.
  Multiple `draw.*` calls do not automatically become one GPU draw call.
- `draw.mesh().indexed_colored(points, indices)` bypasses Nannou's tessellator
  entirely — you supply raw vertices and indices and Nannou forwards them to the
  GPU as-is. This is the escape hatch for high object counts.

---

## General Optimization Patterns

### 1. Batch geometry with `draw.mesh()` for any sketch with many objects

Replace N individual `draw.ellipse()` / `draw.rect()` calls with one
`draw.mesh().indexed_colored()` call that contains all geometry.

```rust
// Instead of:
for obj in &self.objects {
    draw.ellipse().xy(obj.pos).radius(r).color(obj.color);
}

// Do:
draw.mesh().indexed_colored(
    self.mesh_verts.iter().copied(),
    self.mesh_idx.iter().copied(),
);
```

Break-even is roughly a few hundred objects; beyond that the gap widens fast.

**Nannou mesh API specifics (don't guess, these burned time):**
- `points` items must be `(P, C)` where `P: Into<Point3>` — that is `Vec3`,
  **not** `Vec2`. Use `vec3(x, y, 0.0)` for 2D positions.
- `indices` items are `usize`, not `u32`.
- `C: IntoLinSrgba<f32>` — `Hsla`, `Hsl`, `Hsva`, `LinSrgba`, `Srgba` all work.
  `Hsl` and `Hsla` are `Copy`, so `.iter().copied()` works on stored buffers.

### 2. Use quads instead of circles for small particles

A quad (square billboard) is 4 vertices + 6 indices = 2 triangles.
Nannou's tessellated circle is ~64 triangles. At radius ≤ 5 px the difference
is invisible on screen. The vertex count reduction is 16×.

For larger shapes where roundness matters, consider a 6- or 8-sided polygon
(hexagon/octagon) — still 4–8× cheaper than the default circle tessellation.

### 3. Pre-allocate mesh buffers on the struct; reuse every frame

Allocating a fresh `Vec` with 40 000 entries every frame at 60 fps means
the allocator sees ~2.4 million items/sec. Even with a fast allocator this
is measurable overhead and generates GC pressure in profiles.

```rust
pub struct MySketch {
    objects: Vec<Object>,
    mesh_verts: Vec<(Vec3, Hsla)>,  // pre-allocated, cleared each frame
    mesh_idx: Vec<usize>,
}

impl MySketch {
    pub fn new() -> Self {
        Self {
            objects: Vec::new(),
            mesh_verts: Vec::with_capacity(MAX_OBJECTS * 4),
            mesh_idx: Vec::with_capacity(MAX_OBJECTS * 6),
        }
    }
}
```

`Vec::clear()` drops elements but keeps the backing allocation. After the
first frame that fills the buffers to their peak, no further allocation occurs.

### 4. Build geometry in `update()`, not `view()`

The `Sketch` trait's `view()` takes `&self` (immutable). Building geometry
there would require `RefCell` for buffer reuse, or a fresh allocation every
frame. Instead, populate the buffers at the end of `update()` which has
`&mut self`:

```rust
fn update(&mut self, midi: &MidiState, dt: f32) {
    // ... physics, spawn, retain ...
    self.build_mesh();  // repopulate mesh_verts and mesh_idx
}

fn view(&self, draw: &Draw, _win: Rect) {
    if self.mesh_verts.is_empty() { return; }
    draw.mesh().indexed_colored(
        self.mesh_verts.iter().copied(),
        self.mesh_idx.iter().copied(),
    );
}
```

### 5. Cap unbounded collections properly

A per-event spawn loop that only checks the cap before each event can exceed
the cap if multiple events arrive in one frame. Use `saturating_sub` + `min`:

```rust
for _ in midi.note_on_events() {
    let remaining = CAP.saturating_sub(self.objects.len());
    if remaining > 0 {
        self.spawn(spawn_count.min(remaining), ...);
    }
}
```

### 6. Prefer `retain()` for dead-object removal

`Vec::retain()` is a single in-place pass with no extra allocation.
Alternatives like `drain_filter` or rebuilding the vec have worse cache
behaviour or need a temporary buffer.

```rust
self.particles.retain(|p| p.lifetime > 0.0);
```

### 7. Avoid holding the MIDI mutex across slow work

The MIDI callback runs on its own thread and tries to lock the same mutex.
Lock, copy/drain what you need, drop immediately, then do work:

```rust
let snapshot = {
    let mut s = model.midi_state.lock().unwrap();
    let events: Vec<_> = s.recent_events.drain(..).collect();
    MidiState { ccs: s.ccs, notes: s.notes, recent_events: events.into() }
    // lock drops here
};
model.active.update(&snapshot, dt);
```

---

## Per-Sketch Optimization Summary

### `particles`

**Bottleneck removed:** Nannou's per-particle `draw.ellipse()` tessellating
~64 triangles each on the CPU, submitted as N separate draw primitives.

**What was done:**
- Replaced per-particle `draw.ellipse()` with a single
  `draw.mesh().indexed_colored()` call that covers all particles at once.
- Each particle is now a 4-vertex quad (2 triangles) — 16× fewer vertices
  per particle vs. the default circle tessellation.
- `mesh_verts: Vec<(Vec3, Hsla)>` and `mesh_idx: Vec<usize>` live on the
  struct, pre-allocated at `10_000 × 4` and `10_000 × 6` capacity.
- Buffers are rebuilt at the end of `update()` via `build_mesh()` (which has
  `&mut self`), so `view()` only reads — no allocation, no `RefCell` needed.

**Remaining ceiling:** At very high counts (approaching 100k), the CPU-side
`build_mesh()` loop itself becomes the bottleneck. That is the trigger for
Phase 7 step 2 (instanced rendering via a custom `wgpu` pipeline).

---

*Add a new section under "Per-Sketch" for each sketch that receives a
non-trivial optimization pass.*
