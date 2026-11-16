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
    if CHOSEN.swap(mode.code(), Ordering::Relaxed) != mode.code() {
        note_too_slow(None);
    }
}

/// Why Strong gave way to Standard on this computer (DeepFilterNet could
/// not keep up with real time, #101), until another mode is chosen or a
/// capture starts again (which tries Strong again).
static TOO_SLOW: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub(crate) fn note_too_slow(why: Option<String>) {
    if let Ok(mut slow) = TOO_SLOW.lock() {
        *slow = why;
    }
}

/// Strong was chosen but is too slow here, so the microphone gets
/// Standard: why (for a notice), else `None`.
pub fn strong_too_slow() -> Option<String> {
    TOO_SLOW.lock().ok()?.clone()
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


// ------------------------------------------------------------ advanced settings

/// What WebRTC's processing delays the microphone by with its echo canceller
/// on (measured: 9 ms at 48 kHz, its band split and blocks), in samples.
pub(crate) const APM_DELAY: usize = 432;
/// What its noise suppression (Standard) adds to that (measured: 6 ms).
pub(crate) const APM_NS_DELAY: usize = 288;
/// How late DeepFilterNet's cleaned voice comes out (30 ms).
pub(crate) const DFN_DELAY: usize = 1440;

/// The neural model Strong runs. Only DeepFilterNet 3 is built in; a
/// low-latency one (#37) is added here when there is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoiseModel {
    #[default]
    DeepFilterNet3,
}

impl NoiseModel {
    pub const ALL: [Self; 1] = [Self::DeepFilterNet3];

    pub fn key(self) -> &'static str {
        match self {
            Self::DeepFilterNet3 => "dfn3",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::DeepFilterNet3 => "DeepFilterNet 3",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|model| model.key() == key)
    }

    /// How late the model's cleaned voice comes out, in samples.
    pub fn delay(self) -> usize {
        match self {
            Self::DeepFilterNet3 => DFN_DELAY,
        }
    }
}

/// A number setting: its range, step and what it is.
#[derive(Debug, Clone, Copy)]
pub struct NoiseParam {
    pub key: &'static str,
    pub label: &'static str,
    /// One line on what it does.
    pub about: &'static str,
    pub unit: &'static str,
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub default: f32,
    pub get: fn(&NoiseTuning) -> f32,
    pub set: fn(&mut NoiseTuning, f32),
}

/// A setting that is on or off.
#[derive(Debug, Clone, Copy)]
pub struct NoiseSwitch {
    pub key: &'static str,
    pub label: &'static str,
    pub about: &'static str,
    /// Whether it belongs to the Standard path (WebRTC's processing, which
    /// every mode runs) rather than the neural one.
    pub standard: bool,
    pub get: fn(&NoiseTuning) -> bool,
    pub set: fn(&mut NoiseTuning, bool),
}

/// The tunables of noise suppression: DeepFilterNet's and our own smoothing
/// on top of it (Strong), and WebRTC's processing switches (every mode).
/// Whatever is set, [`Self::clamped`] keeps it in a safe range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoiseTuning {
    pub model: NoiseModel,
    /// The most a frame is taken down by, dB (the model alone: 100).
    pub atten_limit_db: f32,
    /// A frame whose local SNR is under this is noise only: dB.
    pub min_snr_db: f32,
    /// Over this the model's coarse (ERB) mask is not applied: dB.
    pub max_erb_snr_db: f32,
    /// Over this the model's fine (deep filtering) stage is not applied: dB.
    pub max_df_snr_db: f32,
    pub post_filter: bool,
    pub post_filter_beta: f32,
    /// A frame clearly louder than the noise floor keeps at least this much
    /// of itself, dB (0: all of what is above the floor).
    pub floor_margin_db: f32,
    /// The most a frame's gain may fall from one 10 ms frame to the next, dB.
    pub max_fall_db: f32,
    pub echo_cancellation: bool,
    pub auto_gain: bool,
    pub high_pass: bool,
}

impl Default for NoiseTuning {
    fn default() -> Self {
        Self {
            model: NoiseModel::default(),
            atten_limit_db: 40.0,
            min_snr_db: -10.0,
            max_erb_snr_db: 30.0,
            max_df_snr_db: 20.0,
            post_filter: false,
            post_filter_beta: 0.02,
            floor_margin_db: -3.0,
            max_fall_db: 2.0,
            echo_cancellation: true,
            auto_gain: true,
            high_pass: true,
        }
    }
}

