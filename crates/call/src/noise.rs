//! How much noise is taken out of this device's microphone before it is
//! sent, as in Discord: Off, Standard (WebRTC's own suppression) or Strong
//! (a neural suppressor, DeepFilterNet 3, on desktops). Chosen once for all
//! calls and the mic test; a call going changes over at once.

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoiseSuppression {
    /// Echo cancellation and gain only.
    Off,
    /// WebRTC's noise suppression.
    Standard,
    /// DeepFilterNet 3: voices kept, keyboards, fans and rooms taken out.
    #[default]
    Strong,
}

impl NoiseSuppression {
    pub const ALL: [Self; 3] = [Self::Off, Self::Standard, Self::Strong];

    /// The name it is kept and shown to tests by.
    pub fn key(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Standard => "standard",
            Self::Strong => "strong",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.key() == key.trim())
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Standard => "Standard",
            Self::Strong => "Strong",
        }
    }

    /// What it does, in a line.
    pub fn about(self) -> &'static str {
        match self {
            Self::Off => "Your microphone as it is (echo is still cancelled).",
            Self::Standard => "Takes out steady noise, like a fan or hum.",
            Self::Strong => "Keeps your voice and takes out the rest: keys, dogs, rooms.",
        }
    }

    /// As kept in a file: `{"mode":"strong"}`.
    pub fn to_json(self) -> String {
        format!("{{\"mode\":\"{}\"}}", self.key())
    }

    /// What [`Self::to_json`] wrote; anything else is the default.
    pub fn from_json(json: &str) -> Self {
        json.split('"')
            .skip_while(|part| *part != "mode")
            .nth(2)
            .and_then(Self::from_key)
            .unwrap_or_default()
    }

    fn code(self) -> u8 {
        match self {
            Self::Off => 1,
            Self::Standard => 2,
            Self::Strong => 3,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.code() == code)
    }
}

static CHOSEN: AtomicU8 = AtomicU8::new(0);

/// Calls and the mic test use `mode` from now on (one going switches now).
pub fn set_noise_suppression(mode: NoiseSuppression) {
    CHOSEN.store(mode.code(), Ordering::Relaxed);
}

/// The mode chosen ([`NoiseSuppression::Strong`] unless set).
pub fn noise_suppression() -> NoiseSuppression {
    NoiseSuppression::from_code(CHOSEN.load(Ordering::Relaxed)).unwrap_or_default()
}

/// What the microphone being captured now is processed with (`None`: none
/// is): set by the capture as it switches, so it can lag a choice.
static APPLIED: AtomicU8 = AtomicU8::new(0);

pub(crate) fn note_applied(mode: Option<NoiseSuppression>) {
    APPLIED.store(mode.map_or(0, NoiseSuppression::code), Ordering::Relaxed);
}

/// The suppression the microphone is processed with right now, while it is
/// captured (a call, the mic test), else `None`.
pub fn applied_noise_suppression() -> Option<NoiseSuppression> {
    NoiseSuppression::from_code(APPLIED.load(Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_keep_and_read_back() {
        for mode in NoiseSuppression::ALL {
            assert_eq!(NoiseSuppression::from_json(&mode.to_json()), mode);
            assert_eq!(NoiseSuppression::from_key(mode.key()), Some(mode));
        }
        assert_eq!(
            NoiseSuppression::from_json("junk"),
            NoiseSuppression::Strong
        );
        assert_eq!(NoiseSuppression::from_json(""), NoiseSuppression::Strong);
    }
}
