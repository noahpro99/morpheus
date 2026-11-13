//! Playback: one thread per open video decodes a little ahead (a few
//! pictures, half a second of sound), plays the sound through rodio and
//! shows each picture when the sound's clock reaches it. Without sound (a
//! silent file, or no output device) the clock is the wall clock.

use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use crate::{Decoder, Frame, Unsupported};

/// Pictures decoded ahead of the one shown.
const VIDEO_AHEAD: usize = 3;
/// Sound decoded ahead of what is playing.
const AUDIO_AHEAD: f64 = 0.5;

#[derive(Clone, Debug)]
pub struct Options {
    /// Sound goes to the default output device (false: decoded, not heard).
    pub audible: bool,
    pub looped: bool,
    pub muted: bool,
    /// 0 to 1.
    pub volume: f32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            audible: true,
            looped: false,
            muted: false,
            volume: 1.0,
        }
    }
}

/// Where playback is, for the controls.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub position: Duration,
    pub duration: Duration,
    pub playing: bool,
    pub ended: bool,
    /// Times a loop went round.
    pub loops: u32,
    /// Sound samples decoded so far (all channels).
    pub audio_samples: u64,
    /// Whether sound reaches an output device now (not muted, playing).
    pub audible: bool,
    pub has_audio: bool,
    pub muted: bool,
    pub volume: f32,
    pub looped: bool,
}

enum Command {
    Play,
    Pause,
    Seek(Duration),
    Muted(bool),
    Volume(f32),
    Loop(bool),
}

/// The sound being played, shared between the decoding thread (fills it)
/// and the output device's callback (empties it, counting what it played).
struct Sound {
    queue: Mutex<VecDeque<f32>>,
    /// Interleaved samples played since `base` was set.
    played: AtomicU64,
    /// When `played` last moved, for a smooth clock between device callbacks.
    marked: Mutex<Instant>,
    running: AtomicBool,
    volume_bits: AtomicU32,
    stop: AtomicBool,
    rate: u32,
    channels: u16,
}

impl Sound {
    fn gain(&self) -> f32 {
        f32::from_bits(self.volume_bits.load(Ordering::Relaxed))
    }
}

/// What rodio pulls the sound through.
struct SoundSource(Arc<Sound>);

impl Iterator for SoundSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let sound = &self.0;
        if sound.stop.load(Ordering::Relaxed) {
            return None;
        }
        if !sound.running.load(Ordering::Relaxed) {
            return Some(0.0);
        }
        let sample = sound.queue.lock().ok().and_then(|mut q| q.pop_front());
        match sample {
            Some(sample) => {
                let played = sound.played.fetch_add(1, Ordering::Relaxed) + 1;
                if played.is_multiple_of(256) && let Ok(mut marked) = sound.marked.lock() {
                    *marked = Instant::now();
                }
                Some(sample * sound.gain())
            }
            // Ran dry: silence, the clock waits.
            None => Some(0.0),
        }
    }
}

impl rodio::Source for SoundSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        std::num::NonZero::new(self.0.channels).unwrap_or(std::num::NonZero::<u16>::MIN)
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        std::num::NonZero::new(self.0.rate).unwrap_or(std::num::NonZero::<u32>::MIN)
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// The playback clock: media time `base` at `since` (while running).
struct WallClock {
    base: Duration,
    since: Option<Instant>,
}

impl WallClock {
    fn now(&self) -> Duration {
        self.base + self.since.map_or(Duration::ZERO, |s| s.elapsed())
    }
    fn set(&mut self, at: Duration, running: bool) {
        self.base = at;
        self.since = running.then(Instant::now);
    }
}

struct Shared {
    status: Mutex<Status>,
    frame: Mutex<Option<Arc<Frame>>>,
}

/// One open video. Dropping it stops its thread.
pub struct Player {
    commands: mpsc::Sender<Command>,
    shared: Arc<Shared>,
}

impl Player {
    /// Opens `bytes` (MP4: H.264 with or without AAC) paused at the start;
    /// the first picture shows at once.
    pub fn open(bytes: Arc<[u8]>, options: Options) -> Result<Self, Unsupported> {
        let decoder = Decoder::open(bytes)?;
        let status = Status {
            duration: decoder.info().duration,
            has_audio: decoder.has_audio(),
            muted: options.muted,
            volume: options.volume,
            looped: options.looped,
            ..Status::default()
        };
        let shared = Arc::new(Shared {
            status: Mutex::new(status),
            frame: Mutex::new(None),
        });
        let (commands, receiver) = mpsc::channel();
        let thread_shared = shared.clone();
        std::thread::Builder::new()
            .name("morpheus-video".into())
            .spawn(move || Playback::new(decoder, options, thread_shared, receiver).run())
            .map_err(|err| Unsupported::Broken(err.to_string()))?;
        Ok(Self { commands, shared })
    }

