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

#[path = "gate.rs"]
#[cfg_attr(
    not(any(target_os = "linux", target_os = "windows", target_os = "macos")),
    allow(dead_code)
)]
mod gate;
pub use gate::{AUTO_MARGIN_DB, AUTO_MIN_DB, MAX_DB as GATE_MAX_DB, MIN_DB as GATE_MIN_DB};

#[path = "noise.rs"]
#[cfg_attr(
    not(any(target_os = "linux", target_os = "windows", target_os = "macos")),
    allow(dead_code)
)]
mod noise;
pub use noise::{
    NoiseModel, NoiseParam, NoiseSwitch, NoiseTuning, applied_noise_tuning, input_sensitivity, noise_tuning, set_noise_tuning,
    NoiseSuppression, applied_noise_suppression, applied_platform_noise_suppression, noise_suppression, set_noise_suppression,
    strong_too_slow,
};

#[path = "volume.rs"]
mod volume;
pub use volume::{
    MAX_VOLUME, TrackSource, input_volume, note_voice_identity, output_volume, set_input_volume,
    set_output_volume, set_user_track_volume, set_user_volume, track_gain, user_track_gain,
    user_track_volume, user_volume, voice_belongs_to, voice_gain, voice_owner,
};

#[cfg(not(target_arch = "wasm32"))]
#[path = "app_activity.rs"]
mod app_activity;
#[cfg(not(target_arch = "wasm32"))]
pub use app_activity::Activity;

#[cfg(not(target_arch = "wasm32"))]
#[path = "trim.rs"]
mod trim;

#[path = "quality.rs"]
mod quality;
pub use quality::{
    PublishedVideo, SCREEN_FPS, ScreenQuality, ScreenResolution, bitrate_for, camera_bitrate,
    codec_backend, hardware_video, hardware_video_allowed, published_video, set_hardware_video,
};

#[path = "screen_audio.rs"]
mod screen_audio;
pub use screen_audio::{
    MAX_SCREEN_VOLUME, ScreenAudio, is_discord_app, screen_audio, screen_audio_of_user, screen_audio_leaves_out_discord,
    screen_volume, sent_screen_audio, set_screen_audio_leaves_out_discord,
};

#[path = "screen_sources.rs"]
mod screen_sources;
pub use screen_sources::{
    ScreenSource, ScreenSourceKind, ScreenSourceProvider, Thumbnail, fit_thumbnail,
    platform_source_provider, shared_screen_source,
};

#[path = "watch.rs"]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
mod watch;

#[path = "diag.rs"]
mod diag;
pub use watch::Thumb;
pub use diag::{CallDiag, OtherSfu, TrackDiag, call_diag, camera_health, camera_info, processing, sent_bytes};

#[path = "noise_model.rs"]
#[cfg_attr(
    not(any(target_os = "linux", target_os = "windows", target_os = "macos")),
    allow(dead_code)
)]
mod noise_model;
pub use noise_model::{
    LOW_DELAY_BYTES, LOW_DELAY_SHA256, LOW_DELAY_URL, low_delay_path, low_delay_url,
    read_low_delay, set_model_dir, sha256_hex,
};
