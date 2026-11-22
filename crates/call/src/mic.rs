//! This device's microphone on desktops, captured here rather than by
//! WebRTC's device module, so it can be cleaned before it is sent: WebRTC's
//! audio processing (echo cancellation against what the speakers play,
//! high-pass, gain, and in Standard its noise suppression), then in Strong
//! DeepFilterNet 3 ([`crate::imp::denoise`]). WebRTC's device module still
//! plays the call (its recording is switched off).
//!
//! One capture serves a call and the mic test together: the call's track
//! gets each cleaned 10 ms frame; the mic test plays them back (also fed to
//! the echo canceller, so the speakers do not feed back into the mic) and
//! shows their level.
//!
//! Test runs ([`AudioMode::Tone`]) capture no device: the "room" is a faint
//! noise floor that goes through the same processing, and the test tone (the
//! "voice") is added after it, as a suppressor keeps a voice. Nothing is
//! played back.

use std::{
    borrow::Cow,
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use futures_util::FutureExt as _;
use livekit::webrtc::{
    audio_frame::AudioFrame,
    audio_source::native::NativeAudioSource,
    native::{apm::AudioProcessingModule, audio_resampler::AudioResampler},
};
use tracing::{info, warn};

use crate::{
    NoiseSuppression, NoiseTuning,
    imp::{
        AudioMode, SAMPLE_RATE,
        denoise::{Denoiser, FRAME},
    },
    imp::mic_pace::{Pace, Timing},
    noise::{note_applied, note_applied_tuning, note_too_slow},
    noise_suppression, noise_tuning,
};

/// WebRTC's processing and, in Strong, the neural suppressor, on 10 ms
/// frames of 48 kHz mono.
pub struct Processor {
    mode: NoiseSuppression,
    /// A real microphone: echo cancellation and gain control on.
    live: bool,
    apm: AudioProcessingModule,
    /// Loaded the first time Strong is used, and kept.
    denoiser: Option<Denoiser>,
    tuning: NoiseTuning,
    scratch: Vec<f32>,
    /// Input sensitivity (#332) and the frames it holds back so that it can look ahead.
    gate: crate::gate::Gate,
    gate_delay: std::collections::VecDeque<i16>,
    /// The microphone frame before, for the gate's pitch.
    prev_raw: Vec<i16>,
}

impl Processor {
    pub fn new(mode: NoiseSuppression, live: bool) -> Self {
        Self {
            mode,
            live,
            apm: Self::apm(mode, live, &NoiseTuning::default()),
            denoiser: None,
            tuning: NoiseTuning::default(),
            scratch: vec![0.0; FRAME],
            gate: crate::gate::Gate::new(),
            gate_delay: std::collections::VecDeque::new(),
            prev_raw: Vec::new(),
        }
    }

    /// Which of WebRTC's processing is on: (echo cancellation, gain control,
    /// high-pass, noise suppression). Echo and gain need a real microphone.
    fn apm_flags(mode: NoiseSuppression, live: bool, tuning: &NoiseTuning) -> (bool, bool, bool, bool) {
        (
            live && tuning.echo_cancellation,
            live && tuning.auto_gain,
            tuning.high_pass,
            mode == NoiseSuppression::Standard,
        )
    }

    fn apm(mode: NoiseSuppression, live: bool, tuning: &NoiseTuning) -> AudioProcessingModule {
        let (echo, gain, high_pass, noise) = Self::apm_flags(mode, live, tuning);
        crate::diag::note_processing(echo, gain);
        AudioProcessingModule::new(echo, gain, high_pass, noise)
    }

    /// Process with `tuning` from the next frame: the neural suppressor and
    /// our smoothing follow at once; WebRTC's processing starts again when
    /// one of its switches changed (its echo canceller relearns the room).
    pub fn set_tuning(&mut self, tuning: NoiseTuning) {
        let tuning = tuning.clamped();
        if tuning == self.tuning {
            return;
        }
        if Self::apm_flags(self.mode, self.live, &tuning)
            != Self::apm_flags(self.mode, self.live, &self.tuning)
        {
            self.apm = Self::apm(self.mode, self.live, &tuning);
        }
        let model_changed = tuning.model != self.tuning.model;
        if let Some(denoiser) = self.denoiser.as_mut() {
            denoiser.tune(&tuning);
        }
        self.tuning = tuning;
        self.gate.set(tuning.gate_auto, tuning.gate_threshold_db);
        if !crate::gate::Gate::active(tuning.gate_auto, tuning.gate_threshold_db) {
            self.gate.reset();
        }
        // Another model: loaded now (about 0.2 s), the old one running till then.
        if model_changed && self.denoiser.is_some() && self.mode == NoiseSuppression::Strong {
            self.load_model();
        }
    }

    /// What the frames are tuned to now.
    pub fn tuning(&self) -> NoiseTuning {
        self.tuning
    }

    /// Process as `mode` from the next frame. WebRTC's processing starts
    /// again (its echo canceller relearns the room in about a second).
    pub fn set_mode(&mut self, mode: NoiseSuppression) {
        if mode == self.mode {
            return;
        }
        let standard = NoiseSuppression::Standard;
        if (mode == standard) != (self.mode == standard) {
            self.apm = Self::apm(mode, self.live, &self.tuning);
        }
        self.mode = mode;
    }

    /// What the frames are processed with now: Strong only once its model
    /// has loaded.
    pub fn applied(&self) -> NoiseSuppression {
        match self.mode {
            NoiseSuppression::Strong if self.denoiser.is_none() => NoiseSuppression::Off,
            mode => mode,
        }
    }

    /// How late the processed microphone comes out, in samples: WebRTC's
    /// echo canceller ([`APM_DELAY`]), its noise suppression in Standard
    /// ([`APM_NS_DELAY`]), DeepFilterNet in Strong ([`DELAY`]).
    pub fn delay(&self) -> usize {
        // The model that runs, which is the built-in one where the low-delay
        // file was not there.
        let mut tuning = self.tuning;
        if let Some(denoiser) = &self.denoiser {
            tuning.model = denoiser.model();
        }
        tuning.delay_samples(self.applied(), self.live)
    }

    /// Strong is chosen and its model not loaded yet (it takes ~0.2 s).
    pub fn needs_model(&self) -> bool {
        self.mode == NoiseSuppression::Strong && self.denoiser.is_none()
    }

    pub fn load_model(&mut self) {
        match Denoiser::new(&self.tuning) {
            Ok(denoiser) => self.denoiser = Some(denoiser),
            Err(err) => {
                warn!(%err, "noise suppression model did not load; using Standard");
                // For good (not retried every frame), and shown as such.
                crate::set_noise_suppression(NoiseSuppression::Standard);
                self.set_mode(NoiseSuppression::Standard);
            }
        }
    }

    /// What the speakers play, for the echo canceller.
    pub fn far_end(&mut self, frame: &mut [i16]) {
        if self.live {
            let _ = self
                .apm
                .process_reverse_stream(frame, SAMPLE_RATE as i32, 1);
        }
    }

    /// The microphone's next 10 ms, cleaned in place.
    #[cfg(test)]
    pub fn process(&mut self, frame: &mut [i16]) {
        self.process_paced(frame, true);
    }

    /// [`Processor::process`], with the neural model only if `neural`: a
    /// frame without it (the capture catching up, #101) is as late and
    /// keeps the voice ([`Denoiser::pass`]). Whether the model ran.
    pub fn process_paced(&mut self, frame: &mut [i16], neural: bool) -> bool {
        if frame.len() != FRAME {
            return false;
        }
        // What the gate measures: the microphone as it comes, as Discord's meter
        // does (not after the gain control, which pumps after a key and would
        // move the floor).
        let level = crate::gate::level_db(frame);
        let sharp = crate::gate::impulsive(frame);
        let voiced = level > self.gate.threshold_db() && crate::gate::voiced(&self.prev_raw, frame);
        self.prev_raw.extend_from_slice(frame);
        let extra = self.prev_raw.len().saturating_sub(2 * FRAME);
        self.prev_raw.drain(..extra);
        let _ = self.apm.process_stream(frame, SAMPLE_RATE as i32, 1);
        let ran = self.suppress(frame, neural);
        self.gate_frame(frame, level, sharp, voiced);
        ran
    }

    /// Input sensitivity (#332): the frame lowered between words, after the
    /// suppressor, which already holds the voice back by its own delay.
    fn gate_frame(&mut self, frame: &mut [i16], level: f32, sharp: bool, voiced: bool) {
        let t = self.tuning;
        crate::noise::note_gate(level, self.gate.threshold_db(), None);
        if !crate::gate::Gate::active(t.gate_auto, t.gate_threshold_db) {
            self.gate_delay.clear();
            return;
        }
        let held = match self.applied() {
            NoiseSuppression::Strong => self.denoiser.as_ref().map_or(0, |d| d.delay() / FRAME),
            _ => 0,
        };
        let want = crate::gate::LOOKAHEAD.saturating_sub(held) * FRAME;
        while self.gate_delay.len() < want {
            self.gate_delay.push_back(0);
        }
        while self.gate_delay.len() > want {
            self.gate_delay.pop_front();
        }
        if want > 0 {
            self.gate_delay.extend(frame.iter().copied());
            for s in frame.iter_mut() {
                *s = self.gate_delay.pop_front().unwrap_or(0);
            }
        }
        let verdict = self.gate.push_features(level, sharp, voiced);
        self.gate.apply(frame, verdict);
        crate::noise::note_gate(level, self.gate.threshold_db(), Some(verdict.open));
    }

    fn suppress(&mut self, frame: &mut [i16], neural: bool) -> bool {
        if self.mode != NoiseSuppression::Strong {
            return false;
        }
        let Some(denoiser) = self.denoiser.as_mut() else {
            return false;
        };
        for (f, s) in self.scratch.iter_mut().zip(frame.iter()) {
            *f = f32::from(*s) / 32768.0;
        }
        if neural {
            denoiser.process(&mut self.scratch);
        } else {
            denoiser.pass(&mut self.scratch);
        }
        for (s, f) in frame.iter_mut().zip(&self.scratch) {
            *s = (f * 32767.0) as i16;
        }
        let volume = crate::input_volume();
        if (volume - 1.0).abs() > 0.001 {
            for s in frame.iter_mut() {
                *s = (f32::from(*s) * volume).clamp(-32768.0, 32767.0) as i16;
            }
        }
        neural
    }
}

// ---------------------------------------------------------------- the capture

/// What the capture thread shares with the rest of the crate.
#[derive(Default)]
struct Shared {
    /// The call's microphone track, while there is one.
    call: Mutex<Option<(u64, NativeAudioSource)>>,
    /// The mic test is on: play the cleaned microphone back.
    testing: AtomicBool,
    stop: AtomicBool,
    /// Open the devices again (another was chosen).
    reopen: AtomicBool,
    /// Fade the microphone in again from the next frame ([`FadeIn`]): a
    /// call started hearing it, or it was unmuted.
    refade: AtomicBool,
    /// The cleaned microphone's level (an `f32`'s bits; full scale 1).
    level: AtomicU32,
    /// What is played back now goes somewhere.
    looping: AtomicBool,
    /// What the speakers play, by whom (48 kHz mono), for the echo canceller.
    far: Mutex<HashMap<String, VecDeque<i16>>>,
    counters: Arc<Counters>,
}

/// How steadily the microphone flows, for tests and the log.
#[derive(Default)]
struct Counters {
    /// Microphone callbacks, and the samples they brought (all channels).
    callbacks: AtomicU64,
    callback_samples: AtomicU64,
    /// The longest time between two microphone callbacks, in µs.
    max_callback_gap: AtomicU64,
    last_callback: AtomicU64,
    /// 10 ms frames given to the call's track.
    sent: AtomicU64,
    /// The longest time between two of them, in µs.
    max_send_gap: AtomicU64,
    last_sent: AtomicU64,
    /// 10 ms frames handed to the mic test's playback.
    looped: AtomicU64,
    /// Samples (48 kHz) the speaker took from the mic test.
    played: AtomicU64,
    /// Times the mic test's playback ran out while playing.
    underruns: AtomicU64,
    /// Samples of it thrown away for being too far behind.
    dropped: AtomicU64,
    /// Samples of it cut for never being needed (see [`FLOOR`]).
    trimmed: AtomicU64,
    /// How long the microphone's samples waited in the system before their
    /// callback (as the host reports it), summed in µs, and how many.
    input_latency: AtomicU64,
    input_latencies: AtomicU64,
    /// From the callback that completed a frame to the frame being sent
    /// (cut, cleaned, handed over), summed and the most, in µs.
    wait: AtomicU64,
    max_wait: AtomicU64,
    waits: AtomicU64,
    /// The mic test's queue when the speaker reads it, summed in samples.
    loop_queue: AtomicU64,
    loop_pulls: AtomicU64,
    /// How long the speaker's samples wait in the system before they are
    /// heard (as the host reports it), summed in µs, and how many.
    output_latency: AtomicU64,
    output_latencies: AtomicU64,
    /// Microphone samples (as 48 kHz mono) thrown away for being late (a
    /// stall of over a second).
    stale: AtomicU64,
    /// Frames that went without DeepFilterNet to catch up ([`super::mic_pace::Pace`]).
    skipped: AtomicU64,
    /// How long a frame took to clean lately (the last 5 s), the median
    /// and 99th percentile, in µs.
    processing_p50: AtomicU64,
    processing_p99: AtomicU64,
    /// The microphone callbacks' length, summed in µs (for the average),
    /// and the shortest and longest since the last log line.
    callback_micros: AtomicU64,
    shortest_callback: AtomicU64,
    longest_callback: AtomicU64,
    /// The suppressor's own delay now, in samples.
    algorithmic: AtomicU64,
}

/// Microseconds since the first call, a clock for [`Counters`].
fn micros() -> u64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_micros() as u64 + 1
}

