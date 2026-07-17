# Mandala Theory — Geometry, Structure & Programmatic Implementation

A working reference for building musically-reactive mandalas with depth and variety. Covers traditional structure, mathematical foundations, petal/form vocabulary, color theory, motion systems, and concrete implementation patterns in Rust/nannou.

---

## 1. What Makes a Mandala — The Core Grammar

All mandalas across all traditions share three invariants:

1. **A defined center** — the _bindu_ (Sanskrit: point/seed). All symmetry radiates from it.
2. **N-fold symmetry** — a repeating motif distributed evenly around the center. Either purely rotational (cyclic group C_N) or rotational + reflective (dihedral group D_N).
3. **Layered structure** — distinct concentric zones, each with its own visual language, from most concentrated at center to most diffuse at boundary.

Everything else — shape, color, motion — is variation on these three.

---

## 2. Symmetry Groups — The Mathematical Backbone

### 2.1 Cyclic vs Dihedral

| Group | Symbol | Description | Visual Effect |
|-------|--------|-------------|---------------|
| Cyclic | C_N | N-fold rotation only | Rotating feel, "spinning" |
| Dihedral | D_N | N-fold rotation + N reflection axes | Mirror-stable, "crystalline" |

Most traditional mandalas use D_N (they have reflection symmetry). Modern generative mandalas often use C_N intentionally to create implied rotation even in a still image.

**Programmatically:**
```rust
// C_N: just rotate the drawing angle
for i in 0..n {
    let angle = i as f32 / n as f32 * TAU;
    draw_motif(cx + r * angle.cos(), cy + r * angle.sin(), angle);
}

// D_N: rotate AND mirror (reflect y before rotating)
for i in 0..n {
    let angle = i as f32 / n as f32 * TAU;
    draw_motif(cx + r * angle.cos(), cy + r * angle.sin(), angle);
    // Mirror: reflect x-coordinate of motif shape
    draw_motif_reflected(cx + r * angle.cos(), cy + r * angle.sin(), angle);
}
```

### 2.2 Symmetry Numbers and Their Feel

| N | Character | Traditional Association |
|---|-----------|------------------------|
| 3 | Trinities, dynamic tension | Triskelion, Celtic, triangle yantras |
| 4 | Cardinal directions, earth, stability | Medicine wheel, Aztec sun quadrants |
| 5 | Phi, life, organic asymmetry | Pentagram, Islamic 5-star patterns |
| 6 | Harmony, perfection | Flower of Life, Star of David, snowflakes |
| 7 | Mystery, cyclical time | Days of week, chakras + center |
| 8 | Infinity paths, completion | Buddhist 8-fold path, ba-gua |
| 12 | Zodiac, chromatic, completeness | Clock face, chromatic scale |
| 16 | Maximum density, complex harmony | Dom7 chords, 16th-note grid |

### 2.3 Quasi-symmetry (Penrose-style)

5-fold symmetry cannot tile a plane periodically — this creates aperiodic, non-repeating patterns that _feel_ symmetrical but never fully repeat. Musically analogous to polyrhythms.

**Implementation idea:** Use two slightly irrational N values (e.g., N=5 and N=5.13) and lerp between them, creating slow drift where petals never quite realign.

---

## 3. Traditional Structural Layers

From center outward, classical Tibetan/Hindu mandalas use a fixed vocabulary of zones:

```
[BINDU] → [INNER CORE] → [PERICARP RING] → [LOTUS RING] → [VAJRA RING] → [FIRE RING] → [GROUND]
```

### Layer Roles

| Layer | Sanskrit | Visual Form | Function |
|-------|----------|-------------|----------|
| Center point | Bindu | Dot, tiny circle | Origin, maximum concentration |
| Core symbol | Dharma-chakra | Star, yantra, lotus | Primary meaning-carrier |
| Pericarp | Karnikā | 8–16 lotus petals | Transition from inner to outer |
| Inner ring | Antara-vṛtta | Geometric interlace | Containment, definition |
| Lotus ring | Padma | Larger petals, layered | Life, unfolding |
| Vajra ring | Vajra | Dots, dashes, lightning bolts | Protection boundary |
| Fire ring | Agni | Pointed flames / triangles | Purification, outer wall |
| Ground | Bhūmi | Open space, sometimes cemetery | Context, the world |

