# Chess Sketch — Rules Reference

Last updated: 2026-06-30

---

## Board

- **100 columns** (`BOARD_COLS`), rows scale to window height.
- Checkered texture (light/dark squares) rendered to a GPU texture, rebuilt only on resize.
- **White** home is the bottom of the screen; **Black** home is the top.

---

## Teams & Palettes

| Team  | Palette         | Fill              | Glow hues       |
|-------|-----------------|-------------------|-----------------|
| White | Warm (orange/gold) | off-white tinted warm | 0.05–0.14 |
| Black | Cool (blue/cyan)   | near-black tinted cool | 0.50–0.70 |

Subtle home-zone bands: amber band at White's home rows, blue band at Black's.

---

## Pieces

### Pawn
- **Placement**: two rows per team at game start (rows-2 & rows-3 for White; rows 1 & 2 for Black).
- **Movement**: advances one step forward each tick toward opponent's back rank.
- **Diagonal capture**: captures enemy pieces on forward-diagonal squares.
- **Pressure system**: 3 consecutive turns of forward pressure on a blocked enemy removes it.
- **Relay/bounce**: when blocked by a friendly pawn, pushes that pawn forward first (one level of relay), then slides into the vacated cell — creating a chain-push effect.
- **Cluster rush**: when 4+ friendly pawns (`CLUSTER_SIZE`) are within 3 columns (`CLUSTER_RADIUS`) of each other, all clustered pawns surge **1–4 random steps** forward that tick.
- **Promotion**: on reaching the opponent's back rank, auto-promotes to a **Queen** (5 lives). Flash explosion fires at the promotion cell.

### Knight
- **Movement**: L-shaped (2 forward + 1 sideways). Alternates preferred lateral direction (`data` field).
- **Auto-capture**: automatically attacks all opponent pieces reachable from its current L-squares before moving.
- **Movement**: prefers to land on an empty L-square; tries opposite side if preferred side is blocked.

### Rook
- **Charge mechanic**: `charge` accumulates passively each tick (`CHARGE_PER_TICK = 0.20`) and via MIDI boosts (`CHARGE_BOOST = 0.38`).
- **Fire**: when fully charged (`charge ≥ 1.0`), fires along its column toward the nearest opponent. Moves to the captured cell if the piece is removed.
- Visual: **8 orbiting plus-signs** filling clockwise as charge builds — bright = charged, dim ghost = empty. Plus-signs rotate with the orbit angle.

### Bishop
- **Charge mechanic**: same as Rook.
- **Fire**: fires diagonally. Alternates preferred diagonal (`data` field) after each shot.
- Visual: **6 orbiting small diamonds (◆)** counter-spinning as charge builds — bright when charged, dim ghost when empty. Counter-rotation distinguishes it visually from the Rook.

### Queen
- **Lives**: starts with **5 lives**. Losing a life changes color (gold → red for White; blue → violet for Black).
- **Charge mechanic**: same as Rook/Bishop.
- **Fire**: scans all 8 directions, fires at the nearest opponent in any direction.
- **One per team** at game start (file 18).
- Visual: ellipse + diamond composite glow, charge outer ring. **5 orbiting plus-signs** — one per life, bright when alive, dim ghost when lost. Plus-signs spin and self-rotate.

### King
- **Lives**: starts with `BOARD_COLS = 100` lives — **one life per starting pawn**.
- **Pawn sync**: each frame, `king.lives` is capped at the current live pawn count. When pawns die, king weakens.
- **Never removed by combat**: King can only die via the pawn-count sync reaching 0.
- **Movement**: 1-step in any of 8 directions, prefers forward. Will capture adjacent enemies.
- **One per team** at game start (file 43).
- Visual: constant size (2.5× base radius regardless of lives), slowly spinning 8-pointed star body, **10 orbiting plus-signs** (each = 10-pawn tier) — bright = alive, dim = depleted. Orbit speed increases when in danger. Hue shifts with depletion. Crown cross + top bar symbol. Red danger ring and faster pulsing below 20% lives.

---

## Idle System

- `idle_ticks` tracks how long each cell has held the same piece (incremented each auto-tick via board-state snapshot).
- **Threshold**: `IDLE_THRESHOLD = 8` ticks (~8 seconds).
- **Effect on non-ranged pieces** (Pawn/Knight/King): forced move attempt when idle threshold is reached; counter resets.
- **Effect on ranged pieces** (Rook/Bishop/Queen): charge boosted by +0.35 when idle, bypassing the normal charge rate.

---

## King Death Event

Triggered when `king.lives` drops to 0 (pawn count reaches 0):