    pub fn play(&self) {
        let _ = self.commands.send(Command::Play);
    }
    pub fn pause(&self) {
        let _ = self.commands.send(Command::Pause);
    }
    pub fn seek(&self, to: Duration) {
        let _ = self.commands.send(Command::Seek(to));
    }
    pub fn set_muted(&self, muted: bool) {
        let _ = self.commands.send(Command::Muted(muted));
    }
    pub fn set_volume(&self, volume: f32) {
        let _ = self.commands.send(Command::Volume(volume.clamp(0.0, 1.0)));
    }
    pub fn set_looped(&self, looped: bool) {
        let _ = self.commands.send(Command::Loop(looped));
    }

    pub fn status(&self) -> Status {
        self.shared.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// The picture showing now (its `index` counts pictures shown).
    pub fn frame(&self) -> Option<Arc<Frame>> {
        self.shared.frame.lock().ok().and_then(|f| f.clone())
    }
}

/// The decoding thread's state.
struct Playback {
    decoder: Decoder,
    options: Options,
    shared: Arc<Shared>,
    commands: mpsc::Receiver<Command>,
    playing: bool,
    ended: bool,
    loops: u32,
    clock: WallClock,
    /// Sound to the device (None: silent file, not audible, or no device).
    sound: Option<Arc<Sound>>,
    /// Media time of the first sample in the sound queue's history
    /// (`played` counts from here).
    sound_base: Duration,
    _sink: Option<rodio::MixerDeviceSink>,
    video: VecDeque<Frame>,
    video_done: bool,
    /// Decoded sound waiting (silent mode: dropped as the clock passes it).
    audio_ahead: Duration,
    audio_done: bool,
    audio_samples: u64,
    shown: u64,
    /// Show the next picture even while paused (after opening or seeking).
    show_next: bool,
    duration: Duration,
}

impl Playback {
    fn new(decoder: Decoder, options: Options, shared: Arc<Shared>, commands: mpsc::Receiver<Command>) -> Self {
        let duration = decoder.info().duration;
        let has_audio = decoder.has_audio();
        let mut this = Self {
            decoder,
            shared,
            commands,
            playing: false,
            ended: false,
            loops: 0,
            clock: WallClock {
                base: Duration::ZERO,
                since: None,
            },
            sound: None,
            sound_base: Duration::ZERO,
            _sink: None,
            video: VecDeque::new(),
            video_done: false,
            audio_ahead: Duration::ZERO,
            audio_done: !has_audio,
            audio_samples: 0,
            shown: 0,
            show_next: true,
            duration,
            options,
        };
        if has_audio && this.options.audible {
            this.open_device();
        }
        this
    }

    /// Opens the default output for the file's sound, if it can.
    fn open_device(&mut self) {
        let Some((rate, channels)) = self.decoder.audio_format() else { return };
        let Ok(mut sink) = rodio::DeviceSinkBuilder::open_default_sink() else {
            return;
        };
        sink.log_on_drop(false);
        let sound = Arc::new(Sound {
            queue: Mutex::new(VecDeque::new()),
            played: AtomicU64::new(0),
            marked: Mutex::new(Instant::now()),
            running: AtomicBool::new(false),
            volume_bits: AtomicU32::new(self.gain().to_bits()),
            stop: AtomicBool::new(false),
            rate,
            channels,
        });
        sink.mixer().add(SoundSource(sound.clone()));
        self.sound = Some(sound);
        self._sink = Some(sink);
    }

    fn gain(&self) -> f32 {
        if self.options.muted { 0.0 } else { self.options.volume }
    }

    /// Media time now: the sound's clock when sound is playing, else the wall's.
    fn now(&self) -> Duration {
        match &self.sound {
            Some(sound) => {
                let per_second = f64::from(sound.rate) * f64::from(sound.channels.max(1));
                let played = sound.played.load(Ordering::Relaxed) as f64 / per_second;
                let since = if self.playing {
                    sound
                        .marked
                        .lock()
                        .map(|m| m.elapsed().min(Duration::from_millis(40)))
                        .unwrap_or_default()
                } else {
                    Duration::ZERO
                };
                // Once the sound has run out (it ends before the pictures),
                // the wall clock carries on from where it stopped.
                let audio = self.sound_base + Duration::from_secs_f64(played) + since;
                if self.audio_done && sound.queue.lock().map(|q| q.is_empty()).unwrap_or(true) {
                    audio.max(self.clock.now())
                } else {
                    audio
                }
            }
            None => self.clock.now(),
        }
    }