impl NoiseTuning {
    pub const PARAMS: [NoiseParam; 7] = [
        NoiseParam {
            key: "atten_limit_db",
            label: "Attenuation limit",
            about: "The most noise is taken down in a frame; lower keeps more of the room's sound and the voice's tails.",
            unit: "dB",
            min: 5.0,
            max: 60.0,
            step: 5.0,
            default: 40.0,
            get: |t| t.atten_limit_db,
            set: |t, v| t.atten_limit_db = v,
        },
        NoiseParam {
            key: "min_snr_db",
            label: "Noise-only threshold",
            about: "A frame whose speech-to-noise ratio is under this is treated as noise alone and silenced.",
            unit: "dB",
            min: -30.0,
            max: 0.0,
            step: 5.0,
            default: -10.0,
            get: |t| t.min_snr_db,
            set: |t, v| t.min_snr_db = v,
        },
        NoiseParam {
            key: "max_erb_snr_db",
            label: "Speech-only threshold (coarse)",
            about: "Over this ratio a frame is clean speech and the model's coarse mask is left off it.",
            unit: "dB",
            min: 10.0,
            max: 50.0,
            step: 5.0,
            default: 30.0,
            get: |t| t.max_erb_snr_db,
            set: |t, v| t.max_erb_snr_db = v,
        },
        NoiseParam {
            key: "max_df_snr_db",
            label: "Speech-only threshold (fine)",
            about: "Over this ratio the model's fine filtering is left off a frame.",
            unit: "dB",
            min: 5.0,
            max: 40.0,
            step: 5.0,
            default: 20.0,
            get: |t| t.max_df_snr_db,
            set: |t, v| t.max_df_snr_db = v,
        },
        NoiseParam {
            key: "post_filter_beta",
            label: "Post-filter strength",
            about: "How hard the post-filter cuts what the model left faint (needs the post-filter on).",
            unit: "",
            min: 0.01,
            max: 0.1,
            step: 0.01,
            default: 0.02,
            get: |t| t.post_filter_beta,
            set: |t, v| t.post_filter_beta = v,
        },
        NoiseParam {
            key: "floor_margin_db",
            label: "Voice floor",
            about: "A frame clearly over the noise floor keeps at least this much of itself, so a voice trailing off is not chopped.",
            unit: "dB",
            min: -12.0,
            max: 0.0,
            step: 1.0,
            default: -3.0,
            get: |t| t.floor_margin_db,
            set: |t, v| t.floor_margin_db = v,
        },
        NoiseParam {
            key: "max_fall_db",
            label: "Fall rate",
            about: "The most the volume may fall from one 10 ms frame to the next; lower fades out slower.",
            unit: "dB / 10 ms",
            min: 0.5,
            max: 10.0,
            step: 0.5,
            default: 2.0,
            get: |t| t.max_fall_db,
            set: |t, v| t.max_fall_db = v,
        },
    ];

    pub const SWITCHES: [NoiseSwitch; 4] = [
        NoiseSwitch {
            key: "post_filter",
            label: "Post-filter",
            about: "A last pass that cuts faint noise the model left, a little harsher on the voice.",
            standard: false,
            get: |t| t.post_filter,
            set: |t, v| t.post_filter = v,
        },
        NoiseSwitch {
            key: "echo_cancellation",
            label: "Echo cancellation",
            about: "Takes what the speakers play out of your microphone (adds 9 ms; a call's echo returns without it).",
            standard: true,
            get: |t| t.echo_cancellation,
            set: |t, v| t.echo_cancellation = v,
        },
        NoiseSwitch {
            key: "auto_gain",
            label: "Automatic gain",
            about: "Keeps your volume even, turning you up when you are quiet.",
            standard: true,
            get: |t| t.auto_gain,
            set: |t, v| t.auto_gain = v,
        },
        NoiseSwitch {
            key: "high_pass",
            label: "High-pass filter",
            about: "Cuts rumble and desk thumps below the voice.",
            standard: true,
            get: |t| t.high_pass,
            set: |t, v| t.high_pass = v,
        },
    ];

    /// `self` with every number inside its range (a NaN is the default)
    /// and rounded to what a step shows.
    pub fn clamped(mut self) -> Self {
        for param in Self::PARAMS {
            let value = (param.get)(&self);
            let value = if value.is_finite() {
                value.clamp(param.min, param.max)
            } else {
                param.default
            };
            (param.set)(&mut self, (value * 1000.0).round() / 1000.0);
        }
        self
    }

    /// `self` with the number setting `key` set to `value`, clamped.
    pub fn with_param(mut self, key: &str, value: f32) -> Self {
        if let Some(param) = Self::PARAMS.iter().find(|p| p.key == key) {
            (param.set)(&mut self, value);
        }
        self.clamped()
    }

    pub fn with_switch(mut self, key: &str, on: bool) -> Self {
        if let Some(switch) = Self::SWITCHES.iter().find(|s| s.key == key) {
            (switch.set)(&mut self, on);
        }
        self
    }

    /// How late the processed microphone comes out for `mode`, in samples
    /// (`live`: a real microphone, which runs the echo canceller).
    pub fn delay_samples(&self, mode: NoiseSuppression, live: bool) -> usize {
        let apm = if live && self.echo_cancellation {
            APM_DELAY
        } else {
            0
        };
        match mode {
            NoiseSuppression::Strong => apm + self.model.delay(),
            NoiseSuppression::Standard => apm + APM_NS_DELAY,
            NoiseSuppression::Off => apm,
        }
    }

    /// [`Self::delay_samples`] in milliseconds.
    pub fn added_latency_ms(&self, mode: NoiseSuppression, live: bool) -> f32 {
        self.delay_samples(mode, live) as f32 * 1000.0 / 48_000.0
    }

