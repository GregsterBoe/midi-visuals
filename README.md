# midi-visuals

A MIDI-reactive visual synthesizer written in Rust using [Nannou](https://nannou.cc). Connect a MIDI controller and watch knobs and keys drive real-time visuals.

## Run

```
cargo run              # start on the default sketch (aurora)
cargo run -- aurora    # start on a specific sketch by name
cargo run -- grid
```

## Keyboard shortcuts

| Key   | Action                        |
|-------|-------------------------------|
| `Tab` | Cycle to the next sketch      |
| `H`   | Toggle the HUD overlay        |

The HUD (top-left) shows the active sketch name, current FPS, and a green dot that flashes on any MIDI input.

## Sketches

### aurora
A glowing circle driven by four knobs.

| CC | Parameter  | Range        |
|----|------------|--------------|
| 24 | Hue        | full spectrum |
| 25 | Radius     | 40 – 300 px  |
| 26 | Saturation | 0.5 – 1.0    |
| 27 | Lightness  | 0.3 – 0.7    |

### grid
An 8×8 grid of cells. Each note-on event lights up the cell at `note % 64` with the note's velocity, then fades out over ~0.7 s.

## Requirements

- Rust (stable)
- A connected MIDI controller
- Linux: `libasound2-dev` for MIDI (`sudo apt install libasound2-dev`)