    fn run(mut self) {
        loop {
            // Commands; block while there is nothing to do.
            let idle = !self.playing && !self.show_next;
            let command = if idle {
                match self.commands.recv() {
                    Ok(command) => Some(command),
                    Err(_) => break,
                }
            } else {
                match self.commands.try_recv() {
                    Ok(command) => Some(command),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => break,
                }
            };
            if let Some(command) = command {
                self.apply(command);
                continue;
            }
            self.fill();
            self.present();
            self.publish();
            if self.playing {
                let wait = self
                    .video
                    .front()
                    .map(|f| f.pts.saturating_sub(self.now()))
                    .unwrap_or(Duration::from_millis(5))
                    .clamp(Duration::from_millis(1), Duration::from_millis(8));
                match self.commands.recv_timeout(wait) {
                    Ok(command) => self.apply(command),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        }
        if let Some(sound) = &self.sound {
            sound.stop.store(true, Ordering::Relaxed);
        }
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::Play => {
                if self.ended {
                    self.ended = false;
                    self.seek_to(Duration::ZERO);
                }
                if !self.playing {
                    self.playing = true;
                    let now = self.now();
                    self.clock.set(now, true);
                    if let Some(sound) = &self.sound {
                        sound.running.store(true, Ordering::Relaxed);
                        if let Ok(mut marked) = sound.marked.lock() {
                            *marked = Instant::now();
                        }
                    }
                }
            }
            Command::Pause => {
                if self.playing {
                    let now = self.now();
                    self.playing = false;
                    self.clock.set(now, false);
                    if let Some(sound) = &self.sound {
                        sound.running.store(false, Ordering::Relaxed);
                    }
                }
            }
            Command::Seek(to) => {
                self.ended = false;
                self.seek_to(to.min(self.duration));
            }
            Command::Muted(muted) => {
                self.options.muted = muted;
                self.set_gain();
            }
            Command::Volume(volume) => {
                self.options.volume = volume;
                self.set_gain();
            }
            Command::Loop(looped) => self.options.looped = looped,
        }
        self.publish();
    }

    fn set_gain(&self) {
        if let Some(sound) = &self.sound {
            sound.volume_bits.store(self.gain().to_bits(), Ordering::Relaxed);
        }
    }

    fn seek_to(&mut self, to: Duration) {
        self.decoder.seek(to);
        self.video.clear();
        self.video_done = false;
        self.audio_done = !self.decoder.has_audio();
        self.audio_ahead = to;
        self.clock.set(to, self.playing);
        self.sound_base = to;
        if let Some(sound) = &self.sound {
            if let Ok(mut queue) = sound.queue.lock() {
                queue.clear();
            }
            sound.played.store(0, Ordering::Relaxed);
            if let Ok(mut marked) = sound.marked.lock() {
                *marked = Instant::now();
            }
        }
        self.show_next = true;
    }

    /// Decodes ahead: sound to half a second past the clock, a few pictures.
    fn fill(&mut self) {
        let now = self.now();
        while !self.audio_done && self.audio_ahead < now + Duration::from_secs_f64(AUDIO_AHEAD) {
            match self.decoder.next_audio() {
                Some(chunk) => {
                    self.audio_samples += chunk.samples.len() as u64;
                    let per_second = f64::from(chunk.rate) * f64::from(chunk.channels.max(1));
                    self.audio_ahead = chunk.pts + Duration::from_secs_f64(chunk.samples.len() as f64 / per_second);
                    if let Some(sound) = &self.sound
                        && let Ok(mut queue) = sound.queue.lock()
                    {
                        queue.extend(chunk.samples);
                    }
                }
                None => self.audio_done = true,
            }
            if !self.playing {
                break;
            }
        }
        while !self.video_done && self.video.len() < VIDEO_AHEAD {
            match self.decoder.next_video() {
                Some(frame) => self.video.push_back(frame),
                None => self.video_done = true,
            }
        }
    }

    /// Shows the latest picture that is due; at the end, loops or stops.
    fn present(&mut self) {
        if self.show_next {
            if let Some(frame) = self.video.pop_front() {
                self.show(frame);
                self.show_next = false;
            } else if self.video_done {
                self.show_next = false;
            }
            return;
        }
        if !self.playing {
            return;
        }
        let now = self.now();
        let mut due = None;
        while self.video.front().is_some_and(|f| f.pts <= now) {
            due = self.video.pop_front();
            // Late: skip to the newest due picture, but keep decoding ahead.
            if self.video.is_empty() && !self.video_done {
                self.fill();
            }
        }
        if let Some(frame) = due {
            self.show(frame);
        }
        if self.video_done && self.video.is_empty() && now >= self.duration {
            if self.options.looped {
                self.loops += 1;
                self.seek_to(Duration::ZERO);
                self.show_next = false;
            } else {
                self.playing = false;
                self.ended = true;
                self.clock.set(self.duration, false);
                if let Some(sound) = &self.sound {
                    sound.running.store(false, Ordering::Relaxed);
                }
            }
        }
    }

    fn show(&mut self, mut frame: Frame) {
        self.shown += 1;
        frame.index = self.shown;
        if let Ok(mut slot) = self.shared.frame.lock() {
            *slot = Some(Arc::new(frame));
        }
    }

    fn publish(&self) {
        let position = if self.ended { self.duration } else { self.now().min(self.duration) };
        if let Ok(mut status) = self.shared.status.lock() {
            *status = Status {
                position,
                duration: self.duration,
                playing: self.playing,
                ended: self.ended,
                loops: self.loops,
                audio_samples: self.audio_samples,
                audible: self.sound.is_some() && self.playing && !self.options.muted && self.options.volume > 0.0,
                has_audio: self.decoder.has_audio(),
                muted: self.options.muted,
                volume: self.options.volume,
                looped: self.options.looped,
            };
        }
    }
}