### 4 Gates (Tibetan)

T-shaped gates at cardinal directions (N/S/E/W). Each gate has:
- A color (white=E, yellow=S, red=W, green=N, in some systems)
- A guardian deity
- A directional quality (spring/summer/autumn/winter)

**Implementation:** Draw 4 T-shapes at 0°, 90°, 180°, 270° — or musically, trigger a gate on each quarter of a bar.

---

## 4. Petal / Motif Library

This is where visual variety comes from. The same N-fold structure can produce radically different mandalas based on what motif is placed.

### 4.1 Ellipse Petal (current implementation)

Simple, clean. Long axis points radially outward.

```rust
draw.ellipse()
    .x_y(cx + r * θ.cos(), cy + r * θ.sin())
    .w_h(length, width)
    .rotate(θ);
```

**Variants:**
- `length/width` ratio controls pointed vs. round petals (>4 = sharp, 1.5 = plump)
- Offset the center outward from ring: petal hovers at edge
- Offset inward: petal crosses the ring center (interlocking effect)

### 4.2 Lotus Petal (Bezier)

Classic pointed petal with concave sides — requires a bezier path.

```rust
// Petal oriented along +x axis, centered at origin, tip at (length, 0)
let base_l = pt2(0.0,  half_w);
let base_r = pt2(0.0, -half_w);
let tip    = pt2(length, 0.0);
let ctrl_l = pt2(length * 0.55, half_w * 0.85);  // inward concave control
let ctrl_r = pt2(length * 0.55, -half_w * 0.85);

// Draw as closed cubic bezier path, then rotate(θ) and translate
let builder = nannou::geom::path::Builder::new();
// base_l → ctrl_l → tip (first half)
// tip → ctrl_r → base_r (second half)
// close with line back to base_l
```

**Key ratio:** ctrl_x / length = 0.55 gives the classic lotus concavity. Moving ctrl_x to 0.35 creates a more flame-like swept shape.

### 4.3 Flame / Swept Petal

Asymmetric — tip curves to one side. Used in Tibetan fire rings. Creates implied rotation even in a static image.

```rust
let ctrl_inner = pt2(length * 0.4, half_w * 0.9);   // concave inward side
let ctrl_outer = pt2(length * 0.7, -half_w * 0.2);  // convex outer side (swept)
// Outer ctrl pushed forward → asymmetric sweep
```

For rotation direction: sweep CW on CW-rotating rings, CCW on CCW rings — reinforces implied motion.

### 4.4 Teardrop / Vesica Form

One rounded end, one pointed end. Rotate so pointed end faces inward (toward bindu) for a "focusing" feel, or outward for "radiating."

```rust
// Approximate: fat ellipse + pointed cap via bezier on one end
// Or: two overlapping circles at ratio 1:√3 (vesica piscis exact)
let vesica_overlap = r * 2.0 / 3.0;  // circles overlap at 2/3 radius
```

### 4.5 Star Spikes

Triangular spikes. Number independent of ring petal count — e.g., 5-spike star inside a 12-petal ring creates layered complexity.

```rust
// Spike: isoceles triangle
let spike_tip   = pt2(spike_len, 0.0);
let spike_base_l = pt2(0.0,  spike_w * 0.5);
let spike_base_r = pt2(0.0, -spike_w * 0.5);
// Repeat for each spike angle, all anchored at center
```

### 4.6 Rose Curve Petals (Polar)

The mathematical rose: `r(θ) = a · cos(k·θ)`

- k=2: 4 petals
- k=3: 3 petals
- k=4: 8 petals
- k=5: 5 petals
- k=1: circle

Key property: **if k is even, you get 2k petals; if k is odd, you get k petals.**

```rust
fn rose_curve_point(a: f32, k: f32, theta: f32) -> Vec2 {
    let r = a * (k * theta).cos();
    pt2(r * theta.cos(), r * theta.sin())
}

// Sample 512 points around [0, TAU] and draw as polyline
let pts: Vec<_> = (0..512).map(|i| {
    let t = i as f32 / 512.0 * TAU;
    rose_curve_point(max_r * 0.4, k_value, t)
}).collect();
draw.polyline().points(pts).color(...);
```

