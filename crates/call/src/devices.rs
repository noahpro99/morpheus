//! Which microphone and speaker calls use. Desktop systems list their
//! devices; phones pick for themselves (the lists come back empty).
//! [`AudioMode::Tone`] lists two stand-ins of each, so tests can choose.
//! PulseAudio's own "default: …" entries are left out: the pickers offer the
//! system default themselves.

use std::sync::Mutex;

use livekit::prelude::{PlatformAudio, PlayoutDeviceId, RecordingDeviceId};
use tracing::warn;

use crate::{AudioMode, Error, Result, runtime};

/// A microphone or speaker: a stable id to save, and a name to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
}

/// The devices this machine has.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioDevices {
    pub microphones: Vec<AudioDevice>,
    pub speakers: Vec<AudioDevice>,
}

/// The chosen devices (`None`: the system's default).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceChoice {
    pub microphone: Option<String>,
    pub speaker: Option<String>,
}

static CHOICE: Mutex<DeviceChoice> = Mutex::new(DeviceChoice {
    microphone: None,
    speaker: None,
});

fn stand_ins(kind: &str) -> Vec<AudioDevice> {
    (1..=2)
        .map(|n| AudioDevice {
            id: format!("test-{kind}-{n}"),
            name: format!("Test {kind} {n}"),
        })
        .collect()
}

/// The microphones and speakers calls can use. Empty where the system
/// chooses (Android, iOS) or there is no audio hardware.
pub async fn audio_devices(mode: AudioMode) -> AudioDevices {
    if mode == AudioMode::Tone {
        return AudioDevices {
            microphones: stand_ins("microphone"),
            speakers: stand_ins("speaker"),
        };
    }
    if cfg!(any(target_os = "android", target_os = "ios")) {
        return AudioDevices::default();
    }
    runtime()
        .spawn_blocking(|| match PlatformAudio::new() {
            Ok(audio) => AudioDevices {
                microphones: audio
                    .recording_devices()
                    .filter(|d| !d.name.starts_with("default:"))
                    .map(|d| AudioDevice {
                        id: d.id.as_str().to_owned(),
                        name: d.name,
                    })
                    .collect(),
                speakers: audio
                    .playout_devices()
                    .filter(|d| !d.name.starts_with("default:"))
                    .map(|d| AudioDevice {
                        id: d.id.as_str().to_owned(),
                        name: d.name,
                    })
                    .collect(),
            },
            Err(err) => {
                warn!(%err, "no audio devices");
                AudioDevices::default()
            }
        })
        .await
        .unwrap_or_default()
}

/// Calls use these devices from now on. A call already going switches over
/// with [`crate::Call::use_chosen_devices`].
pub fn choose_audio_devices(choice: DeviceChoice) {
    if let Ok(mut chosen) = CHOICE.lock() {
        *chosen = choice;
    }
}

/// The devices calls use (as last chosen).
pub fn chosen_audio_devices() -> DeviceChoice {
    CHOICE.lock().map(|c| c.clone()).unwrap_or_default()
}

/// Point the platform audio at the chosen devices: `live` while it is
/// running (stop, change, restart), else before it starts. A device that has
/// gone away leaves the system default.
pub(crate) fn apply(platform: &PlatformAudio, live: bool) -> Result<()> {
    let choice = chosen_audio_devices();
    if let Some(id) = choice.microphone {
        let id = RecordingDeviceId::from_unchecked_guid(&id);
        let result = if live {
            platform.switch_recording_device(&id)
        } else {
            platform.set_recording_device(&id)
        };
        result.map_err(|err| Error::Audio(format!("microphone: {err}")))?;
    }
    if let Some(id) = choice.speaker {
        let id = PlayoutDeviceId::from_unchecked_guid(&id);
        let result = if live {
            platform.switch_playout_device(&id)
        } else {
            platform.set_playout_device(&id)
        };
        result.map_err(|err| Error::Audio(format!("speaker: {err}")))?;
    }
    Ok(())
}
