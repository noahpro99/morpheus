// Call media for Morpheus: a LiveKit room (the MatrixRTC transport; the
// core gets the URL and token) with this device's microphone in it and
// everyone else's audio coming out of the speakers.
//
// Real calls use the platform's audio devices through WebRTC's own device
// module, which also cancels echo and suppresses noise. Tests use
// [`AudioMode::Tone`]: a steady tone instead of the microphone, nothing
// played, and [`CallEvent::Heard`]/[`CallEvent::Quiet`] reporting what
// arrives from each participant.
//
// Like the core, the crate owns its Tokio runtime, so callers can await its
// futures from any executor.

use std::{
    borrow::Cow,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use futures_util::StreamExt;
use livekit::{
    e2ee::{
        E2eeOptions, EncryptionType,
        key_provider::{KeyProvider, KeyProviderOptions},
    },
    options::TrackPublishOptions,
    prelude::*,
    webrtc::{
        audio_frame::AudioFrame,
        audio_source::{AudioSourceOptions, native::NativeAudioSource},
        audio_stream::native::NativeAudioStream,
        video_source::{RtcVideoSource, VideoResolution, native::NativeVideoSource},
    },
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tracing::{debug, info, warn};

#[path = "devices.rs"]
mod devices;
pub use devices::{
    AudioDevice, AudioDevices, CameraDevice, DeviceChoice, audio_devices, cameras,
    choose_audio_devices, choose_camera, chosen_audio_devices, chosen_camera,
};
#[cfg(target_os = "linux")]
#[path = "camera.rs"]
mod camera;
#[path = "platform_camera.rs"]
mod platform_camera;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
#[path = "screen.rs"]
mod screen;
pub use platform_camera::{PlatformCamera, camera_frame, set_platform_camera};
#[path = "video.rs"]
mod video;
pub use video::{
    CAMERA_PATTERN, Frame, LOCAL_CAMERA, LOCAL_SCREEN, SCREEN_PATTERN, TestPattern, latest_frame,
    skip_jpeg_frames,
};

/// Where the call's audio comes from and goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioMode {
    /// The microphone and speakers (with echo cancellation).
    Platform,
    /// A 440 Hz tone instead of the microphone, and nothing played; remote
    /// audio is measured instead ([`CallEvent::Heard`]). For tests.
    Tone,
}

impl AudioMode {
    /// [`AudioMode::Tone`] in test runs (`MORPHEUS_CALL_AUDIO=tone`, or
    /// [`use_test_tone`] where there is no environment to set, Android),
    /// else the platform's devices.
    pub fn from_env() -> Self {
        if TEST_TONE.load(std::sync::atomic::Ordering::Relaxed) {
            return Self::Tone;
        }
        match std::env::var("MORPHEUS_CALL_AUDIO").as_deref() {
            Ok("tone") => Self::Tone,
            _ => Self::Platform,
        }
    }
}

static TEST_TONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether calls here can send video: a webcam on Linux, the app's own
/// camera (phones, [`set_platform_camera`]), or the test pattern.
pub fn can_send_video() -> bool {
    cfg!(target_os = "linux")
        || platform_camera::available()
        || AudioMode::from_env() == AudioMode::Tone
}

/// Whether calls here can share the screen: desktops, or the test pattern.
pub fn can_share_screen() -> bool {
    cfg!(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "windows"
    )) || AudioMode::from_env() == AudioMode::Tone
}

/// Calls send a test tone and play nothing from now on (test builds).
pub fn use_test_tone() {
    TEST_TONE.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Android: WebRTC needs the Java VM and an app context before any call.
#[cfg(target_os = "android")]
pub use livekit::webrtc::android::{initialize_android, initialize_android_context};

/// Something that happened in the call. Participants are LiveKit identities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallEvent {
    Joined(String),
    Left(String),
    /// Who is speaking now (this device included), loudest first.
    Speaking(Vec<String>),
    /// Audio from this participant started arriving ([`AudioMode::Tone`] only).
    Heard(String),
    /// Audio from this participant stopped ([`AudioMode::Tone`] only).
    Quiet(String),
    /// The connection is gone (kicked, server down, network lost).
    Disconnected(String),
    /// Video from this participant started (`on`) or stopped arriving, shown
    /// under `key` ([`latest_frame`]).
    Video {
        identity: String,
        key: String,
        /// A shared screen, not a camera.
        screen: bool,
        on: bool,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not connect to the call: {0}")]
    Connect(String),
    #[error("could not use the microphone: {0}")]
    Audio(String),
    #[error("could not use the camera: {0}")]
    Video(String),
    #[error("call runtime task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
}

pub type Result<T> = std::result::Result<T, Error>;

const SAMPLE_RATE: u32 = 48_000;
/// 10 ms, the frame size WebRTC works in.
const FRAME: usize = (SAMPLE_RATE / 100) as usize;

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        // The app links rustls with two crypto backends (ring here, aws-lc-rs
        // with the Matrix SDK), so rustls cannot pick one for LiveKit's TLS
        // by itself (it panics). Ring, unless something chose already.
        let _ = rustls::crypto::ring::default_provider().install_default();
        #[allow(clippy::expect_used)]
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("morpheus-call")
            .enable_all()
            .build()
            .expect("call runtime")
    })
}

