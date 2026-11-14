//! The sound of others' shared screens, as this device receives it: a track
//! of its own beside each sharer's voice, played at a volume of its own
//! (Discord's stream volume), and what it sounds like here for the interface
//! and the tests. Plain data, so every platform and the browser build have
//! it.

use std::{collections::HashMap, sync::Mutex};

/// What a shared screen sounds like here.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenAudio {
    /// How loud it plays (RMS, 0 to 1, at the volume it is given; 0 while
    /// deafened).
    pub level: f32,
    /// The pitch of each side, by its zero crossings (test tones).
    pub left_hz: f32,
    pub right_hz: f32,
    /// The volume it is played at (1 is as sent).
    pub volume: f32,
    /// The level WebRTC plays it at, from its own figures (after the
    /// volume; 0 to 1, updated twice a second; 0 where nothing plays).
    pub played: f32,
}

static HEARD: Mutex<Option<HashMap<String, ScreenAudio>>> = Mutex::new(None);
static VOLUMES: Mutex<Option<HashMap<String, f32>>> = Mutex::new(None);
static SENT: Mutex<Option<Result<String, String>>> = Mutex::new(None);

/// What the screen `identity` shares sounds like here, while it has sound.
pub fn screen_audio(identity: &str) -> Option<ScreenAudio> {
    HEARD.lock().ok()?.as_ref()?.get(identity).cloned()
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn note_heard(identity: &str, heard: ScreenAudio) {
    if let Ok(mut all) = HEARD.lock() {
        all.get_or_insert_with(HashMap::new)
            .insert(identity.to_owned(), heard);
    }
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn forget_heard(identity: &str) {
    if let Ok(mut all) = HEARD.lock()
        && let Some(all) = all.as_mut()
    {
        all.remove(identity);
    }
}

/// The volume `identity`'s screen sound is played at (1 unless changed).
pub fn screen_volume(identity: &str) -> f32 {
    VOLUMES
        .lock()
        .ok()
        .and_then(|all| all.as_ref()?.get(identity).copied())
        .unwrap_or(1.0)
}

/// Most a screen's sound can be turned up: twice as loud as sent.
pub const MAX_SCREEN_VOLUME: f32 = 2.0;

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn note_volume(identity: &str, volume: f32) -> f32 {
    let volume = volume.clamp(0.0, MAX_SCREEN_VOLUME);
    if let Ok(mut all) = VOLUMES.lock() {
        all.get_or_insert_with(HashMap::new)
            .insert(identity.to_owned(), volume);
    }
    volume
}

/// This device's own share's sound: how it is sent ("48000 Hz stereo
/// bitrate=128000 dtx=false"), why it could not be, or nothing (no share,
/// or one without sound).
pub fn sent_screen_audio() -> Option<Result<String, String>> {
    SENT.lock().ok()?.clone()
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn note_sent(sent: Option<Result<String, String>>) {
    if let Ok(mut now) = SENT.lock() {
        *now = sent;
    }
}
