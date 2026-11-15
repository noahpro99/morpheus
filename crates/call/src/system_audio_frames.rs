//! Captured sound in whatever layout a platform hands it over, out as the
//! shared screen's track takes it: 48 kHz interleaved stereo `i16` in 10 ms
//! frames (see `system_audio.rs`). macOS's ScreenCaptureKit gives 32-bit
//! float, one buffer per channel, at the rate it was asked for (or the
//! device's, if it does not honour that); this turns any linear PCM of one or
//! more channels, planar or interleaved, 16-bit or float, into that. Plain
//! Rust, so it is tested everywhere.

use super::system_audio::{CHANNELS, FRAME, Out, RATE};

/// Samples in a 10 ms stereo frame.
const SAMPLES: usize = FRAME * CHANNELS;

/// How the samples handed to [`Framer::push`] are laid out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Layout {
    /// Frames a second.
    pub rate: f64,
    pub channels: usize,
    /// 32-bit float if so, else signed 16-bit integers.
    pub float: bool,
    /// One buffer with the channels interleaved, else one buffer each.
    pub interleaved: bool,
}

/// CoreAudio's `kAudioFormatLinearPCM` ('lpcm').
pub(crate) const LINEAR_PCM: u32 = u32::from_be_bytes(*b"lpcm");
const FLAG_FLOAT: u32 = 1 << 0;
const FLAG_BIG_ENDIAN: u32 = 1 << 1;
const FLAG_SIGNED: u32 = 1 << 2;
const FLAG_NON_INTERLEAVED: u32 = 1 << 5;

impl Layout {
    /// From the fields of a CoreAudio `AudioStreamBasicDescription`, if it
    /// is a layout this can read.
    pub(crate) fn from_core_audio(
        rate: f64,
        format_id: u32,
        flags: u32,
        bits: u32,
        channels: u32,
    ) -> Result<Self, String> {
        let what = || {
            format!(
                "{} at {rate} Hz, {channels} channels, {bits} bits, flags {flags:#x}",
                String::from_utf8_lossy(&format_id.to_be_bytes())
            )
        };
        if format_id != LINEAR_PCM
            || flags & FLAG_BIG_ENDIAN != 0
            || channels == 0
            || !(rate.is_finite() && rate >= 8_000.0)
        {
            return Err(format!("unreadable sound: {}", what()));
        }
        let float = match (flags & FLAG_FLOAT != 0, bits) {
            (true, 32) => true,
            (false, 16) if flags & FLAG_SIGNED != 0 => false,
            _ => return Err(format!("unreadable sound: {}", what())),
        };
        Ok(Self {
            rate,
            channels: channels as usize,
            float,
            interleaved: flags & FLAG_NON_INTERLEAVED == 0,
        })
    }

    fn bytes_per_sample(&self) -> usize {
        if self.float { 4 } else { 2 }
    }
}

/// Turns captured sound into 10 ms frames for [`Out`], keeping what is left
/// over (part of a frame, the resampler's place) for the next buffer.
pub(crate) struct Framer {
    out: Out,
    /// Stereo input not yet resampled (the last one kept to interpolate
    /// from), and the place of the next output sample among it.
    input: Vec<[f32; 2]>,
    at: f64,
    /// The rate `input` is at; a change starts the resampler over.
    rate: f64,
    /// Output not yet a whole frame.
    pending: Vec<i16>,
    /// Frames sent so far.
    pub(crate) sent: u64,
    /// The loudest sample sent, 0 to 1 (for the self-test's report).
    pub(crate) peak: f32,
}

impl Framer {
    pub(crate) fn new(out: Out) -> Self {
        Self {
            out,
            input: Vec::new(),
            at: 0.0,
            rate: f64::from(RATE),
            pending: Vec::with_capacity(2 * SAMPLES),
            sent: 0,
            peak: 0.0,
        }
    }

    /// Takes `frames` frames laid out as `layout` says: `buffers` holds one
    /// buffer (interleaved) or one per channel (planar), in native byte
    /// order. Short buffers are read as far as they go. Returns the 10 ms
    /// frames sent.
    pub(crate) fn push(&mut self, layout: &Layout, buffers: &[&[u8]], frames: usize) -> usize {
        let wanted = if layout.interleaved { 1 } else { layout.channels };
        if buffers.len() < wanted || layout.channels == 0 {
            return 0;
        }
        let width = layout.bytes_per_sample();
        // The sample of channel `c` in frame `i`.
        let sample = |c: usize, i: usize| -> f32 {
            let (buffer, index) = if layout.interleaved {
                (buffers[0], i * layout.channels + c)
            } else {
                (buffers[c], i)
            };
            let Some(bytes) = buffer.get(index * width..(index + 1) * width) else {
                return 0.0;
            };
            if layout.float {
                f32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
            } else {
                f32::from(i16::from_ne_bytes([bytes[0], bytes[1]])) / 32768.0
            }
        };
        let right = if layout.channels > 1 { 1 } else { 0 };
        if (layout.rate - self.rate).abs() > 0.5 {
            self.rate = layout.rate;
            self.input.clear();
            self.at = 0.0;
        }
        let before = self.sent;
        if (self.rate - f64::from(RATE)).abs() <= 0.5 {
            for i in 0..frames {
                let pair = [sample(0, i), sample(right, i)];
                self.emit(pair);
            }
        } else {
            self.input
                .extend((0..frames).map(|i| [sample(0, i), sample(right, i)]));
            self.resample();
        }
        (self.sent - before) as usize
    }