/// A connected call. Dropping it without [`Call::leave`] also disconnects,
/// less politely.
pub struct Call {
    room: Arc<Room>,
    /// Media keys, for encrypted calls.
    keys: Option<KeyProvider>,
    mic: LocalAudioTrack,
    deafened: Arc<Mutex<bool>>,
    /// Into the merge of who is speaking.
    speech: UnboundedSender<Speech>,
    /// This device's identity in the room.
    me: String,
    /// The platform devices, open for the call's lifetime.
    platform: Option<PlatformAudio>,
    mode: AudioMode,
    /// The camera, while it is on.
    camera: Arc<tokio::sync::Mutex<Option<Outgoing>>>,
    /// The shared screen, while it is.
    screen: Arc<tokio::sync::Mutex<Option<Outgoing>>>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Call {
    /// Connect to the LiveKit room at `url` with `token` and start sending
    /// this device's audio.
    pub async fn connect(
        url: &str,
        token: &str,
        mode: AudioMode,
    ) -> Result<(Self, UnboundedReceiver<CallEvent>)> {
        let (url, token) = (url.to_owned(), token.to_owned());
        runtime()
            .spawn(async move { Self::connect_here(url, token, mode, None).await })
            .await?
    }

    /// Like [`Call::connect`], with media encrypted end to end: this device
    /// encrypts with `own_key`, and hears others once given theirs
    /// ([`Call::set_key`]). The keys travel over Matrix, encrypted.
    pub async fn connect_encrypted(
        url: &str,
        token: &str,
        mode: AudioMode,
        own_key: Vec<u8>,
    ) -> Result<(Self, UnboundedReceiver<CallEvent>)> {
        let (url, token) = (url.to_owned(), token.to_owned());
        runtime()
            .spawn(async move { Self::connect_here(url, token, mode, Some(own_key)).await })
            .await?
    }

    async fn connect_here(
        url: String,
        token: String,
        mode: AudioMode,
        own_key: Option<Vec<u8>>,
    ) -> Result<(Self, UnboundedReceiver<CallEvent>)> {
        // Element Call's settings: per-participant keys (no shared key), the
        // LiveKit defaults for derivation and ratcheting, AES-GCM.
        let keys = own_key
            .as_ref()
            .map(|_| KeyProvider::new(KeyProviderOptions::default()));
        let mut options = RoomOptions::default();
        options.encryption = keys.clone().map(|key_provider| E2eeOptions {
            encryption_type: EncryptionType::Gcm,
            key_provider,
        });
        let (room, room_events) = Room::connect(&url, &token, options)
            .await
            .map_err(|err| Error::Connect(err.to_string()))?;
        let room = Arc::new(room);
        if let (Some(keys), Some(own_key)) = (&keys, own_key) {
            keys.set_key(&room.local_participant().identity(), 0, own_key);
        }
        let (tx, rx) = unbounded_channel();
        let mut tasks = Vec::new();

        let (source, platform) = match mode {
            AudioMode::Platform => {
                let platform = PlatformAudio::new().map_err(|err| Error::Audio(err.to_string()))?;
                if let Err(err) = devices::apply(&platform, false) {
                    warn!(%err, "chosen device unavailable, using the default");
                }
                (platform.rtc_source(), Some(platform))
            }
            AudioMode::Tone => {
                let source =
                    NativeAudioSource::new(AudioSourceOptions::default(), SAMPLE_RATE, 1, 100);
                tasks.push(tokio::spawn(play_tone(source.clone())));
                (RtcAudioSource::Native(source), None)
            }
        };
        let mic = LocalAudioTrack::create_audio_track("microphone", source);
        // Encrypted calls: silent until the encryptor sits on the track, or the
        // first frames would leave in the clear.
        if keys.is_some() {
            mic.mute();
        }
        room.local_participant()
            .publish_track(
                LocalTrack::Audio(mic.clone()),
                TrackPublishOptions {
                    source: TrackSource::Microphone,
                    dtx: true,
                    ..Default::default()
                },
            )
            .await
            .map_err(|err| Error::Connect(err.to_string()))?;
        if keys.is_some() {
            let me = room.local_participant().identity();
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !room
                .e2ee_manager()
                .frame_cryptors()
                .keys()
                .any(|(who, _)| *who == me)
            {
                if std::time::Instant::now() > deadline {
                    let _ = room.close().await;
                    return Err(Error::Connect("media encryption did not start".into()));
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            mic.unmute();
        }

        let deafened = Arc::new(Mutex::new(false));
        let (speech, speech_rx) = unbounded_channel();
        let me = room.local_participant().identity().to_string();
        tasks.push(tokio::spawn(aggregate(speech_rx, tx.clone())));
        tasks.push(tokio::spawn(watch_mic(mic.clone(), me.clone(), speech.clone())));
        tasks.push(tokio::spawn(forward_events(
            room_events,
            tx,
            speech.clone(),
            mode,
            deafened.clone(),
        )));
        Ok((
            Self {
                room,
                keys,
                mic,
                deafened,
                speech,
                me,
                platform,
                mode,
                camera: Arc::default(),
                screen: Arc::default(),
                tasks,
            },
            rx,
        ))
    }

    /// Everyone else in the call (LiveKit identities).
    pub fn participants(&self) -> Vec<String> {
        self.room
            .remote_participants()
            .into_keys()
            .map(|id| id.to_string())
            .collect()
    }

    /// This device's LiveKit identity.
    pub fn identity(&self) -> String {
        self.room.local_participant().identity().to_string()
    }

    /// Hear `identity` in an encrypted call: their media key, as they sent it.
    pub fn set_key(&self, identity: &str, key: Vec<u8>) {
        info!(
            who = identity,
            encrypted = self.keys.is_some(),
            "call key received"
        );
        if let Some(keys) = &self.keys {
            keys.set_key(&ParticipantIdentity(identity.to_owned()), 0, key);
        }
    }

    /// Switch to the devices last chosen ([`choose_audio_devices`]).
    pub fn use_chosen_devices(&self) -> Result<()> {
        let _enter = runtime().enter();
        match &self.platform {
            Some(platform) => devices::apply(platform, true),
            None => Ok(()),
        }
    }

    /// Stop (or resume) sending this device's audio.
    pub fn set_muted(&self, muted: bool) {
        // LiveKit starts tasks here: from an interface thread outside the
        // call's runtime, they need it entered.
        let _enter = runtime().enter();
        if muted {
            self.mic.mute();
            let _ = self.speech.send(Speech::Muted(self.me.clone(), true));
        } else {
            self.mic.unmute();
            let _ = self.speech.send(Speech::Muted(self.me.clone(), false));
        }
    }

    /// Stop (or resume) playing everyone else's audio.
    pub fn set_deafened(&self, deafened: bool) {
        let _enter = runtime().enter();
        if let Ok(mut flag) = self.deafened.lock() {
            *flag = deafened;
        }
        for participant in self.room.remote_participants().values() {
            for publication in participant.track_publications().values() {
                if let Some(RemoteTrack::Audio(track)) = publication.track() {
                    if deafened {
                        track.disable();
                    } else {
                        track.enable();
                    }
                }
            }
        }
    }

    /// Whether the camera is on.
    pub fn camera_on(&self) -> bool {
        self.sending(VideoKind::Camera)
    }

    /// Whether this device's screen is being shared.
    pub fn sharing_screen(&self) -> bool {
        self.sending(VideoKind::Screen)
    }

    fn sending(&self, kind: VideoKind) -> bool {
        self.outgoing(kind)
            .try_lock()
            .map_or(true, |video| video.is_some())
    }

    fn outgoing(&self, kind: VideoKind) -> &Arc<tokio::sync::Mutex<Option<Outgoing>>> {
        match kind {
            VideoKind::Camera => &self.camera,
            VideoKind::Screen => &self.screen,
        }
    }

    /// Turn the camera on (or off). What it sends shows under
    /// [`LOCAL_CAMERA`] too.
    pub fn set_camera(&self, on: bool) -> impl Future<Output = Result<()>> + 'static {
        self.set_video(VideoKind::Camera, on)
    }

    /// Share this device's screen (or stop). Where the desktop asks which
    /// screen or window (Wayland), this waits for the answer; cancelling is
    /// an error. What it sends shows under [`LOCAL_SCREEN`] too.
    pub fn set_screen_share(&self, on: bool) -> impl Future<Output = Result<()>> + 'static {
        self.set_video(VideoKind::Screen, on)
    }

    fn set_video(&self, kind: VideoKind, on: bool) -> impl Future<Output = Result<()>> + 'static {
        let (room, video, mode) = (self.room.clone(), self.outgoing(kind).clone(), self.mode);
        let encrypted = self.keys.is_some();
        async move {
            runtime()
                .spawn(async move {
                    let mut video = video.lock().await;
                    match (on, video.take()) {
                        (true, Some(running)) => *video = Some(running),
                        (true, None) => {
                            *video = Some(Outgoing::start(&room, kind, mode, encrypted).await?)
                        }
                        (false, Some(running)) => running.stop(&room).await,
                        (false, None) => {}
                    }
                    Ok(())
                })
                .await?
        }
    }

