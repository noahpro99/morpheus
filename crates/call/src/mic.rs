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
    let live = mode == AudioMode::Platform;
    let mut processor = Processor::new(noise_suppression(), live);
    let mut input = Input::open(mode);
    let mut output: Option<Output> = None;
    let mut tone = 0f32;
    let mut level = 0f32;
    info!(live, "microphone capture started");
    while !shared.stop.load(Ordering::Relaxed) {
        if shared.reopen.swap(false, Ordering::Relaxed) {
            input = Input::open(mode);
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
            if let Some(Err(err)) = sent {
                warn!(%err, "microphone frame refused");
            }
        }
        if shared.testing.load(Ordering::Relaxed) {
            let out = output.get_or_insert_with(|| Output::open(mode));
            out.play(&frame);
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
    info!("microphone capture stopped");
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
    fn open(mode: AudioMode) -> Self {
        if mode == AudioMode::Tone {
            return Self::Room {
                next: Instant::now(),
                seed: 0x9e37_79b9,
            };
        }
        match Device::open() {
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

/// A microphone through cpal, turned into 10 ms frames of 48 kHz mono.
struct Device {
    _stream: cpal::Stream,
    samples: mpsc::Receiver<Vec<i16>>,
    rate: u32,
    channels: usize,
    pending: Vec<i16>,
    resampler: AudioResampler,
}

fn host() -> cpal::Host {
    // With PulseAudio (or PipeWire's) on Linux; WASAPI, CoreAudio elsewhere.
    cpal::default_host()
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

impl Device {
    fn open() -> Result<Self, String> {
        use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};
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
        let failed = |err: cpal::Error| warn!(%err, "microphone stream");
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                supported.config(),
                move |data: &[f32], _: &_| {
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
                supported.config(),
                move |data: &[i16], _: &_| {
                    let _ = tx.send(data.to_vec());
                },
                failed,
                None,
            ),
            other => return Err(format!("samples as {other:?}")),
        }
        .map_err(|err| err.to_string())?;
        stream.play().map_err(|err| err.to_string())?;
        info!(
            device = %device.description().map(|d| d.name().to_owned()).unwrap_or_default(),
            rate, channels, "microphone open"
        );
        Ok(Self {
            _stream: stream,
            samples,
            rate,
            channels,
            pending: Vec::new(),
            resampler: AudioResampler::default(),
        })
    }

    fn next(&mut self) -> Option<Vec<i16>> {
        let chunk = (self.rate / 100) as usize * self.channels;
        while self.pending.len() < chunk {
            match self.samples.recv_timeout(Duration::from_millis(20)) {
                Ok(more) => self.pending.extend(more),
                Err(_) => return None,
            }
        }
        let raw: Vec<i16> = self.pending.drain(..chunk).collect();
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
        _stream: cpal::Stream,
        queue: Arc<Mutex<VecDeque<f32>>>,
    },
    /// Test runs play nothing, but the frames are looped all the same.
    Nowhere,
    /// The speaker would not open.
    Failed,
}

impl Output {
    fn open(mode: AudioMode) -> Self {
        if mode == AudioMode::Tone {
            return Self::Nowhere;
        }
        match Self::device() {
            Ok(out) => out,
            Err(err) => {
                warn!(%err, "mic test: no speaker");
                Self::Failed
            }
        }
    }

    fn device() -> Result<Self, String> {
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
        let (rate, channels) = (supported.sample_rate(), usize::from(supported.channels()));
        let queue: Arc<Mutex<VecDeque<f32>>> = Arc::default();
        let failed = |err: cpal::Error| warn!(%err, "mic test stream");
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => {
                let mut playback = Playback::new(queue.clone(), rate);
                device.build_output_stream(
                    supported.config(),
                    move |data: &mut [f32], _: &_| {
                        let mono = playback.pull(data.len() / channels);
                        for (out, v) in data.chunks_exact_mut(channels).zip(mono) {
                            out.fill(*v);
                        }
                    },
                    failed,
                    None,
                )
            }
            cpal::SampleFormat::I16 => {
                let mut playback = Playback::new(queue.clone(), rate);
                device.build_output_stream(
                    supported.config(),
                    move |data: &mut [i16], _: &_| {
                        let mono = playback.pull(data.len() / channels);
                        for (out, v) in data.chunks_exact_mut(channels).zip(mono) {
                            out.fill((v * 32767.0) as i16);
                        }
                    },
                    failed,
                    None,
                )
            }
            other => return Err(format!("samples as {other:?}")),
        }
        .map_err(|err| err.to_string())?;
        stream.play().map_err(|err| err.to_string())?;
        Ok(Self::Device {
            _stream: stream,
            queue,
        })
    }

    fn play(&mut self, frame: &[i16]) {
        if let Self::Device { queue, .. } = self
            && let Ok(mut queue) = queue.lock()
        {
            queue.extend(frame.iter().map(|s| f32::from(*s) / 32768.0));
            // No more than 100 ms behind.
            let excess = queue.len().saturating_sub(10 * FRAME);
            queue.drain(..excess);
        }
    }

    fn playing(&self) -> bool {
        !matches!(self, Self::Failed)
    }
}

/// The mic test's sound, 48 kHz mono, read out at a speaker's rate.
struct Playback {
    queue: Arc<Mutex<VecDeque<f32>>>,
    /// Where in the queue the next sample is read (between samples).
    pos: f64,
    /// 48000 / the speaker's rate.
    step: f64,
    out: Vec<f32>,
}

impl Playback {
    fn new(queue: Arc<Mutex<VecDeque<f32>>>, rate: u32) -> Self {
        Self {
            queue,
            pos: 0.0,
            step: f64::from(SAMPLE_RATE) / f64::from(rate),
            out: Vec::new(),
        }
    }

    /// The next `frames` samples (silence where none have come).
    fn pull(&mut self, frames: usize) -> &[f32] {
        self.out.clear();
        self.out.resize(frames, 0.0);
        if let Ok(mut queue) = self.queue.lock() {
            for out in &mut self.out {
                let at = self.pos as usize;
                let Some(&a) = queue.get(at) else {
                    break;
                };
                let b = queue.get(at + 1).copied().unwrap_or(a);
                *out = a + (b - a) * (self.pos - at as f64) as f32;
                self.pos += self.step;
            }
            let used = (self.pos as usize).min(queue.len());
            queue.drain(..used);
            self.pos -= used as f64;
        }
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
