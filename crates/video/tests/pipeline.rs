//! The demux and decode pipeline on the fixtures in `fixtures/` (made with
//! ffmpeg: `av.mp4` 6 s 320x240 30 fps H.264 High with B-frames + AAC-LC
//! 48 kHz stereo 440 Hz; `loop.mp4` 2 s, no sound; `clip.webm` VP9 + Opus).

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use morpheus_video::{Decoder, Options, Player, Unsupported, probe};

const AV: &[u8] = include_bytes!("../fixtures/av.mp4");
const LOOP: &[u8] = include_bytes!("../fixtures/loop.mp4");
const WEBM: &[u8] = include_bytes!("../fixtures/clip.webm");

fn secs(d: Duration) -> f64 {
    d.as_secs_f64()
}

#[test]
fn probe_reads_size_length_and_sound() {
    let info = probe(AV).expect("av.mp4 plays");
    assert_eq!((info.width, info.height), (320, 240));
    assert!((secs(info.duration) - 6.0).abs() < 0.1, "{:?}", info.duration);
    assert!(info.has_audio);
    let info = probe(LOOP).expect("loop.mp4 plays");
    assert!(!info.has_audio, "loop.mp4 has no sound");
    assert!((secs(info.duration) - 2.0).abs() < 0.1, "{:?}", info.duration);
}

#[test]
fn webm_and_garbage_are_unsupported() {
    assert!(matches!(probe(WEBM), Err(Unsupported::Format(_))), "{:?}", probe(WEBM).err());
    assert!(probe(b"not a video at all").is_err());
    // A cut-off MP4 must not panic.
    assert!(probe(&AV[..AV.len() / 3]).is_err() || probe(&AV[..AV.len() / 3]).is_ok());
}

#[test]
fn every_frame_decodes_in_display_order() {
    let mut decoder = Decoder::open(Arc::from(AV)).expect("open");
    let mut frames = Vec::new();
    while let Some(frame) = decoder.next_video() {
        frames.push(frame);
    }
    assert_eq!(frames.len(), 180, "6 s at 30 fps");
    for pair in frames.windows(2) {
        assert!(pair[1].pts > pair[0].pts, "{:?} then {:?}", pair[0].pts, pair[1].pts);
        let step = secs(pair[1].pts - pair[0].pts);
        assert!((step - 1.0 / 30.0).abs() < 0.002, "frames 1/30 s apart: {step}");
    }
    assert_eq!(frames[0].pts, Duration::ZERO);
    let first = &frames[0];
    assert_eq!((first.width, first.height), (320, 240));
    assert_eq!(first.bgra.len(), 320 * 240 * 4);
    assert!(first.bgra.as_chunks::<4>().0.iter().all(|p| p[3] == 255), "opaque");
    // The colours are ffmpeg's (mean R, G, B of frame 0: 120.6 123.5 125.7).
    let mean = |frame: &morpheus_video::Frame, channel: usize| {
        let sum: u64 = frame.bgra.as_chunks::<4>().0.iter().map(|p| u64::from(p[channel])).sum();
        sum as f64 / (frame.bgra.len() / 4) as f64
    };
    let (r, g, b) = (mean(first, 2), mean(first, 1), mean(first, 0));
    assert!(
        (r - 120.6).abs() < 6.0 && (g - 123.5).abs() < 6.0 && (b - 125.7).abs() < 6.0,
        "frame 0 mean rgb {r:.1} {g:.1} {b:.1}"
    );
    // The pattern moves.
    assert_ne!(frames[0].bgra, frames[15].bgra);
    assert_ne!(frames[15].bgra, frames[30].bgra);
}

#[test]
fn the_sound_is_the_440_hz_tone() {
    let mut decoder = Decoder::open(Arc::from(AV)).expect("open");
    let mut samples = Vec::new();
    let mut rate = 0;
    let mut channels = 0;
    let mut first_pts = None;
    while let Some(chunk) = decoder.next_audio() {
        first_pts.get_or_insert(chunk.pts);
        rate = chunk.rate;
        channels = chunk.channels;
        samples.extend(chunk.samples);
    }
    assert_eq!((rate, channels), (48_000, 2));
    assert!(first_pts.expect("some sound") <= Duration::from_millis(50));
    let seconds = samples.len() as f64 / f64::from(rate) / f64::from(channels);
    assert!((seconds - 6.0).abs() < 0.1, "{seconds} s of sound");
    // Left channel, the middle second: 440 cycles, 880 zero crossings.
    let left: Vec<f32> = samples.as_chunks::<2>().0.iter().map(|s| s[0]).collect();
    let middle = &left[2 * 48_000..3 * 48_000];
    let crossings = middle.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
    assert!((crossings as i64 - 880).abs() <= 6, "{crossings} zero crossings");
    let peak = middle.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(peak > 0.05, "audible: peak {peak}");
}