    /// Hang up.
    pub async fn leave(self) {
        let room = self.room.clone();
        let _ = runtime()
            .spawn(async move {
                if let Err(err) = room.close().await {
                    debug!(%err, "closing the call");
                }
            })
            .await;
        // `self` drops here: the tasks stop and the devices close.
    }
}

impl Drop for Call {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        for video in [&self.camera, &self.screen] {
            if let Ok(mut video) = video.try_lock()
                && let Some(video) = video.take()
            {
                video.halt();
            }
        }
        video::forget(LOCAL_CAMERA);
        video::forget(LOCAL_SCREEN);
    }
}

/// Video this device sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VideoKind {
    Camera,
    Screen,
}

/// The camera or the screen, published.
struct Outgoing {
    track: LocalVideoTrack,
    kind: VideoKind,
    stop: Arc<AtomicBool>,
}

/// Where outgoing frames come from.
enum Feed {
    TestPattern(video::TestPattern),
    /// The app's own camera ([`set_platform_camera`]).
    Platform,
    #[cfg(target_os = "linux")]
    Camera(camera::Camera),
    /// Already capturing; frames go out once `sending` is set.
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    Screen(Arc<AtomicBool>),
}

/// How long the desktop's screen picker may take.
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
const PICKER_TIMEOUT: Duration = Duration::from_secs(180);