Non-integer k creates "toothed" aperiodic roses that never fully close — useful for decay/tension states.

### 4.7 Hypotrochoid / Spirograph

A point on a circle rolling inside a larger circle. Produces the classic spirograph shapes.

```
x(t) = (R - r)·cos(t) + d·cos((R-r)/r · t)
y(t) = (R - r)·sin(t) - d·sin((R-r)/r · t)
```

Parameters:
- `R` = outer circle radius (usually = max_r * 0.8)
- `r` = inner rolling circle radius
- `d` = pen distance from inner circle center

`r/R` ratio determines petal count. `d/r` ratio controls fullness (d < r = inner loops, d > r = outer loops).

| r/R ratio | Pattern |
|-----------|---------|
| 1/2 | Straight line (degenerate) |
| 1/3 | 3-pointed delta |
| 2/5 | 5-pointed star shape |
| 3/7 | 7-petal complex |
| 1/4 | Astroid (4-pointed) |

```rust
fn hypotrochoid(big_r: f32, small_r: f32, d: f32, t: f32) -> Vec2 {
    let diff = big_r - small_r;
    pt2(
        diff * t.cos() + d * ((diff / small_r) * t).cos(),
        diff * t.sin() - d * ((diff / small_r) * t).sin(),
    )
}
```

### 4.8 Lissajous Figures

`x = A·sin(a·t + δ)`, `y = B·sin(b·t)`

Ratio a:b determines the shape:
- 1:1 = ellipse/circle (δ=π/2) or diagonal line (δ=0)
- 1:2 = figure-8 / bow-tie
- 1:3 = trefoil variant
- 2:3 = classic Lissajous knot
- 3:4 = complex figure

Musical connection: a:b ratios are interval ratios. Perfect 5th = 3:2, octave = 2:1, minor 3rd ≈ 6:5. **Using the interval ratio of the detected chord's root + fifth as a:b maps music theory directly to Lissajous form.**

```rust
fn lissajous(a_freq: f32, b_freq: f32, delta: f32, t: f32, scale: f32) -> Vec2 {
    pt2(
        scale * (a_freq * t + delta).sin(),
        scale * (b_freq * t).sin(),
    )
}

// For a perfect fifth: a=3, b=2
// For major third: a=5, b=4
// Sample ~1024 points, draw as polyline loop
```

### 4.9 Interlace / Knotwork

Celtic knotwork uses a single continuous line that weaves over and under itself. The key is:
1. Draw a diagonal grid inside a circle
2. Each diagonal becomes a strand
3. At each intersection: alternate over/under

**Simplified approach for generative use:**
- Draw N lines from center at evenly-spaced angles, each of sinusoidal amplitude
- Lines cross → draw a small "bridge" gap to simulate weaving
- Combine with a border ring to anchor ends

---

## 5. Sri Yantra — Deep Dive

The most geometrically precise mandala in existence. Worth understanding in detail.

**Construction:**
1. Draw a circle
2. Place 9 interlocking isoceles triangles:
   - 4 pointing up (Shiva, masculine, fire) — apex angles ≈ 60°, 51°, 46°, 39°
   - 5 pointing down (Shakti, feminine, water) — apex angles ≈ 56°, 50°, 44°, 37°, 32°
3. Their intersections form exactly **43 smaller triangles**
4. At absolute center: a tiny triangle (bindu triangle)
5. Around this: 8-petal lotus, then 16-petal lotus, then 3 rings, then 4 gates on a square

**Why it's hard:** All 9 triangles must share vertices exactly — the 28 intersection points satisfy a system of nonlinear equations. Computers can solve it numerically.

**Approximate programmatic construction:**
```rust
// The 9 triangles defined by their apex angles and radii
// (from Kulaichev 1984 numerical solution)
const SRI_YANTRA_TRIANGLES: [(f32, f32, bool); 9] = [
    // (apex_y_normalized, half_base_normalized, pointing_up)
    ( 0.792,  0.630, true),   // largest up
    (-0.717,  0.546, false),  // largest down
    ( 0.610,  0.477, true),
    (-0.537,  0.413, false),
    ( 0.428,  0.321, true),
    (-0.355,  0.286, false),
    ( 0.244,  0.173, true),
    (-0.164,  0.133, false),
    ( 0.000,  0.000, true),   // central bindu triangle
];
```

