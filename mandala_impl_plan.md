# Mandala — Implementation Plan
## Ranked by visual impact for complexity and variety

---

## Step 1 — PetalForm enum + Bezier petal shapes
**Impact: Foundational — everything above Tier 1 depends on this**

Currently every ring uses an `ellipse()`. One `PetalForm` enum unlocks the entire vocabulary.

### Forms to implement

```rust
enum PetalForm {
    Ellipse,                  // current — keep as fallback
    Lotus,                    // bezier concave petal, pointed tip
    Flame,                    // asymmetric swept bezier, tip curves sideways
    Spike,                    // isoceles triangle, sharp triangular wedge
    Rose { k: f32 },          // polar curve r=cos(k·θ), drawn as closed polyline
    Hypotrochoid { ratio: f32, pen: f32 }, // spirograph, drawn as closed polyline
}
```

### Lotus petal path

Nannou has `draw.path().fill()` using lyon `PathBuilder`. A lotus petal along the +x axis:
```rust
fn build_lotus(b: &mut PathBuilder, length: f32, half_w: f32, concavity: f32) {
    // concavity 0.3..0.7: how much the sides curve inward
    b.move_to(point(0.0, half_w));
    b.cubic_bezier_to(
        point(length * (0.55 - concavity * 0.25), half_w * 0.9),
        point(length * 0.85, half_w * 0.3),
        point(length, 0.0),  // tip
    );
    b.cubic_bezier_to(
        point(length * 0.85, -half_w * 0.3),
        point(length * (0.55 - concavity * 0.25), -half_w * 0.9),
        point(0.0, -half_w),
    );
    b.close();
}
```

Then transform the path by rotation+translation at each petal angle:
```rust
// For petal at angle θ on ring_r:
let transform = Transform::rotation(θ).then_translate(vec2(
    cx + ring_r * θ.cos(),
    cy + ring_r * θ.sin(),
));
draw.path().fill().transform(transform).events(path.iter());
```

### Flame petal

Same as lotus but asymmetric: one ctrl point pushed forward, the other swept sideways.
```rust
// sweep_dir = +1.0 for CW ring spin, -1.0 for CCW
fn build_flame(b: &mut PathBuilder, length: f32, half_w: f32, sweep: f32) {
    b.move_to(point(0.0, half_w));
    b.cubic_bezier_to(
        point(length * 0.35, half_w * 0.95 + sweep * half_w * 0.5),
        point(length * 0.75, half_w * 0.15 + sweep * half_w * 0.7),
        point(length, sweep * half_w * 0.4),  // tip swept
    );
    b.cubic_bezier_to(
        point(length * 0.65, -half_w * 0.25 + sweep * half_w * 0.4),
        point(length * 0.30, -half_w * 0.80),
        point(0.0, -half_w),
    );
    b.close();
}
```

### Spike

Simple triangle — no path builder needed, use `draw.polygon()`:
```rust
fn draw_spike(draw: &Draw, cx: f32, cy: f32, r: f32, theta: f32,
              length: f32, base_w: f32, color: Hsla) {
    let tip    = pt2(cx + (r + length) * theta.cos(), cy + (r + length) * theta.sin());
    let perp_x = -theta.sin() * base_w * 0.5;
    let perp_y =  theta.cos()  * base_w * 0.5;
    let base_l = pt2(cx + r * theta.cos() + perp_x, cy + r * theta.sin() + perp_y);
    let base_r = pt2(cx + r * theta.cos() - perp_x, cy + r * theta.sin() - perp_y);
    draw.polygon().points([tip, base_l, base_r]).color(color);
}
```

### Rose curve (drawn once per ring, not per petal)

```rust
fn draw_rose(draw: &Draw, cx: f32, cy: f32, a: f32, k: f32, rot: f32, color: Hsla) {
    let pts: Vec<_> = (0..=512).map(|i| {
        let t = i as f32 / 512.0 * TAU * 2.0;  // 2× to close all petals
        let r = a * (k * t).cos();
        pt2(cx + r * (t + rot).cos(), cy + r * (t + rot).sin())
    }).collect();
    draw.polyline().stroke_weight(1.5).points(pts).color(color);
}
// k = symmetry N/2 gives N petals (k even → 2k petals, k odd → k petals)
// so: k = N/2 for even N, k = N for odd N
```

