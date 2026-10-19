//! Call media for Morpheus: a LiveKit room (the MatrixRTC transport; the
//! core gets the URL and token) with this device's microphone in it and
//! everyone else's audio coming out of the speakers.
//!
//! Real calls use the platform's audio devices through WebRTC's own device
//! module, which also cancels echo and suppresses noise. Tests use
//! [`AudioMode::Tone`]: a steady tone instead of the microphone, nothing
//! played, and [`CallEvent::Heard`]/[`CallEvent::Quiet`] reporting what
//! arrives from each participant.
//!
//! Like the core, the crate owns its Tokio runtime, so callers can await its
//! futures from any executor.

use std::{
    borrow::Cow,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use futures_util::StreamExt;
use livekit::{
    options::TrackPublishOptions,
    prelude::*,
    webrtc::{
        audio_frame::AudioFrame,
        audio_source::{AudioSourceOptions, native::NativeAudioSource},
        audio_stream::native::NativeAudioStream,
    },
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tracing::{debug, warn};

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
    /// [`AudioMode::Tone`] when `MORPHEUS_CALL_AUDIO=tone` (test runs), else
    /// the platform's devices.
    pub fn from_env() -> Self {
        match std::env::var("MORPHEUS_CALL_AUDIO").as_deref() {
            Ok("tone") => Self::Tone,
            _ => Self::Platform,
        }
    }
}

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
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not connect to the call: {0}")]
    Connect(String),
    #[error("could not use the microphone: {0}")]
    Audio(String),
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
    mic: LocalAudioTrack,
    deafened: Arc<Mutex<bool>>,
    /// Keeps the platform devices open for the call's lifetime.
    _platform: Option<PlatformAudio>,
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
            .spawn(async move { Self::connect_here(url, token, mode).await })
            .await?
    }

    async fn connect_here(
        url: String,
        token: String,
        mode: AudioMode,
    ) -> Result<(Self, UnboundedReceiver<CallEvent>)> {
        let (room, room_events) = Room::connect(&url, &token, RoomOptions::default())
            .await
            .map_err(|err| Error::Connect(err.to_string()))?;
        let room = Arc::new(room);
        let (tx, rx) = unbounded_channel();
        let mut tasks = Vec::new();

        let (source, platform) = match mode {
            AudioMode::Platform => {
                let platform = PlatformAudio::new().map_err(|err| Error::Audio(err.to_string()))?;
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

        let deafened = Arc::new(Mutex::new(false));
        tasks.push(tokio::spawn(forward_events(
            room_events,
            tx,
            mode,
            deafened.clone(),
        )));
        Ok((
            Self {
                room,
                mic,
                deafened,
                _platform: platform,
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

    /// Stop (or resume) sending this device's audio.
    pub fn set_muted(&self, muted: bool) {
        if muted {
            self.mic.mute();
        } else {
            self.mic.unmute();
        }
    }

    /// Stop (or resume) playing everyone else's audio.
    pub fn set_deafened(&self, deafened: bool) {
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
    mode: AudioMode,
    deafened: Arc<Mutex<bool>>,
) {
    while let Some(event) = events.recv().await {
        let out = match event {
            RoomEvent::ParticipantConnected(p) => CallEvent::Joined(p.identity().to_string()),
            RoomEvent::ParticipantDisconnected(p) => CallEvent::Left(p.identity().to_string()),
            RoomEvent::ActiveSpeakersChanged { speakers } => {
                CallEvent::Speaking(speakers.iter().map(|p| p.identity().to_string()).collect())
            }
            RoomEvent::TrackSubscribed {
                track: RemoteTrack::Audio(track),
                participant,
                ..
            } => {
                if deafened.lock().is_ok_and(|d| *d) {
                    track.disable();
                }
                if mode == AudioMode::Tone {
                    tokio::spawn(measure(
                        track,
                        participant.identity().to_string(),
                        tx.clone(),
                    ));
                }
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

/// Report when audio from `who` starts and stops arriving.
async fn measure(track: RemoteAudioTrack, who: String, tx: UnboundedSender<CallEvent>) {
    let mut stream = NativeAudioStream::new(track.rtc_track(), SAMPLE_RATE as i32, 1);
    let mut hearing = false;
    // Quiet only after a run of quiet frames: one lost packet is not silence.
    let mut quiet_frames = 0u32;
    loop {
        let level = match tokio::time::timeout(Duration::from_millis(300), stream.next()).await {
            Ok(Some(frame)) => rms(&frame.data),
            Ok(None) => return,
            // Nothing at all arriving is quiet too.
            Err(_) => 0.0,
        };
        quiet_frames = if level > 500.0 { 0 } else { quiet_frames + 1 };
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

fn rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    (sum / samples.len() as f64).sqrt() as f32
}
