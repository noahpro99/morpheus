#![allow(clippy::manual_async_fn)]
// The browser: the same interface as `native.rs`, with no calls yet (the
// browser's own WebRTC, through LiveKit's JavaScript client, is to come).
// Joining says so; there are no devices to list.

use std::{
    future::Future,
    sync::{Arc, Mutex},
};

use tokio::sync::mpsc::UnboundedReceiver;

/// Where the call's audio comes from and goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioMode {
    Platform,
    Tone,
}

impl AudioMode {
    pub fn from_env() -> Self {
        Self::Platform
    }
}

pub fn can_send_video() -> bool {
    false
}

/// No hardware video encoder in a browser build.
pub fn hardware_encoder() -> Option<String> {
    None
}

pub fn can_share_screen() -> bool {
    false
}

pub fn use_test_tone() {}

/// Something that happened in the call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallEvent {
    Joined(String),
    Left(String),
    Speaking(Vec<String>),
    Heard(String),
    Quiet(String),
    Disconnected(String),
    Video {
        identity: String,
        key: String,
        screen: bool,
        on: bool,
    },
    ScreenAudio {
        identity: String,
        on: bool,
    },
    Screen {
        identity: String,
        on: bool,
    },
    Watchers(usize),
    Thumbnail {
        identity: String,
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
}

pub type Result<T> = std::result::Result<T, Error>;

/// What joining says in a browser.
const NOT_YET: &str = "calls are not in the web app yet: use Morpheus Desktop";

/// A call (never connected in a browser).
pub struct Call {
    _never: (),
}

impl Call {
    pub async fn connect(
        _url: &str,
        _token: &str,
        _mode: AudioMode,
    ) -> Result<(Self, UnboundedReceiver<CallEvent>)> {
        Err(Error::Connect(NOT_YET.into()))
    }

    pub async fn connect_encrypted(
        _url: &str,
        _token: &str,
        _mode: AudioMode,
        _own_key: Vec<u8>,
    ) -> Result<(Self, UnboundedReceiver<CallEvent>)> {
        Err(Error::Connect(NOT_YET.into()))
    }

    pub fn participants(&self) -> Vec<String> {
        Vec::new()
    }

    pub fn identity(&self) -> String {
        String::new()
    }

    pub fn set_key(&self, _identity: &str, _index: u32, _key: Vec<u8>) {}

    pub fn use_chosen_devices(&self) -> Result<()> {
        Ok(())
    }

    pub fn set_muted(&self, _muted: bool) {}

    pub fn set_deafened(&self, _deafened: bool) {}

    pub fn set_output_volume(&self, _volume: f32) {}

    pub fn set_user_volume(&self, user: &str, volume: f32) {
        crate::set_user_volume(user, volume);
    }

    pub fn set_screen_volume(&self, identity: &str, volume: f32) {
        let _ = (identity, volume);
    }

    pub fn camera_on(&self) -> bool {
        false
    }

    pub fn sharing_screen(&self) -> bool {
        false
    }

    pub fn set_camera(&self, _on: bool) -> impl Future<Output = Result<()>> + 'static {
        async { Err(Error::Video(NOT_YET.into())) }
    }

    pub fn set_screen_share_with(
        &self,
        _on: bool,
        _quality: crate::ScreenQuality,
    ) -> impl Future<Output = Result<()>> + 'static {
        async { Err(Error::Video(NOT_YET.into())) }
    }

    pub fn set_screen_share(&self, _on: bool) -> impl Future<Output = Result<()>> + 'static {
        async { Err(Error::Video(NOT_YET.into())) }
    }

    pub async fn leave(self) {}
}

/// A microphone or speaker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioDevices {
    pub microphones: Vec<AudioDevice>,
    pub speakers: Vec<AudioDevice>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceChoice {
    pub microphone: Option<String>,
    pub speaker: Option<String>,
}

static CHOICE: Mutex<DeviceChoice> = Mutex::new(DeviceChoice {
    microphone: None,
    speaker: None,
});
static CAMERA: Mutex<Option<String>> = Mutex::new(None);

pub async fn audio_devices(_mode: AudioMode) -> AudioDevices {
    AudioDevices::default()
}

pub fn choose_audio_devices(choice: DeviceChoice) {
    if let Ok(mut chosen) = CHOICE.lock() {
        *chosen = choice;
    }
}

pub fn chosen_audio_devices() -> DeviceChoice {
    CHOICE.lock().map(|c| c.clone()).unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraDevice {
    pub id: String,
    pub name: String,
}

pub fn cameras(_mode: AudioMode) -> Vec<CameraDevice> {
    Vec::new()
}

pub fn choose_camera(id: Option<String>) {
    if let Ok(mut chosen) = CAMERA.lock() {
        *chosen = id;
    }
}

pub fn chosen_camera() -> Option<String> {
    CAMERA.lock().ok().and_then(|chosen| chosen.clone())
}

/// Starting and stopping the app's camera (phones).
pub struct PlatformCamera {
    pub start: Box<dyn Fn() -> bool + Send + Sync>,
    pub stop: Box<dyn Fn() + Send + Sync>,
}

pub fn set_platform_camera(_camera: PlatformCamera) {}

#[allow(clippy::too_many_arguments)]
pub fn camera_frame(
    _y: &[u8],
    _u: &[u8],
    _v: &[u8],
    _y_stride: usize,
    _uv_stride: usize,
    _uv_pixel_stride: usize,
    _width: u32,
    _height: u32,
    _rotation: u32,
) {
}

pub const LOCAL_CAMERA: &str = "local/camera";
pub const LOCAL_SCREEN: &str = "local/screen";

/// One picture, ready to show.
#[derive(Debug, Clone)]
pub struct Frame {
    pub seq: u64,
    pub jpeg: Arc<Vec<u8>>,
    pub bgra: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
}

pub fn skip_jpeg_frames() {}

pub fn latest_frame(_key: &str) -> Option<Frame> {
    None
}

/// A test run's stand-in for a camera or a screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TestPattern {
    pub width: u32,
    pub height: u32,
    pub key: &'static str,
}

pub const CAMERA_PATTERN: TestPattern = TestPattern {
    width: 1280,
    height: 720,
    key: LOCAL_CAMERA,
};

pub const SCREEN_PATTERN: TestPattern = TestPattern {
    width: 2560,
    height: 1440,
    key: LOCAL_SCREEN,
};

/// Why this platform cannot share its sound.
pub fn screen_audio_unavailable() -> Option<&'static str> {
    Some("Sharing sound isn't available here")
}

pub const SCREEN_TONE_LEFT: f32 = 660.0;
pub const SCREEN_TONE_RIGHT: f32 = 990.0;

/// No hardware video decoder in a browser build.
pub fn hardware_decoder() -> String {
    "none: the browser decodes video".to_owned()
}

/// The small picture of a shared screen (none in a browser yet).
pub fn share_thumbnail(_identity: &str) -> Option<crate::watch::Thumb> {
    None
}