    /// As kept in a file.
    pub fn to_value(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        map.insert("model".into(), self.model.key().into());
        for param in Self::PARAMS {
            map.insert(param.key.into(), f64::from((param.get)(self)).into());
        }
        for switch in Self::SWITCHES {
            map.insert(switch.key.into(), (switch.get)(self).into());
        }
        map.into()
    }

    /// What [`Self::to_value`] wrote: anything missing or wrong is the default,
    /// and numbers are clamped.
    pub fn from_value(value: &serde_json::Value) -> Self {
        let mut tuning = Self::default();
        if let Some(model) = value.get("model").and_then(|m| m.as_str()).and_then(NoiseModel::from_key) {
            tuning.model = model;
        }
        for param in Self::PARAMS {
            if let Some(v) = value.get(param.key).and_then(|v| v.as_f64()) {
                (param.set)(&mut tuning, v as f32);
            }
        }
        for switch in Self::SWITCHES {
            if let Some(v) = value.get(switch.key).and_then(|v| v.as_bool()) {
                (switch.set)(&mut tuning, v);
            }
        }
        tuning.clamped()
    }
}

static TUNING: std::sync::Mutex<Option<NoiseTuning>> = std::sync::Mutex::new(None);
static APPLIED_TUNING: std::sync::Mutex<Option<NoiseTuning>> = std::sync::Mutex::new(None);

/// Calls and the mic test use `tuning` (clamped to a safe range) from now
/// on: one going takes it within a frame.
pub fn set_noise_tuning(tuning: NoiseTuning) {
    if let Ok(mut now) = TUNING.lock() {
        *now = Some(tuning.clamped());
    }
}

/// The tuning chosen ([`NoiseTuning::default`] unless set).
pub fn noise_tuning() -> NoiseTuning {
    TUNING.lock().ok().and_then(|t| *t).unwrap_or_default()
}

pub(crate) fn note_applied_tuning(tuning: Option<NoiseTuning>) {
    if let Ok(mut now) = APPLIED_TUNING.lock() {
        *now = tuning;
    }
}

/// The tuning the captured microphone is processed with right now, else
/// `None` (it can lag a choice by a frame).
pub fn applied_noise_tuning() -> Option<NoiseTuning> {
    APPLIED_TUNING.lock().ok().and_then(|t| *t)
}

#[cfg(test)]
mod tuning_tests {
    use super::*;

    #[test]
    fn every_setting_is_clamped_to_its_range() {
        let wild = [f32::MIN, -1e9, -1.0, 0.0, 1e9, f32::MAX, f32::NAN, f32::INFINITY];
        for param in NoiseTuning::PARAMS {
            for value in wild {
                let tuning = NoiseTuning::default().with_param(param.key, value);
                let got = (param.get)(&tuning);
                assert!(
                    got >= param.min && got <= param.max,
                    "{} = {value} gave {got}, outside {}..={}",
                    param.key,
                    param.min,
                    param.max
                );
            }
            assert!(param.min <= param.default && param.default <= param.max);
            assert_eq!((param.get)(&NoiseTuning::default()), param.default, "{}", param.key);
        }
        // A tuning built by hand (or read from a hand-edited file) is clamped as it is set.
        set_noise_tuning(NoiseTuning {
            atten_limit_db: 1e9,
            max_fall_db: -5.0,
            ..NoiseTuning::default()
        });
        assert_eq!(noise_tuning().atten_limit_db, 60.0);
        assert_eq!(noise_tuning().max_fall_db, 0.5);
        set_noise_tuning(NoiseTuning::default());
    }

    #[test]
    fn settings_keep_and_read_back() {
        let tuning = NoiseTuning::default()
            .with_param("atten_limit_db", 25.0)
            .with_param("post_filter_beta", 0.05)
            .with_switch("post_filter", true)
            .with_switch("echo_cancellation", false)
            .with_switch("auto_gain", false)
            .with_switch("high_pass", false);
        assert_eq!(NoiseTuning::from_value(&tuning.to_value()), tuning);
        assert_eq!(
            NoiseTuning::from_value(&serde_json::json!({ "mode": "strong" })),
            NoiseTuning::default()
        );
        assert_eq!(
            NoiseTuning::from_value(&serde_json::json!({ "atten_limit_db": 500, "max_df_snr_db": "x" })),
            NoiseTuning { atten_limit_db: 60.0, ..NoiseTuning::default() }
        );
    }

    #[test]
    fn the_latency_follows_the_mode_and_the_echo_canceller() {
        let on = NoiseTuning::default();
        let off = on.with_switch("echo_cancellation", false);
        assert_eq!(on.added_latency_ms(NoiseSuppression::Strong, true), 39.0);
        assert_eq!(off.added_latency_ms(NoiseSuppression::Strong, true), 30.0);
        assert_eq!(on.added_latency_ms(NoiseSuppression::Standard, true), 15.0);
        assert_eq!(on.added_latency_ms(NoiseSuppression::Off, true), 9.0);
        assert_eq!(on.added_latency_ms(NoiseSuppression::Off, false), 0.0);
    }
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