---

## 6. Color Architecture

### 6.1 Traditional Buddhist 5-Color System

The 5 Dhyani Buddha colors, mapped to directions and elements:

| Color | Direction | Element | Quality |
|-------|-----------|---------|---------|
| White | Center | Space | Purity, all-encompassing |
| Blue | East | Water | Mirror-like clarity |
| Yellow | South | Earth | Equality, enriching |
| Red | West | Fire | Discriminating awareness |
| Green | North | Air | All-accomplishing action |

**For music mapping:** Map the 5 scale degrees (root, 2nd, 3rd, 5th, 6th) to these colors. Each note played lights its color zone.

### 6.2 Ring Color Strategies

**Hue rotation per ring:**
Each ring shifts hue by a fixed offset. Small offset (0.03–0.05) = analogous harmony. Large offset (0.08–0.15) = more varied.

**Complementary split:**
Inner rings use hue H, outer rings use H + 0.5 (complement). Creates visual tension at the boundary — useful for tension in music.

**Value (lightness) breathing:**
- Inner rings: higher lightness (closer to white/void)
- Outer rings: darker (denser, grounded)
Or reversed for a glowing-center effect.

**Saturation gradient:**
Saturate inward, desaturate outward → focal pull toward center.
Saturate outward → sense of expanding energy.

### 6.3 Chord-to-Color Deep Map

Beyond just hue, chord quality can drive the full HSL triple:

| Chord | Hue | Saturation | Lightness | Feel |
|-------|-----|------------|-----------|------|
| Major | 0.10 (warm gold) | 0.85 | 0.60 | Bright, resolved |
| Minor | 0.65 (blue-violet) | 0.70 | 0.45 | Introspective, shadowed |
| Diminished | 0.80 (red-magenta) | 0.95 | 0.40 | Unstable, dark |
| Augmented | 0.48 (cyan-green) | 0.80 | 0.55 | Eerie, hovering |
| Dom7th | 0.98 (red-orange) | 1.00 | 0.50 | Urgent, driving |
| Sus2 | 0.20 (yellow-green) | 0.65 | 0.65 | Open, ambiguous |
| Sus4 | 0.55 (teal) | 0.70 | 0.55 | Suspended, waiting |
| Major7th | 0.12 (warm amber) | 0.75 | 0.70 | Dreamy, luminous |
| Minor7th | 0.72 (purple) | 0.80 | 0.50 | Melancholic, rich |
| Half-dim | 0.78 (crimson) | 0.90 | 0.45 | Tense, unresolved |

---

## 7. Motion Systems

### 7.1 Counter-Rotation (current)

Odd rings CW, even rings CCW. Speed driven by musical energy.

**Enhancement:** Add phase offsets per ring so they drift in and out of alignment. Two rings "in phase" (petals aligned) feel settled; "anti-phase" (offset by half-petal-width) feels complex.

```rust
let phase_offset = ring_idx as f32 * 0.15 * TAU / n as f32;
let angle = base_angle + phase_offset + ring_rot[ring_idx];
```

### 7.2 Breathing (Scale Pulse)

The entire mandala or individual rings scale sinusoidally:

```rust
let breath_scale = 1.0 + 0.06 * (time * breath_rate * TAU).sin();
// Apply to ring radius before drawing
let effective_r = ring_r * breath_scale;
```

**Musical mapping:** Breath rate = BPM / 60.0 → breathing in tempo. Slower breath (÷2, ÷4) feels like phrase-level motion.

### 7.3 Symmetry Morphing

Smoothly transition between two fold counts when chord changes. During transition, both old and new symmetry coexist with cross-fading alpha.

```rust
struct SymmetryMorph {
    from_n: usize,
    to_n: usize,
    t: f32,  // 0.0 → 1.0
}

// In draw:
if morph.t < 1.0 {
    let alpha_old = (1.0 - morph.t).powi(2);
    let alpha_new = morph.t.powi(2);
    draw_ring_with_n(morph.from_n, alpha_old);
    draw_ring_with_n(morph.to_n,   alpha_new);
} else {
    draw_ring_with_n(current_n, 1.0);
}
```

Morph speed: fast (0.3s) = chord snap; slow (2.0s) = chord evolve.

