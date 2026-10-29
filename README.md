# midi-visuals

A MIDI-reactive visual synthesizer written in Rust using [Nannou](https://nannou.cc). Connect a MIDI controller and watch knobs and keys drive real-time visuals.

## Run

```
cargo run                # start on the default sketch (cardano)
cargo run -- cardano
cargo run -- fireworks
cargo run -- rings
```

## Control panel

While the app runs, it serves a control panel at <http://127.0.0.1:7878>. Press `W` in the
window to open it in the browser, or start with `--open`. From the panel you can:

- switch sketches and trigger their actions (reset, colour mode, rain, …)
- see and turn the knobs and wheels, and hit the pads, where they sit on the MPK mini, labelled
  with what they do in the active sketch, so a MIDI controller is optional. If a control on the
  page doesn't match the hardware, click its label and move or hit the real control to re-map
  it (saved in `presets/controller.json`; "reset layout" restores the defaults)
- load, save and delete presets
- switch notes between MIDI and audio, pick the audio source and pitch engine, and set the
  noise gate on a live level meter
- see the held notes on a keyboard, and the app's log (what used to scroll by in the terminal)

The panel only answers on 127.0.0.1. `--port=<n>` changes the port; when it is taken, the next
free one is used (the window's HUD shows which). `--no-panel` turns it off.

## Keyboard shortcuts

| Key   | Action                          |
|-------|---------------------------------|
| `Tab` | Cycle to the next sketch        |
| `H`   | Toggle the HUD overlay          |
| `S`   | Save current knob state as preset (then type a name + Enter) |
| `Enter` | Load selected preset          |
| `A`   | Switch notes between MIDI and audio input |
| `E`   | Cycle the audio pitch engine    |
| `-` / `=` | Audio noise gate down / up 3 dB |
| `W`   | Open the control panel in the browser |

## Pads

The MPK's pads trigger one-off actions instead of playing notes, in MIDI and audio mode
alike. Anything on MIDI channel 10 (the pads' default) counts as a pad; the keys stay notes.

| Bank | Pads 1–8 |
|------|----------|
| A (notes 36–43) | The active sketch's actions, in the order of its panel buttons. Pads 1–3 are the same in every sketch: layer on / off, surge, tune (see [Two streams](#two-streams)); the sketch's own actions follow (e.g. rings: Rain, Halo, Tune, Clear, Gravity, Push / pull) |
| B (notes 44–51) | Next sketch, MIDI / audio, HUD, Load preset |

Pad 1 is bottom left, pad 5 top left. The panel shows each pad's action and lights it while held.

## Two streams

Every sketch draws two streams:

- **Auto** follows the notes you play (MIDI or audio).
- **Layer** is played by hand on top of it, in the opposite hue. Pad 1 (`G`) switches it on
  and off; pad 2 (`B`) fires a one-off surge, which shows for a moment even while the layer is off.

The knobs tune both streams at once. Pad 3 (`T`) switches them to tune only the auto stream,
then only the layer, then both again. The stream not being tuned keeps its values, so the two can
drift apart. The HUD shows `tune auto` or `tune layer` while only one is tuned.

| Sketch | Layer (pad 1, `G`) | Surge (pad 2, `B`) | Knobs that aren't per stream |
|---|---|---|---|
| cardano | a circle system turning the other way, which ignores the notes | Kick: knocks the layer's circles out of orbit | align, trail |
| droplets | a flow of droplets through a band | Surge: a burst across the band | decay (shared); flow rate, y, width (layer only) |
| fireworks | a barrage of shells at random spots | Volley: five shells across the width | barrage rate (CC30, layer only) |
| kaleidoscope | the contour lines of a second pattern | Surge: thickens and brightens the lines | smooth |
| mandala | a halo of petals round the mandala, turning against it | Surge: stretches the halo and sends a pulse out | — |
| rings | rain from a turning point round the centre | Halo: twelve rings round the centre at once | rain rate, spread (layer only) |

The HUD (top-left) shows the active sketch name, current FPS, and a green dot that flashes on any note or knob input.
Presets are saved per-sketch in `presets/<sketch_name>.txt`. CC 31 scrolls through saved presets.

## Audio input

Notes can come from a microphone or instrument instead of MIDI: press `A` (or start with
`--audio`). Detected notes replace MIDI notes, so every sketch reacts to them as if they were
played on the keyboard. CC knobs keep coming from the MIDI controller.

```
cargo run --release -- mandala --audio                    # start on audio input
cargo run --release -- rings --audio --engine=swift-f0    # pick the engine
AUDIO_DEVICE="USB Audio" AUDIO_CHANNEL=1 cargo run --release -- --audio   # pick device and channel
```

Three engines, cycled with `E`:

| Engine | Polyphonic | Latency* | Best for | How |
|---|---|---|---|---|
| `basic-pitch` (default) | yes | ~100 ms | chords on any instrument | Spotify's [Basic Pitch](https://github.com/spotify/basic-pitch) model over a sliding 2 s window |
| `swift-f0` | no | ~75 ms | voice, lead lines, bass | [SwiftF0](https://github.com/lars76/swift-f0) monophonic pitch model |
| `dsp-poly` | yes | ~80–130 ms | no-model fallback, very low CPU | harmonic-sum multi-pitch with iterative cancellation |

\*Median time from a note's start to its event, measured offline on rendered piano, guitar and
flute clips. Accuracy on those clips: basic-pitch found 97% of chord notes, dsp-poly about 80%, and
swift-f0 100% of melody notes. Basic Pitch uses about one CPU core; the others are negligible.

The HUD shows the engine, input level, gate, CPU load and the currently detected notes. If
notes trigger from background noise, raise the gate with `=`; if quiet playing is missed,
lower it with `-`. The models are vendored in `assets/models/` (SwiftF0: MIT, Basic Pitch:
Apache-2.0); the ONNX Runtime library is downloaded by the `ort` crate on the first build.

### Choosing the input

The easiest way is the **Listen to** menu in the control panel: it lists every PipeWire/Pulse
source, and the level meter shows straight away whether the chosen one carries sound. To
visualise music you're playing, pick the output you listen on under *Playing on this
computer*. The choice switches live and is remembered in `presets/audio.json` together with
the engine and gate.

From the terminal: `AUDIO_DEVICE` only sees ALSA devices (`default`, `pulse`, `USB Audio`, …). To use any
PipeWire/Pulse source, including the *monitor* of an output (to visualize the music you are
listening to), go through the `pulse` device and name the source:

```
pactl list short sources                                   # list sources
pactl get-default-source                                   # what `default` captures
AUDIO_DEVICE=pulse PULSE_SOURCE=<source name> cargo run --release -- --audio
```

To see which source actually carries sound, run `--listen` against it and watch the level
(silence is around -80 dB or lower; a playing instrument or music is roughly -40 to -10 dB):

```
AUDIO_DEVICE=pulse PULSE_SOURCE=<source name> LISTEN_SECS=5 cargo run --release -- --listen
```

Terminal tools for checking detection without the visuals:

```
cargo run --release -- --listen basic-pitch          # print live detections + level/CPU
cargo run --release -- --probe take.wav all          # run every engine over a WAV file
```

Tuning overrides (environment variables): `TRACK_ON`, `TRACK_OFF`, `TRACK_ON_FRAMES`,
`TRACK_OFF_FRAMES` (note on/off thresholds and debounce), `BASIC_PITCH_LOOKAHEAD` (frames,
default 6: lower is faster but misses more), `SWIFT_F0_LOOKAHEAD` (default 1).

## Sketches

### cardano
Spirograph-style epicycloid circles with spring deflection on note-on events.
Multiple independent collections can be layered with angular alignment control.

| Controller   | Parameter   | Range                      |
|--------------|-------------|----------------------------|
| CC 1 (mod wheel) | Hue (base) | full spectrum          |
| Pitch bend   | Hue (target)| full spectrum (0.5 = center) |
| CC 24        | Circles     | 1 – 16 inner circles       |
| CC 25        | Dot radius  | 1 – 80 px                  |
| CC 26        | Speed       | 0.05 – 6                   |
| CC 27        | Ratio       | 0.05 – 8 (inner/outer speed)|
| CC 28        | Orbit radius| 10 – 700 px                |
| CC 29        | Alignment   | 0 – 1 (spread between collections) |
| CC 30        | Trail length| 1 – 500 frames             |

Note-on events send a radial spring impulse — circles deflect outward then snap back.

Key bindings:

| Key | Action                              |
|-----|-------------------------------------|
| `G` | Layer on / off                      |
| `B` | Kick the layer                      |
| `T` | Knobs tune: both → auto → layer     |
| `R` | Clear trail                         |
| `C` | Cycle collections 1 → 2 → 3 → 4 → 1 |

HUD shows `NxM circles` (collections × circles per collection), the chord and tension.

---

### droplets
Simulated rain droplets follow each other's trails, leaving a glowing field. Droplets come in two streams:

- **Auto**: every note played (MIDI or audio) sends a cluster of droplets in at the note's height. High notes enter at the top, low notes at the bottom.
- **Layer**: a stream you control yourself, drawn in the opposite hue. You turn the flow on and off, set where and how wide it enters, and fire one-off surges.

The knobs tune both streams by default. With `T` they tune only the auto stream or only the layer. The other stream keeps its values, so the two can drift apart.

| CC | Parameter   | Range       | Applies to |
|----|-------------|-------------|------------|
| 24 | Base hue    | 0 – 360°    | per stream (layer = hue + 180°) |
| 25 | Flow rate   | 0 – 300 /s  | layer |
| 26 | Attraction  | 0 – 3       | per stream |
| 27 | Jitter      | 0 – 120     | per stream |
| 28 | Decay       | 0.05 – 2.0  | shared trail field |
| 29 | Flow y      | bottom – top | layer |
| 30 | Flow width  | 5 – 100 % of height | layer |

| Key | Action |
|-----|--------|
| `G` | Layer flow on / off |
| `B` | Surge: a burst of layer droplets across the flow band |
| `T` | Knobs tune: both → auto → layer |
| `R` | Clear |

HUD shows the droplet count, trail coverage, and `[flow+tune auto]`-style flags.

---

### fireworks
Note-on events launch glowing shells upward, positioned by note pitch. Each shell bursts into a radial spray of colored particles at its apex. Note velocity scales the explosion size.

| CC | Parameter   | Range       |
|----|-------------|-------------|
| 24 | Hue         | full spectrum |
| 25 | Shell speed | 150 – 800 px/s |
| 26 | Gravity     | 50 – 600    |
| 27 | Particles   | 20 – 300 per burst |
| 28 | Lifetime    | 0.5 – 4.0 s |
| 29 | Hue spread  | 0 – 0.5     |
| 30 | Barrage rate| 0.2 – 8 shells/s (layer) |

Note pitch maps the launch X position (low notes = left, high notes = right). Note velocity maps to explosion radius — a soft press makes a small tight burst, a hard press makes a large expansive one.

| Key | Action |
|-----|--------|
| `G` | Barrage on / off |
| `B` | Volley |
| `T` | Knobs tune: both → auto → layer |
| `R` | Clear |

HUD shows live shell and particle counts.

---

### rings
Note-on events spawn expanding rings positioned by note pitch. Optional gravity and rain mode.

| CC | Parameter  | Range        |
|----|------------|--------------|
| 24 | Hue        | full spectrum |
| 25 | Max radius | 5 – 1200 px  |
| 26 | Speed      | 5 – 800 px/s |
| 27 | Gravity    | 0 – 600      |
| 28 | Rain rate  | 0.1 – 30 /s  |
| 29 | Rain spread| 0 – 200 px   |

Key bindings:

| Key | Action                              |
|-----|-------------------------------------|
| `G` | Toggle rain (the layer)             |
| `B` | Halo                                |
| `T` | Knobs tune: both → auto → layer     |
| `R` | Clear all rings                     |
| `O` | Toggle gravity                      |
| `K` | Flip gravity between push and pull  |

HUD shows ring count and active modes.

---

## Requirements

- Rust (stable)
- A connected MIDI controller
- Linux: `libasound2-dev` for MIDI (`sudo apt install libasound2-dev`)

### Monitoring raw MIDI input

```bash
aconnect -l            # list ports
aseqdump -p <port>     # print all MIDI messages from that port
```
