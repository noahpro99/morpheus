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
    collections::HashMap,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use futures_util::{FutureExt as _, StreamExt};
use livekit::{
    e2ee::{
        E2eeOptions, EncryptionType,
        key_provider::{KeyProvider, KeyProviderOptions},
    },
    options::{
        AudioEncoding, DegradationPreference, TrackPublishOptions, VideoCodec, VideoEncoderBackend,
        VideoEncoding, compute_video_encodings,
    },
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

use crate::quality::ScreenQuality;

#[path = "autorelease.rs"]
mod autorelease;
#[path = "devices.rs"]
mod devices;
pub use devices::{
    AudioDevice, AudioDevices, CameraDevice, DeviceChoice, audio_devices, cameras,
    choose_audio_devices, choose_camera, chosen_audio_devices, chosen_camera,
};
#[cfg(target_os = "linux")]
#[path = "camera.rs"]
mod camera;
#[cfg(target_os = "windows")]
#[path = "camera_win.rs"]
mod camera;
#[cfg(target_os = "macos")]
#[path = "camera_mac.rs"]
mod camera;
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
#[path = "camera_format.rs"]
mod camera_format;
#[path = "platform_camera.rs"]
mod platform_camera;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
#[path = "screen.rs"]
mod screen;
#[cfg(target_os = "windows")]
#[path = "screen_sources_win.rs"]
mod screen_sources_win;
#[cfg(target_os = "windows")]
pub(crate) use screen_sources_win::WindowsSources;
pub use platform_camera::{PlatformCamera, camera_frame, set_platform_camera};
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
#[path = "denoise.rs"]
mod denoise;
#[cfg(target_os = "windows")]
#[path = "ducking.rs"]
mod ducking;
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
#[path = "mic.rs"]
mod mic;
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
#[path = "mic_pace.rs"]
mod mic_pace;
#[cfg(target_os = "windows")]
#[path = "mmcss.rs"]
mod mmcss;
#[path = "system_audio.rs"]
#[cfg_attr(
    not(any(target_os = "linux", target_os = "windows", target_os = "macos")),
    allow(dead_code)
)]
pub(crate) mod system_audio;
pub use system_audio::{
    TONE_LEFT as SCREEN_TONE_LEFT, TONE_RIGHT as SCREEN_TONE_RIGHT, screen_audio_self_test,
    screen_audio_unavailable,
};
#[cfg(target_os = "linux")]
#[path = "system_audio_pulse.rs"]
mod system_audio_pulse;
#[cfg(target_os = "windows")]
#[path = "system_audio_wasapi.rs"]
mod system_audio_wasapi;
#[cfg(target_os = "macos")]
#[path = "system_audio_sck.rs"]
pub(crate) mod system_audio_sck;
#[path = "system_audio_frames.rs"]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod system_audio_frames;
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
pub use mic::{
    AudioStats, audio_stats, mic_level, mic_loopback, mic_test_running, reset_audio_stats,
    start_mic_test, stop_mic_test,
};

/// Phones: no mic test (their microphone is WebRTC's own).
#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
mod no_mic_test {
    pub fn start_mic_test() {}
    pub fn stop_mic_test() {}
    pub fn mic_test_running() -> bool {
        false
    }
    pub fn mic_level() -> f32 {
        0.0
    }
    pub fn mic_loopback() -> bool {
        false
    }
}
#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
pub use no_mic_test::*;
#[path = "preview.rs"]
mod preview;
#[cfg(test)]
#[path = "speech_fixture.rs"]
mod speech_fixture;
pub use preview::{
    PREVIEW_CAMERA, PreviewError, camera_in_use, camera_opens, camera_preview_device,
    camera_preview_key, camera_preview_start, camera_preview_stop, camera_self_test,
};
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

/// Whether calls here can send video: a webcam on Linux, Windows and macOS, the app's own
/// camera (phones, [`set_platform_camera`]), or the test pattern.
pub fn can_send_video() -> bool {
    cfg!(any(target_os = "linux", target_os = "windows", target_os = "macos"))
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
    /// The sound of `identity`'s shared screen started (`on`) or stopped
    /// arriving ([`crate::screen_audio`]).
    ScreenAudio {
        identity: String,
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
#[cfg_attr(
    any(target_os = "linux", target_os = "windows", target_os = "macos"),
    allow(dead_code)
)]
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
    /// This call's place in the microphone capture (desktops).
    _mic: MicGuard,
    mode: AudioMode,
    /// The camera, while it is on.
    camera: Arc<tokio::sync::Mutex<Option<Outgoing>>>,
    /// The shared screen, while it is.
    screen: Arc<tokio::sync::Mutex<Option<Outgoing>>>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
    /// The call's events, for the other SFUs' connections too.
    events: UnboundedSender<CallEvent>,
    /// The other SFUs listened on, by call service ([`Call::subscribe`]).
    others: Others,
    /// Hung up: a connection to another SFU that comes too late is left at once.
    closed: Arc<AtomicBool>,
}

/// The connections to SFUs this device only listens on, by call service.
type Others = Arc<Mutex<HashMap<String, Listening>>>;

/// A connection to someone else's SFU: their media comes in, nothing goes out.
struct Listening {
    room: Arc<Room>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
    /// The call's events: what came from this SFU is said gone in them.
    events: UnboundedSender<CallEvent>,
}

impl Drop for Listening {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

impl Listening {
    fn live(&self) -> bool {
        self.room.connection_state() != ConnectionState::Disconnected
    }
}

/// Which connection something happened on: the one this device publishes
/// on, or another SFU's (by call service).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Source {
    Own,
    Other(String),
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
        // Element Call's settings: per-participant keys (no shared key), its
        // derivation, ratcheting and key ring ([`key_options`]), AES-GCM.
        let keys = own_key
            .as_ref()
            .map(|_| KeyProvider::new(key_options()));
        let mut options = RoomOptions::default();
        options.encryption = keys.clone().map(|key_provider| E2eeOptions {
            encryption_type: EncryptionType::Gcm,
            key_provider,
        });
        let (room, room_events) = Room::connect(&url, &token, options)
            .await
            .map_err(|err| Error::Connect(err.to_string()))?;
        let room = Arc::new(room);
        log_connected(&room, &Source::Own);
        if let (Some(keys), Some(own_key)) = (&keys, own_key) {
            keys.set_key(&room.local_participant().identity(), 0, own_key);
        }
        let (tx, rx) = unbounded_channel();
        let mut tasks = Vec::new();