### 7.4 Fibonacci Spiral Placement

Instead of equal angular spacing, place N petals along a golden-angle spiral. Produces a sunflower-like distribution. The golden angle ≈ 137.508° = 2π / φ².

```rust
const GOLDEN_ANGLE: f32 = 2.399_963; // 2π / φ²  (radians)

for i in 0..n {
    let angle = i as f32 * GOLDEN_ANGLE;
    let radius = max_r * 0.15 * (i as f32).sqrt();  // spiral outward
    draw_petal(cx + radius * angle.cos(), cy + radius * angle.sin(), angle);
}
```

This is NOT traditional mandala symmetry — it's aperiodic and organic. Good for an "Other" chord state or chromatic/atonal passages.

### 7.5 Orbital Petals

Petals orbit their ring center _while_ the ring itself rotates — creating epicyclic motion. Like planets orbiting a sun that itself orbits.

```rust
let ring_angle = ring_rot[i];                 // main ring rotation
let orbit_angle = ring_rot[i] * 1.618;       // petal's own rotation (phi ratio = never syncs)
let petal_offset = pt2(
    small_r * orbit_angle.cos(),
    small_r * orbit_angle.sin(),
);
// Place petal at: ring_point + petal_offset
```

Phi ratio between orbit and ring ensures they never sync — produces a constantly evolving pattern. Using integer ratios (1:2, 2:3) creates stable flower patterns.

### 7.6 Kaleidoscope Mode

Fold the canvas into a symmetric wedge (like folding paper into 1/N of a circle), draw in that wedge, then mirror it N times.

```rust
fn to_wedge_coords(x: f32, y: f32, n: usize) -> (f32, f32) {
    let angle = x.atan2(y).rem_euclid(TAU / n as f32);
    let r = (x*x + y*y).sqrt();
    (r * angle.cos(), r * angle.sin())
}
// Draw anything in the wedge; the render-pass mirrors it automatically
// (Requires render texture + N-fold blit, or shader-based approach)
```

---

## 8. Sacred Geometry Forms

### 8.1 Flower of Life

7 circles, each with radius R, the 6 outer circles centered at distance R from the center.

```rust
let fol_r = max_r * 0.25;
draw.ellipse().x_y(cx, cy).radius(fol_r).stroke(...);
for i in 0..6 {
    let a = i as f32 * TAU / 6.0;
    draw.ellipse()
        .x_y(cx + fol_r * a.cos(), cy + fol_r * a.sin())
        .radius(fol_r).stroke(...);
}
```

The intersections form the "Seed of Life" and "Egg of Life" sub-patterns.

### 8.2 Metatron's Cube

13 circles (Flower of Life + outer 6), then connect ALL center points with straight lines. Produces projections of the cube, tetrahedron, octahedron, icosahedron, and dodecahedron.

The 13 centers:
- 1 at origin
- 6 at distance R (hexagonal ring)
- 6 at distance 2R (outer hexagonal ring)

Lines between all 13 pairs: 13 × 12 / 2 = 78 lines total (filter by distance for aesthetic subsets).

### 8.3 Vesica Piscis

Two equal circles with centers at distance = radius from each other. Their intersection region (the "fish bladder") has:
- Height = 2R
- Width = R√3
- Aspect ratio = 1 : √3 ≈ 1 : 1.732

This ratio appears in: equilateral triangle height, √3 in music temperament. The Vesica is the basis for the trefoil and many Celtic forms.

**Stacked Vesicas (ring of overlapping circles):** Place N circles of radius R at distance R from center, each centered on the Nth-of-TAU angle point. Every adjacent pair creates a Vesica. This produces the N-petal rose as the union of Vesica regions.

### 8.4 Phi / Golden Ratio

φ = (1 + √5) / 2 ≈ 1.61803

Appears in:
- Fibonacci spiral (adjacent petal angles in sunflowers)
- Pentagon/pentagram diagonal:side ratio = φ
- Golden rectangle subdivisions

**For ring radius placement:** Instead of linear spacing, use φ-exponential:
```rust
let ring_radii = [
    max_r / (φ * φ * φ * φ),  // innermost
    max_r / (φ * φ * φ),
    max_r / (φ * φ),
    max_r / φ,
    max_r,                     // outermost
];
// Adjacent ring ratio = φ ≈ 1.618 each time
```