impl Counters {
    /// Note an event at `last` and keep the longest gap between two.
    fn tick(last: &AtomicU64, max_gap: &AtomicU64) {
        let now = micros();
        let before = last.swap(now, Ordering::Relaxed);
        if before != 0 {
            max_gap.fetch_max(now.saturating_sub(before), Ordering::Relaxed);
        }
    }

    fn snapshot(&self) -> AudioStats {
        let get = |a: &AtomicU64| a.load(Ordering::Relaxed);
        AudioStats {
            callbacks: get(&self.callbacks),
            callback_samples: get(&self.callback_samples),
            max_callback_gap: Duration::from_micros(get(&self.max_callback_gap)),
            sent: get(&self.sent),
            max_send_gap: Duration::from_micros(get(&self.max_send_gap)),
            looped: get(&self.looped),
            played: get(&self.played),
            underruns: get(&self.underruns),
            dropped: get(&self.dropped),
            trimmed: get(&self.trimmed),
            input_latency: Duration::from_micros(
                get(&self.input_latency) / get(&self.input_latencies).max(1),
            ),
            wait: Duration::from_micros(get(&self.wait) / get(&self.waits).max(1)),
            max_wait: Duration::from_micros(get(&self.max_wait)),
            algorithmic: Duration::from_micros(get(&self.algorithmic) * 1000 / 48),
            loop_queue: Duration::from_micros(
                get(&self.loop_queue) * 1000 / 48 / get(&self.loop_pulls).max(1),
            ),
            output_latency: Duration::from_micros(
                get(&self.output_latency) / get(&self.output_latencies).max(1),
            ),
            stale: get(&self.stale),
            skipped: get(&self.skipped),
            processing_p50: Duration::from_micros(get(&self.processing_p50)),
            processing_p99: Duration::from_micros(get(&self.processing_p99)),
            callback: Duration::from_micros(
                get(&self.callback_micros) / get(&self.callbacks).max(1),
            ),
        }
    }

    fn reset(&self) {
        for a in [
            &self.callbacks,
            &self.callback_samples,
            &self.max_callback_gap,
            &self.last_callback,
            &self.sent,
            &self.max_send_gap,
            &self.last_sent,
            &self.looped,
            &self.played,
            &self.underruns,
            &self.dropped,
            &self.trimmed,
            &self.input_latency,
            &self.input_latencies,
            &self.wait,
            &self.max_wait,
            &self.waits,
            &self.loop_queue,
            &self.loop_pulls,
            &self.output_latency,
            &self.output_latencies,
            &self.stale,
            &self.skipped,
            &self.processing_p50,
            &self.processing_p99,
            &self.callback_micros,
            &self.shortest_callback,
            &self.longest_callback,
        ] {
            a.store(0, Ordering::Relaxed);
        }
    }
}

/// How the microphone has flowed since the capture started (or
/// [`reset_audio_stats`]): its callbacks, the 10 ms frames sent to the call
/// and the mic test's playback.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioStats {
    pub callbacks: u64,
    pub callback_samples: u64,
    pub max_callback_gap: Duration,
    pub sent: u64,
    pub max_send_gap: Duration,
    pub looped: u64,
    pub played: u64,
    pub underruns: u64,
    pub dropped: u64,
    pub trimmed: u64,
    /// The microphone's own latency (the host's figure), on average.
    pub input_latency: Duration,
    /// From a frame's last callback to it being sent, on average and the most.
    pub wait: Duration,
    pub max_wait: Duration,
    /// The noise suppressor's delay (DeepFilterNet's in Strong).
    pub algorithmic: Duration,
    /// What waits in the mic test's queue for the speaker, on average.
    pub loop_queue: Duration,
    /// The speaker's own latency (the host's figure), on average.
    pub output_latency: Duration,
    /// Microphone samples (48 kHz mono) dropped as late: only after the
    /// capture stalled for over a second.
    pub stale: u64,
    /// Frames sent without DeepFilterNet, to catch up after a stall.
    pub skipped: u64,
    /// What cleaning a 10 ms frame takes lately (the median and 99th
    /// percentile of the last 5 s).
    pub processing_p50: Duration,
    pub processing_p99: Duration,
    /// How much one microphone callback brings, on average.
    pub callback: Duration,
}

impl AudioStats {
    /// From the voice reaching the microphone to it leaving for the call.
    pub fn sent_latency(&self) -> Duration {
        self.input_latency + self.wait + self.algorithmic
    }

    /// From the voice reaching the microphone to the mic test playing it.
    pub fn loopback_latency(&self) -> Duration {
        self.sent_latency() + self.loop_queue + self.output_latency
    }
}

/// [`AudioStats`] of the running capture (all zero when none runs).
pub fn audio_stats() -> AudioStats {
    shared().map(|s| s.counters.snapshot()).unwrap_or_default()
}

pub fn reset_audio_stats() {
    if let Some(shared) = shared() {
        shared.counters.reset();
    }
}

struct Engine {
    shared: Arc<Shared>,
    thread: thread::JoinHandle<()>,
}

static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);
static NEXT_CALL: AtomicU64 = AtomicU64::new(1);

fn running(mode: AudioMode) -> Option<Arc<Shared>> {
    let mut engine = ENGINE.lock().ok()?;
    if let Some(engine) = engine.as_ref().filter(|e| !e.thread.is_finished()) {
        return Some(engine.shared.clone());
    }
    let shared = Arc::new(Shared::default());
    let theirs = shared.clone();
    let thread = thread::Builder::new()
        .name("morpheus-mic".into())
        .spawn(move || capture(&theirs, mode))
        .map_err(|err| warn!(%err, "no microphone thread"))
        .ok()?;
    *engine = Some(Engine {
        shared: shared.clone(),
        thread,
    });
    Some(shared)
}

fn shared() -> Option<Arc<Shared>> {
    ENGINE.lock().ok()?.as_ref().map(|e| e.shared.clone())
}

/// Stop capturing when neither a call nor the mic test wants it.
fn stop_if_unused() {
    let Ok(mut engine) = ENGINE.lock() else {
        return;
    };
    let unused = engine.as_ref().is_some_and(|e| {
        !e.shared.testing.load(Ordering::Relaxed)
            && e.shared.call.lock().map_or(true, |c| c.is_none())
    });
    if unused && let Some(engine) = engine.take() {
        engine.shared.stop.store(true, Ordering::Relaxed);
        let _ = engine.thread.join();
    }
}

/// The call's microphone: cleaned frames go to `source` until
/// [`detach_call`] with what this returns.
pub(crate) fn attach_call(source: NativeAudioSource, mode: AudioMode) -> Option<u64> {
    let shared = running(mode)?;
    let id = NEXT_CALL.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut call) = shared.call.lock() {
        *call = Some((id, source));
    }
    shared.refade.store(true, Ordering::Relaxed);
    Some(id)
}

pub(crate) fn detach_call(id: u64) {
    if let Some(shared) = shared()
        && let Ok(mut call) = shared.call.lock()
        && call.as_ref().is_some_and(|(have, _)| *have == id)
    {
        *call = None;
    }
    stop_if_unused();
}

/// Start the mic test: the microphone as the call would send it (after
/// noise suppression), played back to you, its level in [`mic_level`].
pub fn start_mic_test() {
    if let Some(shared) = running(AudioMode::from_env()) {
        shared.testing.store(true, Ordering::Relaxed);
    }
}

pub fn stop_mic_test() {
    if let Some(shared) = shared() {
        shared.testing.store(false, Ordering::Relaxed);
    }
    stop_if_unused();
}

pub fn mic_test_running() -> bool {
    shared().is_some_and(|s| s.testing.load(Ordering::Relaxed))
}

/// The mic test's level: the cleaned microphone's, 0 to 1 (full scale),
/// rising at once and falling slowly. 0 while no test runs.
pub fn mic_level() -> f32 {
    shared()
        .filter(|s| s.testing.load(Ordering::Relaxed))
        .map_or(0.0, |s| f32::from_bits(s.level.load(Ordering::Relaxed)))
}

/// Whether the mic test is playing the microphone back now.
pub fn mic_loopback() -> bool {
    shared().is_some_and(|s| s.testing.load(Ordering::Relaxed) && s.looping.load(Ordering::Relaxed))
}

/// The call hears the microphone again (unmuted): fade it in, no click.
pub(crate) fn fade_in() {
    if let Some(shared) = shared() {
        shared.refade.store(true, Ordering::Relaxed);
    }
}

/// The chosen devices changed: capture (and play back) on them.
pub(crate) fn reopen_devices() {
    if let Some(shared) = shared() {
        shared.reopen.store(true, Ordering::Relaxed);
    }
}

/// 10 ms or more of what `who` sounds like in the speakers (48 kHz mono).
pub(crate) fn far_end(who: &str, samples: &[i16]) {
    if let Some(shared) = shared() {
        push_far(&shared, who, samples);
    }
}

fn push_far(shared: &Shared, who: &str, samples: &[i16]) {
    let Ok(mut far) = shared.far.lock() else {
        return;
    };
    let queue = far.entry(who.to_owned()).or_default();
    queue.extend(samples);
    // A stream that stopped being read keeps 200 ms at most.
    let excess = queue.len().saturating_sub(10 * FRAME * 2);
    queue.drain(..excess);
}

pub(crate) fn far_end_gone(who: &str) {
    if let Some(shared) = shared() {
        drop_far(&shared, who);
    }
}

fn drop_far(shared: &Shared, who: &str) {
    if let Ok(mut far) = shared.far.lock() {
        far.remove(who);
    }
}

/// The next 10 ms of everything the speakers play, summed.
fn far_frame(shared: &Shared) -> [i16; FRAME] {
    let mut sum = [0i32; FRAME];
    if let Ok(mut far) = shared.far.lock() {
        for queue in far.values_mut() {
            if queue.len() < FRAME {
                continue;
            }
            for (s, v) in sum.iter_mut().zip(queue.drain(..FRAME)) {
                *s += i32::from(v);
            }
        }
    }
    sum.map(|s| s.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16)
}