        // Desktops capture the microphone themselves and clean it first
        // (`mic`); WebRTC's device module only plays the call there.
        #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
        let (source, platform, captured) = {
            let platform = match mode {
                AudioMode::Platform => {
                    // Before WebRTC opens its communications streams.
                    #[cfg(target_os = "windows")]
                    ducking::opt_out("call start");
                    let platform =
                        PlatformAudio::new().map_err(|err| Error::Audio(err.to_string()))?;
                    if let Err(err) = devices::apply(&platform, false) {
                        warn!(%err, "chosen device unavailable, using the default");
                    }
                    use livekit::webrtc::peer_connection_factory::native::PeerConnectionFactoryExt as _;
                    livekit::rtc_engine::lk_runtime::LkRuntime::instance()
                        .pc_factory()
                        .set_adm_recording_enabled(false);
                    Some(platform)
                }
                AudioMode::Tone => None,
            };
            // No queue: the capture's own clock paces the frames.
            let source = NativeAudioSource::new(AudioSourceOptions::default(), SAMPLE_RATE, 1, 0);
            let captured = MicGuard(mic::attach_call(source.clone(), mode));
            (RtcAudioSource::Native(source), platform, captured)
        };
        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        let (source, platform, captured) = match mode {
            AudioMode::Platform => {
                let platform = PlatformAudio::new().map_err(|err| Error::Audio(err.to_string()))?;
                if let Err(err) = devices::apply(&platform, false) {
                    warn!(%err, "chosen device unavailable, using the default");
                }
                (platform.rtc_source(), Some(platform), MicGuard(None))
            }
            AudioMode::Tone => {
                let source =
                    NativeAudioSource::new(AudioSourceOptions::default(), SAMPLE_RATE, 1, 100);
                tasks.push(tokio::spawn(play_tone(source.clone())));
                (RtcAudioSource::Native(source), None, MicGuard(None))
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
        // Again, now the call's audio has started.
        #[cfg(target_os = "windows")]
        if mode == AudioMode::Platform {
            ducking::opt_out("call audio started");
        }

        let deafened = Arc::new(Mutex::new(false));
        let (speech, speech_rx) = unbounded_channel();
        let me = room.local_participant().identity().to_string();
        crate::diag::begin(&url);
        tasks.push(tokio::spawn(watch_connection(room.clone(), mic.clone())));
        tasks.push(tokio::spawn(aggregate(speech_rx, tx.clone())));
        tasks.push(tokio::spawn(watch_mic(
            mic.clone(),
            me.clone(),
            speech.clone(),
        )));
        tasks.push(tokio::spawn(forward_events(
            room_events,
            tx.clone(),
            speech.clone(),
            mode,
            deafened.clone(),
            Source::Own,
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
                _mic: captured,
                mode,
                camera: Arc::default(),
                screen: Arc::default(),
                tasks,
                events: tx,
                others: Arc::default(),
                closed: Arc::default(),
            },
            rx,
        ))
    }

    /// Everyone else in the call (LiveKit identities), on every SFU it is
    /// on, each name once.
    pub fn participants(&self) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        for participant in self.remote_participants() {
            let identity = participant.identity().to_string();
            if !found.contains(&identity) {
                found.push(identity);
            }
        }
        found
    }

    /// The other participants of every connection: the own SFU's, then
    /// the others'.
    fn remote_participants(&self) -> Vec<RemoteParticipant> {
        let mut all: Vec<RemoteParticipant> =
            self.room.remote_participants().into_values().collect();
        if let Ok(others) = self.others.lock() {
            for other in others.values() {
                all.extend(other.room.remote_participants().into_values());
            }
        }
        all
    }

    /// Listen on another SFU of the call too: the one behind `service`
    /// (someone else's homeserver's), at `url` with `token`. Their media
    /// comes in as this connection's does, decrypted with the same keys
    /// ([`Call::set_key`]) and played at the same volumes; nothing is sent
    /// there. Its events come with the call's; its dropping does not end
    /// the call (it is gone from [`Call::listening`], to be connected again).
    pub fn subscribe(
        &self,
        service: &str,
        url: &str,
        token: &str,
    ) -> impl Future<Output = Result<()>> + use<> {
        let (service, url, token) = (service.to_owned(), url.to_owned(), token.to_owned());
        let keys = self.keys.clone();
        let (events, speech, mode) = (self.events.clone(), self.speech.clone(), self.mode);
        let (deafened, others) = (self.deafened.clone(), self.others.clone());
        let closed = self.closed.clone();
        async move {
            runtime()
                .spawn(async move {
                    let mut options = RoomOptions::default();
                    options.encryption = keys.map(|key_provider| E2eeOptions {
                        encryption_type: EncryptionType::Gcm,
                        key_provider,
                    });
                    let (room, room_events) = Room::connect(&url, &token, options)
                        .await
                        .map_err(|err| Error::Connect(err.to_string()))?;
                    let room = Arc::new(room);
                    if closed.load(Ordering::SeqCst) {
                        let _ = room.close().await;
                        return Ok(());
                    }
                    let shown = crate::diag::bare_url(&url);
                    info!(%service, url = %shown, "listening on another SFU");
                    log_connected(&room, &Source::Other(service.clone()));
                    let tasks = vec![
                        tokio::spawn(forward_events(
                            room_events,
                            events.clone(),
                            speech.clone(),
                            mode,
                            deafened,
                            Source::Other(service.clone()),
                        )),
                        tokio::spawn(watch_other(Arc::downgrade(&room), service.clone(), url)),
                    ];
                    let listening = Listening { room, tasks, events };
                    let before = others
                        .lock()
                        .ok()
                        .and_then(|mut others| others.insert(service.clone(), listening));
                    if let Some(before) = before {
                        stop_listening(before, &service, &speech).await;
                    }
                    Ok(())
                })
                .await?
        }
    }

    /// Stop listening on the SFU behind `service`: what came from it is gone.
    pub fn unsubscribe(&self, service: &str) -> impl Future<Output = ()> + use<> {
        let gone = self.others.lock().ok().and_then(|mut o| o.remove(service));
        let (service, speech) = (service.to_owned(), self.speech.clone());
        async move {
            if let Some(gone) = gone {
                let _ = runtime()
                    .spawn(async move { stop_listening(gone, &service, &speech).await })
                    .await;
            }
        }
    }

