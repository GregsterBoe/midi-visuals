# Kaleidoscope Sketch — Implementation Plan

## Overview

A GPU shader–driven kaleidoscope reactive to MIDI/audio. Three ingredients compose the visual:

1. **Periodic base pattern** — `sin(freq * length(w))` over a warped UV gives crisp rings or stripes
2. **Domain warping** — draw the pattern at `uv + fbm(uv)` to produce liquid, organic motion
3. **Kaleidoscopic folding** — convert to polar, mod angle by `TAU/n`, mirror with `abs()` for instant symmetry

A Moiré bonus layer composites two slightly-detuned patterns with `abs(a - b)`.

---

## Architecture

```
Kaleidoscope (struct)
├── wgpu pipeline      — cached at init, never rebuilt per frame
├── uniform_buffer     — single mapped buffer, written each raw_render()
├── bind_group         — uniform binding, rebuilt only on resolution change
├── audio state        — energy, onset impulse, spectral centroid
├── chord state        — ChordKind → n_sym, hue
└── param knobs        — CC 1/24/25/26 mapped to freq/warp/speed/color
```

The sketch uses **`raw_render()`** for the shader pass. `view()` draws only the HUD overlay via nannou's draw API on top. This keeps the two rendering paths clean and independent.

---

## Uniform Buffer Layout

Alignment must be 16-byte boundaries for WGSL structs.

```rust
// Rust side — must match WGSL struct byte-for-byte
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    resolution:  [f32; 2],   // viewport size
    time:        f32,
    level:       f32,         // RMS envelope [0,1]
    freq:        f32,         // ring density (default 18.0)
    warp_str:    f32,         // domain warp strength [0, 1.5]
    n_sym:       f32,         // symmetry fold count
    speed:       f32,         // animation speed multiplier
    centroid:    f32,         // spectral centroid [0,1] → freq mod
    onset:       f32,         // onset impulse, decays exponentially
    moire_mix:   f32,         // second layer blend [0,1]
    moire_delta: f32,         // second layer freq offset
    hue:         f32,         // chord hue [0,1]
    sat:         f32,         // saturation [0,1]
    threshold:   f32,         // smoothstep half-width (default 0.015)
    mode:        f32,         // 0=B&W, 1=colorized, 2=inverted
}
// 16 × f32 = 64 bytes — clean alignment
```

---

## WGSL Shader Design

```wgsl
// Modifiable constants — change at top of file, no recompile of Rust
const FBM_OCTAVES: u32 = 5u;
const PI: f32 = 3.14159265;
const TAU: f32 = 6.28318530;

struct Uniforms { ... }  // mirrors Rust struct above
@group(0) @binding(0) var<uniform> u: Uniforms;

// ── Noise primitives ─────────────────────────────────────────────────────────

fn hash22(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p); let f = fract(p);
    let s = f * f * (3.0 - 2.0 * f);  // smoothstep
    return mix(
        mix(hash22(i),             hash22(i + vec2(1.0,0.0)), s.x),
        mix(hash22(i+vec2(0.,1.)), hash22(i + vec2(1.0,1.0)), s.x),
        s.y
    );
}

fn fbm(p_in: vec2<f32>) -> f32 {
    var p = p_in; var a = 0.5; var s = 0.0;
    for (var i = 0u; i < FBM_OCTAVES; i++) {
        s += a * noise(p);
        p *= 2.02;
        a *= 0.5;
    }
    return s;
}

// ── Kaleidoscope fold ────────────────────────────────────────────────────────

fn kfold(uv_in: vec2<f32>, n: f32) -> vec2<f32> {
    var uv = uv_in;
    let ang = atan2(uv.y, uv.x);
    let rad = length(uv);
    let sector = TAU / n;
    let a = abs(((ang % sector) + sector) % sector - sector * 0.5);
    return vec2<f32>(cos(a), sin(a)) * rad;
}

// ── Main pattern ─────────────────────────────────────────────────────────────

fn pattern(uv: vec2<f32>, freq: f32, time: f32, warp: f32) -> f32 {
    // Two-pass domain warp (Quilez style)
    let q = vec2<f32>(
        fbm(uv * 2.0 + vec2(0.0, time * 0.05)),
        fbm(uv * 2.0 + vec2(5.2, -time * 0.04))
    );
    let w = uv + warp * q;

    // Rings: can swap length(w) → w.x for stripes, w.x+w.y for diagonals
    let r = length(w);
    return sin(freq * r - time * u.speed + 6.0 * fbm(w * 1.5));
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv_raw = (pos.xy - 0.5 * u.resolution) / u.resolution.y;

    // Kaleidoscope fold
    let uv = kfold(uv_raw, u.n_sym);

    // Effective parameters (modulated by audio)
    let freq  = u.freq  + u.centroid * 8.0;     // centroid shifts ring density
    let warp  = u.warp_str + u.onset * 0.5;     // onset kicks the warp
    let t     = u.time;

    // Primary pattern
    let f0 = pattern(uv, freq, t, warp);

    // Moiré layer (slightly detuned angle + freq)
    let uv1  = kfold(uv_raw * 1.003, u.n_sym);  // tiny rotation via scale
    let f1   = pattern(uv1, freq + u.moire_delta, t, warp);
    let moir = abs(f0 - f1);

    // Blend and threshold
    let raw  = mix(f0, moir, u.moire_mix);
    let mask = smoothstep(-u.threshold, u.threshold, raw);

    // Color
    let bw  = vec3<f32>(mask);
    let col = hsl_to_rgb(u.hue, u.sat, 0.35 + mask * 0.5);
    let out = mix(bw, col, step(0.5, u.mode));   // mode 0=B&W, 1=color

    return vec4<f32>(out, 1.0);
}
```

