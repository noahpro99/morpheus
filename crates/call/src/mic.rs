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
    NoiseSuppression,
    imp::{
        AudioMode, SAMPLE_RATE,
        denoise::{Denoiser, FRAME},
    },
    noise::note_applied,
    noise_suppression,
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
    scratch: Vec<f32>,
}

impl Processor {
    pub fn new(mode: NoiseSuppression, live: bool) -> Self {
        Self {
            mode,
            live,
            apm: Self::apm(mode, live),
            denoiser: None,
            scratch: vec![0.0; FRAME],
        }
    }

    fn apm(mode: NoiseSuppression, live: bool) -> AudioProcessingModule {
        AudioProcessingModule::new(live, live, true, mode == NoiseSuppression::Standard)
    }

    /// Process as `mode` from the next frame. WebRTC's processing starts
    /// again (its echo canceller relearns the room in about a second).
    pub fn set_mode(&mut self, mode: NoiseSuppression) {
        if mode == self.mode {
            return;
        }
        let standard = NoiseSuppression::Standard;
        if (mode == standard) != (self.mode == standard) {
            self.apm = Self::apm(mode, self.live);
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

    /// Strong is chosen and its model not loaded yet (it takes ~0.2 s).
    pub fn needs_model(&self) -> bool {
        self.mode == NoiseSuppression::Strong && self.denoiser.is_none()
    }

    pub fn load_model(&mut self) {
        match Denoiser::new() {
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
    pub fn process(&mut self, frame: &mut [i16]) {
        if frame.len() != FRAME {
            return;
        }
        let _ = self.apm.process_stream(frame, SAMPLE_RATE as i32, 1);
        if self.mode != NoiseSuppression::Strong {
            return;
        }
        let Some(denoiser) = self.denoiser.as_mut() else {
            return;
        };
        for (f, s) in self.scratch.iter_mut().zip(frame.iter()) {
            *f = f32::from(*s) / 32768.0;
        }
        denoiser.process(&mut self.scratch);
        for (s, f) in frame.iter_mut().zip(&self.scratch) {
            *s = (f * 32767.0) as i16;
        }
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
    let live = mode == AudioMode::Platform;
    let mut processor = Processor::new(noise_suppression(), live);
    let mut input = Input::open(mode, &counters);
    let mut output: Option<Output> = None;
    let mut tone = 0f32;
    let mut level = 0f32;
    info!(live, "microphone capture started");
    while !shared.stop.load(Ordering::Relaxed) {
        if shared.reopen.swap(false, Ordering::Relaxed) {
            input = Input::open(mode, &counters);
            output = None;
        }
        processor.set_mode(noise_suppression());
        if processor.needs_model() {
            // ~0.2 s on this thread (the model cannot move between
            // threads); what the microphone said meanwhile waits for it.
            processor.load_model();
        }
        note_applied(Some(processor.applied()));
        let Some(mut frame) = input.next() else {
            continue;
        };
        if live {
            let mut far = far_frame(shared);
            processor.far_end(&mut far);
        }
        processor.process(&mut frame);
        if !live {
            add_tone(&mut frame, &mut tone);
        }
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
    shared.level.store(0, Ordering::Relaxed);
    shared.looping.store(false, Ordering::Relaxed);
    let stats = counters.snapshot();
    info!(
        callbacks = stats.callbacks,
        samples_per_callback = stats.callback_samples / stats.callbacks.max(1),
        max_callback_gap_ms = stats.max_callback_gap.as_millis() as u64,
        sent = stats.sent,
        max_send_gap_ms = stats.max_send_gap.as_millis() as u64,
        loopback_underruns = stats.underruns,
        "microphone capture stopped"
    );
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
    Room { next: Instant, seed: u32 },
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

    /// The next 10 ms, or `None` if it has not come yet (try again).
    fn next(&mut self) -> Option<Vec<i16>> {
        match self {
            Self::Room { next, seed } => {
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
                Some(
                    (0..FRAME)
                        .map(|_| {
                            *seed ^= *seed << 13;
                            *seed ^= *seed >> 17;
                            *seed ^= *seed << 5;
                            // About -60 dBFS.
                            ((*seed as i32) >> 21) as i16
                        })
                        .collect(),
                )
            }
            Self::Device(device) => device.next(),
            Self::None => {
                thread::sleep(Duration::from_millis(10));
                None
            }
        }
    }
}

/// A microphone callback with `samples` (all channels) came.
fn counted(counters: &Counters, samples: usize) {
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
    samples: mpsc::Receiver<Vec<i16>>,
    rate: u32,
    channels: usize,
    chunker: Chunker,
    resampler: AudioResampler,
}

/// Cuts samples, as a device's callbacks bring them (any number at a time),
/// into chunks of one size, in order: nothing dropped, padded or repeated.
struct Chunker {
    chunk: usize,
    pending: VecDeque<i16>,
}

impl Chunker {
    fn new(chunk: usize) -> Self {
        Self {
            chunk,
            pending: VecDeque::with_capacity(chunk * 4),
        }
    }

    fn push(&mut self, samples: &[i16]) {
        self.pending.extend(samples);
    }

    /// The next whole chunk, if one has come.
    fn next(&mut self) -> Option<Vec<i16>> {
        (self.pending.len() >= self.chunk).then(|| self.pending.drain(..self.chunk).collect())
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

/// A stream config at 48 kHz if the device has one, else its default.
fn config(
    ranges: Result<Vec<cpal::SupportedStreamConfigRange>, cpal::Error>,
    default: Result<cpal::SupportedStreamConfig, cpal::Error>,
) -> Result<cpal::SupportedStreamConfig, String> {
    use cpal::SampleFormat::{F32, I16};
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
    tx: mpsc::Sender<Vec<i16>>,
    counters: Arc<Counters>,
) -> Result<cpal::Stream, String> {
    use cpal::traits::DeviceTrait as _;
    let failed = |err: cpal::Error| warn!(%err, "microphone stream");
    match format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            config,
            move |data: &[f32], _: &_| {
                counted(&counters, data.len());
                let _ = tx.send(
                    data.iter()
                        .map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
                        .collect(),
                );
            },
            failed,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            config,
            move |data: &[i16], _: &_| {
                counted(&counters, data.len());
                let _ = tx.send(data.to_vec());
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
            resampler: AudioResampler::default(),
        })
    }

    fn next(&mut self) -> Option<Vec<i16>> {
        let raw = loop {
            if let Some(raw) = self.chunker.next() {
                break raw;
            }
            match self.samples.recv_timeout(Duration::from_millis(20)) {
                Ok(more) => self.chunker.push(&more),
                Err(_) => return None,
            }
        };
        if self.rate == SAMPLE_RATE && self.channels == 1 {
            return Some(raw);
        }
        let frame = self.resampler.remix_and_resample(
            &raw,
            self.rate / 100,
            self.channels as u32,
            self.rate,
            1,
            SAMPLE_RATE,
        );
        (frame.len() == FRAME).then(|| frame.to_vec())
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
        // Asked for 20 ms at a time (left to PulseAudio, it kept 2 s queued).
        let stream = match output_stream(
            &device,
            sized(&supported, rate / 50),
            &supported,
            playback(),
        ) {
            Ok(stream) => stream,
            Err(err) => {
                warn!(%err, "mic test: no 20 ms callbacks, using the speaker's own");
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
            move |data: &mut [f32], _: &_| {
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
            move |data: &mut [i16], _: &_| {
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
const PREFILL: usize = 3 * FRAME;
/// The most it lets queue up (200 ms).
const MAX_BEHIND: usize = 20 * FRAME;

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
        }
    }

    fn counting(mut self, counters: Arc<Counters>) -> Self {
        self.counters = counters;
        self
    }

    /// The next `frames` samples (silence while waiting for enough).
    fn pull(&mut self, frames: usize) -> &[f32] {
        self.out.clear();
        self.out.resize(frames, 0.0);
        let Ok(mut queue) = self.queue.lock() else {
            return &self.out;
        };
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
        let mut processor = Processor::new(mode, true);
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
        let events = Arc::new(Mutex::new(Vec::new()));
        let theirs = events.clone();
        let mut in_config = supported.config();
        in_config.buffer_size = fixed("MIC_TIMING_IN");
        let stream = input
            .build_input_stream_raw(
                in_config,
                supported.sample_format(),
                move |data: &cpal::Data, _: &_| {
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
                move |data: &mut cpal::Data, _: &_| {
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
            for _ in 0..500 {
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
        let heard = &heard[first..heard.len() - 1024];
        let silent = heard.iter().filter(|v| **v == 0.0).count();
        let mut skips = 0;
        for w in heard.windows(2) {
            let (a, b) = (
                (w[0] * 32768.0).round() as i32,
                (w[1] * 32768.0).round() as i32,
            );
            if b != a + 1 && !(a == 20_000 && b == 1) && b != 0 && a != 0 {
                skips += 1;
            }
        }
        let stats = counters.snapshot();
        eprintln!("{stats:?}, silent after start {silent}, skips {skips}, latency start {first}");
        assert_eq!(silent, 0, "silence after the playback started");
        assert_eq!(skips, 0, "samples skipped or repeated");
        assert_eq!(stats.underruns, 0);
        assert_eq!(stats.dropped, 0);
        assert!(
            first <= 4 * FRAME + 2048,
            "starts within ~60 ms ({first} samples)"
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
        eprintln!(
            "first frame {} ms after opening",
            opened.elapsed().as_millis()
        );
        // Settled (the model loaded, the playback started).
        thread::sleep(Duration::from_secs(1));
        reset_audio_stats();
        let start = Instant::now();
        thread::sleep(Duration::from_secs(10));
        let stats = audio_stats();
        let wall = start.elapsed();
        stop_mic_test();
        detach_call(id);
        QUIET.store(false, Ordering::Relaxed);
        let expected = wall.as_secs_f64() * 100.0;
        eprintln!(
            "{stats:?} over {wall:?}; callbacks of {} samples on average",
            stats.callback_samples / stats.callbacks.max(1)
        );
        assert!(
            stats.max_send_gap <= Duration::from_millis(20),
            "a gap of {:?} between frames to the call",
            stats.max_send_gap
        );
        assert!(
            (stats.sent as f64 - expected).abs() <= expected * 0.02,
            "{} frames sent in {wall:?}",
            stats.sent
        );
        assert!(
            (stats.looped as f64 - expected).abs() <= expected * 0.02,
            "{} frames looped in {wall:?}",
            stats.looped
        );
        assert_eq!(stats.underruns, 0, "the mic test's playback ran dry");
        assert_eq!(stats.dropped, 0, "the mic test's playback skipped");
        let played = stats.played as f64 / f64::from(SAMPLE_RATE);
        assert!(
            (played - wall.as_secs_f64()).abs() < 0.1,
            "{played:.2} s played in {wall:?}"
        );
    }
}