    /// Linear interpolation from `self.rate` to [`RATE`], as far as the input
    /// goes; the last input sample stays to interpolate from next time.
    fn resample(&mut self) {
        let step = self.rate / f64::from(RATE);
        while self.at + 1.0 < self.input.len() as f64 {
            let i = self.at as usize;
            let t = (self.at - i as f64) as f32;
            let (a, b) = (self.input[i], self.input[i + 1]);
            self.emit([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
            self.at += step;
        }
        let used = (self.at as usize).min(self.input.len().saturating_sub(1));
        self.input.drain(..used);
        self.at -= used as f64;
    }

    fn emit(&mut self, pair: [f32; 2]) {
        for v in pair {
            let v = v.clamp(-1.0, 1.0);
            self.peak = self.peak.max(v.abs());
            self.pending.push((v * 32767.0).round() as i16);
        }
        if self.pending.len() == SAMPLES {
            (self.out)(&self.pending);
            self.pending.clear();
            self.sent += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::imp::system_audio::zero_crossing_hz;

    fn collecting() -> (Framer, Arc<Mutex<Vec<Vec<i16>>>>) {
        let got = Arc::new(Mutex::new(Vec::new()));
        let into = got.clone();
        let framer = Framer::new(Box::new(move |frame: &[i16]| {
            into.lock().expect("lock").push(frame.to_vec())
        }));
        (framer, got)
    }

    fn f32_bytes(samples: impl IntoIterator<Item = f32>) -> Vec<u8> {
        samples.into_iter().flat_map(f32::to_ne_bytes).collect()
    }

    fn i16_bytes(samples: impl IntoIterator<Item = i16>) -> Vec<u8> {
        samples.into_iter().flat_map(i16::to_ne_bytes).collect()
    }

    const PLANAR_FLOAT: Layout = Layout {
        rate: 48_000.0,
        channels: 2,
        float: true,
        interleaved: false,
    };

    #[test]
    fn core_audio_formats_are_read_or_refused() {
        // ScreenCaptureKit's: float, non-interleaved, packed.
        let sck = Layout::from_core_audio(48_000.0, LINEAR_PCM, 0x29, 32, 2).expect("float");
        assert_eq!(sck, PLANAR_FLOAT);
        let int = Layout::from_core_audio(44_100.0, LINEAR_PCM, 0x0c, 16, 2).expect("i16");
        assert!(!int.float && int.interleaved && int.rate == 44_100.0);
        // Compressed, big-endian, 24-bit and unsigned are not read.
        let aac = u32::from_be_bytes(*b"aac ");
        assert!(Layout::from_core_audio(48_000.0, aac, 0, 0, 2).is_err());
        assert!(Layout::from_core_audio(48_000.0, LINEAR_PCM, 0x0e, 16, 2).is_err());
        assert!(Layout::from_core_audio(48_000.0, LINEAR_PCM, 0x0c, 24, 2).is_err());
        assert!(Layout::from_core_audio(48_000.0, LINEAR_PCM, 0x08, 16, 2).is_err());
        assert!(Layout::from_core_audio(48_000.0, LINEAR_PCM, 0x29, 32, 0).is_err());
    }

    #[test]
    fn planar_float_comes_out_interleaved_in_ten_millisecond_frames() {
        let (mut framer, got) = collecting();
        // 15 ms: left rising, right falling.
        let n = FRAME + FRAME / 2;
        let left = f32_bytes((0..n).map(|i| i as f32 / n as f32));
        let right = f32_bytes((0..n).map(|i| -(i as f32) / n as f32));
        assert_eq!(framer.push(&PLANAR_FLOAT, &[&left, &right], n), 1);
        // Another 5 ms finishes the second frame.
        let rest = f32_bytes(vec![0.5; FRAME / 2]);
        assert_eq!(framer.push(&PLANAR_FLOAT, &[&rest, &rest], FRAME / 2), 1);
        let got = got.lock().expect("lock");
        assert_eq!(got.len(), 2);
        assert!(got.iter().all(|f| f.len() == FRAME * CHANNELS));
        let first = &got[0];
        assert_eq!((first[0], first[1]), (0, 0));
        let i = 100;
        let want = (i as f32 / n as f32 * 32767.0).round() as i16;
        assert_eq!((first[2 * i], first[2 * i + 1]), (want, -want));
        // The second frame: the rest of the ramp, then the 0.5s.
        assert_eq!(got[1][2 * (FRAME / 2)], 16384);
        assert_eq!(*got[1].last().expect("sample"), 16384);
        assert!((framer.peak - 1.0).abs() < 0.01, "peak {}", framer.peak);
    }

    #[test]
    fn interleaved_i16_at_48k_passes_through_unchanged() {
        let (mut framer, got) = collecting();
        let samples: Vec<i16> = (0..(3 * FRAME * CHANNELS) as i32)
            .map(|i| (i * 7 % 30_000 - 15_000) as i16)
            .collect();
        let layout = Layout {
            rate: 48_000.0,
            channels: 2,
            float: false,
            interleaved: true,
        };
        // In uneven pieces.
        let bytes = i16_bytes(samples.iter().copied());
        let mut at = 0;
        for frames in [7, 500, 333, 600] {
            let end = (at + frames * 4).min(bytes.len());
            framer.push(&layout, &[&bytes[at..end]], (end - at) / 4);
            at = end;
        }
        let out: Vec<i16> = got.lock().expect("lock").concat();
        assert_eq!(out, samples);
    }

    #[test]
    fn mono_goes_to_both_sides_and_more_channels_keep_the_first_two() {
        let (mut framer, got) = collecting();
        let mono = Layout {
            rate: 48_000.0,
            channels: 1,
            float: true,
            interleaved: true,
        };
        framer.push(&mono, &[&f32_bytes(vec![0.25; FRAME])], FRAME);
        let surround = Layout {
            channels: 6,
            ..mono
        };
        let frame: Vec<f32> = [0.1, -0.2, 0.9, 0.9, 0.9, 0.9].repeat(FRAME);
        framer.push(&surround, &[&f32_bytes(frame)], FRAME);
        let got = got.lock().expect("lock");
        assert!(got[0].iter().all(|&s| s == 8192));
        assert!(got[1].chunks(2).all(|p| p == [3277, -6553]), "{:?}", &got[1][..4]);
    }

    #[test]
    fn other_rates_are_resampled_to_48k_without_seams() {
        // 1 kHz on the left, 3 kHz on the right, at 44.1 kHz, in buffers of
        // 512 (ScreenCaptureKit's usual size) and one of 100.
        let rate = 44_100.0f64;
        let n = 44_100usize;
        let tone = |hz: f64, i: usize| (std::f64::consts::TAU * hz * i as f64 / rate).sin() as f32 * 0.5;
        let layout = Layout {
            rate,
            ..PLANAR_FLOAT
        };
        let (mut framer, got) = collecting();
        let mut i = 0;
        let mut sizes = [100usize].into_iter().chain(std::iter::repeat(512));
        while i < n {
            let len = sizes.next().unwrap_or(512).min(n - i);
            let left = f32_bytes((i..i + len).map(|k| tone(1_000.0, k)));
            let right = f32_bytes((i..i + len).map(|k| tone(3_000.0, k)));
            framer.push(&layout, &[&left, &right], len);
            i += len;
        }
        let out: Vec<i16> = got.lock().expect("lock").concat();
        // A second of input is a second of output (less part of a frame).
        let frames = out.len() / CHANNELS;
        assert!((47_500..=48_000).contains(&frames), "{frames} frames");
        let left = zero_crossing_hz(&out, CHANNELS, 0);
        let right = zero_crossing_hz(&out, CHANNELS, 1);
        assert!((left - 1_000.0).abs() < 10.0, "left {left}");
        assert!((right - 3_000.0).abs() < 30.0, "right {right}");
        // No seams: each step is no bigger than the sine's steepest.
        let steepest = (0.5 * std::f64::consts::TAU * 3_000.0 / 48_000.0 * 32767.0 * 1.1) as i32;
        for pair in out.chunks_exact(CHANNELS).collect::<Vec<_>>().windows(2) {
            let jump = (i32::from(pair[1][1]) - i32::from(pair[0][1])).abs();
            assert!(jump <= steepest, "jump {jump} > {steepest}");
        }
    }

    #[test]
    fn short_or_missing_buffers_do_not_panic() {
        let (mut framer, got) = collecting();
        let left = f32_bytes(vec![0.5; 10]);
        // Planar with one buffer: nothing.
        assert_eq!(framer.push(&PLANAR_FLOAT, &[&left], 10), 0);
        // Claims more frames than the buffers hold: the rest is silence.
        let right = f32_bytes(vec![0.5; 3]);
        assert_eq!(framer.push(&PLANAR_FLOAT, &[&left, &right], FRAME), 1);
        let got = got.lock().expect("lock");
        assert_eq!(got[0][2 * 5 + 1], 0);
        assert_eq!(got[0][2 * 5], 16384);
    }
}