### Hypotrochoid (spirograph ring)

```rust
fn draw_hypotrochoid(draw: &Draw, cx: f32, cy: f32, big_r: f32, small_r: f32,
                     d: f32, rot: f32, color: Hsla) {
    let steps = 2048;
    let diff = big_r - small_r;
    let ratio = diff / small_r;
    // Number of cycles to close: lcm-based, approximate with large step count
    let pts: Vec<_> = (0..=steps).map(|i| {
        let t = i as f32 / steps as f32 * TAU * small_r.ceil();
        pt2(
            cx + diff * (t + rot).cos() + d * (ratio * (t + rot)).cos(),
            cy + diff * (t + rot).sin() - d * (ratio * (t + rot)).sin(),
        )
    }).collect();
    draw.polyline().stroke_weight(1.2).points(pts).color(color);
}
// ratio presets for chord types:
// Major (N=8):       small_r/big_r = 3/8  → 8-petal spirograph
// Minor (N=6):       small_r/big_r = 2/6  → 6-petal
// Diminished (N=12): small_r/big_r = 5/12 → 12-petal
// Augmented (N=4):   small_r/big_r = 1/4  → astroid (4-pointed)
// Dom7th (N=16):     small_r/big_r = 7/16 → 16-point
```

---

## Step 2 — Per-chord RingSpec table
**Impact: Makes each chord look like a fundamentally different mandala**

Replace the static constants (`RING_R_FRAC`, `RING_PETAL_DIM`, etc.) with a per-chord ring specification.

### Data structure

```rust
#[derive(Clone)]
struct RingSpec {
    r_frac:   f32,          // fraction of max_r for this ring's center radius
    n_mult:   f32,          // multiplier on base symmetry N (can be non-integer)
    form:     PetalForm,
    spin:     f32,          // +1.0 CW, -1.0 CCW
    len_mult: f32,          // petal radial length as fraction of ring_spacing
    wid_mult: f32,          // petal tangential width as fraction of ring_spacing
    hue_off:  f32,          // hue delta added to chord base hue
    lum:      f32,
    alpha:    f32,
}
```

### Per-chord tables

```
Major (N=8): 5 rings
  0: r=0.12  Spike       n×1   — sharp inner star
  1: r=0.26  Lotus       n×1   — classic elegant petal
  2: r=0.38  Ellipse     n×2   — dense middle ring
  3: r=0.48  Lotus       n×1   — outer layer, large petals
  4: r=0.57  Rose{k=4}   n×1   — continuous rose curve border

Minor (N=6): 5 rings
  0: r=0.12  Lotus       n×1   — soft inner petal
  1: r=0.25  Ellipse     n×2   — dense inner
  2: r=0.37  Flame       n×1   — moody swept middle
  3: r=0.49  Ellipse     n×1   — outer ellipse
  4: r=0.58  Spike       n×3   — dense fine spikes at edge

Diminished (N=12): 5 rings
  0: r=0.11  Spike       n×1   — radial spikes outward
  1: r=0.22  Flame       n×1   — chaos ring
  2: r=0.33  Hypotrochoid{5/12} — continuous spirograph
  3: r=0.44  Spike       n×1   — second spike ring
  4: r=0.56  Rose{k=6}   n×1   — 12-petal rose curve

Augmented (N=4): 4 rings (fewer = emptier = more eerie)
  0: r=0.14  Spike       n×1   — bold 4 spikes
  1: r=0.30  Hypotrochoid{1/4} — astroid spirograph (4-pointed)
  2: r=0.46  Spike       n×2   — 8 thinner spikes
  3: r=0.58  Rose{k=2}   n×1   — 4-petal rose

Dom7th (N=16): 6 rings (most dense)
  0: r=0.10  Spike       n×1   — fine 16 spikes
  1: r=0.20  Lotus       n×1   — inner lotus
  2: r=0.30  Ellipse     n×2   — double-density middle
  3: r=0.40  Flame       n×1   — driven ring
  4: r=0.50  Hypotrochoid{7/16} — complex spirograph
  5: r=0.58  Rose{k=8}   n×1   — 16-petal outer rose

Other (N=5): 4 rings
  0: r=0.14  Lotus       n×1   — single lotus ring
  1: r=0.30  Ellipse     n×1   — simple ellipse
  2: r=0.46  Rose{k=5/2} n×1   — 5-petal rose (non-integer k)
  3: r=0.57  Spike       n×2   — 10 fine spikes
```