/// The capture thread: 10 ms at a time until stopped.
fn capture(shared: &Shared, mode: AudioMode) {
    let counters = shared.counters.clone();
    // A clip is processed as a real microphone is (echo, gain, high-pass).
    let clip = mode == AudioMode::Tone && std::env::var_os("MORPHEUS_CALL_CLIP").is_some();
    let live = mode == AudioMode::Platform || clip;
    let mut processor = Processor::new(noise_suppression(), live);
    processor.set_tuning(noise_tuning());
    // The model first (~0.2 s, on this thread: it cannot move between
    // threads), or the microphone's first 0.2 s would queue behind it and
    // the call start that late.
    if processor.needs_model() {
        processor.load_model();
    }
    let mut input = Input::open(mode, &counters);
    let mut output: Option<Output> = None;
    let mut tone = 0f32;
    let mut level = 0f32;
    // From silence, at the start and after any break in the signal (#30).
    let mut fade = FadeIn::new();
    let mut last_frame = Instant::now();
    // Windows: this thread as MMCSS "Pro Audio", for as long as it runs.
    #[cfg(target_os = "windows")]
    let _pro_audio = super::mmcss::ProAudio::join();
    // Linux and macOS: the same, as a real-time audio thread (#332).
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    super::rtprio::raise();
    let mut pace = Pace::new();
    let mut timing = Timing::default();
    let mut report = Report::default();
    let mut last_report = Instant::now();
    let mut chosen = noise_suppression();
    // DeepFilterNet could not keep up: Standard while Strong is chosen.
    let mut too_slow = false;
    note_too_slow(None);
    info!(live, "microphone capture started");
    while !shared.stop.load(Ordering::Relaxed) {
        if shared.reopen.swap(false, Ordering::Relaxed) {
            input = Input::open(mode, &counters);
            output = None;
            fade.restart();
        }
        let wanted = noise_suppression();
        if wanted != chosen {
            // Another mode chosen: Strong (again) is tried afresh.
            chosen = wanted;
            too_slow = false;
            pace.reset();
        }
        processor.set_mode(if too_slow && wanted == NoiseSuppression::Strong {
            NoiseSuppression::Standard
        } else {
            wanted
        });
        processor.set_tuning(noise_tuning());
        if processor.needs_model() {
            // Strong chosen during the capture: ~0.2 s on this thread; the
            // microphone's backlog meanwhile is worked off after it,
            // without DeepFilterNet ([`Pace`]).
            processor.load_model();
            pace.reset();
        }
        note_applied(Some(processor.applied()));
        note_applied_tuning(Some(processor.tuning()));
        counters
            .algorithmic
            .store(processor.delay() as u64, Ordering::Relaxed);
        let stale = counters.stale.load(Ordering::Relaxed);
        let Some((mut frame, arrived)) = input.next() else {
            continue;
        };
        // A backlog dropped, or nothing for long (the device stalled): the
        // signal breaks off there, so it comes back from silence. Or a call
        // started hearing it, or it was unmuted.
        let refade = shared.refade.swap(false, Ordering::Relaxed);
        if refade
            || counters.stale.load(Ordering::Relaxed) != stale
            || last_frame.elapsed() > BREAK
        {
            fade.restart();
        }
        last_frame = Instant::now();
        // Far behind (a burst after a stall) and DeepFilterNet would take
        // long to work it off: this frame goes without it (#101).
        let neural = pace.neural(input.behind_ms());
        let started = Instant::now();
        if live {
            let mut far = far_frame(shared);
            processor.far_end(&mut far);
        }
        let ran = processor.process_paced(&mut frame, neural);
        let took = started.elapsed();
        if processor.applied() == NoiseSuppression::Strong {
            pace.note(ran, took);
            if !ran {
                counters.skipped.fetch_add(1, Ordering::Relaxed);
            }
            if let Some(why) = pace.too_slow() {
                warn!(%why, "Strong noise suppression cannot keep up here: using Standard");
                note_too_slow(Some(why));
                too_slow = true;
                pace.reset();
            }
        }
        if let Some((p50, p99)) = timing.note(took) {
            let micros = |ms: f64| (ms * 1000.0) as u64;
            counters.processing_p50.store(micros(p50), Ordering::Relaxed);
            counters.processing_p99.store(micros(p99), Ordering::Relaxed);
        }
        if last_report.elapsed() >= REPORT_EVERY {
            last_report = Instant::now();
            report.log(&counters, &mut timing, processor.applied());
        }
        if !live {
            add_tone(&mut frame, &mut tone);
            let volume = crate::input_volume();
            if (volume - 1.0).abs() > 0.001 {
                for s in frame.iter_mut() {
                    *s = (f32::from(*s) * volume).clamp(-32768.0, 32767.0) as i16;
                }
            }
        }
        fade.apply(&mut frame);
        let rms = (frame.iter().map(|s| f32::from(*s).powi(2)).sum::<f32>() / FRAME as f32).sqrt()
            / 32768.0;
        level = rms.max(level * 0.85);
        shared.level.store(level.to_bits(), Ordering::Relaxed);
        if let Ok(call) = shared.call.lock()
            && let Some((_, source)) = call.as_ref()
        {
            let sent = source
                .capture_frame(&AudioFrame {
                    data: Cow::Borrowed(&frame),
                    sample_rate: SAMPLE_RATE,
                    num_channels: 1,
                    samples_per_channel: FRAME as u32,
                })
                .now_or_never();
            match sent {
                Some(Ok(())) => {
                    counters.sent.fetch_add(1, Ordering::Relaxed);
                    let waited = arrived.elapsed().as_micros() as u64;
                    counters.wait.fetch_add(waited, Ordering::Relaxed);
                    counters.max_wait.fetch_max(waited, Ordering::Relaxed);
                    counters.waits.fetch_add(1, Ordering::Relaxed);
                    Counters::tick(&counters.last_sent, &counters.max_send_gap);
                }
                Some(Err(err)) => warn!(%err, "microphone frame refused"),
                None => warn!("microphone frame not taken at once"),
            }
        }
        if shared.testing.load(Ordering::Relaxed) {
            let out = output.get_or_insert_with(|| Output::open(mode, &counters));
            out.play(&frame);
            counters.looped.fetch_add(1, Ordering::Relaxed);
            if live {
                push_far(shared, "mic test", &frame);
            }
        } else if output.take().is_some() {
            drop_far(shared, "mic test");
        }
        shared.looping.store(
            output.as_ref().is_some_and(Output::playing),
            Ordering::Relaxed,
        );
    }
    note_applied(None);
    note_applied_tuning(None);
    shared.level.store(0, Ordering::Relaxed);
    shared.looping.store(false, Ordering::Relaxed);
    report.log(&counters, &mut timing, processor.applied());
    let stats = counters.snapshot();
    info!(
        callbacks = stats.callbacks,
        samples_per_callback = stats.callback_samples / stats.callbacks.max(1),
        callback_ms = %format!("{:.1}", ms(stats.callback)),
        max_callback_gap_ms = stats.max_callback_gap.as_millis() as u64,
        sent = stats.sent,
        max_send_gap_ms = stats.max_send_gap.as_millis() as u64,
        loopback_underruns = stats.underruns,
        dropped_as_late_ms = stats.stale / 48,
        without_dfn = stats.skipped,
        "microphone capture stopped"
    );
}

/// How often the capture's timing goes in the log.
const REPORT_EVERY: Duration = Duration::from_secs(30);

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// The capture's timing in the log, every [`REPORT_EVERY`] and when it
/// stops (#101): what cleaning a frame took (median, 99th percentile, most),
/// the shortest and longest microphone callback, and what was dropped as
/// late or went without DeepFilterNet, since the last line.
#[derive(Default)]
struct Report {
    stale: u64,
    skipped: u64,
}

impl Report {
    fn log(&mut self, counters: &Counters, timing: &mut Timing, mode: NoiseSuppression) {
        let (frames, p50, p99, most) = timing.report();
        let ms = |micros: u64| format!("{:.1}", micros as f64 / 1000.0);
        let stale = counters.stale.load(Ordering::Relaxed);
        let skipped = counters.skipped.load(Ordering::Relaxed);
        info!(
            ?mode,
            frames,
            processing_p50_ms = %format!("{p50:.2}"),
            processing_p99_ms = %format!("{p99:.2}"),
            processing_max_ms = %format!("{most:.2}"),
            shortest_callback_ms = %ms(match counters.shortest_callback.swap(0, Ordering::Relaxed) {
                0 => 0,
                kept => u64::MAX - kept,
            }),
            longest_callback_ms = %ms(counters.longest_callback.swap(0, Ordering::Relaxed)),
            max_callback_gap_ms = counters.max_callback_gap.load(Ordering::Relaxed) / 1000,
            dropped_as_late_ms = stale.saturating_sub(self.stale) / 48,
            without_dfn = skipped.saturating_sub(self.skipped),
            "microphone timing"
        );
        self.stale = stale;
        self.skipped = skipped;
    }
}

/// Sleeps now and then when `MORPHEUS_CALL_STALL` says so (#332).
fn stall_now() {
    static PLAN: std::sync::OnceLock<Option<(Duration, Duration)>> = std::sync::OnceLock::new();
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    let plan = PLAN.get_or_init(|| {
        let text = std::env::var("MORPHEUS_CALL_STALL").ok()?;
        let (period, stall) = text.split_once(':')?;
        Some((Duration::from_millis(period.parse().ok()?), Duration::from_millis(stall.parse().ok()?)))
    });
    let Some((period, stall)) = *plan else { return };
    let Ok(mut last) = LAST.lock() else { return };
    let at = last.get_or_insert_with(Instant::now);
    if at.elapsed() >= period {
        *last = Some(Instant::now());
        drop(last);
        thread::sleep(stall);
    }
}

/// The recording `MORPHEUS_CALL_CLIP` names, if any (a test run, #332).
fn clip_from_env() -> Option<Vec<i16>> {
    let bytes = std::fs::read(std::env::var_os("MORPHEUS_CALL_CLIP")?).ok()?;
    let clip: Vec<i16> = bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    (!clip.is_empty()).then_some(clip)
}

/// The test tone (440 Hz, 8000 of 32768), the "voice" of test runs.
fn add_tone(frame: &mut [i16], phase: &mut f32) {
    let step = 440.0 * std::f32::consts::TAU / SAMPLE_RATE as f32;
    for s in frame {
        *phase = (*phase + step) % std::f32::consts::TAU;
        *s = s.saturating_add((phase.sin() * 8_000.0) as i16);
    }
}

/// Where the microphone's frames come from.
enum Input {
    /// Test runs: a faint noise floor, in real time.
    /// With `MORPHEUS_CALL_CLIP` (raw 48 kHz mono s16le), that recording on a loop
    /// instead, through the same processing as a real microphone (#332).
    Room { next: Instant, seed: u32, clip: Option<(Vec<i16>, usize)> },
    /// The chosen microphone (or the system's).
    Device(Device),
    /// No microphone: nothing, a frame's time at a time.
    None,
}

impl Input {
    fn open(mode: AudioMode, counters: &Arc<Counters>) -> Self {
        if mode == AudioMode::Tone {
            return Self::Room {
                next: Instant::now(),
                seed: 0x9e37_79b9,
                clip: clip_from_env().map(|clip| (clip, 0)),
            };
        }
        match Device::open(counters.clone()) {
            Ok(device) => Self::Device(device),
            Err(err) => {
                warn!(%err, "no microphone");
                Self::None
            }
        }
    }

    /// The next 10 ms and when its last samples came, or `None` if it has
    /// not come yet (try again).
    fn next(&mut self) -> Option<(Vec<i16>, Instant)> {
        match self {
            Self::Room { next, seed, clip } => {
                // `MORPHEUS_CALL_STALL=<period ms>:<stall ms>` (test runs): the capture
                // thread is not run for a while now and then, as under a busy CPU.
                stall_now();
                let now = Instant::now();
                if *next > now {
                    thread::sleep(*next - now);
                }
                // Late by more than a few frames (the model loading): start
                // the clock again rather than rush.
                *next = if now.saturating_duration_since(*next) > Duration::from_millis(50) {
                    now
                } else {
                    *next
                } + Duration::from_millis(10);
                if let Some((clip, at)) = clip {
                    let frame = (0..FRAME).map(|k| clip[(*at + k) % clip.len()]).collect();
                    *at = (*at + FRAME) % clip.len();
                    return Some((frame, Instant::now()));
                }
                Some((
                    (0..FRAME)
                        .map(|_| {
                            *seed ^= *seed << 13;
                            *seed ^= *seed >> 17;
                            *seed ^= *seed << 5;
                            // About -60 dBFS.
                            ((*seed as i32) >> 21) as i16
                        })
                        .collect(),
                    Instant::now(),
                ))
            }
            Self::Device(device) => device.next(),
            Self::None => {
                thread::sleep(Duration::from_millis(10));
                None
            }
        }
    }

    /// How far behind the device the frame just taken is (ms): what waits
    /// beyond a callback's worth.
    fn behind_ms(&self) -> f64 {
        match self {
            Self::Device(device) => {
                device.chunker.behind() as f64 * 1000.0
                    / (f64::from(device.rate) * device.channels as f64)
            }
            _ => 0.0,
        }
    }
}

/// A microphone callback with `samples` (all channels) came, of a stream
/// bringing `per_second` samples a second (all channels).
fn counted(counters: &Counters, samples: usize, per_second: u64, info: &cpal::InputCallbackInfo) {
    let micros = samples as u64 * 1_000_000 / per_second.max(1);
    counters.callback_micros.fetch_add(micros, Ordering::Relaxed);
    counters.longest_callback.fetch_max(micros, Ordering::Relaxed);
    // Kept as u64::MAX less it, so 0 (reset) is none yet and the shortest
    // is the largest kept.
    counters
        .shortest_callback
        .fetch_max(u64::MAX - micros, Ordering::Relaxed);
    let at = info.timestamp();
    counters.input_latency.fetch_add(
        at.callback.duration_since(at.capture).as_micros() as u64,
        Ordering::Relaxed,
    );
    counters.input_latencies.fetch_add(1, Ordering::Relaxed);
    counters.callbacks.fetch_add(1, Ordering::Relaxed);
    counters
        .callback_samples
        .fetch_add(samples as u64, Ordering::Relaxed);
    Counters::tick(&counters.last_callback, &counters.max_callback_gap);
}