    /// The call services of the other SFUs listened on, and whether each
    /// is still connected (one that dropped is to be connected again).
    pub fn listening(&self) -> Vec<(String, bool)> {
        self.others
            .lock()
            .map(|others| {
                others
                    .iter()
                    .map(|(service, other)| (service.clone(), other.live()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// This device's LiveKit identity.
    pub fn identity(&self) -> String {
        self.room.local_participant().identity().to_string()
    }

    /// Hear `identity` in an encrypted call: their media key at `index`, as
    /// they sent it (Element Call moves to the next index when it changes key).
    pub fn set_key(&self, identity: &str, index: u32, key: Vec<u8>) {
        info!(
            who = identity,
            index,
            encrypted = self.keys.is_some(),
            "call key received"
        );
        if let Some(keys) = &self.keys {
            keys.set_key(&ParticipantIdentity(identity.to_owned()), (index % 256) as i32, key);
        }
    }

    /// Switch to the devices last chosen ([`choose_audio_devices`]).
    pub fn use_chosen_devices(&self) -> Result<()> {
        let _enter = runtime().enter();
        #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
        mic::reopen_devices();
        #[cfg(target_os = "windows")]
        if self.platform.is_some() {
            ducking::opt_out("devices changed");
        }
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
            // Back from silence without a click (#30).
            #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
            mic::fade_in();
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
        for participant in &self.remote_participants() {
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

    /// Play `identity`'s shared screen's sound at `volume` (1 as sent, up to
    /// [`crate::MAX_SCREEN_VOLUME`]), now and whenever it comes again.
    pub fn set_screen_volume(&self, identity: &str, volume: f32) {
        let _enter = runtime().enter();
        let volume = crate::screen_audio::note_volume(identity, volume);
        for participant in &self.remote_participants() {
            if participant.identity().as_str() != identity {
                continue;
            }
            for publication in participant.track_publications().values() {
                if publication.source() == TrackSource::ScreenshareAudio
                    && let Some(RemoteTrack::Audio(track)) = publication.track()
                {
                    apply_screen_volume(&track, volume);
                }
            }
        }
    }

    /// Play `user`'s voice at `volume` (1 as sent, 0 for none) on top of the
    /// output volume, now and whenever it comes again.
    pub fn set_user_volume(&self, user: &str, volume: f32) {
        let _enter = runtime().enter();
        crate::set_user_volume(user, volume);
        for participant in &self.remote_participants() {
            let identity = participant.identity().to_string();
            if !crate::voice_belongs_to(&identity, user) {
                continue;
            }
            for publication in participant.track_publications().values() {
                if publication.source() != TrackSource::ScreenshareAudio
                    && let Some(RemoteTrack::Audio(track)) = publication.track()
                {
                    track
                        .rtc_track()
                        .set_volume(f64::from(crate::voice_gain(&identity)));
                }
            }
        }
    }

    /// Play voice audio at `volume` (1.0 as sent, up to [`crate::MAX_VOLUME`]).
    pub fn set_output_volume(&self, volume: f32) {
        let _enter = runtime().enter();
        crate::set_output_volume(volume);
        for participant in &self.remote_participants() {
            let vol = crate::voice_gain(participant.identity().as_str());
            for publication in participant.track_publications().values() {
                if publication.source() != TrackSource::ScreenshareAudio
                    && let Some(RemoteTrack::Audio(track)) = publication.track()
                {
                    track.rtc_track().set_volume(f64::from(vol));
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
        self.set_video(VideoKind::Camera, on, ScreenQuality::default(), None)
    }

    /// Share this device's screen (or stop). Where the desktop asks which
    /// screen or window (Wayland), this waits for the answer; cancelling is
    /// an error. What it sends shows under [`LOCAL_SCREEN`] too.
    pub fn set_screen_share(&self, on: bool) -> impl Future<Output = Result<()>> + 'static {
        self.set_screen_share_with(on, ScreenQuality::default())
    }

    /// [`Self::set_screen_share`] at `quality`: the size and frame rate it is
    /// sent at, and the encoding that goes with them.
    pub fn set_screen_share_with(
        &self,
        on: bool,
        quality: ScreenQuality,
    ) -> impl Future<Output = Result<()>> + 'static {
        self.set_screen_share_from(on, quality, None)
    }

    /// [`Self::set_screen_share_with`] from `source`, picked in the app
    /// ([`crate::ScreenSourceProvider`]); `None` is the system's picker
    /// (Wayland's portal, macOS's) or, without one, the first screen.
    pub fn set_screen_share_from(
        &self,
        on: bool,
        quality: ScreenQuality,
        source: Option<crate::ScreenSource>,
    ) -> impl Future<Output = Result<()>> + 'static {
        self.set_video(VideoKind::Screen, on, quality, source)
    }

    fn set_video(
        &self,
        kind: VideoKind,
        on: bool,
        quality: ScreenQuality,
        source: Option<crate::ScreenSource>,
    ) -> impl Future<Output = Result<()>> + 'static {
        let (room, video, mode) = (self.room.clone(), self.outgoing(kind).clone(), self.mode);
        let encrypted = self.keys.is_some();
        async move {
            runtime()
                .spawn(async move {
                    let mut video = video.lock().await;
                    match (on, video.take()) {
                        (true, Some(running)) => *video = Some(running),
                        (true, None) => {
                            *video = Some(
                                Outgoing::start(&room, kind, mode, encrypted, quality, source)
                                    .await?,
                            )
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
        self.closed.store(true, Ordering::SeqCst);
        // Every SFU at once: a quit waits for this (#47).
        let others: Vec<Arc<Room>> = self
            .others
            .lock()
            .map(|mut others| {
                others
                    .drain()
                    .map(|(_, other)| other.room.clone())
                    .collect()
            })
            .unwrap_or_default();
        let _ = runtime()
            .spawn(async move {
                let closing = std::iter::once(room).chain(others).map(|room| async move {
                    if let Err(err) = room.close().await {
                        debug!(%err, "closing the call");
                    }
                });
                futures_util::future::join_all(closing).await;
            })
            .await;
        // `self` drops here: the tasks stop and the devices close.
    }
}

impl Drop for Call {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
        crate::diag::end();
        for task in &self.tasks {
            task.abort();
        }
        if let Ok(mut others) = self.others.lock() {
            for (_, other) in others.drain() {
                for task in &other.tasks {
                    task.abort();
                }
            }
        }
        // The last user lets go of WebRTC's device module: its playout (a
        // communications stream, which Windows ducks other apps for) stops.
        if self.platform.take().is_some() {
            use livekit::webrtc::peer_connection_factory::native::PeerConnectionFactoryExt as _;
            let runtime = livekit::rtc_engine::lk_runtime::LkRuntime::instance();
            let factory = runtime.pc_factory();
            info!(
                active = factory.is_platform_adm_active(),
                users = factory.platform_adm_ref_count(),
                "call devices released"
            );
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
        crate::screen_audio::note_sent(None);
        crate::screen_sources::note_shared(None);
    }
}

/// Let go of another SFU: its tasks stop, and what came from it (pictures,
/// a screen's sound, who spoke) is said gone, then it is left.
async fn stop_listening(gone: Listening, service: &str, speech: &UnboundedSender<Speech>) {
    for task in &gone.tasks {
        task.abort();
    }
    for participant in gone.room.remote_participants().into_values() {
        let identity = participant.identity().to_string();
        for publication in participant.track_publications().into_values() {
            match publication.track() {
                Some(RemoteTrack::Video(track)) => {
                    let key = format!("{identity}/{}", track.sid());
                    video::forget(&key);
                    let _ = gone.events.send(CallEvent::Video {
                        identity: identity.clone(),
                        key,
                        screen: publication.source() == TrackSource::Screenshare,
                        on: false,
                    });
                }
                Some(RemoteTrack::Audio(_))
                    if publication.source() == TrackSource::ScreenshareAudio =>
                {
                    crate::screen_audio::forget_heard(&identity);
                    let _ = gone.events.send(CallEvent::ScreenAudio {
                        identity: identity.clone(),
                        on: false,
                    });
                }
                _ => {}
            }
        }
        #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
        mic::far_end_gone(&identity);
        let _ = speech.send(Speech::Gone(identity));
    }
    let _ = speech.send(Speech::Server(Source::Other(service.to_owned()), Vec::new()));
    crate::diag::forget_other(service);
    if let Err(err) = gone.room.close().await {
        debug!(%err, service, "leaving another SFU");
    }
}

/// Keep the diagnostics' line for another SFU up to date.
async fn watch_other(room: std::sync::Weak<Room>, service: String, url: String) {
    let url = crate::diag::bare_url(&url);
    while let Some(room) = room.upgrade() {
        let state = format!("{:?}", room.connection_state()).to_lowercase();
        crate::diag::note_other(crate::diag::OtherSfu {
            service: service.clone(),
            url: url.clone(),
            state,
        });
        drop(room);
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// A call's hold on the microphone capture: let go when dropped.
struct MicGuard(Option<u64>);

impl Drop for MicGuard {
    fn drop(&mut self) {
        #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
        if let Some(id) = self.0.take() {
            mic::detach_call(id);
        }
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
    /// A shared screen's sound, when it has some.
    sound: Option<ScreenSound>,
    /// A camera: the preview knows the call has it (let go when this drops).
    _claim: Option<preview::CallClaim>,
}

/// The computer's sound, published beside its screen.
struct ScreenSound {
    track: LocalAudioTrack,
    /// Captures until dropped.
    _capture: system_audio::Capture,
}

/// Where outgoing frames come from.
enum Feed {
    /// A test run's picture: its colours, size and frames a second.
    TestPattern(video::TestPattern, u32, u32, u32),
    /// The app's own camera ([`set_platform_camera`]).
    Platform,
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
    Camera(camera::Camera),
    /// Already capturing; frames go out once the source is put in the slot.
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    Screen(Arc<OnceLock<NativeVideoSource>>),
}

/// The codec video is sent in: H.264 where this computer has a hardware
/// encoder for it (Linux: VAAPI on Intel and AMD, NVENC on NVIDIA, which
/// are H.264 encoders), VP8 in software otherwise. If the hardware encoder
/// fails to start, libwebrtc encodes H.264 in software (OpenH264) instead.
/// `MORPHEUS_HW_VIDEO=0` keeps to VP8 in software.
fn video_codec() -> VideoCodec {
    static CODEC: OnceLock<VideoCodec> = OnceLock::new();
    *CODEC.get_or_init(|| {
        let backends: Vec<_> = VideoEncoderBackend::list_available().into_iter().collect();
        let hardware = cfg!(target_os = "linux")
            && std::env::var("MORPHEUS_HW_VIDEO").as_deref() != Ok("0")
            && backends
                .iter()
                .any(|b| matches!(b, VideoEncoderBackend::Nvenc | VideoEncoderBackend::Vaapi));
        let codec = if hardware {
            VideoCodec::H264
        } else {
            VideoCodec::VP8
        };
        info!(?backends, ?codec, "video encoders available");
        codec
    })
}

/// Log which encoder sends a published video (`kind`: camera or screen),
/// from its sender's stats, when it starts and whenever it changes (a
/// hardware encoder that fails is replaced by a software one), and note it
/// for the interface and tests.
async fn watch_encoder(
    track: LocalVideoTrack,
    key: &'static str,
    kind: &'static str,
    stop: Arc<AtomicBool>,
) {
    use livekit::webrtc::stats::RtcStats;
    let mut last: Option<String> = None;
    while !stop.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_secs(if last.is_some() { 2 } else { 1 })).await;
        let Ok(stats) = track.get_stats().await else {
            continue;
        };
        let loss = stats.iter().find_map(|s| match s {
            RtcStats::RemoteInboundRtp(r) if r.stream.kind == "video" => {
                Some(r.remote_inbound.fraction_lost * 100.0)
            }
            _ => None,
        });
        let Some(sent) = stats.into_iter().find_map(|s| match s {
            RtcStats::OutboundRtp(o)
                if o.stream.kind == "video" && !o.outbound.encoder_implementation.is_empty() =>
            {
                Some(o)
            }
            _ => None,
        }) else {
            continue;
        };
        let implementation = sent.outbound.encoder_implementation.clone();
        if !stop.load(Ordering::Relaxed) {
            crate::diag::note_track(crate::TrackDiag {
                label: kind.to_owned(),
                direction: "sending",
                backend: Some(crate::codec_backend(&implementation, true)),
                implementation: implementation.clone(),
                width: sent.outbound.frame_width,
                height: sent.outbound.frame_height,
                fps: sent.outbound.frames_per_second,
                bitrate: Some(sent.outbound.target_bitrate).filter(|b| *b > 0.0),
                loss_percent: loss,
                jitter_ms: None,
            });
        }
        if last.as_ref() != Some(&implementation) {
            let encoder = crate::codec_backend(&implementation, true);
            info!(kind, encoder, %implementation, "video encoder");
            if !stop.load(Ordering::Relaxed) {
                crate::quality::note_encoder(key, encoder);
            }
            last = Some(implementation);
        }
    }
    crate::diag::forget_track(kind);
}

/// Log which decoder shows someone's video (NVDEC or software), from the
/// receiver's stats, when it starts and whenever it changes.
async fn watch_decoder(track: RemoteVideoTrack, who: String) {
    let label = format!("{who}/{}", track.sid());
    watch_decoder_inner(&track, &who, &label).await;
    crate::diag::forget_track(&label);
}

async fn watch_decoder_inner(track: &RemoteVideoTrack, who: &str, label: &str) {
    use livekit::webrtc::stats::RtcStats;
    let started = std::time::Instant::now();
    let mut last: Option<String> = None;
    let mut frames = 0;
    let mut bytes: Option<(u64, std::time::Instant)> = None;
    loop {
        tokio::time::sleep(Duration::from_secs(if last.is_some() { 2 } else { 1 })).await;
        let Ok(stats) = track.get_stats().await else {
            return;
        };
        let decoding = stats.into_iter().find_map(|s| match s {
            RtcStats::InboundRtp(i)
                if i.stream.kind == "video" && !i.inbound.decoder_implementation.is_empty() =>
            {
                Some(i)
            }
            _ => None,
        });
        match decoding {
            // Stopped (unsubscribed or paused): nothing more to log.
            Some(i) if last.is_some() && i.inbound.frames_decoded == frames => return,
            Some(i) => {
                frames = i.inbound.frames_decoded;
                let implementation = i.inbound.decoder_implementation.clone();
                let now = std::time::Instant::now();
                let bitrate = bytes.and_then(|(before, at)| {
                    let secs = now.duration_since(at).as_secs_f64();
                    (i.inbound.bytes_received >= before && secs > 0.0)
                        .then(|| (i.inbound.bytes_received - before) as f64 * 8.0 / secs)
                });
                bytes = Some((i.inbound.bytes_received, now));
                let received = i.received.packets_received as f64;
                let lost = i.received.packets_lost.max(0) as f64;
                crate::diag::note_track(crate::TrackDiag {
                    label: label.to_owned(),
                    direction: "receiving",
                    backend: Some(crate::codec_backend(&implementation, false)),
                    implementation: implementation.clone(),
                    width: i.inbound.frame_width,
                    height: i.inbound.frame_height,
                    fps: i.inbound.frames_per_second,
                    bitrate,
                    loss_percent: (received + lost > 0.0).then(|| lost / (received + lost) * 100.0),
                    jitter_ms: Some(i.received.jitter * 1000.0),
                });
                if last.as_ref() != Some(&implementation) {
                    let decoder = crate::codec_backend(&implementation, false);
                    info!(%who, track = %track.sid(), decoder, %implementation, "video decoder");
                    last = Some(implementation);
                }
            }
            None if last.is_some() || started.elapsed() > Duration::from_secs(60) => return,
            None => {}
        }
    }
}

/// How long the desktop's screen picker may take.
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
const PICKER_TIMEOUT: Duration = Duration::from_secs(180);

impl Outgoing {
    async fn start(
        room: &Room,
        kind: VideoKind,
        mode: AudioMode,
        encrypted: bool,
        quality: ScreenQuality,
        picked: Option<crate::ScreenSource>,
    ) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let screen = kind == VideoKind::Screen;
        // The camera is the call's from here (a preview lets go of it).
        let claim = if screen {
            None
        } else {
            Some(preview::CallClaim::take().await)
        };
        let (feed, source) = match (mode, kind) {
            (AudioMode::Tone, VideoKind::Camera) => {
                let (width, height) =
                    video::test_size("MORPHEUS_CALL_CAMERA_SIZE", video::CAMERA_PATTERN);
                if let Some(claim) = &claim {
                    claim.using(preview::test_camera_id());
                }
                (
                    Feed::TestPattern(video::CAMERA_PATTERN, width, height, video::FPS as u32),
                    NativeVideoSource::new(VideoResolution { width, height }, false),
                )
            }
            (AudioMode::Tone, VideoKind::Screen) => {
                // A screen of its own size, sent at what the choice allows.
                let (native_w, native_h) =
                    video::test_size("MORPHEUS_CALL_SCREEN_SIZE", video::SCREEN_PATTERN);
                let (width, height) = quality.fit(native_w, native_h);
                (
                    Feed::TestPattern(video::SCREEN_PATTERN, width, height, quality.fps),
                    NativeVideoSource::new(
                        VideoResolution { width, height },
                        quality.is_screencast(),
                    ),
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
            #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
            (AudioMode::Platform, VideoKind::Camera) => {
                let device = tokio::task::spawn_blocking(camera::Camera::open)
                    .await?
                    .map_err(|err| Error::Video(err.to_string()))?;
                if let Some(claim) = &claim {
                    claim.using(device.id().to_owned());
                }
                let source = NativeVideoSource::new(device.resolution(), false);
                (Feed::Camera(device), source)
            }
            #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
            (AudioMode::Platform, VideoKind::Screen) => {
                // The screen's size is known with its first picture; the
                // source (whose size the call is told) is made then, and the
                // capture sends to it once the track is up.
                let sink: Arc<OnceLock<NativeVideoSource>> = Arc::default();
                let (started, first) = tokio::sync::oneshot::channel();
                screen::run(sink.clone(), quality, picked.clone(), stop.clone(), started);
                let first = tokio::time::timeout(PICKER_TIMEOUT, first).await;
                let (width, height) = match first {
                    Ok(Ok(Ok(size))) => size,
                    Ok(Ok(Err(why))) => return Err(Error::Video(why)),
                    _ => {
                        stop.store(true, Ordering::Relaxed);
                        return Err(Error::Video("no picture from the screen".into()));
                    }
                };
                let source = NativeVideoSource::new(
                    VideoResolution { width, height },
                    quality.is_screencast(),
                );
                (Feed::Screen(sink), source)
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
        let resolution = source.video_resolution();
        let options = if screen {
            // One full-size layer (a downscaled copy of a screen is what
            // made it blurry), the bitrate for its size and speed, and
            // sharpness before smoothness.
            TrackPublishOptions {
                source: track_source,
                video_codec: video_codec(),
                simulcast: false,
                video_encoding: Some(VideoEncoding {
                    max_bitrate: quality.max_bitrate(resolution.width, resolution.height),
                    max_framerate: f64::from(quality.fps),
                }),
                degradation_preference: Some(DegradationPreference::MaintainResolution),
                ..Default::default()
            }
        } else {
            TrackPublishOptions {
                source: track_source,
                video_codec: video_codec(),
                // A camera of 720p or more has a bitrate of its own (the
                // default for a 16:9 picture is read off the next preset up).
                video_encoding: (resolution.width * resolution.height >= 1280 * 720).then(|| {
                    VideoEncoding {
                        max_bitrate: crate::camera_bitrate(resolution.width, resolution.height),
                        max_framerate: 30.0,
                    }
                }),
                ..Default::default()
            }
        };
        let encodings = compute_video_encodings(resolution.width, resolution.height, &options);
        let top = encodings
            .iter()
            .filter_map(|e| e.max_bitrate)
            .max()
            .unwrap_or(0);
        let fps = encodings
            .iter()
            .filter_map(|e| e.max_framerate)
            .fold(0.0, f64::max);
        let published_as = crate::PublishedVideo {
            width: resolution.width,
            height: resolution.height,
            fps: fps.round() as u32,
            max_bitrate: top,
            layers: encodings.len(),
            hint: if screen {
                quality.content_hint()
            } else {
                "motion"
            },
            degradation: if screen {
                "maintain-resolution"
            } else {
                "maintain-framerate"
            },
            encoder: None,
        };
        let options_codec = options.video_codec;
        let track =
            LocalVideoTrack::create_video_track(name, RtcVideoSource::Native(source.clone()));
        let published = room
            .local_participant()
            .publish_track(LocalTrack::Video(track.clone()), options)
            .await;
        let publication = match published {
            Ok(publication) => publication,
            Err(err) => {
                stop.store(true, Ordering::Relaxed);
                return Err(Error::Video(err.to_string()));
            }
        };
        info!(
            track = %publication.sid(),
            kind = if screen { "screen" } else { "camera" },
            published = %published_as,
            codec = ?options_codec,
            "video track published"
        );
        let key = if screen { LOCAL_SCREEN } else { LOCAL_CAMERA };
        crate::quality::note_published(key, published_as);
        tokio::spawn(watch_encoder(
            track.clone(),
            key,
            if screen { "screen" } else { "camera" },
            stop.clone(),
        ));
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
            Feed::TestPattern(pattern, width, height, fps) => {
                tokio::spawn(video::play_test_pattern(
                    pattern,
                    (width, height, fps),
                    source,
                    stop.clone(),
                ));
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
            #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
            Feed::Camera(device) => device.run(source, stop.clone()),
            #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
            Feed::Screen(sink) => {
                let _ = sink.set(source.clone());
            }
        }
        // What was picked in the app, for the interface and tests.
        if screen {
            crate::screen_sources::note_shared(picked);
        }
        // The screen's sound, when wanted: the share goes on without it if
        // there is none to be had.
        // (Not even tried where the platform has no way: macOS before 13.)
        let sound = if screen
            && quality.audio
            && (mode == AudioMode::Tone || screen_audio_unavailable().is_none())
        {
            match ScreenSound::publish(room, mode, encrypted).await {
                Ok(sound) => Some(sound),
                Err(why) => {
                    warn!(%why, "screen shared without its sound");
                    crate::screen_audio::note_sent(Some(Err(why)));
                    None
                }
            }
        } else {
            None
        };
        Ok(Self {
            track,
            kind,
            stop,
            sound,
            _claim: claim,
        })
    }

    async fn stop(self, room: &Room) {
        self.halt();
        if let Some(sound) = &self.sound {
            if let Err(err) = room
                .local_participant()
                .unpublish_track(&sound.track.sid())
                .await
            {
                debug!(%err, "unpublishing the screen's sound");
            }
        }
        if self.kind == VideoKind::Screen {
            crate::screen_audio::note_sent(None);
            crate::screen_sources::note_shared(None);
        }
        if let Err(err) = room
            .local_participant()
            .unpublish_track(&self.track.sid())
            .await
        {
            debug!(%err, "unpublishing video");
        }
        let key = match self.kind {
            VideoKind::Camera => LOCAL_CAMERA,
            VideoKind::Screen => LOCAL_SCREEN,
        };
        crate::quality::forget_published(key);
        video::forget(key);
    }

    /// Stop sending frames.
    fn halt(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// A shared screen's sound is sent at a music bitrate, with no gaps for
/// silence (DTX cuts quiet passages of music) and no redundancy copies.
const SCREEN_AUDIO_BITRATE: u64 = 128_000;

impl ScreenSound {
    async fn publish(
        room: &Room,
        mode: AudioMode,
        encrypted: bool,
    ) -> std::result::Result<Self, String> {
        // Music and films as they are: no echo cancelling, gain or noise
        // suppression. No queue: the capture's own clock paces it.
        let source = NativeAudioSource::new(
            AudioSourceOptions {
                echo_cancellation: false,
                noise_suppression: false,
                auto_gain_control: false,
            },
            system_audio::RATE,
            system_audio::CHANNELS as u32,
            0,
        );
        let feeding = source.clone();
        let capture = system_audio::start(
            mode,
            Box::new(move |frame: &[i16]| {
                let _ = feeding
                    .capture_frame(&AudioFrame {
                        data: Cow::Borrowed(frame),
                        sample_rate: system_audio::RATE,
                        num_channels: system_audio::CHANNELS as u32,
                        samples_per_channel: system_audio::FRAME as u32,
                    })
                    .now_or_never();
            }),
        )?;
        let track =
            LocalAudioTrack::create_audio_track("screen-audio", RtcAudioSource::Native(source));
        // Encrypted calls: silent until the encryptor sits on the track.
        if encrypted {
            track.mute();
        }
        let publication = room
            .local_participant()
            .publish_track(
                LocalTrack::Audio(track.clone()),
                TrackPublishOptions {
                    source: TrackSource::ScreenshareAudio,
                    dtx: false,
                    red: false,
                    stereo: true,
                    audio_encoding: Some(AudioEncoding {
                        max_bitrate: SCREEN_AUDIO_BITRATE,
                    }),
                    ..Default::default()
                },
            )
            .await
            .map_err(|err| err.to_string())?;
        if encrypted {
            let key = (room.local_participant().identity(), publication.sid());
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !room.e2ee_manager().frame_cryptors().contains_key(&key) {
                if std::time::Instant::now() > deadline {
                    let _ = room.local_participant().unpublish_track(&key.1).await;
                    return Err("its encryption did not start".into());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            track.unmute();
        }
        let sent = format!(
            "{} Hz stereo bitrate={SCREEN_AUDIO_BITRATE} dtx=false",
            system_audio::RATE
        );
        info!(track = %publication.sid(), %sent, "screen sound published");
        crate::screen_audio::note_sent(Some(Ok(sent)));
        Ok(Self {
            track,
            _capture: capture,
        })
    }
}

/// How loud WebRTC plays a screen's sound (its measured level, before this,
/// is scaled by the volume where it is reported).
fn apply_screen_volume(track: &RemoteAudioTrack, volume: f32) {
    track.rtc_track().set_volume(f64::from(volume));
}

/// Play and measure the sound of `who`'s shared screen until it ends: its
/// level (at the volume it is played at) and pitch for the interface and
/// tests, and what it puts out of the speakers for the echo canceller.
async fn watch_screen_audio(
    track: RemoteAudioTrack,
    who: String,
    echo: bool,
    deafened: Arc<Mutex<bool>>,
) {
    use system_audio::{CHANNELS, FRAME};
    let mut stream = NativeAudioStream::new(
        track.rtc_track(),
        system_audio::RATE as i32,
        CHANNELS as i32,
    );
    let far = format!("{who} (screen)");
    // A tenth of a second at a time.
    let mut window: Vec<i16> = Vec::with_capacity(10 * FRAME * CHANNELS);
    let mut mono = Vec::with_capacity(FRAME);
    // What WebRTC says it plays at, asked every fifth window.
    let mut played = 0f32;
    let mut windows = 0u32;
    loop {
        // Deafened, WebRTC plays it silent (after this sink: the samples
        // still come).
        let volume = if deafened.lock().is_ok_and(|d| *d) {
            0.0
        } else {
            crate::screen_volume(&who)
        };
        match tokio::time::timeout(Duration::from_millis(300), stream.next()).await {
            Ok(Some(frame)) => {
                if echo {
                    mono.clear();
                    mono.extend(frame.data.chunks_exact(CHANNELS).map(|pair| {
                        let sum: i32 = pair.iter().map(|s| i32::from(*s)).sum();
                        ((sum as f32 / CHANNELS as f32) * volume)
                            .clamp(f32::from(i16::MIN), f32::from(i16::MAX))
                            as i16
                    }));
                    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
                    mic::far_end(&far, &mono);
                }
                window.extend_from_slice(&frame.data);
                if window.len() < 10 * FRAME * CHANNELS {
                    continue;
                }
                windows += 1;
                if windows % 5 == 1
                    && let Ok(stats) = track.get_stats().await
                {
                    use livekit::webrtc::stats::RtcStats;
                    if let Some(level) = stats.iter().find_map(|s| match s {
                        RtcStats::InboundRtp(i) if i.stream.kind == "audio" => {
                            Some(i.inbound.audio_level as f32)
                        }
                        _ => None,
                    }) {
                        played = level;
                    }
                }
                crate::screen_audio::note_heard(
                    &who,
                    crate::ScreenAudio {
                        level: rms(&window) / 32_768.0 * volume,
                        left_hz: system_audio::zero_crossing_hz(&window, CHANNELS, 0),
                        right_hz: system_audio::zero_crossing_hz(&window, CHANNELS, 1),
                        volume,
                        played,
                    },
                );
                window.clear();
            }
            Ok(None) => break,
            // Nothing arriving (deafened, or the sharer's computer is quiet).
            Err(_) => {
                window.clear();
                crate::screen_audio::note_heard(
                    &who,
                    crate::ScreenAudio {
                        level: 0.0,
                        left_hz: 0.0,
                        right_hz: 0.0,
                        volume,
                        played: 0.0,
                    },
                );
            }
        }
    }
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
    mic::far_end_gone(&far);
    let _ = (far, mono, echo);
    crate::screen_audio::forget_heard(&who);
}

/// A 440 Hz tone at a speaking level, in real time.
#[cfg_attr(
    any(target_os = "linux", target_os = "windows", target_os = "macos"),
    allow(dead_code)
)]
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
    source: Source,
) {
    // Everyone in the call is on this device's own SFU too (to listen, if
    // not to publish): who comes and goes is said from there alone, once.
    let own = source == Source::Own;
    let mut voice_tasks: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();
    let mut measure_tasks: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();
    while let Some(event) = events.recv().await {
        // What happens in a call goes to the log: when someone cannot be
        // heard or seen, it says whether they arrived, sent anything, and
        // whether it decrypted.
        log_event(&event, &source);
        let out = match event {
            RoomEvent::ParticipantConnected(p) => {
                info!(who = %p.identity(), ?source, "joined the call");
                if !own {
                    continue;
                }
                CallEvent::Joined(p.identity().to_string())
            }
            RoomEvent::ParticipantDisconnected(p) => {
                let identity = p.identity().to_string();
                info!(who = %identity, ?source, "left the call");
                if let Some(task) = voice_tasks.remove(&identity) {
                    task.abort();
                }
                if let Some(task) = measure_tasks.remove(&identity) {
                    task.abort();
                }
                #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
                mic::far_end_gone(&identity);
                let _ = speech.send(Speech::Gone(identity.clone()));
                if !own {
                    continue;
                }
                CallEvent::Left(identity)
            }
            RoomEvent::E2eeStateChanged { participant, state } => {
                info!(who = %participant.identity(), ?state, "call encryption");
                continue;
            }
            RoomEvent::ActiveSpeakersChanged { speakers } => {
                let _ = speech.send(Speech::Server(
                    source.clone(),
                    speakers.iter().map(|p| p.identity().to_string()).collect(),
                ));
                continue;
            }
            RoomEvent::TrackMuted {
                participant: Participant::Remote(p),
                publication,
            } if publication.source() == TrackSource::Microphone => {
                let _ = speech.send(Speech::Muted(p.identity().to_string(), true));
                continue;
            }
            RoomEvent::TrackUnmuted {
                participant: Participant::Remote(p),
                publication,
            } if publication.source() == TrackSource::Microphone => {
                let _ = speech.send(Speech::Muted(p.identity().to_string(), false));
                continue;
            }
            // A shared screen's sound: not their voice (it does not make them
            // speak), played at its own volume.
            RoomEvent::TrackSubscribed {
                track: RemoteTrack::Audio(track),
                participant,
                publication,
            } if publication.source() == TrackSource::ScreenshareAudio => {
                let identity = participant.identity().to_string();
                info!(who = %identity, track = %track.sid(), "screen sound arriving");
                if deafened.lock().is_ok_and(|d| *d) {
                    track.disable();
                }
                apply_screen_volume(&track, crate::screen_volume(&identity));
                tokio::spawn(watch_screen_audio(
                    track,
                    identity.clone(),
                    mode == AudioMode::Platform,
                    deafened.clone(),
                ));
                CallEvent::ScreenAudio { identity, on: true }
            }
            RoomEvent::TrackUnsubscribed {
                track: RemoteTrack::Audio(_),
                participant,
                publication,
            } => {
                let identity = participant.identity().to_string();
                if publication.source() == TrackSource::ScreenshareAudio {
                    crate::screen_audio::forget_heard(&identity);
                    CallEvent::ScreenAudio {
                        identity,
                        on: false,
                    }
                } else {
                    if let Some(task) = voice_tasks.remove(&identity) {
                        task.abort();
                    }
                    if let Some(task) = measure_tasks.remove(&identity) {
                        task.abort();
                    }
                    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
                    mic::far_end_gone(&identity);
                    let _ = speech.send(Speech::Unmeasured(identity));
                    continue;
                }
            }
            RoomEvent::TrackSubscribed {
                track: RemoteTrack::Audio(track),
                participant,
                ..
            } => {
                let identity = participant.identity().to_string();
                info!(who = %identity, track = %track.sid(), "audio arriving");
                if deafened.lock().is_ok_and(|d| *d) {
                    track.disable();
                } else {
                    track.enable();
                }
                track
                    .rtc_track()
                    .set_volume(f64::from(crate::voice_gain(&identity)));
                if let Some(prev) = voice_tasks.remove(&identity) {
                    prev.abort();
                }
                voice_tasks.insert(
                    identity.clone(),
                    tokio::spawn(watch_voice(
                        track.clone(),
                        identity.clone(),
                        speech.clone(),
                        mode == AudioMode::Platform,
                    )),
                );
                if mode == AudioMode::Tone {
                    if let Some(prev) = measure_tasks.remove(&identity) {
                        prev.abort();
                    }
                    measure_tasks.insert(
                        identity.clone(),
                        tokio::spawn(measure(track, identity, tx.clone())),
                    );
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
                tokio::spawn(watch_decoder(track.clone(), identity.clone()));
                tokio::spawn(video::show_remote(
                    track,
                    key.clone(),
                    publication.source() == TrackSource::Screenshare,
                ));
                // The top layer, if it is the sort of track that has layers: the
                // pictures are drawn large.
                publication.set_video_quality(livekit::track::VideoQuality::High);
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
            // Another SFU dropping is not the call ending: it is connected again.
            RoomEvent::Disconnected { reason } if !own => {
                warn!(?source, ?reason, "another SFU dropped");
                continue;
            }
            RoomEvent::Disconnected { reason } => CallEvent::Disconnected(format!("{reason:?}")),
            _ => continue,
        };
        if tx.send(out).is_err() {
            return;
        }
    }
}

/// A short name for a LiveKit identity or room in the log, the same in
/// every run and the same as `morpheus_core::short_id` gives (FNV-1a): a
/// log can be shared without the names in it and still say who is who.
pub fn short_id(name: &str) -> String {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in name.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    format!("{:06x}", hash & 0x00ff_ffff)
}

/// Which SFU connection something came over, for the log.
fn sfu_name(source: &Source) -> Cow<'_, str> {
    match source {
        Source::Own => Cow::Borrowed("own"),
        Source::Other(service) => {
            let rest = service.split_once("://").map_or(service.as_str(), |(_, rest)| rest);
            Cow::Borrowed(rest.split('/').next().unwrap_or(rest))
        }
    }
}

/// A remote participant's tracks, for the log: source, codec, muted, sid.
fn tracks_of(participant: &RemoteParticipant) -> Vec<String> {
    participant
        .track_publications()
        .values()
        .map(|p| {
            format!(
                "{:?}/{}{}{}",
                p.source(),
                p.mime_type(),
                if p.is_muted() { "/muted" } else { "" },
                if p.is_subscribed() { "/subscribed" } else { "" },
            )
        })
        .collect()
}

/// Everything about the others' tracks that decides whether they are heard
/// and seen, at INFO: who connects and leaves on which SFU (and what they
/// already publish), each track published, subscribed or failing to be,
/// its codec, and each change of a participant's media encryption.
fn log_event(event: &RoomEvent, source: &Source) {
    let sfu = sfu_name(source);
    match event {
        RoomEvent::ParticipantConnected(p) => info!(
            who = %short_id(p.identity().as_str()),
            %sfu,
            can_publish = ?p.permission().map(|perm| perm.can_publish),
            tracks = ?tracks_of(p),
            "participant connected"
        ),
        RoomEvent::ParticipantDisconnected(p) => info!(
            who = %short_id(p.identity().as_str()),
            %sfu,
            "participant disconnected"
        ),
        RoomEvent::ParticipantPermissionChanged { participant, permission } => info!(
            who = %short_id(participant.identity().as_str()),
            %sfu,
            can_publish = ?permission.as_ref().map(|perm| perm.can_publish),
            "participant permission changed"
        ),
        RoomEvent::TrackPublished { publication, participant } => info!(
            who = %short_id(participant.identity().as_str()),
            %sfu,
            source = ?publication.source(),
            codec = %publication.mime_type(),
            track = %publication.sid(),
            encryption = ?publication.encryption_type(),
            "remote track published"
        ),
        RoomEvent::TrackUnpublished { publication, participant } => info!(
            who = %short_id(participant.identity().as_str()),
            %sfu,
            source = ?publication.source(),
            track = %publication.sid(),
            "remote track unpublished"
        ),
        RoomEvent::TrackSubscribed { publication, participant, .. } => info!(
            who = %short_id(participant.identity().as_str()),
            %sfu,
            source = ?publication.source(),
            codec = %publication.mime_type(),
            track = %publication.sid(),
            encryption = ?publication.encryption_type(),
            "remote track subscribed"
        ),
        RoomEvent::TrackUnsubscribed { publication, participant, .. } => info!(
            who = %short_id(participant.identity().as_str()),
            %sfu,
            source = ?publication.source(),
            track = %publication.sid(),
            "remote track unsubscribed"
        ),
        RoomEvent::TrackSubscriptionFailed { participant, error, track_sid } => warn!(
            who = %short_id(participant.identity().as_str()),
            %sfu,
            track = %track_sid,
            %error,
            "remote track could not be subscribed"
        ),
        RoomEvent::E2eeStateChanged { participant, state } => {
            let tracks = match participant {
                Participant::Remote(p) => tracks_of(p),
                Participant::Local(_) => Vec::new(),
            };
            info!(
                who = %short_id(participant.identity().as_str()),
                %sfu,
                ?state,
                ?tracks,
                "media encryption state changed"
            );
        }
        _ => {}
    }
}

/// What is in a room just connected to, for the log: this device's name
/// and whether it may publish, and everyone already there with their
/// tracks (they come with no "connected" event).
fn log_connected(room: &Room, source: &Source) {
    let sfu = sfu_name(source);
    let me = room.local_participant();
    info!(
        %sfu,
        me = %short_id(me.identity().as_str()),
        livekit_room = %short_id(room.name().as_str()),
        can_publish = ?me.permission().map(|perm| perm.can_publish),
        others = room.remote_participants().len(),
        "connected to an SFU"
    );
    for p in room.remote_participants().values() {
        info!(
            who = %short_id(p.identity().as_str()),
            %sfu,
            can_publish = ?p.permission().map(|perm| perm.can_publish),
            tracks = ?tracks_of(p),
            "participant already in the call"
        );
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
    /// A server's list (the fallback), from the connection it came on.
    Server(Source, Vec<String>),
    /// A measured state (and level, to order by) for `who`.
    Measured {
        who: String,
        speaking: bool,
        level: f32,
    },
    /// `who` is no longer measured (no frames): the server decides again.
    Unmeasured(String),
    /// `who` muted their microphone (or not): muted is never speaking.
    Muted(String, bool),
    /// `who` left.
    Gone(String),
}

/// Merge everything into [`CallEvent::Speaking`], sent only when the set
/// changes.
async fn aggregate(mut input: UnboundedReceiver<Speech>, out: UnboundedSender<CallEvent>) {
    use std::collections::{HashMap, HashSet};
    let mut measured: HashMap<String, (bool, f32)> = HashMap::new();
    let mut servers: HashMap<Source, Vec<String>> = HashMap::new();
    let mut muted: HashSet<String> = HashSet::new();
    let mut last: Vec<String> = Vec::new();
    while let Some(item) = input.recv().await {
        match item {
            Speech::Server(source, list) => {
                servers.insert(source, list);
            }
            Speech::Measured {
                who,
                speaking,
                level,
            } => {
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
                for server in servers.values_mut() {
                    server.retain(|s| *s != who);
                }
            }
        }
        let mut server: Vec<String> = servers.values().flatten().cloned().collect();
        server.sort();
        server.dedup();
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
///
/// `echo`: what they say also goes to the echo canceller, as the speakers
/// play it (a deafened track carries no frames, so nothing then).
async fn watch_voice(
    track: RemoteAudioTrack,
    who: String,
    speech: UnboundedSender<Speech>,
    echo: bool,
) {
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
                #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
                if echo {
                    mic::far_end(&who, &frame.data);
                }
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
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
    mic::far_end_gone(&who);
    let _ = echo;
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
            RtcStats::MediaSource(m) if m.source.kind == "audio" => {
                Some(m.audio.audio_level as f32)
            }
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

/// Measure the call's connection for the diagnostics (`crate::diag`): its
/// state, and from the microphone track's sender stats the round trip, loss,
/// jitter and what our voice is sent at.
async fn watch_connection(room: Arc<Room>, mic: LocalAudioTrack) {
    use livekit::webrtc::stats::RtcStats;
    let mut last_sent: Option<(u64, std::time::Instant)> = None;
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let state = format!("{:?}", room.connection_state()).to_lowercase();
        crate::diag::update(|d| d.state = state);
        let Ok(stats) = mic.get_stats().await else {
            continue;
        };
        let mut rtt = None;
        let (mut loss, mut jitter, mut sent) = (None, None, None);
        for stat in stats {
            match stat {
                RtcStats::CandidatePair(pair) if pair.candidate_pair.current_round_trip_time > 0.0 => {
                    rtt = Some(pair.candidate_pair.current_round_trip_time * 1000.0);
                }
                RtcStats::RemoteInboundRtp(r) if r.stream.kind == "audio" => {
                    loss = Some(r.remote_inbound.fraction_lost * 100.0);
                    jitter = Some(r.received.jitter * 1000.0);
                    if rtt.is_none() && r.remote_inbound.round_trip_time > 0.0 {
                        rtt = Some(r.remote_inbound.round_trip_time * 1000.0);
                    }
                }
                RtcStats::OutboundRtp(o) if o.stream.kind == "audio" => {
                    sent = Some(o.sent.bytes_sent);
                }
                _ => {}
            }
        }
        let now = std::time::Instant::now();
        let bitrate = match (sent, last_sent) {
            (Some(bytes), Some((before, at))) if bytes >= before => {
                let secs = now.duration_since(at).as_secs_f64();
                (secs > 0.0).then(|| (bytes - before) as f64 * 8.0 / secs)
            }
            _ => None,
        };
        if let Some(bytes) = sent {
            last_sent = Some((bytes, now));
        }
        crate::diag::update(|d| {
            d.rtt_ms = rtt.or(d.rtt_ms);
            d.loss_percent = loss.or(d.loss_percent);
            d.jitter_ms = jitter.or(d.jitter_ms);
            d.audio_bitrate = bitrate.or(d.audio_bitrate);
        });
    }
}

/// Media keys as Element Call (livekit-client's `MatrixKeyProvider`) uses them.
#[cfg(test)]
#[allow(clippy::expect_used)]
mod element_call_keys {
    use super::*;

    /// A key ratcheted once: what livekit-client's `ratchet` gives for a key
    /// Element Call imported (HKDF-SHA256, salt `LKFrameEncryptionKey`, 128
    /// zero bytes of info, 256 bits). The same derivation makes the frame
    /// key, so equal ratchets mean both sides encrypt with the same key.
    fn browser_ratchet(key: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; 32];
        hkdf::Hkdf::<sha2::Sha256>::new(Some(b"LKFrameEncryptionKey"), key)
            .expand(&[0u8; 128], &mut out)
            .expect("hkdf");
        out
    }

    #[test]
    fn short_ids_are_the_cores() {
        // `morpheus_core::short_id` pins the same value.
        assert_eq!(short_id("@me:localhost:DEV"), "8eae53");
    }

    #[test]
    fn keys_derive_as_in_element_call() {
        let keys = KeyProvider::new(key_options());
        let who = ParticipantIdentity("@ellie:localhost:ELEMENTDEV".into());
        let key: Vec<u8> = (0u8..16).collect();
        keys.set_key(&who, 0, key.clone());
        assert_eq!(keys.ratchet_key(&who, 0), Some(browser_ratchet(&key)));
    }

    #[test]
    fn every_key_index_element_call_uses_has_a_place() {
        // Element Call's keys go round 0..=255 (`keyringSize: 256`); a
        // frame naming an index past the ring is not decrypted.
        assert_eq!(key_options().key_ring_size, 256);
        assert_eq!(key_options().ratchet_window_size, 10);
    }
}

/// The media key settings, Element Call's (livekit-client's
/// `MatrixKeyProvider`): a key is HKDF material (the browser imports it so),
/// ratcheted up to 10 times when frames stop decrypting, in a ring of 256
/// (its key indices go round 0..=255). LiveKit's Rust default, PBKDF2 in a
/// ring of 16, decrypts nothing of Element Call's.
fn key_options() -> KeyProviderOptions {
    KeyProviderOptions {
        ratchet_window_size: 10,
        key_ring_size: 256,
        key_derivation_algorithm: livekit::e2ee::key_provider::KeyDerivationAlgorithm::HKDF,
        ..KeyProviderOptions::default()
    }
}