A `hsl_to_rgb` utility function lives in the shader (pure math, ~10 lines).

---

## Modifiable Knobs (the "recipe levers")

| Parameter | Default | Range | Effect |
|-----------|---------|-------|--------|
| `freq` | 18.0 | 5–40 | Ring density — lower = sparse, higher = dense lace |
| `warp_str` | 0.6 | 0–1.5 | Warp depth — <0.3 rippling, >1.0 blob dissolution |
| `n_sym` | 6.0 | 3–16 | Fold count — 3=triangle, 6=snowflake, 12=crystal |
| `speed` | 0.8 | 0–3 | Animation rate |
| `threshold` | 0.015 | 0.005–0.08 | Edge softness — narrow=crisp, wide=blurry |
| `moire_mix` | 0.0 | 0–1 | Second layer blend — 0=off, 1=full Moiré |
| `moire_delta`| 1.5 | 0–5 | Freq offset between Moiré layers |
| `mode` | 0.0 | 0/1/2 | Color mode toggle |
| `FBM_OCTAVES`| 5 | 2–8 | Compile-time quality/perf lever |

Pattern shape variants (change one line in `pattern()`):
- `length(w)` → concentric rings (default)
- `w.x` → vertical stripes
- `w.x + w.y` → diagonal bars
- `sin(freq * w.x) * sin(freq * w.y)` → checkerboard grid

---

## MIDI / Audio Mapping

```
ChordKind::symmetry()  →  n_sym       (Major=8, Minor=6, Dim=12, Aug=4, Dom7=16)
ChordKind::hue()       →  hue         (chord color identity)
energy                 →  warp_str    (high energy = more liquid warping)
tension                →  freq        (+tension adds ring density)
onset impulse          →  onset       (kicks warp on note attacks, decays ~0.3s)
spectral centroid      →  centroid    (timbre brightness shifts freq)

CC 1   →  hue     [0,1]
CC 24  →  speed   [0.1, 3.0]
CC 25  →  freq    [5.0, 35.0]
CC 26  →  warp_str[0.0, 1.4]
```

Onset impulse logic in `update()`:
```rust
// On note-on detection:
self.onset = (self.onset + 0.8).min(1.0);
// Each frame:
self.onset *= (-dt * 5.0_f32).exp();  // decay τ ≈ 0.2s
```

---

## Implementation Steps

### Step 1 — Scaffold (no visuals yet)
- Create `src/sketches/kaleidoscope.rs` with empty `Kaleidoscope` struct
- Implement `Sketch` trait: `update()` advances time, `view()` draws HUD text, `raw_render()` is a no-op
- Register in `mod.rs`
- `cargo check` passes

