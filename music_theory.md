# Music Theory in midi-visuals

This file documents the musical concepts used in the project — both what is
currently implemented per sketch and the broader theory primitives available
for future integration.

---

## Primitives — the building blocks

### Note number (pitch)

MIDI note numbers run 0–127. Middle C is 60. Each semitone is one unit.
The full 88-key piano range is roughly notes 21–108. In the sketches,
pitch is normalised as `note / 127.0 → [0, 1]` and mapped to a spatial axis.

**Convention used here:**
- Low notes → left / bottom of the screen
- High notes → right / top of the screen

This preserves the visual intuition of a keyboard laid flat: bass on the left,
treble on the right.

---

### Velocity

MIDI velocity (0–127, normalised to [0, 1]) encodes how hard a key was struck.
It is the primary expressive channel for a performer. Musically it conveys:

- **Dynamics** — pianissimo (soft, ~0.1) through fortissimo (hard, ~1.0)
- **Articulation** — staccato notes tend to have lower velocity than legato ones
- **Emphasis** — downbeats and accented notes are typically harder

In the sketches, velocity is the primary driver of *visual intensity*: size,
brightness, burst radius, impulse strength.

---

### Intervals and consonance

An interval is the semitone distance between two simultaneously held notes.
The Western tradition ranks intervals by consonance (stability) to dissonance
(tension):

| Semitones | Interval name     | Quality         |
|-----------|-------------------|-----------------|
| 0         | Unison            | Perfect consonance |
| 12        | Octave            | Perfect consonance |
| 7         | Perfect 5th       | Strong consonance  |
| 5         | Perfect 4th       | Consonance         |
| 4         | Major 3rd         | Soft consonance    |
| 3         | Minor 3rd         | Soft consonance    |
| 9         | Major 6th         | Mild consonance    |
| 8         | Minor 6th         | Mild consonance    |
| 2         | Major 2nd         | Mild dissonance    |
| 10        | Minor 7th         | Mild dissonance    |
| 11        | Major 7th         | Sharp dissonance   |
| 1         | Minor 2nd         | Sharp dissonance   |
| 6         | Tritone (aug 4th) | Maximum dissonance |

Given any set of held notes, the **most dissonant interval pair** can be found
and mapped to a `tension` float in [0, 1]. That single value can modulate any
visual parameter — jitter, color shift, chaos, speed.

---

### Chord quality

Three or more simultaneous notes form a chord. The *quality* of a chord
(determined by its interval set, independent of root) is one of the strongest
emotional signals in music:

| Quality     | Interval pattern (semitones) | Emotional feel         |
|-------------|------------------------------|------------------------|
| Major       | 0–4–7                        | Bright, stable, open   |
| Minor       | 0–3–7                        | Dark, introspective    |
| Diminished  | 0–3–6                        | Tense, unstable        |
| Augmented   | 0–4–8                        | Eerie, unresolved      |
| Dom 7th     | 0–4–7–10                     | Strong pull to resolve |
| Maj 7th     | 0–4–7–11                     | Dreamy, ambiguous      |
| Min 7th     | 0–3–7–10                     | Melancholy, jazzy      |

Detection: collect all currently-held note numbers, reduce to pitch classes
(`note % 12`), sort, and compare against the patterns above (transposed to
every root).

---

### Scale conformity

A scale is a set of 7 pitch classes that form the harmonic vocabulary of a
passage. Given a detected root and mode (e.g., C major = {C,D,E,F,G,A,B}),
each new note can be labelled:

- **Diatonic** — belongs to the scale; reinforces the established tonality
- **Chromatic** — outside the scale; creates colour, tension, or surprise

The ratio of chromatic to diatonic notes over a rolling window quantifies how
far the player is straying from the home key.

---

### Melodic intervals (voice leading)

The interval between *consecutive* notes in a single melodic line:

- **Stepwise motion** (1–2 semitones) — smooth, conjunct, calm
- **Skip** (3–4 semitones) — gentle leap, expressive
- **Leap** (5+ semitones) — dramatic, surprising

Voice-leading theory prefers stepwise motion and resolves leaps by stepping
back. Tracking this gives a "smoothness" signal that works even for single-note
(monophonic) playing.

---

### Tension and resolution

The dominant seventh chord (V7) creates strong expectation of resolving to the
tonic (I). This V→I motion is the most powerful tension-release gesture in
tonal music. It can be detected by watching for a dom-7th chord followed by a
major or minor chord whose root is a perfect 5th below. When detected:

- Tension phase: visual compression, instability, dissonance markers
- Release: burst, expansion, colour normalisation

---

### Harmonic overtones and beating

Two notes whose frequencies are related by simple integer ratios (e.g., 2:1,
3:2) produce smooth, fused sound. Complex ratios produce *beating* — a
periodic amplitude fluctuation at the difference frequency. For two notes `a`
and `b` (in MIDI):

