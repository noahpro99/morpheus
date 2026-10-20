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
        }
    }
}

const RATE: u32 = 24_000;

/// A mono 16-bit WAV of the sound's notes, each struck and fading.
fn wav(sound: Sound) -> Vec<u8> {
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
    let audible = morpheus_call::AudioMode::from_env() == morpheus_call::AudioMode::Platform;
    let play = if audible {
        format!("new Audio('{}').play().catch(() => {{}});", data_url(sound))
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