### Step 2 — Full-screen quad pipeline
- Write WGSL shader to `src/sketches/kaleidoscope.wgsl` (embedded with `include_str!`)
- Build wgpu render pipeline in `Kaleidoscope::new()`:
  - Vertex shader: emit a hardcoded clip-space triangle covering the screen (3 verts, no buffer needed)
  - Fragment shader: the kaleidoscope code
  - Bind group layout: one uniform binding
- Create uniform buffer (64 bytes, `UNIFORM | COPY_DST`)
- Store pipeline + bind_group in struct

### Step 3 — Basic pattern (no warp, no fold)
- `raw_render()` writes uniforms and dispatches a render pass
- Start with plain `sin(freq * length(uv))` — verify the full-screen quad works
- Add smoothstep threshold
- Should show concentric B/W rings

### Step 4 — Kaleidoscopic fold
- Add `kfold()` to shader, wire `n_sym` uniform
- Test with n=6 (snowflake), verify fold symmetry
- Verify chord change updates n_sym in real time

### Step 5 — Domain warp
- Add `noise()` + `fbm()` to shader
- Wire `warp_str`, `centroid`, `onset` uniforms
- Tune default constants for a good looking baseline

### Step 6 — MIDI wiring
- Add `ChordKind` detection (can reuse the same detect logic from mandala)
- Wire energy/tension from `MidiState`
- Wire onset impulse decay
- Add CC params

### Step 7 — Moiré layer
- Add second `pattern()` call with `moire_delta` offset
- Blend with `abs(f0 - f1)` weighted by `moire_mix`
- Toggle with key `M` (maps moire_mix 0↔1)

### Step 8 — Color mode
- Add `hsl_to_rgb()` to shader
- Wire `hue` from chord, `sat` from CC
- Toggle B/W vs color with key `C`

### Step 9 — Pattern shape toggle (key `P`)
- Cycle through: rings → stripes → diagonal → checkerboard
- Implemented as a `u32` uniform `pattern_mode` switching in shader

### Step 10 — Performance tuning levers
- `FBM_OCTAVES` as a runtime uniform (not compile-time const) so it can be reduced live
- Resolution scale factor: render to half-res texture, upscale — halves fragment invocations
- Second domain warp pass (`w` → `r` → second fbm) is optional, toggle with key `Q`

---

## Optimization Plan

| Lever | Cost reduction | How |
|-------|---------------|-----|
| `fbm_octaves` = 3 | ~35% faster | drop from 5→3 octaves |
| Half-resolution render | ~75% faster | render to `half_res` texture, blit |
| Skip Moiré layer | ~30% faster | `moire_mix = 0` bypasses second pattern() call |
| Simple noise (no fbm, single octave) | ~50% faster | swap for non-fbm hash warp |
| Simpler hash (trianglewave) | ~10% faster | replace `sin(dot)` hash with bit-hash |

All levers are runtime uniforms or key toggles — no shader recompilation required except FBM_OCTAVES if kept compile-time.

---

## File Structure

```
src/sketches/
├── kaleidoscope.rs       — Rust struct, pipeline setup, uniform upload, MIDI wiring
├── kaleidoscope.wgsl     — pure shader, no Rust dependencies
```

The shader is a standalone WGSL file embedded at compile time with `include_str!("kaleidoscope.wgsl")`. This lets the shader be edited and tested independently (e.g. with naga-cli for validation) without touching Rust.

---

## Key Risks and Mitigations

**wgpu pipeline setup boilerplate** — this is the hardest part. Look at `cardano.rs`'s `raw_render()` if it has one, otherwise reference nannou's wgpu examples. The pipeline is built once and cached; the only per-frame work is `queue.write_buffer()` + render pass dispatch.

**Uniform struct alignment** — `#[repr(C)]` + `bytemuck::Pod` enforces correct packing. Verify 64-byte total size with `std::mem::size_of::<Uniforms>() == 64`. If the WGSL struct layout diverges, the shader will silently read garbage values.

**Texture format** — must match the swapchain surface format. Use `device.surface_texture_format()` or `TextureFormat::Bgra8UnormSrgb` (common nannou default) for the pipeline's color target.

**n_sym as float in mod** — `atan2` returns `(-π, π]`; the fold needs to bring angle into `[0, TAU/n)` before `abs()`. The WGSL `((ang % sector) + sector) % sector` pattern handles the negative-angle case correctly.