/// Tests measure the mic test's playback without playing the microphone
/// out loud.
static QUIET: AtomicBool = AtomicBool::new(false);

fn gain() -> f32 {
    if QUIET.load(Ordering::Relaxed) {
        0.0
    } else {
        1.0
    }
}

/// A microphone through cpal, turned into 10 ms frames of 48 kHz mono.
struct Device {
    _stream: cpal::Stream,
    samples: mpsc::Receiver<(Vec<i16>, Instant)>,
    /// When the samples waiting in `chunker` last came.
    arrived: Instant,
    counters: Arc<Counters>,
    rate: u32,
    channels: usize,
    chunker: Chunker,
    resampler: AudioResampler,
}

/// A backlog of more chunks (10 ms) than this is a stall, and dropped.
pub(super) const STALL_CHUNKS: usize = 100;

/// Cuts samples, as a device's callbacks bring them (any number at a time),
/// into chunks of one size, in order: nothing dropped, padded or repeated.
pub(super) struct Chunker {
    chunk: usize,
    pending: VecDeque<i16>,
    /// The most samples one callback has brought.
    largest: usize,
}

impl Chunker {
    pub(super) fn new(chunk: usize) -> Self {
        Self {
            chunk,
            pending: VecDeque::with_capacity(chunk * 4),
            largest: 0,
        }
    }

    pub(super) fn push(&mut self, samples: &[i16]) {
        self.largest = self.largest.max(samples.len());
        self.pending.extend(samples);
    }

    /// The next whole chunk, if one has come.
    pub(super) fn next(&mut self) -> Option<Vec<i16>> {
        (self.pending.len() >= self.chunk).then(|| self.pending.drain(..self.chunk).collect())
    }

    /// What waits beyond a callback's worth (the most one brought, up to
    /// 100 ms): how far the frames taken are behind the device. A device's
    /// callback leaves up to that much waiting without anything being late.
    pub(super) fn behind(&self) -> usize {
        self.pending
            .len()
            .saturating_sub(self.largest.min(10 * self.chunk))
    }

    /// More than [`STALL_CHUNKS`] (1 s) waiting, beyond two callbacks: the
    /// capture thread stalled far beyond anything a device delivers, so all
    /// but the newest chunk is dropped and the call does not stay that late.
    /// Anything less is worked off instead ([`super::mic_pace::Pace`] skips
    /// DeepFilterNet for frames to catch up): a burst after a 50-200 ms
    /// stall, a Windows driver's 40 ms callbacks, a slow frame now and then
    /// are all kept (#101: 80 ms was cut, and heard as gaps). How many
    /// samples went.
    pub(super) fn drop_stale(&mut self) -> usize {
        let callback = self.largest.min(10 * self.chunk);
        let max_queued = (STALL_CHUNKS * self.chunk).max(2 * callback + 2 * self.chunk);
        if self.pending.len() <= max_queued {
            return 0;
        }
        let late = self.pending.len() - self.chunk;
        self.pending.drain(..late);
        late
    }
}

/// How long the microphone takes to come up to full level (30 ms).
const FADE: usize = 30 * SAMPLE_RATE as usize / 1000;
/// No frame for this long is a break in the signal (a stalled device), not
/// a callback that came late: the microphone fades in again after it.
const BREAK: Duration = Duration::from_millis(200);

/// The microphone faded in, so the call hears no click: when the capture
/// starts, when the call is attached, and after any gap or dropped backlog.
struct FadeIn {
    /// Samples of the ramp done.
    done: usize,
}

impl FadeIn {
    fn new() -> Self {
        Self { done: 0 }
    }

    /// From silence again (the signal broke off, or a call starts hearing it).
    fn restart(&mut self) {
        self.done = 0;
    }

    /// `frame` (48 kHz mono), scaled by the ramp where it still runs: a
    /// raised cosine, so neither its start nor its end is a corner.
    fn apply(&mut self, frame: &mut [i16]) {
        if self.done >= FADE {
            return;
        }
        for s in frame.iter_mut() {
            if self.done >= FADE {
                break;
            }
            let x = self.done as f32 / FADE as f32;
            let gain = 0.5 - 0.5 * (std::f32::consts::PI * x).cos();
            *s = (f32::from(*s) * gain) as i16;
            self.done += 1;
        }
    }
}

fn host() -> cpal::Host {
    // With PulseAudio (or PipeWire's) on Linux; WASAPI, CoreAudio elsewhere.
    cpal::default_host()
}

/// `config` with callbacks of `frames` (per channel). Without a size asked
/// for, PulseAudio (PipeWire's too) chooses its defaults: the microphone
/// then came in 341 ms bursts (16384 samples) and a speaker kept 2 s
/// queued, so the call got its frames in bursts and the mic test stuttered.
fn sized(config: &cpal::SupportedStreamConfig, frames: u32) -> cpal::StreamConfig {
    let mut config = config.config();
    config.buffer_size = cpal::BufferSize::Fixed(frames);
    config
}

/// The device `id` names (as WebRTC's device module lists it: on Linux the
/// PulseAudio name, as cpal's), by id or name, else the system's default.
fn find<I: Iterator<Item = cpal::Device>>(
    devices: Option<I>,
    id: Option<&str>,
    default: Option<cpal::Device>,
) -> Option<cpal::Device> {
    use cpal::traits::DeviceTrait as _;
    if let (Some(devices), Some(id)) = (devices, id) {
        let found = devices.into_iter().find(|d| {
            d.id().is_ok_and(|have| have.id() == id)
                || d.description().is_ok_and(|have| have.name() == id)
        });
        if found.is_some() {
            return found;
        }
        warn!(id, "chosen device not found; using the default");
    }
    default
}

/// On macOS a stream opened at another rate or format than the device runs
/// at sets the device's (cpal sets the hardware format): its clock changes
/// for everyone using it, the call's own playout on a headset included,
/// and that is heard as a click or a burst of noise when a call starts
/// (#2, #30). There the device is used as it runs and resampled here.
const KEEP_DEVICE_FORMAT: bool = cfg!(target_os = "macos");

/// The stream config to open a device with ([`choose_config`]).
fn config(
    ranges: Result<Vec<cpal::SupportedStreamConfigRange>, cpal::Error>,
    default: Result<cpal::SupportedStreamConfig, cpal::Error>,
) -> Result<cpal::SupportedStreamConfig, String> {
    choose_config(ranges, default, KEEP_DEVICE_FORMAT)
}

/// The device's current format (`default`) when `keep_current` and it is
/// one we can cut into 10 ms and read; else 48 kHz if the device has it,
/// else its default.
fn choose_config(
    ranges: Result<Vec<cpal::SupportedStreamConfigRange>, cpal::Error>,
    default: Result<cpal::SupportedStreamConfig, cpal::Error>,
    keep_current: bool,
) -> Result<cpal::SupportedStreamConfig, String> {
    use cpal::SampleFormat::{F32, I16};
    let default = match default {
        Ok(current)
            if keep_current
                && current.sample_rate() % 100 == 0
                && matches!(current.sample_format(), F32 | I16) =>
        {
            return Ok(current);
        }
        other => other,
    };
    let at_48k = ranges.ok().and_then(|ranges| {
        let mut usable: Vec<_> = ranges
            .into_iter()
            .filter(|r| matches!(r.sample_format(), F32 | I16))
            .filter_map(|r| r.try_with_sample_rate(SAMPLE_RATE))
            .collect();
        usable.sort_by_key(|c| (c.channels(), c.sample_format() != F32));
        usable.into_iter().next()
    });
    match at_48k {
        Some(config) => Ok(config),
        None => default.map_err(|err| err.to_string()),
    }
}

/// The microphone's stream: each callback's samples, as i16, to `tx`.
fn input_stream(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    format: cpal::SampleFormat,
    tx: mpsc::Sender<(Vec<i16>, Instant)>,
    counters: Arc<Counters>,
) -> Result<cpal::Stream, String> {
    use cpal::traits::DeviceTrait as _;
    let failed = |err: cpal::Error| warn!(%err, "microphone stream");
    let per_second = u64::from(config.sample_rate) * u64::from(config.channels);
    match format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            config,
            move |data: &[f32], info: &cpal::InputCallbackInfo| {
                counted(&counters, data.len(), per_second, info);
                let _ = tx.send((
                    data.iter()
                        .map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
                        .collect(),
                    Instant::now(),
                ));
            },
            failed,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            config,
            move |data: &[i16], info: &cpal::InputCallbackInfo| {
                counted(&counters, data.len(), per_second, info);
                let _ = tx.send((data.to_vec(), Instant::now()));
            },
            failed,
            None,
        ),
        other => return Err(format!("samples as {other:?}")),
    }
    .map_err(|err| err.to_string())
}

impl Device {
    fn open(counters: Arc<Counters>) -> Result<Self, String> {
        let counters_kept = counters.clone();
        use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};
        let started = Instant::now();
        let host = host();
        let chosen = crate::imp::chosen_audio_devices().microphone;
        let device = find(
            host.input_devices().ok(),
            chosen.as_deref(),
            host.default_input_device(),
        )
        .ok_or("no microphone")?;
        let supported = config(
            device.supported_input_configs().map(Iterator::collect),
            device.default_input_config(),
        )?;
        let (rate, channels) = (supported.sample_rate(), usize::from(supported.channels()));
        if rate % 100 != 0 {
            return Err(format!("{rate} Hz cannot be cut into 10 ms"));
        }
        let (tx, samples) = mpsc::channel();
        let format = supported.sample_format();
        // A callback every 10 ms, as WebRTC's own capture asks; a host that
        // cannot (WASAPI shared mode may refuse) gets its default.
        let stream = match input_stream(
            &device,
            sized(&supported, rate / 100),
            format,
            tx.clone(),
            counters.clone(),
        ) {
            Ok(stream) => stream,
            Err(err) => {
                warn!(%err, "microphone: no 10 ms callbacks, using the device's own");
                input_stream(&device, supported.config(), format, tx, counters)?
            }
        };
        stream.play().map_err(|err| err.to_string())?;
        info!(
            device = %device.description().map(|d| d.name().to_owned()).unwrap_or_default(),
            rate, channels, took_ms = started.elapsed().as_millis() as u64, "microphone open"
        );
        Ok(Self {
            _stream: stream,
            samples,
            rate,
            channels,
            chunker: Chunker::new((rate / 100) as usize * channels),
            arrived: Instant::now(),
            counters: counters_kept,
            resampler: AudioResampler::default(),
        })
    }

    fn next(&mut self) -> Option<(Vec<i16>, Instant)> {
        // Whatever has come, at once.
        while let Ok((more, at)) = self.samples.try_recv() {
            self.chunker.push(&more);
            self.arrived = at;
        }
        let late = self.chunker.drop_stale();
        if late > 0 {
            // As 48 kHz mono, whatever the device runs at.
            let late = late as u64 * u64::from(SAMPLE_RATE) / u64::from(self.rate) / self.channels as u64;
            warn!(late_ms = late / 48, "microphone: the capture stalled; its backlog dropped");
            self.counters.stale.fetch_add(late, Ordering::Relaxed);
        }
        let raw = loop {
            if let Some(raw) = self.chunker.next() {
                break raw;
            }
            match self.samples.recv_timeout(Duration::from_millis(20)) {
                Ok((more, at)) => {
                    self.chunker.push(&more);
                    self.arrived = at;
                }
                Err(_) => return None,
            }
        };
        if self.rate == SAMPLE_RATE && self.channels == 1 {
            return Some((raw, self.arrived));
        }
        let frame = self.resampler.remix_and_resample(
            &raw,
            self.rate / 100,
            self.channels as u32,
            self.rate,
            1,
            SAMPLE_RATE,
        );
        (frame.len() == FRAME).then(|| (frame.to_vec(), self.arrived))
    }
}

/// Where the mic test plays the microphone back.
enum Output {
    /// The chosen speaker (or the system's): 48 kHz mono in, the device's
    /// rate and channels out.
    Device {
        _stream: Option<cpal::Stream>,
        queue: Arc<Mutex<VecDeque<f32>>>,
        counters: Arc<Counters>,
    },
    /// Test runs play nothing, but the frames are looped all the same.
    Nowhere,
    /// The speaker would not open.
    Failed,
}

impl Output {
    fn open(mode: AudioMode, counters: &Arc<Counters>) -> Self {
        if mode == AudioMode::Tone {
            return Self::Nowhere;
        }
        match Self::device(counters.clone()) {
            Ok(out) => out,
            Err(err) => {
                warn!(%err, "mic test: no speaker");
                Self::Failed
            }
        }
    }

