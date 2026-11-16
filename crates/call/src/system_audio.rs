//! What this computer plays, sent with a shared screen (Discord's "share
//! audio"): every other app's sound, but none of this app's own (the call's
//! voices, its chimes, a video it plays), or everyone in the call would hear
//! themselves come back. 48 kHz stereo in 10 ms frames, with no processing
//! (no echo cancellation or noise suppression: it is music and films too).
//!
//! - Linux: one PulseAudio record stream on each other app's playback
//!   (`direct_on_input`, what pavucontrol's per-app meters use), mixed here.
//!   PipeWire's PulseAudio server and PulseAudio itself both do this; apps
//!   are followed as they start and stop. This app's own streams are told
//!   apart by their `application.process.id`.
//! - Windows: WASAPI process loopback of everything but this process tree
//!   (Windows 10 2004 and later).
//! - macOS 13 and later: a ScreenCaptureKit stream of sound alone, less this
//!   process, of what was picked in the system's picker or else the main
//!   display (`system_audio_sck.rs`).
//! - Test runs ([`AudioMode::Tone`]): a tone, [`TONE_LEFT`] Hz on the left
//!   and [`TONE_RIGHT`] Hz on the right, instead of the computer's sound.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use crate::imp::AudioMode;

/// Samples a second.
pub(crate) const RATE: u32 = 48_000;
/// Frames (per channel) in 10 ms.
pub(crate) const FRAME: usize = (RATE / 100) as usize;
/// Channels: stereo.
pub(crate) const CHANNELS: usize = 2;

/// Captured sound buffers that could not be read (ScreenCaptureKit's), for
/// the self-test's report.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) static UNREADABLE: AtomicU64 = AtomicU64::new(0);

/// The test runs' screen sound, left and right (the voice's tone is 440 Hz).
pub const TONE_LEFT: f32 = 660.0;
pub const TONE_RIGHT: f32 = 990.0;

/// Where captured frames go: 10 ms of interleaved stereo at a time.
pub(crate) type Out = Box<dyn FnMut(&[i16]) + Send + 'static>;