1. Large explosion at the king's position (scale ×5).
2. King and all non-pawn pieces of the dead team are removed.
3. **Cascade**: remaining pawns of the dead team explode left→right, 4 per burst every 40ms.
4. **Pause**: 700ms wait after cascade ends.
5. **Rebuild**: new starting position appears center-outward — back-rank pieces first (center → edges), then pawns (edges → center), 6 pieces per burst every 40ms.

---

## MIDI Controls

- **White keys** → White team, **black keys** → Black team.
- Note number modulo 100 → file index.
- Each MIDI note-on triggers `try_move` for that team/file.
- MIDI notes also boost charge on ranged pieces (`CHARGE_BOOST = 0.38`).

---

## Music-Reactive Systems

### Energy bar (right screen edge)
Tracks note density over a 4-second sliding window. Glows **warm orange/red** when music is building (more notes recently), cools to **blue** when slowing. Top marker pulses faster at peak energy. A small pip above/below indicates trend direction (up = building, down = fading).

### Chord detection → Special actions
Fired once when the set of held pitch classes changes and a triad or seventh chord is recognized. Each chord triggers a spectacular chain-move with shock rings and a full-board color flash:

| Chord | Action | Visual |
|-------|--------|--------|
| **Major** | **Harmonic Surge** — all Rooks/Bishops/Queens charge to full and fire simultaneously | Golden rings from every firing piece |
| **Minor** | **Dark Wave** — all Knights double-hop (L-move twice per tick) | Violet rings per Knight, violet flash |
| **Diminished** | **Tremor** — all Pawns surge 3 steps forward | Full-board magenta shockwave ring |
| **Augmented** | **Phase Shift** — all Bishops flip diagonal and fire twice | Cyan rings per Bishop |
| **Dom7th** | **V7 Discharge** — Queens fire 3 rapid volleys + massive central ring | Crimson rings + large central shockwave |

### High-velocity chain ripple (`CHAIN_VEL_THRESH = 0.68`)
Notes struck hard activate ±1 and ±2 adjacent files simultaneously. A horizontal sweep bar expands from the center column, bright leading edges mark the chain's reach.

### Perfect 5th resonance
When a new note forms a perfect 5th (7 semitones) with any currently held note, both corresponding files activate simultaneously. A blue connecting beam spans the two resonant columns and fades over 2 seconds.

### Harmonic tension tint
A pulsing red board wash proportional to the dissonance of held notes. Tritone = maximum (0.88 tension), perfect 5th ≈ zero. Pulse rate scales with dissonance intensity.

---

## Auto-Tick System

Every `TICK_INTERVAL = 1.0` second:

1. Passive charge all Rooks/Bishops/Queens.
2. Auto-fire all fully charged pieces.
3. Auto-advance `AUTO_FILES = 20` random non-ranged pieces.
4. Idle tracking snapshot; force-move/charge-boost idle pieces.
5. Pawn cluster rush check.

---

## Keyboard

| Key | Action |
|-----|--------|
| `R` | Hard reset — clears board and rebuilds fresh starting position immediately. |

---

## Constants Quick Reference

| Constant | Value | Meaning |
|----------|-------|---------|
| `BOARD_COLS` | 100 | Number of files |
| `BOARD_MAX_ROWS` | 300 | Max board height |
| `PRESSURE_MAX` | 3 | Turns of pressure to break through |
| `TICK_INTERVAL` | 1.0s | Auto-tick period |
| `AUTO_FILES` | 20 | Files auto-advanced per tick |
| `CHARGE_PER_TICK` | 0.20 | Passive charge rate |
| `CHARGE_BOOST` | 0.38 | MIDI charge bonus |
| `IDLE_THRESHOLD` | 8 | Ticks before idle action |
| `CLUSTER_RADIUS` | 3 | Column radius for cluster |
| `CLUSTER_SIZE` | 4 | Min pawns for cluster rush |
| `CASCADE_BURST` | 4 | Pawns exploded per cascade step |
| `CASCADE_INTERVAL` | 0.04s | Cascade step period |
| `REBUILD_BURST` | 6 | Pieces placed per rebuild step |
| `REBUILD_INTERVAL` | 0.04s | Rebuild step period |
| `REBUILD_WAIT` | 0.7s | Pause between cascade and rebuild |
| `ENERGY_WINDOW` | 4.0s | Density half-window for energy bar |
| `ENERGY_PEAK` | 20 notes | Notes/window = full energy bar |
| `CHAIN_VEL_THRESH` | 0.68 | Velocity threshold for chain ripple |