    fn device(counters: Arc<Counters>) -> Result<Self, String> {
        use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};
        let host = host();
        let chosen = crate::imp::chosen_audio_devices().speaker;
        let device = find(
            host.output_devices().ok(),
            chosen.as_deref(),
            host.default_output_device(),
        )
        .ok_or("no speaker")?;
        let supported = config(
            device.supported_output_configs().map(Iterator::collect),
            device.default_output_config(),
        )?;
        let rate = supported.sample_rate();
        let queue: Arc<Mutex<VecDeque<f32>>> = Arc::default();
        let playback = || Playback::new(queue.clone(), rate).counting(counters.clone());
        // Asked for 10 ms at a time (left to PulseAudio, it kept 2 s queued;
        // 20 ms kept 41 ms, 10 ms keeps about 20).
        let stream = match output_stream(
            &device,
            sized(&supported, rate / 100),
            &supported,
            playback(),
        ) {
            Ok(stream) => stream,
            Err(err) => {
                warn!(%err, "mic test: no 10 ms callbacks, using the speaker's own");
                output_stream(&device, supported.config(), &supported, playback())?
            }
        };
        stream.play().map_err(|err| err.to_string())?;
        Ok(Self::Device {
            _stream: Some(stream),
            queue,
            counters,
        })
    }

    fn play(&mut self, frame: &[i16]) {
        if let Self::Device {
            queue, counters, ..
        } = self
            && let Ok(mut queue) = queue.lock()
        {
            queue.extend(frame.iter().map(|s| f32::from(*s) / 32768.0));
            // Fallen too far behind (the speaker's clock slower than the
            // microphone's, or a stall): back to a short wait.
            if queue.len() > MAX_BEHIND {
                let excess = queue.len() - (PREFILL + FRAME);
                queue.drain(..excess);
                counters.dropped.fetch_add(excess as u64, Ordering::Relaxed);
            }
        }
    }

    fn playing(&self) -> bool {
        !matches!(self, Self::Failed)
    }
}

/// The speaker's stream, filled by `playback` (mono to every channel).
fn output_stream(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    supported: &cpal::SupportedStreamConfig,
    mut playback: Playback,
) -> Result<cpal::Stream, String> {
    use cpal::traits::DeviceTrait as _;
    let channels = usize::from(supported.channels());
    let failed = |err: cpal::Error| warn!(%err, "mic test stream");
    match supported.sample_format() {
        cpal::SampleFormat::F32 => device.build_output_stream(
            config,
            move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                playback.note_latency(info);
                let mono = playback.pull(data.len() / channels);
                for (out, v) in data.chunks_exact_mut(channels).zip(mono) {
                    out.fill(*v * gain());
                }
            },
            failed,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            config,
            move |data: &mut [i16], info: &cpal::OutputCallbackInfo| {
                playback.note_latency(info);
                let mono = playback.pull(data.len() / channels);
                for (out, v) in data.chunks_exact_mut(channels).zip(mono) {
                    out.fill((v * gain() * 32767.0) as i16);
                }
            },
            failed,
            None,
        ),
        other => return Err(format!("samples as {other:?}")),
    }
    .map_err(|err| err.to_string())
}

/// What the mic test's playback waits for before it plays (and again after
/// running out), on top of one speaker callback's worth: the microphone's
/// callbacks and the speaker's do not take turns evenly.
const PREFILL: usize = 2 * FRAME;
/// The most it lets queue up (80 ms): steadily it holds [`PREFILL`] to
/// about that plus a speaker callback and a microphone frame; above, it
/// fell behind and would stay that late.
const MAX_BEHIND: usize = 8 * FRAME;
/// What the mic test's queue keeps at its lowest, against the microphone's
/// frames and the speaker's callbacks not taking turns evenly (15 ms).
const FLOOR: usize = 720;
/// Callbacks over which the lowest is taken (about half a second).
const WINDOW: usize = 50;

/// The mic test's sound, 48 kHz mono, read out at a speaker's rate.
struct Playback {
    queue: Arc<Mutex<VecDeque<f32>>>,
    /// Where in the queue the next sample is read (between samples).
    pos: f64,
    /// 48000 / the speaker's rate.
    step: f64,
    out: Vec<f32>,
    counters: Arc<Counters>,
    /// Playing (else waiting for [`PREFILL`]).
    playing: bool,
    /// The least left in the queue after a callback, over the last [`WINDOW`]
    /// callbacks: what it holds beyond [`FLOOR`] is only lateness.
    low: usize,
    pulls: usize,
}

impl Playback {
    fn new(queue: Arc<Mutex<VecDeque<f32>>>, rate: u32) -> Self {
        Self {
            queue,
            pos: 0.0,
            step: f64::from(SAMPLE_RATE) / f64::from(rate),
            out: Vec::new(),
            counters: Arc::default(),
            playing: false,
            low: usize::MAX,
            pulls: 0,
        }
    }

    fn counting(mut self, counters: Arc<Counters>) -> Self {
        self.counters = counters;
        self
    }

    fn note_latency(&self, info: &cpal::OutputCallbackInfo) {
        let at = info.timestamp();
        self.counters.output_latency.fetch_add(
            at.playback.duration_since(at.callback).as_micros() as u64,
            Ordering::Relaxed,
        );
        self.counters.output_latencies.fetch_add(1, Ordering::Relaxed);
    }