This makes outer rings proportionally larger — like a nautilus shell cross-section.

---

## 9. Structural Concepts for Distinct Mandala Types

These are complete mandala "personalities" — each uses a different combination of the above elements.

### Concept A: Crystal Yantra (Minimal Geometric)

- Strict D_N dihedral symmetry (has reflection lines)
- Only straight-edged elements: triangles, lines, squares
- Sri Yantra-inspired interlocking triangles
- High saturation, crystalline colors (blues, whites)
- No motion blur — sharp edges only
- Musical map: major/minor → 6/8 triangles, chord inversion → triangle orientation flip

### Concept B: Lotus Garden (Organic, Multi-layer)

- Multiple rings with different petal types per ring
- Inner: star spikes; middle: lotus bezier petals; outer: ellipse petals
- Each ring's petal count is a harmonic of the base N (×1, ×2, ×3)
- "Breathing" motion dominant (scale pulse)
- Soft saturation, warm hues
- Musical map: note velocity → ring brightness; chord quality → N

### Concept C: Spirograph Cathedral (Mathematical Curves)

- No discrete petals — continuous hypotrochoid/epitrochoid curves
- Multiple concentric spirograph curves with different r/R ratios
- Lissajous figure at center driven by interval ratio
- Rose curves on outer ring
- Monochromatic or dual-hue (tone + complement)
- Musical map: chord interval ratios → r/R ratio of spirograph

### Concept D: Fractal Bloom (Self-similar)

- Petals at each ring spawn sub-petals at 1/3 scale
- Recursive to depth 2-3
- Fibonacci spiral petal placement option
- Counter-rotation at each fractal level
- Musical map: note density → recursion depth visible (more energy = more sub-petals revealed)

### Concept E: Tibetan Palace (Traditional Structure)

- Square outer boundary with 4 T-gates at cardinal directions
- 3 concentric ring "walls" (fire, vajra, lotus)
- 8-petal lotus layer, then 16-petal layer
- Inner 5-element color zoning (white/blue/yellow/red/green sectors)
- Musical map: each of the 5 notes of pentatonic scale illuminates one color sector

### Concept F: Islamic Star Polygon (Geometric Interlace)

- Foundation: regular {N/k} star polygon (N points, skip k between)
- E.g., {8/3} = octagram, {12/5} = 12-point Islamic star
- Lines extended and intersected to create girih tile pattern
- Clean stroke-only (no fill), high contrast
- Musical map: N = chord symmetry, k = interval (3rd=2, 4th=3, 5th=4, etc.)

---

## 10. Programmatic Implementation Patterns

### 10.1 Bezier Petal Builder

```rust
struct Petal {
    length: f32,
    width: f32,
    concavity: f32,  // 0.0 = straight, 1.0 = fully cusped
    sweep: f32,      // 0.0 = symmetric, ±1.0 = flame swept left/right
}

impl Petal {
    fn points(&self) -> [Vec2; 6] {
        let l = self.length;
        let w = self.width * 0.5;
        let c = self.concavity;
        let s = self.sweep;
        // Control points for cubic bezier pair, petal along +x axis
        [
            pt2(0.0, w),                              // base left
            pt2(l * (0.55 - c * 0.2), w * (1.0 - c * 0.5) + s * w * 0.5),  // ctrl left
            pt2(l, s * w * 0.3),                       // tip (slight sweep)
            pt2(l * (0.55 - c * 0.2), -w * (1.0 - c * 0.5) + s * w * 0.3), // ctrl right
            pt2(0.0, -w),                              // base right
            pt2(0.0, 0.0),                             // center (close)
        ]
    }
}
```

### 10.2 Ring Layer Spec

Decouple ring data from drawing. Each ring is described by a `RingSpec`:

```rust
struct RingSpec {
    r_frac: f32,              // fraction of max_r
    n_mult: f32,              // multiplier on base symmetry N (can be fractional)
    form: PetalForm,          // Ellipse, Lotus, Flame, Spike, Rose, Spirograph
    spin_dir: f32,            // +1.0 CW, -1.0 CCW
    spin_phase: f32,          // phase offset (0.0..1.0 of petal spacing)
    hue_offset: f32,          // added to base hue
    sat_mult: f32,            // multiplier on base saturation
    lum: f32,                 // fixed lightness for this ring
    alpha: f32,               // base alpha
    dihedral: bool,           // if true, add reflection (D_N instead of C_N)
}
```

