//! Terminal tools for checking engines without the visuals.
//!
//! `--probe file.wav [engine|all] [gate_db]` streams a file through each engine
//! in 10 ms chunks, exactly as live input would arrive, and prints every note
//! event stamped with the time it would have fired. Comparing those stamps to
//! the file's real note starts gives each engine's end-to-end latency.
//!
//! `--listen [engine]` prints live detections from the audio input.

use std::time::{Duration, Instant};

use super::resample::{DcBlocker, Resampler};
use super::tracker::Tracker;
use super::{build_engine, engine_index, AudioInput, DEFAULT_GATE_DB, ENGINES};

const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

pub fn note_name(n: u8) -> String {
    format!("{}{}", NAMES[n as usize % 12], n as i32 / 12 - 1)
}

pub fn run(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or("usage: --probe <file.wav> [engine|all] [gate_db]")?;
    let which: Vec<usize> = match args.get(1).map(String::as_str) {
        None | Some("all") => (0..ENGINES.len()).collect(),
        Some(name) => vec![engine_index(name).ok_or(format!("unknown engine '{name}', expected one of {ENGINES:?}"))?],
    };
    let gate = args.get(2).and_then(|g| g.parse().ok()).unwrap_or(DEFAULT_GATE_DB);

    let (audio, rate) = read_wav(path)?;
    let secs = audio.len() as f32 / rate as f32;
    eprintln!("{path}: {secs:.2} s at {rate} Hz, gate {gate} dB");

    for idx in which {
        let mut engine = build_engine(idx)?;
        let mut dc = DcBlocker::new(rate);
        let mut resampler = Resampler::new(rate, engine.sample_rate());
        let mut tracker = Tracker::new(engine.tracker());
        let (mut buf, mut frames, mut events, mut block) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let chunk = (rate / 100) as usize;
        let mut busy = 0.0f32;

        for (ci, c) in audio.chunks(chunk).enumerate() {
            block.clear();
            block.extend_from_slice(c);
            dc.process(&mut block);
            buf.clear();
            resampler.process(&block, &mut buf);
            let t0 = Instant::now();
            engine.push(&buf, &mut frames);
            busy += t0.elapsed().as_secs_f32();
            for f in frames.drain(..) {
                tracker.step(&f, gate, &mut events);
            }
            let t = ((ci + 1) * chunk) as f32 / rate as f32;
            for (n, v, on) in events.drain(..) {
                println!("{} {t:.3} {} {n} {} {v:.2}", engine.name(), if on { "ON " } else { "OFF" }, note_name(n));
            }
        }
        eprintln!("{}: {:.1}x real time", engine.name(), secs / busy.max(1e-6));
    }
    Ok(())
}

pub fn listen(args: &[String]) -> Result<(), String> {
    let engine = match args.first() {
        Some(name) => engine_index(name).ok_or(format!("unknown engine '{name}', expected one of {ENGINES:?}"))?,
        None => 0,
    };
    let secs = std::env::var("LISTEN_SECS").ok().and_then(|v| v.parse::<f32>().ok());
    let input = AudioInput::start(engine, DEFAULT_GATE_DB)?;
    let start = Instant::now();
    let mut last_meter = Instant::now();
    eprintln!("Listening{}. Ctrl-C to stop.", secs.map_or(String::new(), |s| format!(" for {s} s")));
    while secs.is_none_or(|s| start.elapsed().as_secs_f32() < s) {
        std::thread::sleep(Duration::from_millis(10));
        let mut s = input.state.lock().unwrap();
        let t = start.elapsed().as_secs_f32();
        for e in s.recent_events.drain(..) {
            println!("{t:7.3} {} {} {:.2}", if e.on { "ON " } else { "OFF" }, note_name(e.note), e.velocity);
        }
        if last_meter.elapsed() > Duration::from_secs(1) {
            last_meter = Instant::now();
            let held: Vec<String> = (0..128u8).filter(|&n| s.notes[n as usize].on).map(note_name).collect();
            eprintln!("{t:7.1}s  {:6.1} dB  cpu {:3.0}%  held [{}]", s.level_db, s.load * 100.0, held.join(" "));
        }
    }
    Ok(())
}

fn read_wav(path: &str) -> Result<(Vec<f32>, u32), String> {
    let mut r = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = r.spec();
    let ch = spec.channels as usize;
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>().map(|s| s.map(|v| v as f32 * scale)).collect()
        }
    }
    .map_err(|e| e.to_string())?;
    let mono = interleaved.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect();
    Ok((mono, spec.sample_rate))
}