    /// The next `frames` samples (silence while waiting for enough).
    fn pull(&mut self, frames: usize) -> &[f32] {
        self.out.clear();
        self.out.resize(frames, 0.0);
        let Ok(mut queue) = self.queue.lock() else {
            return &self.out;
        };
        if self.playing {
            self.counters.loop_queue.fetch_add(queue.len() as u64, Ordering::Relaxed);
            self.counters.loop_pulls.fetch_add(1, Ordering::Relaxed);
        }
        // What this callback reads, and the sample after it.
        let needed = (self.pos + frames as f64 * self.step).ceil() as usize + 1;
        if !self.playing {
            if queue.len() < needed + PREFILL {
                return &self.out;
            }
            self.playing = true;
        }
        for out in &mut self.out {
            let at = self.pos as usize;
            let Some(&a) = queue.get(at) else {
                // Ran out: wait for enough again.
                self.counters.underruns.fetch_add(1, Ordering::Relaxed);
                self.playing = false;
                break;
            };
            let b = queue.get(at + 1).copied().unwrap_or(a);
            *out = a + (b - a) * (self.pos - at as f64) as f32;
            self.pos += self.step;
        }
        let used = (self.pos as usize).min(queue.len());
        queue.drain(..used);
        self.pos -= used as f64;
        // The queue stays at whatever it held when it started (the two
        // clocks run alike): what it never needed is cut, at most once per
        // window.
        if self.playing {
            self.low = self.low.min(queue.len());
            self.pulls += 1;
            if self.pulls >= WINDOW {
                if self.low > FLOOR + FRAME / 2 {
                    let late = self.low - FLOOR;
                    queue.drain(..late);
                    self.counters.trimmed.fetch_add(late as u64, Ordering::Relaxed);
                }
                self.low = usize::MAX;
                self.pulls = 0;
            }
        }
        self.counters
            .played
            .fetch_add(used as u64, Ordering::Relaxed);
        &self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imp::{
        denoise::DELAY,
        speech_fixture::{HOP, change_db, noisy_speech},
    };

    /// The whole of what the microphone goes through, Off then Strong.
    fn run(mode: NoiseSuppression, input: &[f32]) -> Vec<f32> {
        // The suppressors alone: the gate (input sensitivity) has its own tests.
        run_tuned(mode, input, NoiseTuning { gate_auto: false, gate_threshold_db: -100.0, ..NoiseTuning::default() })
    }

    fn run_tuned(mode: NoiseSuppression, input: &[f32], tuning: NoiseTuning) -> Vec<f32> {
        let mut processor = Processor::new(mode, true);
        processor.set_tuning(tuning);
        if processor.needs_model() {
            processor.load_model();
        }
        assert_eq!(processor.applied(), mode);
        let mut out = Vec::with_capacity(input.len());
        let mut silence = [0i16; HOP];
        for chunk in input.chunks_exact(HOP) {
            let mut frame: Vec<i16> = chunk.iter().map(|s| (s * 32767.0) as i16).collect();
            processor.far_end(&mut silence);
            processor.process(&mut frame);
            out.extend(frame.iter().map(|s| f32::from(*s) / 32768.0));
        }
        out
    }

    /// What the speakers play comes back into the microphone 60 ms later,
    /// quieter: once the echo canceller has learnt the room it is mostly
    /// gone (the mic test's playback and the call's voices are fed to it
    /// the same way).
    #[test]
    fn the_echo_of_the_speakers_is_cancelled() {
        let far = noisy_speech(8, 0.3, 0.0).voice;
        let delay = 60 * 48;
        let mut processor = Processor::new(NoiseSuppression::Off, true);
        let (mut echo_in, mut echo_out) = (0f64, 0f64);
        for (i, chunk) in far.chunks_exact(HOP).enumerate() {
            let start = i * HOP;
            let mut speaker: Vec<i16> = chunk.iter().map(|s| (s * 32767.0) as i16).collect();
            let mut mic: Vec<i16> = (start..start + HOP)
                .map(|n| {
                    n.checked_sub(delay)
                        .map_or(0, |at| (far[at] * 0.5 * 32767.0) as i16)
                })
                .collect();
            let before: f64 = mic.iter().map(|s| f64::from(*s).powi(2)).sum();
            processor.far_end(&mut speaker);
            processor.process(&mut mic);
            // After 3 s of learning.
            if start > 3 * 48_000 {
                echo_in += before;
                echo_out += mic.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>();
            }
        }
        let db = 10.0 * (echo_out / echo_in).log10();
        eprintln!("echo after the canceller: {db:.1} dB");
        assert!(db <= -15.0, "the echo only went down {db:.1} dB");
    }

    /// How late each mode's output is (the lag that best matches it to the
    /// input) is what [`Processor::delay`] says, which the latency figures
    /// count: WebRTC's echo canceller's 9 ms, 6 more for its noise
    /// suppression in Standard, DeepFilterNet's 30 more in Strong.
    #[test]
    fn each_mode_delays_the_voice_only_by_its_suppressor() {
        // Chirps (300 Hz to 6 kHz in 0.3 s, then 0.2 s of quiet): unlike a
        // voice, nothing in them repeats, so only the true lag matches.
        let input: Vec<f32> = (0..4 * 48_000)
            .map(|n| {
                let t = (n % 24_000) as f32 / 48_000.0;
                if t >= 0.3 {
                    return 0.0;
                }
                let (f0, f1, len) = (300.0f32, 6_000.0f32, 0.3f32);
                let k = (f1 / f0).ln() / len;
                let phase = std::f32::consts::TAU * f0 * ((k * t).exp() - 1.0) / k;
                0.3 * phase.sin()
            })
            .collect();
        for mode in [
            NoiseSuppression::Off,
            NoiseSuppression::Standard,
            NoiseSuppression::Strong,
        ] {
            let out = run_tuned(mode, &input, NoiseTuning::default());
            let max_lag = 100 * 48;
            let span = 48_000..input.len() - max_lag;
            let lag = (0..max_lag)
                .max_by(|&a, &b| {
                    let score = |lag: usize| -> f64 {
                        span.clone()
                            .map(|n| f64::from(input[n]) * f64::from(out[n + lag]))
                            .sum()
                    };
                    score(a).total_cmp(&score(b))
                })
                .unwrap_or(0);
            let ms = lag as f64 / 48.0;
            let mut processor = Processor::new(mode, true);
            if processor.needs_model() {
                processor.load_model();
            }
            let expected = processor.delay();
            eprintln!("{mode:?}: output {ms:.1} ms behind");
            assert!(
                lag.abs_diff(expected) <= 48,
                "{mode:?} is {ms:.1} ms behind, not {:.1} ms",
                expected as f64 / 48.0
            );
        }
    }

    /// A voice trailing off over noise (30 dB over 0.9 s, to the end of each
    /// phrase): until it is down near the noise (6 dB above it) it stays in
    /// rather than being gated to silence, and its level changes smoothly
    /// from one 10 ms frame to the next (no chopping).
    #[test]
    fn a_quiet_end_of_speech_fades_out_smoothly_instead_of_chopping() {
        use crate::imp::speech_fixture::{fading_speech, frame_energy};
        /// The voice's energy over the noise's that counts as still talking.
        const AUDIBLE: f64 = 4.0;
        /// The most a voice still talking may lose (dB).
        const FLOOR: f64 = -12.0;
        /// The most its level may change from one frame to the next (dB).
        const STEP: f64 = 8.0;
        let speech = fading_speech(10, 0.05, 0.01);
        let mix = speech.mix();
        let mut failures = Vec::new();
        for mode in [
            NoiseSuppression::Off,
            NoiseSuppression::Standard,
            NoiseSuppression::Strong,
        ] {
            let out = run(mode, &mix);
            let mut processor = Processor::new(mode, true);
            if processor.needs_model() {
                processor.load_model();
            }
            let delay = processor.delay();
            // Per vowel frame of a fade with the voice still talking: how
            // much of its energy comes out (dB).
            let kept: Vec<Option<f64>> = (0..speech.tail.len())
                .map(|f| {
                    let voice = frame_energy(&speech.voice, f, 0);
                    let noise = frame_energy(&speech.noise, f, 0);
                    let out = frame_energy(&out, f, delay).max(1e-12);
                    (f >= 100 && speech.tail[f] && voice >= AUDIBLE * noise)
                        .then(|| 10.0 * (out / voice).log10())
                })
                .collect();
            let lowest = kept.iter().flatten().copied().fold(f64::MAX, f64::min);
            let gated = kept.iter().flatten().filter(|k| **k < FLOOR).count();
            let steps: Vec<f64> = kept
                .windows(2)
                .filter_map(|w| Some((w[1]? - w[0]?).abs()))
                .collect();
            let biggest = steps.iter().copied().fold(0.0, f64::max);
            let chops = steps.iter().filter(|s| **s > STEP).count();
            eprintln!(
                "{mode:?}: {} tail frames, lowest {lowest:.1} dB ({gated} under {FLOOR}), largest step {biggest:.1} dB ({chops} over {STEP})",
                kept.iter().flatten().count()
            );
            if gated > 0 {
                failures.push(format!("{mode:?} gated the tail ({lowest:.1} dB)"));
            }
            if chops > 0 {
                failures.push(format!("{mode:?} chopped the tail ({biggest:.1} dB steps)"));
            }
        }
        assert!(failures.is_empty(), "{failures:?}");
    }

    /// What each mode lets through of keyboard clicks between words, against
    /// the voice (#332): the clicks' mean frame energy in the pauses, in dB
    /// under the voice's, and the voice kept, against Off. Printed for every
    /// mode (`--nocapture`).
    #[test]
    fn keyboard_clicks_between_words_are_mostly_taken_out() {
        use crate::imp::speech_fixture::{frame_energy, typing};
        for (name, peak) in [("loud keys", 0.15f32), ("soft keys", 0.03)] {
            let speech = noisy_speech(10, 0.05, 0.004);
            let keys = typing(10, peak, 7);
            let mix: Vec<f32> = speech
                .mix()
                .iter()
                .zip(&keys.clicks)
                .map(|(m, k)| m + k)
                .collect();
            let off = run(NoiseSuppression::Off, &mix);
            let loose = NoiseTuning { floor_margin_db: -12.0, max_fall_db: 10.0, atten_limit_db: 60.0, ..NoiseTuning::default() };
            for (label, mode, tuning) in [("", NoiseSuppression::Off, NoiseTuning::default()), ("", NoiseSuppression::Standard, NoiseTuning::default()), ("", NoiseSuppression::Strong, NoiseTuning::default()), (" (no keep-floor, 60 dB, fast fall)", NoiseSuppression::Strong, loose)] {
                let out = run_tuned(mode, &mix, tuning);
                let mut probe = Processor::new(mode, true);
                if probe.needs_model() {
                    probe.load_model();
                }
                let delay = probe.delay();
                let frames = out.len() / HOP;
                let mean = |sel: &dyn Fn(usize) -> bool, x: &[f32], d: usize| {
                    let fs: Vec<usize> = (100..frames - 10).filter(|f| sel(*f)).collect();
                    fs.iter().map(|f| frame_energy(x, *f, d)).sum::<f64>() / fs.len().max(1) as f64
                };
                let key_only = |f: usize| speech.pause[f] && keys.sounding[f];
                let vowel = |f: usize| speech.speech[f];
                let voice_out = mean(&vowel, &out, delay);
                let keys_out = mean(&key_only, &out, delay);
                let keys_in = mean(&key_only, &mix, 0);
                let voice_off = mean(&vowel, &off, 0);
                let keys_vs_voice = 10.0 * (keys_out / voice_out).log10();
                let voice_vs_off = 10.0 * (voice_out / voice_off).log10();
                // Chopping: vowel frames that lose over 10 dB against Off.
                let chopped = (100..frames - 10)
                    .filter(|f| speech.speech[*f])
                    .filter(|f| frame_energy(&out, *f, delay) < frame_energy(&off, *f, 0) * 0.1)
                    .count();
                eprintln!(
                    "CLICKS {name} {mode:?}{label}: clicks {keys_vs_voice:.1} dB under the voice (input {:.1}), clicks cut {:.1} dB, voice {voice_vs_off:.1} dB vs off, {chopped} vowel frames lost >10 dB",
                    10.0 * (keys_in / mean(&vowel, &mix, 0)).log10(),
                    10.0 * (keys_out / keys_in).log10(),
                );
            }
        }
    }

    /// Writes the two-client voice e2e's clips (`MORPHEUS_CLIP_OUT=<dir>`):
    /// `speech_keys.pcm` (voice, a quiet room and loud typing, 10 s, 48 kHz
    /// mono s16le), `voice.pcm` (the voice alone) and `frames.mask` (a byte
    /// per 10 ms frame: 1 middle of a vowel, 2 pause, 4 a key sounding).
    /// Voice frames Strong takes more than 10 dB out of, at several room
    /// noise levels (#332): Strong's choppiness is syllables dropped where
    /// the model takes the voice for noise.
    #[test]
    fn strong_keeps_the_voice_in_a_noisy_room() {
        use crate::imp::speech_fixture::frame_energy;
        for noise in [0.004f32, 0.015, 0.03, 0.06] {
            let speech = noisy_speech(10, 0.05, noise);
            let mix = speech.mix();
            let out = run(NoiseSuppression::Strong, &mix);
            let mut probe = Processor::new(NoiseSuppression::Strong, true);
            probe.load_model();
            let delay = probe.delay();
            let (mut vowels, mut lost) = (0, 0);
            for f in 100..mix.len() / HOP - 10 {
                if !speech.speech[f] {
                    continue;
                }
                vowels += 1;
                let voice = frame_energy(&speech.voice, f, 0);
                let kept = frame_energy(&out, f, delay);
                if kept < voice * 0.1 {
                    lost += 1;
                }
            }
            eprintln!("NOISY room {noise}: {lost} of {vowels} vowel frames lost >10 dB");
        }
    }

    #[test]
    #[ignore = "writes the e2e fixtures"]
    fn write_the_e2e_clips() {
        use crate::imp::speech_fixture::typing;
        let dir = std::path::PathBuf::from(std::env::var_os("MORPHEUS_CLIP_OUT").expect("MORPHEUS_CLIP_OUT"));
        let speech = noisy_speech(10, 0.05, 0.004);
        // Two seconds of typing, five times over: the clip repeats every 2 s in its
        // frame classes, so a recording can be lined up without ambiguity.
        let once = typing(2, 0.15, 7);
        let keys = crate::imp::speech_fixture::Typing {
            clicks: once.clicks.repeat(5),
            sounding: once.sounding.repeat(5),
        };
        let pcm = |x: &[f32]| -> Vec<u8> {
            x.iter().flat_map(|v| ((v * 32767.0) as i16).to_le_bytes()).collect()
        };
        let mix: Vec<f32> = speech.mix().iter().zip(&keys.clicks).map(|(m, k)| m + k).collect();
        std::fs::write(dir.join("speech_keys.pcm"), pcm(&mix)).expect("write");
        std::fs::write(dir.join("voice.pcm"), pcm(&speech.voice)).expect("write");
        let mask: Vec<u8> = (0..speech.speech.len())
            .map(|f| u8::from(speech.speech[f]) | (u8::from(speech.pause[f]) << 1) | (u8::from(keys.sounding[f]) << 2))
            .collect();
        std::fs::write(dir.join("frames.mask"), mask).expect("write");
    }

    #[test]
    fn strong_takes_out_the_noise_off_leaves_and_both_keep_the_voice() {
        let speech = noisy_speech(10, 0.05, 0.03);
        let mix = speech.mix();
        let off = run(NoiseSuppression::Off, &mix);
        let strong = run(NoiseSuppression::Strong, &mix);
        // Strong against Off: the same processing but for the suppressor,
        // whose output is DELAY later.
        let pauses = change_db(&off, &strong, &speech.pause, DELAY);
        let voice = change_db(&off, &strong, &speech.speech, DELAY);
        let off_pauses = change_db(&mix, &off, &speech.pause, 0);
        eprintln!(
            "strong vs off: pauses {pauses:.1} dB, voice {voice:.1} dB; off vs input: pauses {off_pauses:.1} dB"
        );
        assert!(
            pauses <= -10.0,
            "Strong took only {pauses:.1} dB of noise out"
        );
        assert!(voice >= -3.0, "Strong lost {voice:.1} dB of the voice");
        assert!(
            off_pauses > -6.0,
            "Off took {off_pauses:.1} dB of noise out"
        );
    }
}

#[cfg(test)]
mod engine_tests {
    use super::*;

    fn wait(what: &str, ok: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ok() {
            assert!(Instant::now() < deadline, "{what}");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn the_mic_test_meters_the_test_tone_and_plays_it_back_until_stopped() {
        crate::imp::use_test_tone();
        assert_eq!(mic_level(), 0.0);
        start_mic_test();
        assert!(mic_test_running());
        wait("the tone on the meter", || mic_level() > 0.08);
        wait("played back", mic_loopback);
        wait("processed as chosen", || {
            crate::applied_noise_suppression() == Some(crate::noise_suppression())
        });
        stop_mic_test();
        assert!(!mic_test_running());
        assert_eq!(mic_level(), 0.0);
        assert!(!mic_loopback());
        assert_eq!(crate::applied_noise_suppression(), None, "nothing captured");
    }
}

/// Measurements on this machine's real devices (`cargo test -p
/// morpheus-call --lib -- --ignored --nocapture timing`).
#[cfg(test)]
mod timing {
    use super::*;
    use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};

    fn summary(name: &str, events: &[(Instant, usize)]) {
        let sizes: Vec<usize> = events.iter().map(|e| e.1).collect();
        let gaps: Vec<f64> = events
            .windows(2)
            .map(|w| (w[1].0 - w[0].0).as_secs_f64() * 1000.0)
            .collect();
        let mut sorted = gaps.clone();
        sorted.sort_by(f64::total_cmp);
        let pct = |p: f64| {
            sorted
                .get(((sorted.len() as f64 - 1.0) * p) as usize)
                .copied()
                .unwrap_or(0.0)
        };
        let mut distinct = sizes.clone();
        distinct.sort_unstable();
        distinct.dedup();
        eprintln!(
            "{name}: {} callbacks, sizes {:?}, interval ms min {:.2} p50 {:.2} p99 {:.2} max {:.2}",
            events.len(),
            &distinct[..distinct.len().min(12)],
            pct(0.0),
            pct(0.5),
            pct(0.99),
            pct(1.0)
        );
    }

    #[test]
    #[ignore = "needs a real microphone and speaker"]
    fn timing_of_the_real_devices() {
        // MIC_TIMING_HOST=alsa, MIC_TIMING_IN=480, MIC_TIMING_OUT=960 (frames).
        let host = match std::env::var("MIC_TIMING_HOST").as_deref() {
            Ok("alsa") => cpal::host_from_id(cpal::HostId::Alsa).unwrap(),
            _ => host(),
        };
        let fixed = |var: &str| {
            std::env::var(var)
                .ok()
                .and_then(|v| v.parse().ok())
                .map_or(cpal::BufferSize::Default, cpal::BufferSize::Fixed)
        };
        let input = host.default_input_device().expect("a microphone");
        let supported = config(
            input.supported_input_configs().map(Iterator::collect),
            input.default_input_config(),
        )
        .unwrap();
        eprintln!("host {:?} input config {:?}", host.id(), supported);
        let latency_in: Arc<Mutex<Vec<Duration>>> = Arc::default();
        let latency_out: Arc<Mutex<Vec<Duration>>> = Arc::default();
        let (lat_in, lat_out) = (latency_in.clone(), latency_out.clone());
        let events = Arc::new(Mutex::new(Vec::new()));
        let theirs = events.clone();
        let mut in_config = supported.config();
        in_config.buffer_size = fixed("MIC_TIMING_IN");
        let stream = input
            .build_input_stream_raw(
                in_config,
                supported.sample_format(),
                move |data: &cpal::Data, info: &cpal::InputCallbackInfo| {
                    let t = info.timestamp();
                    lat_in.lock().unwrap().push(t.callback.duration_since(t.capture));
                    theirs.lock().unwrap().push((Instant::now(), data.len()));
                },
                |err| eprintln!("input error {err}"),
                None,
            )
            .unwrap();
        stream.play().unwrap();
        let output = host.default_output_device().expect("a speaker");
        let out_supported = config(
            output.supported_output_configs().map(Iterator::collect),
            output.default_output_config(),
        )
        .unwrap();
        eprintln!("output config {:?}", out_supported);
        let out_events = Arc::new(Mutex::new(Vec::new()));
        let theirs = out_events.clone();
        let mut out_config = out_supported.config();
        out_config.buffer_size = fixed("MIC_TIMING_OUT");
        let out = output
            .build_output_stream_raw(
                out_config,
                out_supported.sample_format(),
                move |data: &mut cpal::Data, info: &cpal::OutputCallbackInfo| {
                    let t = info.timestamp();
                    lat_out.lock().unwrap().push(t.playback.duration_since(t.callback));
                    theirs.lock().unwrap().push((Instant::now(), data.len()));
                },
                |err| eprintln!("output error {err}"),
                None,
            )
            .unwrap();
        out.play().unwrap();
        thread::sleep(Duration::from_secs(4));
        drop(stream);
        drop(out);
        summary("input", &events.lock().unwrap());
        summary("output", &out_events.lock().unwrap());
        for (name, all) in [("input", latency_in), ("output", latency_out)] {
            let all = all.lock().unwrap();
            // After the first second (the server settles).
            let settled: Vec<f64> = all.iter().skip(all.len() / 4).map(|d| d.as_secs_f64() * 1000.0).collect();
            eprintln!(
                "{name} device latency ms: avg {:.1} max {:.1}",
                settled.iter().sum::<f64>() / settled.len().max(1) as f64,
                settled.iter().copied().fold(0.0, f64::max)
            );
        }
    }

    #[test]
    #[ignore = "a measurement"]
    fn timing_of_the_processing() {
        for mode in [
            NoiseSuppression::Off,
            NoiseSuppression::Standard,
            NoiseSuppression::Strong,
        ] {
            let mut processor = Processor::new(mode, true);
            if processor.needs_model() {
                processor.load_model();
            }
            let mut seed = 1u32;
            let mut times = Vec::new();
            for _ in 0..6000 {
                let mut frame: Vec<i16> = (0..FRAME)
                    .map(|_| {
                        seed ^= seed << 13;
                        seed ^= seed >> 17;
                        seed ^= seed << 5;
                        ((seed as i32) >> 18) as i16
                    })
                    .collect();
                let mut far = [0i16; FRAME];
                let start = Instant::now();
                processor.far_end(&mut far);
                processor.process(&mut frame);
                times.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            times.sort_by(f64::total_cmp);
            eprintln!(
                "{mode:?}: per 10 ms frame avg {:.3} ms p99 {:.3} max {:.3}",
                times.iter().sum::<f64>() / times.len() as f64,
                times[times.len() * 99 / 100],
                times[times.len() - 1]
            );
        }
        // The gate alone (level, sharpness, pitch, decision).
        let mut gate = crate::gate::Gate::new();
        let mut seed = 7u32;
        let mut times = Vec::new();
        for _ in 0..6000 {
            let frame: Vec<i16> = (0..FRAME)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    ((seed as i32) >> 18) as i16
                })
                .collect();
            let start = Instant::now();
            gate.push_frame(&frame);
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(f64::total_cmp);
        eprintln!(
            "Gate alone: per 10 ms frame avg {:.3} ms p99 {:.3} max {:.3}",
            times.iter().sum::<f64>() / times.len() as f64,
            times[times.len() * 99 / 100],
            times[times.len() - 1]
        );
    }
}

/// The microphone flows without gaps: into 10 ms frames, to the call and
/// out of the mic test's playback.
#[cfg(test)]
mod flow {
    use super::*;