### 10.3 Star Polygon Drawer

```rust
fn draw_star_polygon(draw: &Draw, cx: f32, cy: f32, r: f32, n: usize, skip: usize, color: Rgba) {
    let mut pts = Vec::new();
    let step = skip * 2;  // connect every `skip`-th point
    let mut i = 0;
    loop {
        let a1 = i as f32 / n as f32 * TAU;
        let a2 = (i + skip) as f32 / n as f32 * TAU;
        pts.push(pt2(cx + r * a1.cos(), cy + r * a1.sin()));
        pts.push(pt2(cx + r * a2.cos(), cy + r * a2.sin()));
        i = (i + skip) % n;
        if i == 0 { break; }
    }
    draw.polyline().stroke_weight(2.0).points(pts).color(color);
}
```

### 10.4 Energy-to-Motion Mapping

```rust
struct EnergyState {
    raw:    f32,   // current note density (0..1)
    smooth: f32,   // exponential smoothing (τ ≈ 0.5s)
    peak:   f32,   // decaying peak tracker
}

impl EnergyState {
    fn update(&mut self, note_hit: bool, dt: f32) {
        if note_hit { self.raw = (self.raw + 0.25).min(1.0); }
        self.raw  *= (1.0 - dt * 2.5);                   // decay
        self.smooth += (self.raw - self.smooth) * dt * 2.0;  // smooth
        if self.raw > self.peak { self.peak = self.raw; }
        self.peak *= (1.0 - dt * 0.3);                   // slow peak decay
    }
    fn spin_speed(&self) -> f32 { 0.1 + self.smooth * 1.8 }
    fn breath_amp(&self) -> f32 { 0.02 + self.peak * 0.10 }
}
```

### 10.5 Chord-to-Form Decision Table

```rust
fn ring_specs_for_chord(chord: ChordKind, energy: f32) -> Vec<RingSpec> {
    match chord {
        ChordKind::Major => vec![
            RingSpec { form: PetalForm::Ellipse,    n_mult: 1.0, spin_dir: -1.0, r_frac: 0.15, .. },
            RingSpec { form: PetalForm::Lotus,      n_mult: 1.0, spin_dir:  1.0, r_frac: 0.30, .. },
            RingSpec { form: PetalForm::Ellipse,    n_mult: 2.0, spin_dir: -1.0, r_frac: 0.45, .. },
            RingSpec { form: PetalForm::Spike,      n_mult: 1.0, spin_dir:  1.0, r_frac: 0.58, .. },
        ],
        ChordKind::Diminished => vec![
            RingSpec { form: PetalForm::Flame,      n_mult: 1.0, spin_dir:  1.0, r_frac: 0.12, .. },
            RingSpec { form: PetalForm::StarPolygon { skip: 5 }, n_mult: 1.0, spin_dir: -1.0, r_frac: 0.28, .. },
            RingSpec { form: PetalForm::Rose { k: 6.0 }, n_mult: 1.0, spin_dir:  1.0, r_frac: 0.44, .. },
        ],
        // etc.
    }
}
```

### 10.6 Concentric Ring Radius Strategies

```rust
// Linear
fn linear_radii(n: usize, max_r: f32) -> Vec<f32> {
    (1..=n).map(|i| i as f32 / n as f32 * max_r).collect()
}

// Phi-exponential (nautilus shell)
fn phi_radii(n: usize, max_r: f32) -> Vec<f32> {
    const PHI: f32 = 1.618_034;
    let scale = max_r / PHI.powi(n as i32 - 1);
    (0..n).map(|i| scale * PHI.powi(i as i32)).collect()
}

// Harmonic series (r ∝ 1/n) — more dense at center
fn harmonic_radii(n: usize, max_r: f32) -> Vec<f32> {
    let total: f32 = (1..=n).map(|i| 1.0 / i as f32).sum();
    let mut acc = 0.0;
    (1..=n).map(|i| { acc += 1.0 / i as f32; acc / total * max_r }).collect()
}
```