/// The capture, running until dropped.
pub(crate) struct Capture {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    #[cfg(target_os = "linux")]
    _pulse: Option<crate::imp::system_audio_pulse::Running>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        #[cfg(target_os = "linux")]
        drop(self._pulse.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Why this platform cannot share its sound, if it cannot (shown by the
/// audio choice when sharing).
pub fn screen_audio_unavailable() -> Option<&'static str> {
    #[cfg(target_os = "macos")]
    return macos_unavailable(crate::imp::system_audio_sck::macos_major());
    #[cfg(not(target_os = "macos"))]
    if cfg!(any(target_os = "linux", target_os = "windows")) {
        None
    } else {
        Some("Sharing sound isn't available here")
    }
}

/// Why a Mac on macOS `major` cannot share its sound, if it cannot:
/// ScreenCaptureKit captures sound from macOS 13.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub(crate) fn macos_unavailable(major: isize) -> Option<&'static str> {
    (major < 13).then_some("Sharing your computer's sound needs macOS 13 or later")
}

/// Start capturing what the computer plays (or the test tone) into `out`.
pub(crate) fn start(mode: AudioMode, out: Out) -> Result<Capture, String> {
    let stop = Arc::new(AtomicBool::new(false));
    if mode == AudioMode::Tone {
        let running = stop.clone();
        let thread = std::thread::Builder::new()
            .name("morpheus-screen-tone".into())
            .spawn(move || play_tone(&running, out))
            .map_err(|err| err.to_string())?;
        return Ok(Capture {
            stop,
            thread: Some(thread),
            #[cfg(target_os = "linux")]
            _pulse: None,
        });
    }
    #[cfg(target_os = "linux")]
    {
        let running = crate::imp::system_audio_pulse::start(
            crate::imp::system_audio_pulse::Filter::others(),
            out,
        )?;
        Ok(Capture {
            stop,
            thread: None,
            _pulse: Some(running),
        })
    }
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        #[cfg(target_os = "macos")]
        use crate::imp::system_audio_sck::run;
        #[cfg(target_os = "windows")]
        use crate::imp::system_audio_wasapi::run;
        let running = stop.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("morpheus-screen-audio".into())
            .spawn(move || run(&running, out, tx))
            .map_err(|err| err.to_string())?;
        // Started, or why not (ScreenCaptureKit gets up to 10 s for each of
        // its two answers).
        match rx.recv_timeout(Duration::from_secs(25)) {
            Ok(Ok(())) => Ok(Capture {
                stop,
                thread: Some(thread),
            }),
            Ok(Err(why)) => {
                let _ = thread.join();
                Err(why)
            }
            Err(_) => {
                stop.store(true, Ordering::Relaxed);
                Err("the system's sound did not start".into())
            }
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        let _ = out;
        Err(screen_audio_unavailable()
            .unwrap_or("not available")
            .to_owned())
    }
}

/// A check of the real capture (not the test tone) for CI's Macs and for
/// trying it by hand: capture for `seconds`, then say how many 10 ms frames
/// came and how loud the loudest was, or why it did not start.
pub fn screen_audio_self_test(seconds: f32) -> Result<String, String> {
    let seen = Arc::new(std::sync::Mutex::new((0u64, 0i32)));
    UNREADABLE.store(0, Ordering::Relaxed);
    let into = seen.clone();
    let began = Instant::now();
    let capture = start(
        AudioMode::Platform,
        Box::new(move |frame: &[i16]| {
            if let Ok(mut seen) = into.lock() {
                seen.0 += 1;
                let loudest = frame.iter().map(|s| i32::from(*s).abs()).max().unwrap_or(0);
                seen.1 = seen.1.max(loudest);
            }
        }),
    )?;
    let took = began.elapsed();
    std::thread::sleep(Duration::from_secs_f32(seconds.max(0.0)));
    drop(capture);
    let (frames, peak) = seen.lock().map(|s| *s).unwrap_or_default();
    Ok(format!(
        "started in {} ms; {frames} frames ({:.1} s of sound) in {seconds} s; peak {:.3}; {} unreadable",
        took.as_millis(),
        frames as f32 / 100.0,
        peak as f32 / 32768.0,
        UNREADABLE.load(Ordering::Relaxed)
    ))
}

/// The test tone, in real time (by the clock, not by a sleep each time, so
/// it does not drift slow).
fn play_tone(stop: &AtomicBool, mut out: Out) {
    let steps = [TONE_LEFT, TONE_RIGHT].map(|hz| hz * std::f32::consts::TAU / RATE as f32);
    let mut phase = [0f32; 2];
    let mut frame = vec![0i16; FRAME * CHANNELS];
    let began = Instant::now();
    let mut sent = 0u64;
    while !stop.load(Ordering::Relaxed) {
        for pair in frame.chunks_exact_mut(CHANNELS) {
            for (c, s) in pair.iter_mut().enumerate() {
                phase[c] = (phase[c] + steps[c]) % std::f32::consts::TAU;
                *s = (phase[c].sin() * 8_000.0) as i16;
            }
        }
        out(&frame);
        sent += 1;
        let due = began + Duration::from_millis(sent * 10);
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
}

/// Sums 10 ms frames of several streams into one, kept from clipping hard.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn mix_into(sum: &mut [i32], frame: impl IntoIterator<Item = i16>) {
    for (s, v) in sum.iter_mut().zip(frame) {
        *s += i32::from(v);
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn clamp(sum: &[i32], out: &mut [i16]) {
    for (o, s) in out.iter_mut().zip(sum) {
        *o = (*s).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
    }
}

/// Frequency analysis for tests and the receiving side's figures: how many
/// times each channel crosses zero, as a frequency.
pub(crate) fn zero_crossing_hz(samples: &[i16], channels: usize, channel: usize) -> f32 {
    let mut crossings = 0usize;
    let mut last: Option<bool> = None;
    let mut frames = 0usize;
    for frame in samples.chunks_exact(channels) {
        frames += 1;
        let v = frame[channel];
        // A little hysteresis: noise around zero is not a crossing.
        let now = if v > 200 {
            Some(true)
        } else if v < -200 {
            Some(false)
        } else {
            None
        };
        if let Some(now) = now {
            if last.is_some_and(|was| was != now) {
                crossings += 1;
            }
            last = Some(now);
        }
    }
    if frames == 0 {
        return 0.0;
    }
    crossings as f32 / 2.0 / (frames as f32 / RATE as f32)
}

/// The power of `hz` in one channel of interleaved samples, 0 to about 0.25
/// for a full-scale sine (Goertzel).
#[cfg(test)]
pub(crate) fn power_at(samples: &[i16], channels: usize, channel: usize, hz: f32) -> f32 {
    let w = std::f32::consts::TAU * hz / RATE as f32;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0f32, 0f32);
    let mut n = 0usize;
    for frame in samples.chunks_exact(channels) {
        let x = f32::from(frame[channel]) / 32768.0;
        let s = x + coeff * s1 - s2;
        s2 = s1;
        s1 = s;
        n += 1;
    }
    if n == 0 {
        return 0.0;
    }
    let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
    power / (n as f32 * n as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_test_tone_is_stereo_at_its_two_pitches() {
        let got = Arc::new(std::sync::Mutex::new(Vec::<i16>::new()));
        let into = got.clone();
        let capture = start(
            AudioMode::Tone,
            Box::new(move |frame| into.lock().expect("lock").extend_from_slice(frame)),
        )
        .expect("starts");
        std::thread::sleep(Duration::from_millis(300));
        drop(capture);
        let got = got.lock().expect("lock").clone();
        assert!(got.len() >= 20 * FRAME * CHANNELS, "{} samples", got.len());
        let left = zero_crossing_hz(&got, 2, 0);
        let right = zero_crossing_hz(&got, 2, 1);
        assert!((left - TONE_LEFT).abs() < 15.0, "left {left}");
        assert!((right - TONE_RIGHT).abs() < 15.0, "right {right}");
        assert!(power_at(&got, 2, 0, TONE_LEFT) > 100.0 * power_at(&got, 2, 0, TONE_RIGHT));
    }

    #[test]
    fn macs_share_their_sound_from_macos_13() {
        for old in [11, 12] {
            assert!(macos_unavailable(old).is_some_and(|why| why.contains("macOS 13")));
        }
        for new in [13, 14, 15, 26] {
            assert_eq!(macos_unavailable(new), None);
        }
    }
}