### Implementation

Add a method to `ChordKind`:
```rust
impl ChordKind {
    fn ring_specs(self) -> Vec<RingSpec> { ... }
}
```

The `Mandala` struct stores `current_specs: Vec<RingSpec>` (updated on chord change). `draw_ring_layer` dispatches on `spec.form` via `match`.

---

## Step 3 — Lissajous Core
**Impact: The center changes completely per chord — most visible area of the mandala**

Replace the current N-spike star at the center with a Lissajous figure whose a:b ratio is determined by the chord's primary interval.

### Interval detection

```rust
fn dominant_interval(notes: &[NoteState; 128]) -> (u32, u32) {
    let held: Vec<u8> = (0..128u8).filter(|&n| notes[n as usize].on).collect();
    if held.len() < 2 { return (1, 1); }
    // Use highest-priority interval in the held set
    let root = held[0];
    let ivs: Vec<u8> = held.iter().map(|&n| (n + 12 - root) % 12).collect();
    // Priority: 5th > maj3 > min3 > 4th > maj6 > tritone > everything else
    match () {
        _ if ivs.contains(&7)  => (3, 2),   // perfect 5th → trefoil
        _ if ivs.contains(&4)  => (5, 4),   // major 3rd → 5:4 figure
        _ if ivs.contains(&3)  => (6, 5),   // minor 3rd → 6:5 figure
        _ if ivs.contains(&5)  => (4, 3),   // perfect 4th → 4:3 figure
        _ if ivs.contains(&9)  => (5, 3),   // major 6th
        _ if ivs.contains(&10) => (7, 4),   // minor 7th → complex
        _ if ivs.contains(&6)  => (45, 32), // tritone → near-irrational, drifting
        _                       => (1, 1),  // unison/octave → slow ellipse
    }
}
```

### Drawing the Lissajous

```rust
fn draw_lissajous_core(&self, draw: &Draw, cx: f32, cy: f32, max_r: f32) {
    let (a, b) = self.lissajous_ratio;
    let scale = max_r * 0.095;
    let delta = self.time * 0.08;  // slowly rotating phase — keeps it alive
    // Sample enough points to close: period = TAU * lcm(a,b)/a steps
    let n_steps = (a.max(b) * 128).min(2048) as usize;
    let pts: Vec<_> = (0..=n_steps).map(|i| {
        let t = i as f32 / n_steps as f32 * TAU * b as f32;
        pt2(
            cx + scale * (a as f32 * t + delta).sin(),
            cy + scale * (b as f32 * t).sin(),
        )
    }).collect();
    let hue = self.chord.hue(self.p_hue);
    draw.polyline()
        .stroke_weight(2.0)
        .points(pts)
        .color(hsla(hue, self.p_sat, 0.85, 0.90));
    // Bright center dot on top
    draw.ellipse().x_y(cx, cy).radius(max_r * 0.018)
        .color(hsla(hue, self.p_sat, 0.95, 1.0));
}
```

**Store** `lissajous_ratio: (u32, u32)` on the struct, updated when pitch mask changes.

For the tritone (45:32 ratio), sample at integer steps: loop from 0 to 32×TAU, and the figure drifts. This is intentional — the phase delta makes it never quite close, visually representing irresolution.

---

## Step 4 — Symmetry Morphing
**Impact: Chord transitions become animated events, not instant snaps**

On chord change, keep drawing BOTH the old and new symmetry simultaneously with cross-fading alpha.

### Data