### 10.7 Tension-to-Shape Morphing

Dissonance (tension) can distort petal shape rather than just scale:

```rust
fn petal_shape_at_tension(t: f32) -> Petal {
    Petal {
        length:     lerp(base_len,  base_len * 1.8, t),     // elongate
        width:      lerp(base_wid,  base_wid * 0.4, t),     // thin out
        concavity:  lerp(0.3,       0.0,             t),     // lose curvature
        sweep:      lerp(0.0,       0.6 * spin_dir,  t),     // flame lean
    }
}
```

Full tension = the mandala's petals become sharp, swept flame spikes. Resolution = they fatten back into lotus form.

---

## 11. Musical Interval → Geometry Mapping

Intervals have frequency ratios. These ratios can directly drive geometric parameters:

| Interval | Ratio | Geometry use |
|----------|-------|--------------|
| Unison | 1:1 | Single ring, pure circle |
| Minor 2nd | 16:15 | Beats → oscillating jitter |
| Major 2nd | 9:8 | Slight tension, asymmetric petal |
| Minor 3rd | 6:5 | Lissajous 6:5 |
| Major 3rd | 5:4 | Lissajous 5:4, stable pentagon/square hybrid |
| Perfect 4th | 4:3 | Lissajous 4:3 |
| Tritone | 45:32 | Irrational-feeling, near 1.41 (√2) |
| Perfect 5th | 3:2 | Lissajous 3:2 (classic trefoil/infinity) |
| Major 6th | 5:3 | Lissajous 5:3 |
| Major 7th | 15:8 | Near octave, tense |
| Octave | 2:1 | Lissajous 2:1 (figure-8) |

**Key insight:** Consonant intervals have simple integer ratios → Lissajous figures close on themselves cleanly. Dissonant intervals have complex ratios → figures drift, never repeat. This maps interval consonance to visual closure/openness automatically.

```rust
fn interval_lissajous(semitones: u8, time: f32, scale: f32) -> Vec<Vec2> {
    let (a, b) = match semitones {
        0 =>  (1, 1), 3 =>  (6, 5), 4 =>  (5, 4),
        5 =>  (4, 3), 7 =>  (3, 2), 9 =>  (5, 3),
        12 => (2, 1), _ =>  (7, 5), // complex for dissonant
    };
    (0..512).map(|i| {
        let t = i as f32 / 512.0 * TAU * lcm(a, b) as f32;
        pt2(scale * (a as f32 * t).sin(), scale * (b as f32 * t).sin())
    }).collect()
}
```

---

## 12. Sketch Architecture Suggestions

### Direction A: Replace current ellipses with dynamic petal types

The current mandala uses only `PetalForm::Ellipse`. Adding a `PetalForm` enum and chord-driven selection would add major visual diversity with the existing ring structure.

### Direction B: Add a second mandala mode — Spirograph/Mathematical

A mode where each ring is a continuous hypotrochoid curve rather than discrete petals. Chord changes the r/R ratio, morphing the curve form. Very different visual language from the petal mandala.

### Direction C: Lissajous Core

Replace the core star with a Lissajous figure driven by the detected chord's root-to-fifth interval ratio. When no chord is held, it's a slow ellipse (1:1 slightly detuned). When playing a 5th, it becomes the stable trefoil. Dom7th → complex 7:4 knot form.

### Direction D: Mandala Taxonomy Mode

Multiple concurrent sub-mandalas at different scales, each locked to a different chord quality. Playing a major chord activates the golden-ratio mandala; playing diminished activates the star polygon mandala. They coexist at different sizes.

---

## 13. Reference Sources

- **Kulaichev (1984)**: Numerical solution for Sri Yantra construction
- **Islamic Geometric Patterns** — Keith Critchlow
- **Sacred Geometry** — Robert Lawlor (Vesica, phi, Platonic solids)
- **The Geometry of Art and Life** — Matila Ghyka (phi in nature/art)
- **Tibetan Buddhist mandalas** — reference: Brauen (1997) _The Mandala: Sacred Circle in Tibetan Buddhism_
- **Rose curves / hypotrochoids** — Mathworld (mathworld.wolfram.com)
- **Lissajous figures** — Harmonograph: A Visual Guide to the Mathematics of Music — Anthony Ashton