    /// The mic test's playback, fed 10 ms frames whenever the microphone's
    /// callbacks bring them (unevenly, as PulseAudio does), read by a
    /// speaker asking for 1024 samples every 21.3 ms: once it has started,
    /// it never runs out or skips.
    #[test]
    fn the_mic_test_playback_plays_on_without_gaps() {
        let queue: Arc<Mutex<VecDeque<f32>>> = Arc::default();
        let counters = Arc::new(Counters::default());
        let mut playback = Playback::new(queue.clone(), SAMPLE_RATE).counting(counters.clone());
        let mut output = Output::Device {
            _stream: None,
            queue: queue.clone(),
            counters: counters.clone(),
        };
        // Microphone callbacks every 5 to 15 ms (10 on average), each
        // bringing what came since the last; the sample values count up so
        // a gap or a repeat shows.
        let mut next_in = 0u64;
        let mut next_out = 0u64;
        let mut value = 0u32;
        let mut pending = 0usize;
        let mut heard: Vec<f32> = Vec::new();
        let mut jitter = 0x1234_5678u32;
        for us in 0..10_000_000u64 {
            if us == next_in {
                jitter ^= jitter << 13;
                jitter ^= jitter >> 17;
                jitter ^= jitter << 5;
                let wait = 5_000 + u64::from(jitter % 10_001);
                pending += (wait as usize) * 48 / 1000;
                next_in += wait;
                while pending >= FRAME {
                    pending -= FRAME;
                    let frame: Vec<i16> = (0..FRAME)
                        .map(|_| {
                            value += 1;
                            (value % 20_000) as i16 + 1
                        })
                        .collect();
                    output.play(&frame);
                }
            }
            if us == next_out {
                heard.extend_from_slice(playback.pull(1024));
                next_out += 21_333;
            }
        }
        let first = heard
            .iter()
            .position(|v| *v != 0.0)
            .expect("something played");
        // How far behind the microphone the speaker is at the end.
        let last = (heard[heard.len() - 1] * 32768.0).round() as i64;
        let behind = (i64::from(value % 20_000) + 1 - last).rem_euclid(20_000);
        let heard = &heard[first..heard.len() - 1024];
        let silent = heard.iter().filter(|v| **v == 0.0).count();
        let mut skips = 0;
        // After the first second and a half (the queue cut down to what it
        // needs, once: a jump).
        for w in heard[72_000..].windows(2) {
            let (a, b) = (
                (w[0] * 32768.0).round() as i32,
                (w[1] * 32768.0).round() as i32,
            );
            if b != a + 1 && !(a == 20_000 && b == 1) && b != 0 && a != 0 {
                skips += 1;
            }
        }
        let stats = counters.snapshot();
        eprintln!(
            "{stats:?}, silent after start {silent}, skips {skips}, latency start {first}, behind at the end {behind}"
        );
        assert_eq!(silent, 0, "silence after the playback started");
        assert_eq!(skips, 0, "samples skipped or repeated");
        assert_eq!(stats.underruns, 0);
        assert_eq!(stats.dropped, 0);
        assert!(
            first <= 4 * FRAME + 2048,
            "starts within ~60 ms ({first} samples)"
        );
        assert!(
            behind <= 40 * 48,
            "the speaker is {:.1} ms behind the microphone",
            behind as f64 / 48.0
        );
    }

    /// Callbacks of any size (441, 1024, 37, ...) come out as 10 ms chunks
    /// holding every sample once, in order.
    #[test]
    fn irregular_callbacks_make_whole_10_ms_chunks_in_order() {
        for channels in [1usize, 2] {
            let mut chunker = Chunker::new(FRAME * channels);
            let mut next = 0i32;
            let mut out = Vec::new();
            for (i, size) in [441usize, 1024, 37, 1, 960, 4799, 480, 16384, 3]
                .iter()
                .cycle()
                .take(200)
                .enumerate()
            {
                let samples: Vec<i16> = (0..*size)
                    .map(|_| {
                        next += 1;
                        (next % 30_000) as i16
                    })
                    .collect();
                chunker.push(&samples);
                while let Some(chunk) = chunker.next() {
                    assert_eq!(chunk.len(), FRAME * channels, "callback {i}");
                    out.extend(chunk);
                }
            }
            let total = next as usize;
            assert_eq!(out.len(), total / (FRAME * channels) * FRAME * channels);
            for (i, s) in out.iter().enumerate() {
                assert_eq!(i32::from(*s), (i as i32 + 1) % 30_000, "sample {i}");
            }
            assert_eq!(chunker.pending.len(), total - out.len(), "the rest waits");
        }
    }

    /// A device that calls back with 40 ms at a time (a Windows driver
    /// whose shared-mode period is longer than the 10 ms asked for, a
    /// Bluetooth headset) loses nothing, nor does a 200 ms burst after a
    /// stall (#101): only a backlog of over a second is dropped as late.
    #[test]
    fn large_callbacks_are_not_dropped_as_late() {
        let mut chunker = Chunker::new(FRAME);
        let (mut taken, mut dropped) = (0, 0);
        for _ in 0..100 {
            chunker.push(&[1; 4 * FRAME]);
            dropped += chunker.drop_stale();
            while chunker.next().is_some() {
                taken += 1;
            }
        }
        assert_eq!((taken, dropped), (400, 0));
        // A burst: 200 ms waiting is kept.
        for _ in 0..5 {
            chunker.push(&[1; 4 * FRAME]);
        }
        assert_eq!(chunker.drop_stale(), 0);
        // A stall: 1.2 s waiting, of which all but the newest 10 ms go.
        for _ in 0..25 {
            chunker.push(&[1; 4 * FRAME]);
        }
        assert_eq!(chunker.drop_stale(), 119 * FRAME);
        assert_eq!(chunker.pending.len(), FRAME);
        // 10 ms callbacks: more than 1 s waiting is late.
        let mut small = Chunker::new(FRAME);
        for _ in 0..STALL_CHUNKS {
            small.push(&[1; FRAME]);
        }
        assert_eq!(small.drop_stale(), 0);
        small.push(&[1; FRAME]);
        assert_eq!(small.drop_stale(), STALL_CHUNKS * FRAME);
    }

    /// CoreAudio calls back with 512 or 1024 samples (10.7 or 21.3 ms): a
    /// capture thread late by a whole callback now and then (two arrive
    /// together) drops nothing (#2's "taser" was a drop every callback).
    #[test]
    fn coreaudio_sized_callbacks_with_a_late_thread_drop_nothing() {
        for size in [512usize, 1024] {
            let mut chunker = Chunker::new(FRAME);
            let mut dropped = 0;
            for i in 0..500 {
                chunker.push(&[1; 1024][..size]);
                // Every fifth callback the thread was busy: the next one comes too.
                if i % 5 == 0 {
                    chunker.push(&[1; 1024][..size]);
                }
                dropped += chunker.drop_stale();
                // It takes one chunk per pass, two passes per callback at most.
                for _ in 0..3 {
                    let _ = chunker.next();
                }
            }
            assert_eq!(dropped, 0, "{size}-sample callbacks");
        }
    }

    /// On macOS (`keep_current`) a device running at 44.1 kHz is opened at
    /// 44.1 kHz and resampled here, not switched to 48 kHz under everyone
    /// using it; elsewhere 48 kHz is asked for. A current format that will
    /// not do (not whole 10 ms, not samples we read) still gets 48 kHz.
    #[test]
    fn macos_opens_a_device_at_the_rate_it_runs() {
        use cpal::{SampleFormat, SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange};
        let any = SupportedBufferSize::Range { min: 14, max: 4096 };
        let ranges = || {
            Ok(vec![
                SupportedStreamConfigRange::new(2, 44_100, 48_000, any.clone(), SampleFormat::F32),
                SupportedStreamConfigRange::new(1, 8_000, 96_000, any.clone(), SampleFormat::F32),
            ])
        };
        let running = |rate, format| Ok(SupportedStreamConfig::new(2, rate, any.clone(), format));

        let kept = choose_config(ranges(), running(44_100, SampleFormat::F32), true).expect("config");
        assert_eq!((kept.sample_rate(), kept.channels()), (44_100, 2), "as it runs");
        let asked = choose_config(ranges(), running(44_100, SampleFormat::F32), false).expect("config");
        assert_eq!((asked.sample_rate(), asked.channels()), (48_000, 1), "48 kHz mono");
        // 11.025 kHz is no whole 10 ms; 24-bit samples are not read here.
        for unusable in [running(11_025, SampleFormat::F32), running(44_100, SampleFormat::I24)] {
            let fallback = choose_config(ranges(), unusable, true).expect("config");
            assert_eq!(fallback.sample_rate(), 48_000);
        }
    }

    /// A full-scale tone, the microphone's frames as they come.
    fn loud(frames: usize) -> Vec<Vec<i16>> {
        (0..frames)
            .map(|f| {
                (0..FRAME)
                    .map(|i| {
                        let t = (f * FRAME + i) as f32 / SAMPLE_RATE as f32;
                        ((t * 440.0 * std::f32::consts::TAU).sin() * 30_000.0) as i16
                    })
                    .collect()
            })
            .collect()
    }

    /// How far the signal ever jumps from one sample to the next, in `out`.
    fn biggest_step(out: &[i16]) -> i32 {
        out.windows(2)
            .map(|w| (i32::from(w[1]) - i32::from(w[0])).abs())
            .max()
            .unwrap_or(0)
    }

