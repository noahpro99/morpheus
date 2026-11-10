//! Call chimes, Discord-style: someone joins or leaves the call you're in,
//! you mute or deafen. Short bell-like tones made here (a WAV each) and
//! played by the webview. Test runs (`AudioMode::Tone`) keep quiet and only
//! note what would have played, in `window.morpheusSounds`.

use std::sync::OnceLock;

use base64::{Engine, engine::general_purpose::STANDARD};
use dioxus::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    /// You, or someone else, joined the call (rising).
    Join,
    /// Someone left, or you hung up (falling).
    Leave,
    Mute,
    Unmute,
    Deafen,
    Undeafen,
    /// Someone is calling (repeats while it rings).
    Ring,
    /// Your call ringing the other person (repeats until they answer).
    Ringback,
}

impl Sound {
    fn key(self) -> &'static str {
        match self {
            Self::Join => "join",
            Self::Leave => "leave",
            Self::Mute => "mute",
            Self::Unmute => "unmute",
            Self::Deafen => "deafen",
            Self::Undeafen => "undeafen",
            Self::Ring => "ring",
            Self::Ringback => "ringback",
        }
    }

    /// The notes (Hz) and how long each rings (ms).
    fn notes(self) -> &'static [(f32, u32)] {
        match self {
            Self::Join => &[(587.33, 110), (880.0, 260)],
            Self::Leave => &[(880.0, 110), (587.33, 260)],
            Self::Mute => &[(392.0, 140)],
            Self::Unmute => &[(659.25, 140)],
            Self::Deafen => &[(523.25, 90), (392.0, 160)],
            Self::Undeafen => &[(392.0, 90), (523.25, 160)],
            Self::Ring => &[(659.25, 120), (830.61, 120), (987.77, 120), (1318.51, 320)],
            Self::Ringback => &[(440.0, 380), (440.0, 380)],
        }
    }
}

const RATE: u32 = 24_000;

/// The sound's notes as mono 16-bit samples at RATE, each struck and fading.
fn samples(sound: Sound) -> Vec<i16> {
    let mut samples: Vec<i16> = Vec::new();
    for &(freq, ms) in sound.notes() {
        let n = (RATE * ms / 1000) as usize;
        for i in 0..n {
            let t = i as f32 / RATE as f32;
            // A quick attack (no click), then a bell's decay.
            let attack = (i as f32 / 120.0).min(1.0);
            let decay = (-t * 9.0).exp();
            let tone = (t * freq * std::f32::consts::TAU).sin()
                + 0.3 * (t * freq * 2.0 * std::f32::consts::TAU).sin();
            samples.push((tone * attack * decay * 6_000.0) as i16);
        }
    }
    samples
}

/// A mono 16-bit WAV of the sound.
fn wav(sound: Sound) -> Vec<u8> {
    let samples = samples(sound);
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

fn data_url(sound: Sound) -> &'static str {
    static URLS: OnceLock<Vec<(Sound, String)>> = OnceLock::new();
    let urls = URLS.get_or_init(|| {
        [
            Sound::Join,
            Sound::Leave,
            Sound::Mute,
            Sound::Unmute,
            Sound::Deafen,
            Sound::Undeafen,
            Sound::Ring,
        ]
        .into_iter()
        .map(|s| {
            (
                s,
                format!("data:audio/wav;base64,{}", STANDARD.encode(wav(s))),
            )
        })
        .collect()
    });
    urls.iter()
        .find(|(s, _)| *s == sound)
        .map(|(_, url)| url.as_str())
        .unwrap_or_default()
}

/// Play `sound` (test runs: only note it).
pub fn play(sound: Sound) {
    // The ringtone, where the app plays it itself (a desktop, which rings
    // with the window in the tray too): only noted here.
    if rings_natively(sound) {
        document::eval(&format!(
            "(window.morpheusSounds ??= []).push('{}');",
            sound.key()
        ));
        return;
    }
    let audible = morpheus_call::AudioMode::from_env() == morpheus_call::AudioMode::Platform;
    let play = if audible {
        // Whether it played (or why not: a page may only play sound after a
        // tap or click), kept for the tests and logged.
        // One element per sound, kept: an Audio nothing refers to can be
        // collected before it plays, and then it never does.
        format!(
            "const chimes = (window.morpheusChimes ??= {{}});\
             const a = (chimes['{key}'] ??= new Audio('{}'));\
             a.currentTime = 0;\
             a.play()\
             .then(() => (window.morpheusSoundPlays ??= []).push('{key}: played'))\
             .catch(e => {{ (window.morpheusSoundPlays ??= []).push('{key}: ' + e.name); console.warn('sound not played', '{key}', e); }});",
            data_url(sound),
            key = sound.key()
        )
    } else {
        String::new()
    };
    document::eval(&format!(
        "(window.morpheusSounds ??= []).push('{}'); {play}",
        sound.key()
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chimes_are_wavs_of_the_right_length() {
        let bytes = wav(Sound::Join);
        assert_eq!(&bytes[..4], b"RIFF");
        // 370 ms of 16-bit mono at 24 kHz, plus the 44-byte header.
        assert_eq!(
            bytes.len(),
            44 + 2 * (RATE as usize * 110 / 1000 + RATE as usize * 260 / 1000)
        );
    }
}

/// Plays a ringtone outside the web view: `(sound's name, mono 16-bit
/// samples, sample rate)`. A desktop sets one: closed to the tray its page
/// is suspended and can't play, and the ring must still be heard.
type NativePlayer = Box<dyn Fn(&'static str, &[i16], u32) + Send + Sync>;
static NATIVE: OnceLock<NativePlayer> = OnceLock::new();

/// Play the ringtone (`Sound::Ring`) through `player` from now on.
pub fn set_native_player(player: impl Fn(&'static str, &[i16], u32) + Send + Sync + 'static) {
    let _ = NATIVE.set(Box::new(player));
}

/// Whether the ringtone is the native player's: then the app (not the
/// page) rings, with `ring_natively`, and the page's ring is only noted.
fn rings_natively(sound: Sound) -> bool {
    sound == Sound::Ring && NATIVE.get().is_some()
}

/// Ring once through the native player, if there is one.
pub fn ring_natively() {
    let Some(player) = NATIVE.get() else { return };
    static RING: OnceLock<Vec<i16>> = OnceLock::new();
    player(
        Sound::Ring.key(),
        RING.get_or_init(|| samples(Sound::Ring)),
        RATE,
    );
}