```rust
struct SymmetryMorph {
    from_n:    usize,
    from_specs: Vec<RingSpec>,
    to_n:      usize,
    t:         f32,           // 0.0 → 1.0
    speed:     f32,           // how fast to morph (default 1.8)
}
```

Add `morph: Option<SymmetryMorph>` to `Mandala`.

### Update

```rust
if let Some(m) = &mut self.morph {
    m.t += dt * m.speed;
    if m.t >= 1.0 { self.morph = None; }
}
```

On chord change:
```rust
self.morph = Some(SymmetryMorph {
    from_n:    old_sym,
    from_specs: old_specs.clone(),
    to_n:      self.symmetry,
    t:         0.0,
    speed:     1.8,
});
```

### Drawing

```rust
if let Some(m) = &self.morph {
    let alpha_old = ease_out(1.0 - m.t);
    let alpha_new = ease_in(m.t);
    // Draw old specs with alpha_old
    for spec in &m.from_specs {
        self.draw_ring_with_spec(draw, cx, cy, max_r, spec, m.from_n, alpha_old);
    }
    // Draw new specs with alpha_new
    for spec in &self.current_specs {
        self.draw_ring_with_spec(draw, cx, cy, max_r, spec, self.symmetry, alpha_new);
    }
} else {
    // normal draw
}

fn ease_in(t: f32)  -> f32 { t * t }
fn ease_out(t: f32) -> f32 { 1.0 - (1.0 - t) * (1.0 - t) }
```

---

## Step 5 — Tension → Petal Shape Morph
**Impact: Dissonance visibly deforms the mandala — petals mutate toward flame spikes**

Currently tension only elongates petals (`1.0 + tension * 0.55`). Extend to morph the bezier shape.

### In the petal drawing dispatch

When form is `Lotus` or `Ellipse`, the draw function receives `tension: f32` and blends toward `Flame`:

```rust
fn draw_petal_at(draw: &Draw, form: &PetalForm, x: f32, y: f32,
                 angle: f32, length: f32, half_w: f32,
                 tension: f32, spin_dir: f32, color: Hsla) {
    match form {
        PetalForm::Ellipse => {
            if tension < 0.3 {
                draw.ellipse().x_y(x, y).w_h(length, half_w * 2.0).rotate(angle).color(color);
            } else {
                // Blend toward flame as tension rises
                let sweep = (tension - 0.3) / 0.7 * 0.8 * spin_dir;
                let path = build_flame_path(length, half_w * (1.0 - tension * 0.4), sweep);
                draw_path_at(draw, path, x, y, angle, color);
            }
        }
        PetalForm::Lotus => {
            let concavity = 0.45 * (1.0 - tension * 0.6);  // concavity → 0 (straighter) under tension
            let sweep = tension * 0.65 * spin_dir;           // sweeps sideways
            let path = build_lotus_or_flame_path(length, half_w, concavity, sweep);
            draw_path_at(draw, path, x, y, angle, color);
        }
        // Spikes and Rose curves don't respond to tension (they're already sharp)
        _ => { /* draw normally */ }
    }
}
```

Also: saturate the hue toward the tension color (red-magenta) at high tension:
```rust
let tension_hue = 0.82;  // red-magenta
let effective_hue = lerp(base_hue, tension_hue, tension * 0.4);
```

---

## Step 6 — Dihedral Symmetry for Stable Chords
**Impact: Major/Augmented feel crystalline and static; Minor/Diminished feel spinning**

Dihedral = draw each motif AND its mirror image. This doubles visual density and adds reflection axes.

```rust
impl ChordKind {
    fn dihedral(self) -> bool {
        matches!(self, Self::Major | Self::Augmented)
    }
}
```

In the ring drawing loop, if `dihedral` is true, draw each petal twice: once at angle θ, once at -θ (mirrored across the x-axis through the ring center):

```rust
for i in 0..n {
    let angle = rot + i as f32 * TAU / n as f32;
    draw_petal_at(draw, form, ..., angle, ...);
    if spec.dihedral {
        // Mirror: flip the angle across 0 (reflect across the ring's radial axis)
        let mirror_angle = -angle + 2.0 * rot;  // reflect across current rot line
        draw_petal_at(draw, form, ..., mirror_angle, ...);
    }
}
```

