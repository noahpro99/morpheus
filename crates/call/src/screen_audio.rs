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
    if let Some(user) = crate::voice_owner(identity) {
        return crate::user_track_gain(&user, crate::TrackSource::ScreenShare);
    }
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

/// Settings → Voice & Video → "Leave Discord out of shared sound" (on unless
/// turned off). This app's own sound is always left out (the call's voices
/// would come back to everyone); Discord's only while this says so. Read as
/// the shared sound follows apps starting and stopping, so a change applies
/// to a share going now within a second or so.
static LEAVE_OUT_DISCORD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn set_screen_audio_leaves_out_discord(on: bool) {
    LEAVE_OUT_DISCORD.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn screen_audio_leaves_out_discord() -> bool {
    LEAVE_OUT_DISCORD.load(std::sync::atomic::Ordering::Relaxed)
}

/// Other Discord clients, by the names their programs and bundles go by
/// (Vencord's Vesktop and Equibop, WebCord, ArmCord / Legcord, Dorion).
const DISCORD_CLIENTS: &[&str] = &["vesktop", "equibop", "webcord", "armcord", "legcord", "dorion"];

/// Whether an app, by any name it goes by (its name, its program with or
/// without a folder or `.exe`, a Flatpak id, a macOS bundle id), is Discord
/// or a Discord client: "Discord", "DiscordPTB", "Discord Canary",
/// "C:\…\Discord.exe", "com.discordapp.Discord", "com.hnc.Discord",
/// "dev.vencord.Vesktop", ...
pub fn is_discord_app(name: &str) -> bool {
    let name = name.trim().trim_end_matches('\0');
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).to_lowercase();
    !base.is_empty()
        && (base.contains("discord") || DISCORD_CLIENTS.iter().any(|c| base.contains(c)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discord_and_its_clients_are_known_by_any_of_their_names() {
        for name in [
            "Discord",
            "discord",
            "DiscordPTB",
            "DiscordCanary",
            "Discord Canary",
            "Discord.exe",
            "C:\\Users\\me\\AppData\\Local\\Discord\\app-1.0.9200\\Discord.exe",
            "/opt/discord/Discord",
            "/usr/lib/discord/Discord\0",
            "com.discordapp.Discord",
            "com.hnc.Discord",
            "com.hnc.Discord.helper.Renderer",
            "Vesktop",
            "vesktop.exe",
            "dev.vencord.Vesktop",
            "WebCord",
            "io.github.spacingbat3.webcord",
            "Equibop",
            "legcord",
            "ArmCord",
        ] {
            assert!(is_discord_app(name), "{name:?} is Discord");
        }
        for name in [
            "",
            "morpheus-desktop",
            "Morpheus",
            "firefox",
            "Spotify",
            "chrome.exe",
            "/usr/bin/mpv",
            "com.spotify.client",
            "Chromium",
            "steam",
        ] {
            assert!(!is_discord_app(name), "{name:?} is not Discord");
        }
    }

    #[test]
    fn discord_is_left_out_unless_turned_off() {
        assert!(screen_audio_leaves_out_discord(), "on by default");
        set_screen_audio_leaves_out_discord(false);
        assert!(!screen_audio_leaves_out_discord());
        set_screen_audio_leaves_out_discord(true);
        assert!(screen_audio_leaves_out_discord());
    }
}
