//! Settings that belong to this device rather than the account, kept as
//! marker files in the data directory so every part of the app (the desktop
//! notifier, the Android push service in Kotlin) can read them cheaply.

use std::path::Path;

/// Present when notifications are off on this device.
pub const NOTIFICATIONS_OFF: &str = "notifications_off";

/// Whether this device shows notifications (on unless turned off).
pub fn notifications_enabled(data_dir: &Path) -> bool {
    !data_dir.join(NOTIFICATIONS_OFF).exists()
}

pub fn set_notifications_enabled(data_dir: &Path, enabled: bool) -> std::io::Result<()> {
    let marker = data_dir.join(NOTIFICATIONS_OFF);
    if enabled {
        match std::fs::remove_file(marker) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        }
    } else {
        std::fs::write(marker, "")
    }
}

/// The microphone and speaker chosen for calls (absent: system defaults).
pub const AUDIO_DEVICES: &str = "audio_devices.json";

pub fn audio_devices(data_dir: &Path) -> morpheus_call::DeviceChoice {
    let saved: serde_json::Value = std::fs::read(data_dir.join(AUDIO_DEVICES))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let field = |name: &str| saved.get(name).and_then(|v| v.as_str()).map(str::to_owned);
    morpheus_call::DeviceChoice {
        microphone: field("microphone"),
        speaker: field("speaker"),
    }
}

pub fn set_audio_devices(
    data_dir: &Path,
    choice: &morpheus_call::DeviceChoice,
) -> std::io::Result<()> {
    let json = serde_json::json!({
        "microphone": choice.microphone,
        "speaker": choice.speaker,
    });
    std::fs::write(data_dir.join(AUDIO_DEVICES), json.to_string())
}