The dihedral version also stops rotating (or rotates 4× slower) to emphasize its crystalline quality:
```rust
let effective_spin = if self.chord.dihedral() { spec.spin * 0.15 } else { spec.spin };
```

---

## Step 7 — Phi Ring Radii
**Impact: Subtle but changes overall proportions — nautilus shell spacing feels more organic**

Replace the current linear `RING_R_FRAC` per chord with phi-exponential computed radii.

```rust
const PHI: f32 = 1.618_034;

fn phi_radii(n_rings: usize) -> Vec<f32> {
    // Innermost ring at 1/φ⁴ of max_r, each successive ring × φ
    // Normalized so the outermost ring hits 0.58 max_r
    let outer = 0.58f32;
    let inner = outer / PHI.powi(n_rings as i32 - 1);
    (0..n_rings).map(|i| inner * PHI.powi(i as i32)).collect()
}
```

The `RingSpec.r_frac` values in the per-chord table (Step 2) should be generated via this function instead of hardcoded. This way, ring spacing is always in golden ratio regardless of how many rings a chord has.

---

## Step 8 — Breathing Pulse
**Impact: Mandala feels alive even when no notes are playing — organic expansion/contraction**

Add a sinusoidal scale factor applied to all ring radii. Rate tied to energy.

```rust
// In Mandala struct:
breath_phase: f32,

// In update():
let breath_rate = 0.4 + self.energy * 1.2;  // 0.4 Hz quiet → 1.6 Hz dense playing
self.breath_phase = (self.breath_phase + dt * breath_rate * TAU).rem_euclid(TAU);

// In draw_ring_layer() / draw_ring_with_spec():
let breath = 1.0 + 0.055 * self.breath_phase.sin();
let effective_r = ring_r * breath;
// Also apply opposite phase to petal size (rings shrink → petals grow slightly)
let breath_len = 1.0 - 0.025 * self.breath_phase.sin();
let p_len = spec_len * breath_len;
```

At high energy, the breathing becomes fast enough to feel like pulsing with the music. Slow breath = meditative.

---

## Step 9 — Star Polygon Border
**Impact: Outer boundary changes per chord, adds geometric precision**

Replace the current dot ring with a stroke-only `{N/k}` star polygon.

```rust
impl ChordKind {
    fn border_skip(self) -> usize {
        match self {
            Self::Major      => 3,   // {8/3} octagram
            Self::Minor      => 2,   // {6/2} hexagram (Star of David)
            Self::Diminished => 5,   // {12/5} Islamic 12-point star
            Self::Augmented  => 1,   // {4/1} square (no skip, just the N-gon)
            Self::Dom7th     => 7,   // {16/7} complex 16-point star
            Self::Other      => 2,   // {5/2} pentagram
        }
    }
}

fn draw_star_polygon(draw: &Draw, cx: f32, cy: f32, r: f32, n: usize, skip: usize,
                     rot: f32, color: Hsla) {
    // Walk the star by jumping `skip` vertices at a time
    let mut pts = Vec::new();
    let mut i = 0usize;
    let start = 0;
    loop {
        let a = rot + i as f32 / n as f32 * TAU;
        pts.push(pt2(cx + r * a.cos(), cy + r * a.sin()));
        i = (i + skip) % n;
        if i == start && pts.len() > 1 { break; }
    }
    pts.push(pts[0]);  // close
    draw.polyline().stroke_weight(1.5).points(pts).color(color);
}
// Called at r = max_r * 0.63, rotating slowly with RING_SPIN[4]
```

For Augmented (skip=1), it becomes just a square outline — which fits its rigid, eerie quality perfectly.

---

## Step 10 — 4 Gates for Dom7th
**Impact: Niche but Dom7th now has unique structural architecture no other chord has**

T-shaped gateway elements at cardinal directions, only drawn when `chord == Dom7th`.