    /// The microphone comes up from silence over 30 ms, not at once (#30:
    /// a click on joining): the first sample is silent, the level rises
    /// steadily, and from then on the frames pass untouched.
    #[test]
    fn the_microphone_fades_in_when_it_starts() {
        let mut fade = FadeIn::new();
        let input = loud(6);
        let mut out = Vec::new();
        for frame in &input {
            let mut frame = frame.clone();
            fade.apply(&mut frame);
            out.extend(frame);
        }
        let flat: Vec<i16> = input.concat();
        assert_eq!(out[0], 0, "starts from silence");
        // Within the ramp, never louder than the signal; in its first half,
        // at most half as loud.
        for (i, (o, s)) in out.iter().zip(&flat).enumerate().take(FADE) {
            let s = i32::from(*s).abs();
            let allowed = if i < FADE / 2 { s / 2 + 1 } else { s };
            assert!(i32::from(*o).abs() <= allowed, "sample {i}: {o} of {s}");
        }
        assert_eq!(&out[FADE..], &flat[FADE..], "untouched once faded in");
        // No step bigger than the tone's own steepest.
        assert!(biggest_step(&out) <= biggest_step(&flat));
    }

    /// After a gap (no samples for a while, a backlog dropped, a call
    /// attached) it fades in again rather than jumping back to full level.
    #[test]
    fn the_microphone_fades_in_again_after_a_gap() {
        let mut fade = FadeIn::new();
        for frame in loud(10) {
            let mut frame = frame;
            fade.apply(&mut frame);
        }
        fade.restart();
        let mut frame = loud(1).remove(0);
        // Mid-wave, at full level: the worst place to start again.
        frame.rotate_left(FRAME / 4 + 27);
        let before = frame.clone();
        fade.apply(&mut frame);
        assert_eq!(frame[0], 0, "starts again from silence");
        assert!(
            i32::from(frame[1]).abs() < i32::from(before[1]).abs() / 50 + 2,
            "and rises: {} of {}",
            frame[1],
            before[1]
        );
        assert!(frame.iter().zip(&before).all(|(a, b)| a.abs() <= b.abs()));
    }

    /// The real microphone (and speaker, played silently) for 10 s, as the
    /// mic test in a call runs it: a 10 ms frame reaches the call every 10
    /// ms, none lost or doubled, and the playback never runs dry.
    #[test]
    #[ignore = "needs a real microphone and speaker: cargo test -p morpheus-call --lib -- --ignored --nocapture real_microphone"]
    fn the_real_microphone_reaches_the_call_and_the_mic_test_steadily() {
        use livekit::webrtc::audio_source::AudioSourceOptions;
        QUIET.store(true, Ordering::Relaxed);
        let source = NativeAudioSource::new(AudioSourceOptions::default(), SAMPLE_RATE, 1, 0);
        let opened = Instant::now();
        let id = attach_call(source, AudioMode::Platform).expect("the capture runs");
        start_mic_test();
        let deadline = Instant::now() + Duration::from_secs(10);
        while audio_stats().sent == 0 {
            assert!(Instant::now() < deadline, "no frame from the microphone");
            thread::sleep(Duration::from_millis(1));
        }
        eprintln!("first frame {} ms after opening", opened.elapsed().as_millis());
        let mut failures = Vec::new();
        for mode in [
            NoiseSuppression::Strong,
            NoiseSuppression::Standard,
            NoiseSuppression::Off,
        ] {
            crate::set_noise_suppression(mode);
            let deadline = Instant::now() + Duration::from_secs(5);
            while crate::applied_noise_suppression() != Some(mode) {
                assert!(Instant::now() < deadline, "{mode:?} not applied");
                thread::sleep(Duration::from_millis(10));
            }
            // Settled (the playback started).
            thread::sleep(Duration::from_secs(1));
            reset_audio_stats();
            let start = Instant::now();
            thread::sleep(Duration::from_secs(10));
            let stats = audio_stats();
            let wall = start.elapsed();
            let expected = wall.as_secs_f64() * 100.0;
            let ms = |d: Duration| d.as_secs_f64() * 1000.0;
            eprintln!(
                "{mode:?}: {stats:?} over {wall:?}; callbacks of {} samples on average",
                stats.callback_samples / stats.callbacks.max(1)
            );
            eprintln!(
                "{mode:?} latency ms: mic {:.1} + wait {:.1} (max {:.1}) + suppressor {:.1} = sent {:.1}; + loop queue {:.1} + speaker {:.1} = mic test {:.1}",
                ms(stats.input_latency),
                ms(stats.wait),
                ms(stats.max_wait),
                ms(stats.algorithmic),
                ms(stats.sent_latency()),
                ms(stats.loop_queue),
                ms(stats.output_latency),
                ms(stats.loopback_latency()),
            );
            let mut check = |ok: bool, what: String| {
                if !ok {
                    failures.push(format!("{mode:?}: {what}"));
                }
            };
            check(
                stats.max_send_gap <= Duration::from_millis(20),
                format!("a gap of {:?} between frames to the call", stats.max_send_gap),
            );
            check(
                (stats.sent as f64 - expected).abs() <= expected * 0.02,
                format!("{} frames sent in {wall:?}", stats.sent),
            );
            check(
                (stats.looped as f64 - expected).abs() <= expected * 0.02,
                format!("{} frames looped in {wall:?}", stats.looped),
            );
            check(stats.underruns == 0, format!("{} underruns", stats.underruns));
            check(stats.dropped == 0, format!("{} samples skipped", stats.dropped));
            check(stats.stale == 0, format!("{} samples dropped as late", stats.stale));
            let played = stats.played as f64 / f64::from(SAMPLE_RATE);
            check(
                (played - wall.as_secs_f64()).abs() < 0.1,
                format!("{played:.2} s played in {wall:?}"),
            );
            // The processing's delay plus at most 40 ms; the mic test's
            // round trip within 60 ms more.
            check(
                stats.sent_latency() <= stats.algorithmic + Duration::from_millis(40),
                format!("sent {:.1} ms after it was said", ms(stats.sent_latency())),
            );
            check(
                stats.loopback_latency() <= stats.sent_latency() + Duration::from_millis(60),
                format!("the mic test plays {:.1} ms late", ms(stats.loopback_latency())),
            );
        }
        stop_mic_test();
        detach_call(id);
        QUIET.store(false, Ordering::Relaxed);
        assert!(failures.is_empty(), "{failures:#?}");
    }
}

#[cfg(test)]
mod tuning_tests {
    use super::*;

    fn strong() -> Processor {
        let mut processor = Processor::new(NoiseSuppression::Strong, true);
        processor.load_model();
        assert_eq!(processor.applied(), NoiseSuppression::Strong);
        processor
    }

    /// Choosing the low-delay model in a running chain swaps it in at once,
    /// and the chain's delay follows what runs (10 ms of model, not 30).
    #[test]
    fn choosing_the_low_delay_model_swaps_it_in_and_the_delay_follows() {
        let _guard = crate::noise_model::tests::DIR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(file) = crate::noise_model::tests::checkout_model() else {
            eprintln!("DeepFilterNet checkout not found; skipping");
            return;
        };
        let dir = std::env::temp_dir().join(format!("morpheus-ll-chain-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::copy(&file, dir.join(crate::noise_model::LOW_DELAY_FILE)).expect("copy");
        crate::noise_model::set_model_dir(dir.clone());
        let mut processor = strong();
        processor.set_tuning(NoiseTuning { gate_auto: false, gate_threshold_db: -100.0, ..processor.tuning() });
        let before = processor.delay();
        let tuning = NoiseTuning {
            model: crate::noise::NoiseModel::DeepFilterNet3LowDelay,
            ..processor.tuning()
        };
        processor.set_tuning(tuning);
        assert_eq!(before - processor.delay(), 960, "20 ms less");
        assert_eq!(processor.applied(), NoiseSuppression::Strong);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every number setting, moved off its default, is what the model (or the
    /// smoothing) is set to afterwards, and a setting out of range reaches the
    /// chain clamped.
    #[test]
    fn every_neural_setting_reaches_the_model() {
        let mut processor = strong();
        let defaults = processor.denoiser.as_ref().expect("model").model_settings();
        assert_eq!(defaults, (Some(10f32.powf(-40.0 / 20.0)), false, 0.0, -10.0, 30.0, 20.0));

        let set = |processor: &mut Processor, key: &str, value: f32| {
            processor.set_tuning(NoiseTuning::default().with_param(key, value));
            processor.denoiser.as_ref().expect("model").model_settings()
        };
        let (atten, ..) = set(&mut processor, "atten_limit_db", 20.0);
        assert_eq!(atten, Some(10f32.powf(-20.0 / 20.0)));
        // Out of range: the safe end, not the wild value.
        let (atten, ..) = set(&mut processor, "atten_limit_db", 1000.0);
        assert_eq!(atten, Some(10f32.powf(-60.0 / 20.0)));
        let (.., min, _, _) = set(&mut processor, "min_snr_db", -20.0);
        assert_eq!(min, -20.0);
        let (.., min, _, _) = set(&mut processor, "min_snr_db", -500.0);
        assert_eq!(min, -30.0);
        let (.., erb, _) = set(&mut processor, "max_erb_snr_db", 40.0);
        assert_eq!(erb, 40.0);
        let (.., df) = set(&mut processor, "max_df_snr_db", 10.0);
        assert_eq!(df, 10.0);

        // The post-filter: off is a zero beta, on takes the beta.
        processor.set_tuning(NoiseTuning::default().with_switch("post_filter", true).with_param("post_filter_beta", 0.05));
        let (_, on, beta, ..) = processor.denoiser.as_ref().expect("model").model_settings();
        assert!(on);
        assert!((beta - 0.05).abs() < 1e-6, "{beta}");
        processor.set_tuning(NoiseTuning::default().with_switch("post_filter", false).with_param("post_filter_beta", 0.05));
        let (_, on, beta, ..) = processor.denoiser.as_ref().expect("model").model_settings();
        assert!(!on);
        assert_eq!(beta, 0.0);

        // Our smoothing.
        processor.set_tuning(
            NoiseTuning::default()
                .with_param("floor_margin_db", -6.0)
                .with_param("max_fall_db", 5.0),
        );
        let (keep, fall) = processor.denoiser.as_ref().expect("model").smoothing();
        assert!((keep - 10f32.powf(-6.0 / 20.0)).abs() < 1e-6);
        assert_eq!(fall, 5.0);
        processor.set_tuning(NoiseTuning::default().with_param("max_fall_db", 1e6));
        assert_eq!(processor.denoiser.as_ref().expect("model").smoothing().1, 10.0);
    }

    /// A tuning set before the model loads is the one it loads with.
    #[test]
    fn a_tuning_set_before_the_model_loads_is_what_it_loads_with() {
        let mut processor = Processor::new(NoiseSuppression::Strong, true);
        processor.set_tuning(NoiseTuning::default().with_param("atten_limit_db", 15.0));
        processor.load_model();
        let (atten, ..) = processor.denoiser.as_ref().expect("model").model_settings();
        assert_eq!(atten, Some(10f32.powf(-15.0 / 20.0)));
    }

    /// WebRTC's switches (echo cancellation, gain, high-pass) are what its
    /// processing is built with, and a real microphone only has the first two.
    #[test]
    fn webrtc_switches_reach_its_processing() {
        let t = NoiseTuning::default();
        let flags = |mode, live, t: &NoiseTuning| Processor::apm_flags(mode, live, t);
        assert_eq!(flags(NoiseSuppression::Off, true, &t), (true, true, true, false));
        assert_eq!(flags(NoiseSuppression::Standard, true, &t), (true, true, true, true));
        let off = t
            .with_switch("echo_cancellation", false)
            .with_switch("auto_gain", false)
            .with_switch("high_pass", false);
        assert_eq!(flags(NoiseSuppression::Standard, true, &off), (false, false, false, true));
        assert_eq!(flags(NoiseSuppression::Off, false, &t), (false, false, true, false));
        // The processor rebuilds its processing for them, and keeps its tuning.
        let mut processor = Processor::new(NoiseSuppression::Off, true);
        processor.set_tuning(off);
        assert_eq!(processor.tuning(), off);
    }

    /// The latency the settings page shows is what the processor delays by.
    #[test]
    fn the_latency_shown_is_the_delay_of_the_processing() {
        for mode in NoiseSuppression::ALL {
            for echo in [true, false] {
                let tuning = NoiseTuning::default().with_switch("echo_cancellation", echo);
                let mut processor = Processor::new(mode, true);
                processor.set_tuning(tuning);
                if processor.needs_model() {
                    processor.load_model();
                }
                assert_eq!(processor.delay(), tuning.delay_samples(mode, true));
            }
        }
    }
}