```
beat_freq ≈ 440 × 2^((b-69)/12) − 440 × 2^((a-69)/12)   Hz
```

Beat frequencies below ~20 Hz are heard as rhythmic pulsing; above that they
merge into roughness (dissonance). This can drive a literal oscillation rate in
a visual.

---

## Per-sketch — current theory mapping

### `cardano`

| Source | Value read | Effect |
|--------|-----------|--------|
| Note velocity | `ev.velocity` | Radial spring impulse strength; base alpha |
| *(note number not used)* | — | The spring fires outward in the current orbital direction regardless of pitch |

**Current theory depth:** dynamics only. Pitch carries no meaning — any note
triggers the same geometric response scaled by how hard it is hit.

**Natural extensions:** map note pitch to impulse direction (high notes push
outward from the top of the orbit, low notes from the bottom); detect
consonance between held notes and modulate spring stiffness (consonant = tight
snap-back, dissonant = loose wobble).

---

### `droplets`

| Source | Value read | Effect |
|--------|-----------|--------|
| Note number | `ev.note / 127.0` | Y-position of spawned droplet cluster: high notes → top of screen, low notes → bottom |
| Note velocity | `ev.velocity` | Droplet radius and alpha (harder hit = bigger, brighter droplets) |

**Current theory depth:** pitch as spatial position (vertical axis); velocity as
intensity. The pitch mapping creates a keyboard-like visual: playing a scale
produces a sweep of droplet clusters from bottom to top.

**Natural extensions:** consonant intervals between simultaneously held notes
could widen the spawn spread (intervals merge into one zone); dissonant
intervals could spawn clusters that visually repel each other.

---

### `fireworks`

| Source | Value read | Effect |
|--------|-----------|--------|
| Note number | `ev.note / 127.0` | Horizontal launch position: low notes → left, high notes → right |
| Note velocity | `ev.velocity` | Burst radius (burst_vel = 80 + velocity × 520 px/s): soft = tight, hard = expansive |
| Held notes (chord) | `detect_chord()` | Overrides burst hue and burst form (see table below) |
| Held notes (tension) | `tension_from_notes()` | Jitters rising shell X position when tension > 0.3 |
| Chord transition Dom7→I | V7→Major/Minor transition | Grand finale: all shells burst instantly + centred mega-ring spawned |

**Chord quality → hue and form:**

| Chord | Hue | Form | Feel |
|-------|-----|------|------|
| Major | warm gold (0.10) | Ring | Stable, complete circle |
| Minor | blue-violet (0.65) | Spiral | Introspective, curving inward |
| Diminished | deep magenta (0.80) | Star (5-arm) | Tense, sharp, angular |
| Augmented | alien cyan (0.48) | Cross (4-arm) | Eerie, unresolved |
| Dom7th | blood red (0.98) | Star | Strong pull; jitter intensifies |
| Single note | CC 24 base hue | Cycles (Scatter → Ring → Star → Spiral → Cross) | |

**Current theory depth:** the richest of all sketches. Pitch, velocity, chord
quality, harmonic tension, and tension-resolution are all mapped to distinct
visual parameters. Playing a dominant-seventh chord and then resolving to a
major or minor triad triggers the V7→I grand finale.

**Remaining extension ideas:** melodic interval size (larger leap = faster
shell ascent); scale conformity colouring (chromatic notes = desaturated shell).

---

### `particles`

| Source | Value read | Effect |
|--------|-----------|--------|
| Note events | presence only (`for _ in midi.note_on_events()`) | Triggers a spawn burst; note number and velocity are **not read** |

**Current theory depth:** none — every note is treated identically regardless
of pitch or velocity. The sketch reacts only to the rhythmic density of playing
(more notes = more particles), not to any musical content.

**Natural extensions:** this sketch has the most headroom. Pitch could map
spawn position (keyboard layout across screen); velocity to spawn count and
particle speed; consonance between held notes to particle spread (tight cluster
vs scattered cloud); the current hue could shift based on whether the played
note is diatonic or chromatic.

---

### `rings`

| Source | Value read | Effect |
|--------|-----------|--------|
| Note number | `ev.note / 127.0` | Horizontal center of spawned ring: low → left, high → right |
| Note velocity | `ev.velocity` | Max radius (`r_max = max_r × (0.5 + velocity × 0.5)`) and base alpha |
| Note number | `ev.note / 127.0 × 0.3` | Small hue offset per ring (higher notes shift colour slightly) |

**Current theory depth:** pitch as horizontal position and subtle hue shift;
velocity as size and brightness. The hue-offset-by-pitch is the most
theory-aware feature in the codebase: playing a chord produces rings at
different X positions with slightly different hues, echoing the pitch
relationships visually.