#[test]
fn seeking_lands_on_the_frame_and_sound_asked_for() {
    let mut decoder = Decoder::open(Arc::from(AV)).expect("open");
    for _ in 0..10 {
        decoder.next_video();
    }
    decoder.seek(Duration::from_millis(3000));
    let frame = decoder.next_video().expect("a frame after the seek");
    assert!(
        (secs(frame.pts) - 3.0).abs() < 0.02,
        "first frame after seeking to 3 s: {:?}",
        frame.pts
    );
    let chunk = decoder.next_audio().expect("sound after the seek");
    assert!((secs(chunk.pts) - 3.0).abs() < 0.05, "sound after seeking: {:?}", chunk.pts);
    // Back to the start.
    decoder.seek(Duration::ZERO);
    assert_eq!(decoder.next_video().expect("frame").pts, Duration::ZERO);
}

/// The player in real time, its clock the wall clock (no sound device).
#[test]
fn the_player_plays_pauses_seeks_and_loops() {
    let player = Player::open(Arc::from(AV), Options { audible: false, ..Options::default() }).expect("open");
    assert!(!player.status().playing, "starts paused");
    player.play();
    let started = Instant::now();
    std::thread::sleep(Duration::from_millis(800));
    let status = player.status();
    let wall = secs(started.elapsed());
    assert!(status.playing);
    assert!(
        (secs(status.position) - wall).abs() < 0.25,
        "position {:?} after {wall:.2} s",
        status.position
    );
    let frame = player.frame().expect("a frame shown");
    assert!(frame.index >= 15, "frames advance: shown #{}", frame.index);
    assert!((secs(frame.pts) - secs(status.position)).abs() < 0.1, "the frame shown is the one due");
    assert!(status.audio_samples > 0, "sound decoded");

    player.pause();
    std::thread::sleep(Duration::from_millis(100));
    let paused = player.status().position;
    let index = player.frame().expect("frame").index;
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(player.status().position, paused, "pause stops the clock");
    assert_eq!(player.frame().expect("frame").index, index, "and the pictures");

    player.seek(Duration::from_millis(3000));
    std::thread::sleep(Duration::from_millis(300));
    let status = player.status();
    assert!((secs(status.position) - 3.0).abs() < 0.05, "seeked while paused: {:?}", status.position);
    let frame = player.frame().expect("frame");
    assert!((secs(frame.pts) - 3.0).abs() < 0.05, "the picture at 3 s shows: {:?}", frame.pts);

    // A loop goes round.
    let looped = Player::open(
        Arc::from(LOOP),
        Options { audible: false, looped: true, muted: true, ..Options::default() },
    )
    .expect("open loop");
    looped.play();
    let mut wrapped = false;
    let mut last = Duration::ZERO;
    let deadline = Instant::now() + Duration::from_millis(3500);
    while Instant::now() < deadline {
        let now = looped.status().position;
        if now + Duration::from_millis(500) < last {
            wrapped = true;
        }
        last = now;
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(wrapped, "the 2 s loop came round within 3.5 s");
    assert!(looped.status().playing && !looped.status().ended);
    assert!(looped.status().loops >= 1);

    // Without a loop it ends.
    let once = Player::open(Arc::from(LOOP), Options { audible: false, ..Options::default() }).expect("open");
    once.seek(Duration::from_millis(1700));
    once.play();
    std::thread::sleep(Duration::from_millis(700));
    let status = once.status();
    assert!(status.ended && !status.playing, "ended: {status:?}");
}

/// Decoding speed on a file of your own (`MORPHEUS_VIDEO_BENCH=path.mp4
/// cargo test -p morpheus-video --release -- --ignored bench`): average and
/// worst time per picture (decode and conversion to BGRA), and per second of
/// sound.
#[test]
#[ignore = "needs MORPHEUS_VIDEO_BENCH"]
fn bench_decoding() {
    let Ok(path) = std::env::var("MORPHEUS_VIDEO_BENCH") else { return };
    let bytes = std::fs::read(path).expect("read");
    let mut decoder = Decoder::open(Arc::from(bytes)).expect("open");
    let mut times = Vec::new();
    loop {
        let started = Instant::now();
        let Some(frame) = decoder.next_video() else { break };
        times.push((started.elapsed(), frame.width, frame.height));
    }
    let total: Duration = times.iter().map(|t| t.0).sum();
    let worst = times.iter().map(|t| t.0).max().unwrap_or_default();
    let started = Instant::now();
    let mut sound = 0.0;
    while let Some(chunk) = decoder.next_audio() {
        sound += chunk.samples.len() as f64 / f64::from(chunk.rate) / f64::from(chunk.channels);
    }
    eprintln!(
        "{} pictures {}x{}: {:.2} ms each on average, worst {:.2} ms; sound {:.1} s in {:.1} ms",
        times.len(),
        times.first().map_or(0, |t| t.1),
        times.first().map_or(0, |t| t.2),
        total.as_secs_f64() * 1000.0 / times.len().max(1) as f64,
        worst.as_secs_f64() * 1000.0,
        sound,
        started.elapsed().as_secs_f64() * 1000.0
    );
}
