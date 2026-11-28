//! Strong noise suppression on a phone (#531). A phone's microphone is captured by the app (Android's AudioRecord, in 10 ms frames of
//! 48 kHz mono) rather than by WebRTC's device module, handed to [`push_mic`], cleaned here with DeepFilterNet 3 (the desktop's
//! [`Denoiser`], run with tract) and given to the call's audio source. WebRTC's own device recording is off while it runs; the phone's
//! echo canceller stays on the capture (the app asks for the voice-communication source).
//!
//! [`Cleaner`] is the part that needs no call: frames in, cleaned frames out (30 ms later), so the path is tested, and timed, alone.

use std::{borrow::Cow, collections::VecDeque, sync::{Mutex, atomic::{AtomicU64, Ordering}}};

use futures_util::FutureExt as _;
use livekit::webrtc::{audio_frame::AudioFrame, audio_source::native::NativeAudioSource};

use crate::{
    gate::{Gate, LOOKAHEAD, level_db},
    imp::{
        SAMPLE_RATE,
        denoise::{Denoiser, FRAME},
    },
    noise::NoiseTuning,
};

/// Cleans a stream of 16-bit samples, whatever sizes they arrive in: DeepFilterNet when `model` (Strong), then the input sensitivity gate (#545)
/// when one is set. Either may be missing; with neither the samples come out as they went in.
pub struct Cleaner {
    denoiser: Option<Denoiser>,
    gate: Option<Gate>,
    /// The frames the gate looks ahead by, as samples, for what the model's own delay does not already give.
    gate_delay: VecDeque<i16>,
    pending: Vec<i16>,
    scratch: Vec<f32>,
}

impl Cleaner {
    /// Loads the model when asked (built in; about a second on a phone); `gate` is the input sensitivity: automatic, or this threshold in dBFS.
    pub fn new(model: bool, gate: Option<(bool, f32)>) -> Result<Self, String> {
        let denoiser = if model { Some(Denoiser::new(&NoiseTuning::default())?) } else { None };
        let gate = gate.map(|(auto, db)| {
            let mut gate = Gate::new();
            gate.set(auto, db);
            gate
        });
        Ok(Self { denoiser, gate, gate_delay: VecDeque::new(), pending: Vec::new(), scratch: vec![0.0; FRAME] })
    }

    /// Feed `samples`; whole 10 ms frames come back cleaned, a part of a frame waits for the next call. What comes out is
    /// [`Self::delay`] samples behind what went in.
    pub fn clean(&mut self, samples: &[i16]) -> Vec<i16> {
        self.pending.extend_from_slice(samples);
        let whole = self.pending.len() / FRAME * FRAME;
        let mut out = Vec::with_capacity(whole);
        for raw in self.pending.drain(..whole).collect::<Vec<_>>().chunks(FRAME) {
            let mut frame: Vec<i16> = raw.to_vec();
            if let Some(denoiser) = self.denoiser.as_mut() {
                for (to, from) in self.scratch.iter_mut().zip(raw) {
                    *to = f32::from(*from) / 32768.0;
                }
                denoiser.process(&mut self.scratch);
                frame = self.scratch.iter().map(|s| (s * 32767.0).round().clamp(-32768.0, 32767.0) as i16).collect();
            }
            if let Some(gate) = self.gate.as_mut() {
                // The gate decides on the frame as it came in, looking a few frames ahead: the voice is held back as long as that, less what the model's own delay gives.
                let level = level_db(raw);
                let verdict = gate.push_frame(raw);
                let held = self.denoiser.as_ref().map_or(0, |d| d.delay() / FRAME);
                let want = LOOKAHEAD.saturating_sub(held) * FRAME;
                while self.gate_delay.len() < want {
                    self.gate_delay.push_back(0);
                }
                while self.gate_delay.len() > want {
                    self.gate_delay.pop_front();
                }
                if want > 0 {
                    self.gate_delay.extend(frame.iter().copied());
                    for sample in frame.iter_mut() {
                        *sample = self.gate_delay.pop_front().unwrap_or(0);
                    }
                }
                gate.apply(&mut frame, verdict);
                crate::noise::note_gate(level, gate.threshold_db(), Some(verdict.open));
            }
            out.extend(frame);
        }
        out
    }

    /// How late the cleaned voice comes out, in samples.
    pub fn delay(&self) -> usize {
        let model = self.denoiser.as_ref().map_or(0, Denoiser::delay);
        model + if self.gate.is_some() { (LOOKAHEAD * FRAME).saturating_sub(model) } else { 0 }
    }
}

/// What a call going to start should take from the app: the model (Strong), and the input sensitivity gate when one is set (#545).
pub(crate) fn wants() -> (bool, Option<(bool, f32)>) {
    let tuning = crate::noise_tuning();
    let gate = Gate::active(tuning.gate_auto, tuning.gate_threshold_db).then_some((tuning.gate_auto, tuning.gate_threshold_db));
    (crate::noise_suppression() == crate::NoiseSuppression::Strong, gate)
}

/// Cleaned 10 ms frames sent into calls since the app started (tests read it to see the path run).
static CLEANED: AtomicU64 = AtomicU64::new(0);

pub fn frames_cleaned() -> u64 {
    CLEANED.load(Ordering::Relaxed)
}

/// A test tone as the microphone, through the same cleaning ([`AudioMode::Tone`] on a phone with Strong chosen).
pub(crate) async fn tone_through_cleaner() {
    let mut phase = 0f32;
    let step = 440.0 * std::f32::consts::TAU / SAMPLE_RATE as f32;
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(10));
    loop {
        tick.tick().await;
        let samples: Vec<i16> = (0..FRAME)
            .map(|_| {
                phase = (phase + step) % std::f32::consts::TAU;
                (phase.sin() * 8_000.0) as i16
            })
            .collect();
        if !push_mic(&samples) {
            return;
        }
    }
}