impl Outgoing {
    async fn start(room: &Room, kind: VideoKind, mode: AudioMode, encrypted: bool) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let screen = kind == VideoKind::Screen;
        let (feed, source) = match (mode, kind) {
            (AudioMode::Tone, _) => {
                let pattern = if screen {
                    video::SCREEN_PATTERN
                } else {
                    video::CAMERA_PATTERN
                };
                let resolution = VideoResolution {
                    width: pattern.width,
                    height: pattern.height,
                };
                (
                    Feed::TestPattern(pattern),
                    NativeVideoSource::new(resolution, screen),
                )
            }
            (AudioMode::Platform, VideoKind::Camera) if platform_camera::available() => {
                let source = NativeVideoSource::new(
                    VideoResolution {
                        width: 640,
                        height: 480,
                    },
                    false,
                );
                (Feed::Platform, source)
            }
            #[cfg(target_os = "linux")]
            (AudioMode::Platform, VideoKind::Camera) => {
                let device = tokio::task::spawn_blocking(camera::Camera::open)
                    .await?
                    .map_err(|err| Error::Video(err.to_string()))?;
                let source = NativeVideoSource::new(device.resolution(), false);
                (Feed::Camera(device), source)
            }
            #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
            (AudioMode::Platform, VideoKind::Screen) => {
                let source = NativeVideoSource::new(
                    VideoResolution {
                        width: 1920,
                        height: 1080,
                    },
                    true,
                );
                let sending = Arc::new(AtomicBool::new(false));
                let (started, first) = tokio::sync::oneshot::channel();
                screen::run(source.clone(), stop.clone(), sending.clone(), started);
                let first = tokio::time::timeout(PICKER_TIMEOUT, first).await;
                match first {
                    Ok(Ok(Ok(()))) => {}
                    Ok(Ok(Err(why))) => return Err(Error::Video(why)),
                    _ => {
                        stop.store(true, Ordering::Relaxed);
                        return Err(Error::Video("no picture from the screen".into()));
                    }
                }
                (Feed::Screen(sending), source)
            }
            #[allow(unreachable_patterns)]
            (AudioMode::Platform, _) => {
                return Err(Error::Video("not available on this device yet".into()));
            }
        };
        let (name, track_source) = if screen {
            ("screen", TrackSource::Screenshare)
        } else {
            ("camera", TrackSource::Camera)
        };
        let track =
            LocalVideoTrack::create_video_track(name, RtcVideoSource::Native(source.clone()));
        let published = room
            .local_participant()
            .publish_track(
                LocalTrack::Video(track.clone()),
                TrackPublishOptions {
                    source: track_source,
                    ..Default::default()
                },
            )
            .await;
        let publication = match published {
            Ok(publication) => publication,
            Err(err) => {
                stop.store(true, Ordering::Relaxed);
                return Err(Error::Video(err.to_string()));
            }
        };
        // Encrypted calls: no frames until the encryptor sits on the track,
        // or the first would leave in the clear.
        if encrypted {
            let key = (room.local_participant().identity(), publication.sid());
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !room.e2ee_manager().frame_cryptors().contains_key(&key) {
                if std::time::Instant::now() > deadline {
                    stop.store(true, Ordering::Relaxed);
                    let _ = room.local_participant().unpublish_track(&key.1).await;
                    return Err(Error::Video("video encryption did not start".into()));
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        match feed {
            Feed::TestPattern(pattern) => {
                tokio::spawn(video::play_test_pattern(pattern, source, stop.clone()));
            }
            Feed::Platform => {
                let (source, running) = (source.clone(), stop.clone());
                let started =
                    tokio::task::spawn_blocking(move || platform_camera::run(source, running))
                        .await
                        .unwrap_or(false);
                if !started {
                    stop.store(true, Ordering::Relaxed);
                    let _ = room
                        .local_participant()
                        .unpublish_track(&publication.sid())
                        .await;
                    return Err(Error::Video("the camera did not start".into()));
                }
            }
            #[cfg(target_os = "linux")]
            Feed::Camera(device) => device.run(source, stop.clone()),
            #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
            Feed::Screen(sending) => sending.store(true, Ordering::Relaxed),
        }
        Ok(Self { track, kind, stop })
    }

    async fn stop(self, room: &Room) {
        self.halt();
        if let Err(err) = room
            .local_participant()
            .unpublish_track(&self.track.sid())
            .await
        {
            debug!(%err, "unpublishing video");
        }
        video::forget(match self.kind {
            VideoKind::Camera => LOCAL_CAMERA,
            VideoKind::Screen => LOCAL_SCREEN,
        });
    }

    /// Stop sending frames.
    fn halt(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// A 440 Hz tone at a speaking level, in real time.
async fn play_tone(source: NativeAudioSource) {
    let mut phase = 0f32;
    let step = 440.0 * std::f32::consts::TAU / SAMPLE_RATE as f32;
    let mut tick = tokio::time::interval(Duration::from_millis(10));
    loop {
        tick.tick().await;
        let samples: Vec<i16> = (0..FRAME)
            .map(|_| {
                phase = (phase + step) % std::f32::consts::TAU;
                (phase.sin() * 8_000.0) as i16
            })
            .collect();
        let frame = AudioFrame {
            data: Cow::Owned(samples),
            sample_rate: SAMPLE_RATE,
            num_channels: 1,
            samples_per_channel: FRAME as u32,
        };
        if let Err(err) = source.capture_frame(&frame).await {
            warn!(%err, "tone frame refused");
        }
    }
}

async fn forward_events(
    mut events: UnboundedReceiver<RoomEvent>,
    tx: UnboundedSender<CallEvent>,
    speech: UnboundedSender<Speech>,
    mode: AudioMode,
    deafened: Arc<Mutex<bool>>,
) {
    while let Some(event) = events.recv().await {
        // What happens in a call goes to the log: when someone cannot be
        // heard or seen, it says whether they arrived, sent anything, and
        // whether it decrypted.
        let out = match event {
            RoomEvent::ParticipantConnected(p) => {
                info!(who = %p.identity(), "joined the call");
                CallEvent::Joined(p.identity().to_string())
            }
            RoomEvent::ParticipantDisconnected(p) => {
                info!(who = %p.identity(), "left the call");
                let _ = speech.send(Speech::Gone(p.identity().to_string()));
                CallEvent::Left(p.identity().to_string())
            }
            RoomEvent::E2eeStateChanged { participant, state } => {
                info!(who = %participant.identity(), ?state, "call encryption");
                continue;
            }
            RoomEvent::ActiveSpeakersChanged { speakers } => {
                let _ = speech.send(Speech::Server(
                    speakers.iter().map(|p| p.identity().to_string()).collect(),
                ));
                continue;
            }
            RoomEvent::TrackMuted { participant: Participant::Remote(p), publication }
                if publication.kind() == TrackKind::Audio =>
            {
                let _ = speech.send(Speech::Muted(p.identity().to_string(), true));
                continue;
            }
            RoomEvent::TrackUnmuted { participant: Participant::Remote(p), publication }
                if publication.kind() == TrackKind::Audio =>
            {
                let _ = speech.send(Speech::Muted(p.identity().to_string(), false));
                continue;
            }
            RoomEvent::TrackSubscribed {
                track: RemoteTrack::Audio(track),
                participant,
                ..
            } => {
                info!(who = %participant.identity(), track = %track.sid(), "audio arriving");
                if deafened.lock().is_ok_and(|d| *d) {
                    track.disable();
                }
                tokio::spawn(watch_voice(
                    track.clone(),
                    participant.identity().to_string(),
                    speech.clone(),
                ));
                if mode == AudioMode::Tone {
                    tokio::spawn(measure(
                        track,
                        participant.identity().to_string(),
                        tx.clone(),
                    ));
                }
                continue;
            }
            RoomEvent::TrackSubscribed {
                track: RemoteTrack::Video(track),
                participant,
                publication,
            } => {
                let identity = participant.identity().to_string();
                let key = format!("{identity}/{}", track.sid());
                info!(who = %identity, track = %track.sid(), "video arriving");
                tokio::spawn(video::show_remote(track, key.clone()));
                CallEvent::Video {
                    identity,
                    key,
                    screen: publication.source() == TrackSource::Screenshare,
                    on: true,
                }
            }
            RoomEvent::TrackUnsubscribed {
                track: RemoteTrack::Video(track),
                participant,
                publication,
            } => {
                let identity = participant.identity().to_string();
                let key = format!("{identity}/{}", track.sid());
                video::forget(&key);
                CallEvent::Video {
                    identity,
                    key,
                    screen: publication.source() == TrackSource::Screenshare,
                    on: false,
                }
            }
            RoomEvent::Disconnected { reason } => CallEvent::Disconnected(format!("{reason:?}")),
            _ => continue,
        };
        if tx.send(out).is_err() {
            return;
        }
    }
}

/// Report when audio from `who` starts and stops arriving.
async fn measure(track: RemoteAudioTrack, who: String, tx: UnboundedSender<CallEvent>) {
    let mut stream = NativeAudioStream::new(track.rtc_track(), SAMPLE_RATE as i32, 1);
    let mut hearing = false;
    // Quiet only after a run of quiet frames: one lost packet is not silence.
    let mut quiet_frames = 0u32;
    loop {
        // The tone itself, not just sound: media that fails to decrypt can
        // still decode into noise, which is not hearing anyone.
        let tone = match tokio::time::timeout(Duration::from_millis(300), stream.next()).await {
            Ok(Some(frame)) => is_tone(&frame.data),
            Ok(None) => return,
            // Nothing at all arriving is quiet too.
            Err(_) => false,
        };
        quiet_frames = if tone { 0 } else { quiet_frames + 1 };
        let now = if hearing {
            quiet_frames < 20
        } else {
            quiet_frames == 0
        };
        if now != hearing {
            hearing = now;
            let event = if now {
                CallEvent::Heard(who.clone())
            } else {
                CallEvent::Quiet(who.clone())
            };
            if tx.send(event).is_err() {
                return;
            }
        }
    }
}

/// Whether a 10 ms frame sounds like the test tone: loud enough, and
/// crossing zero about as often as 440 Hz does (~9 times; noise, many more).
fn is_tone(samples: &[i16]) -> bool {
    let crossings = samples
        .windows(2)
        .filter(|pair| (pair[0] < 0) != (pair[1] < 0))
        .count();
    rms(samples) > 500.0 && (5..=14).contains(&crossings)
}

fn rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    (sum / samples.len() as f64).sqrt() as f32
}

// Who is speaking, worked out here instead of waiting for the server.
//
// LiveKit's `ActiveSpeakersChanged` is computed by the SFU on an interval
// with smoothing, so the ring lagged by up to a second. Like Discord, the
// client measures: each subscribed remote audio track's decoded level (a
// `NativeAudioStream`, one 10 ms frame at a time: RMS over 480 samples, no
// allocation of ours) and this device's microphone (the send stream's own
// audio level, which libwebrtc reports after echo cancellation and noise
// suppression, polled every 50 ms). Each goes through a [`Gate`]: a few
// loud frames to switch on, and a hold after the last loud one before it
// switches off, so it neither flickers between words nor trips on a click.
// Only changes of the set leave this module. The server's list stays as
// the fallback for anyone not measured (no frames arriving, or a deafened
// track that carries none).

/// Level (0 to 1, full scale) at which speech starts, and the lower one it
/// has to fall under before the hold runs (so a trailing syllable keeps it).
const ATTACK: f32 = 0.02;
const RELEASE: f32 = 0.01;
/// How long the level stays under [`RELEASE`] before the ring goes off.
const HOLD: Duration = Duration::from_millis(350);

/// Speaking or not from a stream of levels.
struct Gate {
    on: bool,
    /// Loud readings in a row.
    run: u32,
    /// Loud readings in a row needed to switch on.
    need: u32,
    /// Since when the level has been under [`RELEASE`], while on.
    quiet_since: Option<std::time::Instant>,
}

impl Gate {
    fn new(need: u32) -> Self {
        Self {
            on: false,
            run: 0,
            need,
            quiet_since: None,
        }
    }

    fn reset(&mut self) {
        self.on = false;
        self.run = 0;
        self.quiet_since = None;
    }

    /// Take a reading; whether speaking now.
    fn feed(&mut self, level: f32, now: std::time::Instant) -> bool {
        if level >= ATTACK {
            self.run += 1;
            self.quiet_since = None;
            if self.run >= self.need {
                self.on = true;
            }
        } else if level >= RELEASE && self.on {
            self.quiet_since = None;
        } else {
            self.run = 0;
            if self.on {
                let since = *self.quiet_since.get_or_insert(now);
                if now.duration_since(since) >= HOLD {
                    self.on = false;
                    self.quiet_since = None;
                }
            }
        }
        self.on
    }
}

/// What the measuring tasks and the server tell the aggregator.
enum Speech {
    /// The server's list (the fallback).
    Server(Vec<String>),
    /// A measured state (and level, to order by) for `who`.
    Measured { who: String, speaking: bool, level: f32 },
    /// `who` is no longer measured (no frames): the server decides again.
    Unmeasured(String),
    /// `who` muted their microphone (or not): muted is never speaking.
    Muted(String, bool),
    /// `who` left.
    Gone(String),
}

/// Merge everything into [`CallEvent::Speaking`], sent only when the set
/// changes.
async fn aggregate(
    mut input: UnboundedReceiver<Speech>,
    out: UnboundedSender<CallEvent>,
) {
    use std::collections::{HashMap, HashSet};
    let mut measured: HashMap<String, (bool, f32)> = HashMap::new();
    let mut server: Vec<String> = Vec::new();
    let mut muted: HashSet<String> = HashSet::new();
    let mut last: Vec<String> = Vec::new();
    while let Some(item) = input.recv().await {
        match item {
            Speech::Server(list) => server = list,
            Speech::Measured { who, speaking, level } => {
                measured.insert(who, (speaking, level));
            }
            Speech::Unmeasured(who) => {
                measured.remove(&who);
            }
            Speech::Muted(who, true) => {
                muted.insert(who);
            }
            Speech::Muted(who, false) => {
                muted.remove(&who);
            }
            Speech::Gone(who) => {
                measured.remove(&who);
                muted.remove(&who);
                server.retain(|s| *s != who);
            }
        }
        debug!(?measured, ?server, ?muted, "speech inputs");
        let mut now: Vec<(String, f32)> = measured
            .iter()
            .filter(|(_, (on, _))| *on)
            .map(|(who, (_, level))| (who.clone(), *level))
            .collect();
        now.extend(
            server
                .iter()
                .filter(|who| !measured.contains_key(*who))
                .map(|who| (who.clone(), 0.0)),
        );
        now.retain(|(who, _)| !muted.contains(who));
        now.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let now: Vec<String> = now.into_iter().map(|(who, _)| who).collect();
        let same = {
            let mut a = now.clone();
            let mut b = last.clone();
            a.sort();
            b.sort();
            a == b
        };
        if !same {
            last = now.clone();
            if out.send(CallEvent::Speaking(now)).is_err() {
                return;
            }
        }
    }
}

/// Measure what `who` sounds like (a subscribed remote track) and report
/// when they start and stop speaking.
async fn watch_voice(track: RemoteAudioTrack, who: String, speech: UnboundedSender<Speech>) {
    let mut stream = NativeAudioStream::new(track.rtc_track(), SAMPLE_RATE as i32, 1);
    // 20 ms over the attack level to start.
    let mut gate = Gate::new(2);
    let mut reported: Option<bool> = None;
    let mut last_frame = std::time::Instant::now();
    // Frames have arrived (and not stopped for a second).
    let mut got = false;
    loop {
        let level = match tokio::time::timeout(Duration::from_millis(50), stream.next()).await {
            Ok(Some(frame)) => {
                last_frame = std::time::Instant::now();
                got = true;
                rms(&frame.data) / 32_768.0
            }
            Ok(None) => break,
            // Nothing arriving: silence, and the clock for the hold goes on.
            Err(_) => {
                if got && last_frame.elapsed() > Duration::from_secs(1) {
                    got = false;
                    gate.reset();
                    if reported.take().is_some()
                        && speech.send(Speech::Unmeasured(who.clone())).is_err()
                    {
                        return;
                    }
                }
                0.0
            }
        };
        if !got {
            continue;
        }
        let now = gate.feed(level, std::time::Instant::now());
        if reported != Some(now) {
            reported = Some(now);
            let sent = speech.send(Speech::Measured {
                who: who.clone(),
                speaking: now,
                level,
            });
            if sent.is_err() {
                return;
            }
        }
    }
    let _ = speech.send(Speech::Unmeasured(who));
}

/// Measure this device's microphone while it is not muted.
async fn watch_mic(mic: LocalAudioTrack, me: String, speech: UnboundedSender<Speech>) {
    use livekit::webrtc::stats::RtcStats;
    let mut gate = Gate::new(1);
    let mut reported: Option<bool> = None;
    let mut failures = 0u32;
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        if mic.is_muted() {
            gate.reset();
            if reported != Some(false) {
                reported = Some(false);
                let _ = speech.send(Speech::Measured {
                    who: me.clone(),
                    speaking: false,
                    level: 0.0,
                });
            }
            continue;
        }
        let Ok(stats) = mic.get_stats().await else {
            failures += 1;
            if failures == 20 && reported.take().is_some() {
                let _ = speech.send(Speech::Unmeasured(me.clone()));
            }
            continue;
        };
        // Muted while the question was out: its answer is stale.
        if mic.is_muted() {
            continue;
        }
        let Some(level) = stats.iter().find_map(|s| match s {
            RtcStats::MediaSource(m) if m.source.kind == "audio" => Some(m.audio.audio_level as f32),
            _ => None,
        }) else {
            continue;
        };
        failures = 0;
        let now = gate.feed(level, std::time::Instant::now());
        if reported != Some(now) {
            reported = Some(now);
            if speech
                .send(Speech::Measured {
                    who: me.clone(),
                    speaking: now,
                    level,
                })
                .is_err()
            {
                return;
            }
        }
    }
}

#[cfg(test)]
mod speech_tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn a_gate_switches_on_fast_and_off_after_the_hold() {
        let t = Instant::now();
        let ms = |n| t + Duration::from_millis(n);
        let mut gate = Gate::new(2);
        assert!(!gate.feed(0.1, ms(0)), "one loud frame is a click");
        assert!(gate.feed(0.1, ms(10)));
        // Between words: quiet for less than the hold.
        assert!(gate.feed(0.0, ms(20)));
        assert!(gate.feed(0.0, ms(300)));
        assert!(gate.feed(0.1, ms(310)), "speech again resets the hold");
        assert!(gate.feed(0.0, ms(320)));
        assert!(gate.feed(0.015, ms(500)), "above the release level holds");
        assert!(gate.feed(0.0, ms(510)));
        assert!(!gate.feed(0.0, ms(870)), "off after the hold");
        // A click in silence is not speech.
        assert!(!gate.feed(0.5, ms(900)));
        assert!(!gate.feed(0.0, ms(910)));
        assert!(!gate.feed(0.5, ms(920)));
    }
}