**Natural extensions:** the hue offset could be tied to interval quality rather
than raw pitch — a perfect 5th between two notes could produce a complementary
hue pair, a tritone an intentionally clashing pair. The ring expand-speed could
vary with melodic interval size (larger leap = faster expansion).

---

## Burst form library (`fireworks` only)

Each shell carries a `BurstForm` that controls how particles are emitted at
the apex. The form is selected by chord quality; single notes cycle through all
forms in sequence.

| Form | Pattern | Assigned chord |
|------|---------|----------------|
| `Scatter` | Random radial — fully uniform sphere | single note (cycle 0) |
| `Ring` | Evenly-spaced circle ± small jitter | Major |
| `Star` | 5-arm star; arm-centre particles faster | Diminished / Dom7th |
| `Spiral` | Golden-angle Archimedean; inner slower | Minor |
| `Cross` | 4-arm cross ± narrow spread | Augmented |

**Current animal library** (from `assets/` SVGs, generated via `scripts/gen_zodiac.py`):
butterfly, cock, elephant, gecko, horse, pig, cobra, spider, cat, squirrel, frog

**Extension path:** drop a new SVG silhouette into `assets/`, add an entry to
`FILES` in `scripts/gen_zodiac.py`, and re-run it:
```
python3 scripts/gen_zodiac.py > src/sketches/zodiac_points.rs
```
No changes to `fireworks.rs` required — `BurstForm::cycle()` picks up new
animals automatically from the `ANIMALS` registry in `zodiac_points.rs`.

---

## Unimplemented concepts — implementation sketch

### Consonance/dissonance `tension` float

Computed from currently-held `MidiState::notes`:

```rust
fn consonance_score(semitones: u8) -> f32 {
    // 0.0 = maximally dissonant, 1.0 = perfect consonance
    match semitones % 12 {
        0  => 1.00,  // unison / octave
        7  => 0.85,  // perfect 5th
        5  => 0.80,  // perfect 4th
        4  => 0.70,  // major 3rd
        3  => 0.65,  // minor 3rd
        9  => 0.60,  // major 6th
        8  => 0.55,  // minor 6th
        2  => 0.35,  // major 2nd
        10 => 0.30,  // minor 7th
        11 => 0.15,  // major 7th
        1  => 0.10,  // minor 2nd
        6  => 0.00,  // tritone
        _  => 0.50,
    }
}

fn tension(notes: &[NoteState; 128]) -> f32 {
    let held: Vec<u8> = (0..128u8)
        .filter(|&n| notes[n as usize].on)
        .collect();
    if held.len() < 2 { return 0.0; }
    // Find the most dissonant interval pair
    let min_consonance = held.iter().enumerate()
        .flat_map(|(i, &a)| held[i+1..].iter().map(move |&b| {
            consonance_score(b.wrapping_sub(a))
        }))
        .fold(1.0f32, f32::min);
    1.0 - min_consonance   // tension is the inverse of consonance
}
```

`tension` in [0, 1] can then modulate any sketch parameter as an additional
source alongside CC knobs.

---

### Chord quality detection

```rust
enum ChordQuality { Major, Minor, Diminished, Augmented, Other }

fn chord_quality(notes: &[NoteState; 128]) -> Option<ChordQuality> {
    let mut classes: Vec<u8> = (0..128u8)
        .filter(|&n| notes[n as usize].on)
        .map(|n| n % 12)
        .collect::<std::collections::HashSet<_>>()
        .into_iter().collect();
    classes.sort();
    if classes.len() < 3 { return None; }
    // Normalise to root = 0 and check interval set
    let root = classes[0];
    let intervals: Vec<u8> = classes.iter().map(|&c| (c + 12 - root) % 12).collect();
    match intervals.as_slice() {
        i if i.contains(&4) && i.contains(&7) && !i.contains(&6) => Some(ChordQuality::Major),
        i if i.contains(&3) && i.contains(&7) && !i.contains(&6) => Some(ChordQuality::Minor),
        i if i.contains(&3) && i.contains(&6)                    => Some(ChordQuality::Diminished),
        i if i.contains(&4) && i.contains(&8)                    => Some(ChordQuality::Augmented),
        _                                                          => Some(ChordQuality::Other),
    }
}
```

---

### Where to hook these in

The cleanest integration point is `MidiState` — add computed fields updated
each frame in `update()` before calling `sketch.update()`:

```rust
// In main.rs update(), after taking the snapshot:
let tension = music::tension(&midi_snapshot.notes);
let chord   = music::chord_quality(&midi_snapshot.notes);
// Pass these into sketch.update() via an extended context struct,
// or add them as fields on MidiState itself.
```

This keeps each sketch stateless with respect to music theory — the analysis
lives in one place and every sketch can read from it.
