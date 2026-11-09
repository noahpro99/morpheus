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

/// What a notification on this device gives away (absent: everything).
/// Holds `sender` or `hidden`; the Android push service reads it too.
pub const NOTIFICATION_CONTENT: &str = "notification_content";

/// How much a notification shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NotificationContent {
    /// Who sent what, and where.
    #[default]
    Full,
    /// Only who sent it.
    Sender,
    /// Nothing: "Someone sent a message".
    Hidden,
}

impl NotificationContent {
    pub const ALL: [Self; 3] = [Self::Full, Self::Sender, Self::Hidden];

    /// As kept in the file (and in the Settings radio's value).
    pub fn key(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Sender => "sender",
            Self::Hidden => "hidden",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "Name and message",
            Self::Sender => "Name only",
            Self::Hidden => "Nothing (\u{201c}Someone sent a message\u{201d})",
        }
    }
}

pub fn notification_content(data_dir: &Path) -> NotificationContent {
    let kept = std::fs::read_to_string(data_dir.join(NOTIFICATION_CONTENT)).unwrap_or_default();
    NotificationContent::ALL
        .into_iter()
        .find(|c| c.key() == kept.trim())
        .unwrap_or_default()
}

pub fn set_notification_content(
    data_dir: &Path,
    content: NotificationContent,
) -> std::io::Result<()> {
    let path = data_dir.join(NOTIFICATION_CONTENT);
    if content == NotificationContent::Full {
        match std::fs::remove_file(path) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        }
    } else {
        std::fs::write(path, content.key())
    }
}

/// Where each account on this device was last (`{user: {room, chat}}`), so
/// starting the app again, even after quitting it, opens there.
pub const LAST_PLACE: &str = "last_place.json";

/// The room last open, and on a phone whether its chat was in front.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastPlace {
    pub room: String,
    pub chat: bool,
}

fn last_places(data_dir: &Path) -> serde_json::Map<String, serde_json::Value> {
    std::fs::read(data_dir.join(LAST_PLACE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn last_place(data_dir: &Path, user: &str) -> Option<LastPlace> {
    let places = last_places(data_dir);
    let place = places.get(user)?;
    Some(LastPlace {
        room: place.get("room")?.as_str()?.to_owned(),
        chat: place.get("chat").and_then(|c| c.as_bool()).unwrap_or(false),
    })
}

pub fn set_last_place(data_dir: &Path, user: &str, place: &LastPlace) -> std::io::Result<()> {
    let mut places = last_places(data_dir);
    places.insert(
        user.to_owned(),
        serde_json::json!({ "room": place.room, "chat": place.chat }),
    );
    std::fs::write(
        data_dir.join(LAST_PLACE),
        serde_json::Value::Object(places).to_string(),
    )
}