/// The call that wants the phone's microphone: frames go to the thread that cleans them (the model is not `Send`, so it lives in one thread).
static CALL: Mutex<Option<std::sync::mpsc::Sender<Vec<i16>>>> = Mutex::new(None);

/// A call starts taking its microphone from the app (Strong): `source` is its track's. False when the model could not load (the call
/// then keeps the platform's microphone).
pub(crate) fn attach(source: NativeAudioSource, model: bool, gate: Option<(bool, f32)>) -> bool {
    let (frames, incoming) = std::sync::mpsc::channel::<Vec<i16>>();
    let (ready, loaded) = std::sync::mpsc::sync_channel::<bool>(1);
    let spawned = std::thread::Builder::new().name("morpheus-noise".into()).spawn(move || {
        let mut cleaner = match Cleaner::new(model, gate) {
            Ok(cleaner) => {
                let _ = ready.send(true);
                cleaner
            }
            Err(err) => {
                tracing::warn!(%err, "the noise model did not load; the platform microphone is used");
                let _ = ready.send(false);
                return;
            }
        };
        while let Ok(samples) = incoming.recv() {
            let cleaned = cleaner.clean(&samples);
            for frame in cleaned.chunks(FRAME) {
                CLEANED.fetch_add(1, Ordering::Relaxed);
                let _ = source
                    .capture_frame(&AudioFrame { data: Cow::Borrowed(frame), sample_rate: SAMPLE_RATE, num_channels: 1, samples_per_channel: FRAME as u32 })
                    .now_or_never();
            }
        }
    });
    if spawned.is_err() || !loaded.recv().unwrap_or(false) {
        return false;
    }
    if let Ok(mut call) = CALL.lock() {
        *call = Some(frames);
    }
    true
}

pub(crate) fn detach() {
    // Dropping the sender ends the cleaning thread.
    if let Ok(mut call) = CALL.lock() {
        *call = None;
    }
}

/// A call is waiting for the app's microphone frames.
pub fn mic_wanted() -> bool {
    CALL.lock().is_ok_and(|call| call.is_some())
}

/// The app's microphone: `samples` (16-bit, 48 kHz, mono, any length) are cleaned and sent into the call. Returns whether a call took them.
pub fn push_mic(samples: &[i16]) -> bool {
    CALL.lock().is_ok_and(|call| call.as_ref().is_some_and(|frames| frames.send(samples.to_vec()).is_ok()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// 10 ms frames of a 220 Hz "voice" with a louder hiss over it.
    fn noisy(frames: usize) -> Vec<i16> {
        let mut seed = 1u32;
        (0..frames * FRAME)
            .map(|n| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let hiss = ((seed >> 16) as f32 / 32768.0 - 1.0) * 0.2;
                let voice = (n as f32 * 2.0 * std::f32::consts::PI * 220.0 / 48_000.0).sin() * 0.2;
                ((voice + hiss) * 32767.0) as i16
            })
            .collect()
    }

    fn rms(samples: &[i16]) -> f32 {
        (samples.iter().map(|s| f32::from(*s).powi(2)).sum::<f32>() / samples.len().max(1) as f32).sqrt()
    }

    #[test]
    fn a_gate_above_the_voice_closes_and_one_below_it_stays_open() {
        let tone: Vec<i16> = (0..100 * FRAME).map(|n| ((n as f32 * 2.0 * std::f32::consts::PI * 220.0 / 48_000.0).sin() * 4000.0) as i16).collect();
        // The tone is about -18 dBFS: a manual threshold of -5 shuts it out, -60 lets it through.
        let loud = |out: &[i16]| out[out.len() - 20 * FRAME..].iter().map(|s| i32::from(*s).abs()).max().unwrap_or(0);
        let mut shut = Cleaner::new(false, Some((false, -5.0))).expect("no model to load");
        let closed = shut.clean(&tone);
        assert!(loud(&closed) < 300, "closed: the tone is taken down to almost nothing: {}", loud(&closed));
        let mut open = Cleaner::new(false, Some((false, -60.0))).expect("no model to load");
        let through = open.clean(&tone);
        assert!(loud(&through) > 3500, "open: the tone comes through: {}", loud(&through));
        assert_eq!(through.len(), tone.len(), "every frame comes back");
    }

    #[test]
    fn frames_in_come_out_cleaned_whatever_sizes_they_arrive_in_and_a_frame_takes_far_less_than_ten_milliseconds() {
        let mut cleaner = Cleaner::new(true, None).expect("the built-in model loads");
        let input = noisy(100);
        let mut out = Vec::new();
        let started = Instant::now();
        // Odd sizes, as a phone's buffers come: nothing is lost, the rest waits for the next push.
        for chunk in input.chunks(700) {
            out.extend(cleaner.clean(chunk));
        }
        let per_frame = started.elapsed() / 100;
        assert_eq!(out.len() / FRAME, input.len() / FRAME, "every whole frame comes back");
        let _ = cleaner.delay();
        // Real time on a phone is 10 ms a frame; the model must leave room for the rest (the budget here is generous for a loaded machine).
        eprintln!("DeepFilterNet: {per_frame:?} per 10 ms frame");
        assert!(per_frame < std::time::Duration::from_millis(10), "slower than real time: {per_frame:?} per frame");
        // The hiss goes down: the cleaned tail (after the model has settled) is quieter than the input.
        let tail = out.len() - 20 * FRAME;
        assert!(rms(&out[tail..]) < rms(&input[tail..]) * 0.9, "the noise is taken down: {} vs {}", rms(&out[tail..]), rms(&input[tail..]));
    }
}
