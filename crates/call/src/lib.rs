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
//!
//! A browser (apps/web) has no native WebRTC: there the same interface says
//! calls are not available yet (they will go through the browser's own).

#[cfg(not(target_arch = "wasm32"))]
#[path = "native.rs"]
mod imp;
#[cfg(target_arch = "wasm32")]
#[path = "web.rs"]
mod imp;

pub use imp::*;

#[path = "noise.rs"]
#[cfg_attr(
    not(any(target_os = "linux", target_os = "windows", target_os = "macos")),
    allow(dead_code)
)]
mod noise;
pub use noise::{
    NoiseSuppression, applied_noise_suppression, noise_suppression, set_noise_suppression,
};

#[path = "volume.rs"]
mod volume;
pub use volume::{
    MAX_VOLUME, input_volume, output_volume, set_input_volume, set_output_volume,
};

#[path = "quality.rs"]
mod quality;
pub use quality::{
    PublishedVideo, SCREEN_FPS, ScreenQuality, ScreenResolution, bitrate_for, camera_bitrate,
    published_video,
};

#[path = "screen_audio.rs"]
mod screen_audio;
pub use screen_audio::{
    MAX_SCREEN_VOLUME, ScreenAudio, screen_audio, screen_volume, sent_screen_audio,
};