```rust
fn draw_gates(&self, draw: &Draw, cx: f32, cy: f32, max_r: f32) {
    if self.chord != ChordKind::Dom7th { return; }
    let gate_r = max_r * 0.68;
    let bar_len = max_r * 0.08;
    let post_len = max_r * 0.06;
    let hue = self.chord.hue(self.p_hue);

    for i in 0..4 {
        let angle = i as f32 * FRAC_PI_2 + self.ring_rot[4] * 0.05; // nearly static
        let gx = cx + gate_r * angle.cos();
        let gy = cy + gate_r * angle.sin();
        let perp_x = -angle.sin();
        let perp_y =  angle.cos();
        // Horizontal bar of T
        draw.line()
            .start(pt2(gx - perp_x * bar_len, gy - perp_y * bar_len))
            .end(  pt2(gx + perp_x * bar_len, gy + perp_y * bar_len))
            .stroke_weight(2.5)
            .color(hsla(hue, self.p_sat, 0.72, 0.75));
        // Vertical post of T (pointing inward)
        draw.line()
            .start(pt2(gx, gy))
            .end(  pt2(gx - angle.cos() * post_len, gy - angle.sin() * post_len))
            .stroke_weight(2.5)
            .color(hsla(hue, self.p_sat, 0.72, 0.75));
    }
}
```

---

## Step 11 — Fibonacci Scatter Mode (key toggle)
**Impact: Completely different petal placement logic — aperiodic, organic, breaking all symmetry**

Press `F` to toggle. Instead of N evenly-spaced petals, distribute petals on the golden angle spiral.

```rust
const GOLDEN_ANGLE: f32 = 2.399_963;  // 2π / φ²

fn draw_fibonacci_ring(&self, draw: &Draw, cx: f32, cy: f32, max_r: f32, n: usize) {
    let n_petals = n * 8;  // more petals since spread loosely
    for i in 0..n_petals {
        let angle = i as f32 * GOLDEN_ANGLE + self.ring_rot[0];
        let r = max_r * 0.08 + max_r * 0.50 * (i as f32 / n_petals as f32).sqrt();
        let petal_size = max_r * 0.028 * (1.0 - i as f32 / n_petals as f32* 0.5);
        let hue = (self.chord.hue(self.p_hue) + i as f32 * 0.003).rem_euclid(1.0);
        draw.ellipse()
            .x_y(cx + r * angle.cos(), cy + r * angle.sin())
            .radius(petal_size)
            .color(hsla(hue, self.p_sat, 0.65, 0.70));
    }
}
```

When toggled on, replaces the normal ring layers entirely. The mandala dissolves into an organic petal cloud — useful as a "released" state between chords.

---

## Dependency Graph

```
Step 1 (PetalForm)
  └── Step 2 (RingSpec table)          ← depends on Step 1
        ├── Step 5 (Tension morph)     ← depends on Step 1+2
        └── Step 6 (Dihedral)          ← depends on Step 2

Step 3 (Lissajous core)               ← independent
Step 4 (Symmetry morph)               ← independent (works with any ring draw)
Step 7 (Phi radii)                    ← done as part of Step 2 ring specs
Step 8 (Breathing)                    ← independent, easy add
Step 9 (Star polygon border)          ← independent, replaces dot border
Step 10 (4 Gates)                     ← independent, additive draw
Step 11 (Fibonacci mode)              ← independent, key toggle
```

## Suggested Work Order

| Priority | Step | Why |
|----------|------|-----|
| 1 | Step 1 — PetalForm enum | Unblocks Steps 2, 5, 6 |
| 2 | Step 2 — RingSpec table | Core variety payload — everything looks different |
| 3 | Step 3 — Lissajous core | Independent, high center impact |
| 4 | Step 4 — Symmetry morph | Makes transitions feel musical |
| 5 | Step 5 — Tension shape | Deepens the existing tension system |
| 6 | Step 6 — Dihedral | Crystalline vs spinning distinction |
| 7 | Step 9 — Star polygon border | Visual polish, replaces dots |
| 8 | Step 8 — Breathing | Low effort, high feel |
| 9 | Step 10 — Gates | Unique Dom7th architecture |
| 10 | Step 11 — Fibonacci toggle | Nice mode switch |
| 11 | Step 7 — Phi radii | Subtle proportion improvement |
