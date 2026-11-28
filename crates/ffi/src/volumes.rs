//! The volumes in a call: the microphone's and the speaker's, and how loud
//! each person plays for you. Kept as the desktop keeps them (`audio_devices.json`,
//! `person-volumes.json`), so the two agree, and applied by the call's own
//! gains (`morpheus-call`'s `volume`).

use morpheus_view_model::calls::{
    PersonVolume, read_device_prefs, read_person_volumes, write_device_prefs, write_person_volume,
};

use crate::Morpheus;

const NOISE_FILE: &str = "noise-suppression.json";

/// The microphone's and the speaker's volume (1 as it is, up to 2).
#[derive(Debug, Clone, uniffi::Record)]
pub struct CallVolumes {
    pub input: f32,
    pub output: f32,
}

/// What this account chose for one person's voice.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PersonVolumeItem {
    /// 1 as sent, up to 2.
    pub volume: f32,
    /// Silenced for you alone.
    pub muted: bool,
    /// How loud their shared screen's sound plays for you (1 as sent, up to 2).
    pub stream: f32,
    /// Their shared screen's sound is silenced for you alone.
    pub stream_muted: bool,
}

impl Morpheus {
    /// The noise suppression kept (the desktop keeps it in the same file): Strong when nothing was chosen.
    fn noise_mode(&self) -> morpheus_call::NoiseSuppression {
        std::fs::read_to_string(self.dir.join(NOISE_FILE))
            .map(|json| morpheus_call::NoiseSuppression::from_json(&json))
            // A phone starts on Standard: Strong (DeepFilterNet) is a choice, until a phone has measured how it runs (#531).
            .unwrap_or(morpheus_call::NoiseSuppression::Standard)
    }

    fn account_id(&self) -> String {
        self.account().map(|a| a.user_id).unwrap_or_default()
    }

    /// Make the kept volumes the ones calls play at (at the start of a call).
    pub(crate) fn apply_saved_volumes(&self) {
        let prefs = read_device_prefs(&self.dir);
        morpheus_call::set_input_volume(prefs.input_volume);
        morpheus_call::set_output_volume(prefs.output_volume);
        morpheus_call::set_noise_suppression(self.noise_mode());
        self.apply_sensitivity();
        for (user, volume) in read_person_volumes(&self.dir, &self.account_id()) {
            morpheus_call::set_user_volume(&user, volume.voice_gain());
        }
    }
}

#[uniffi::export]
impl Morpheus {
    /// The noise suppression choices (#434, #531): Off, Standard (the phone's own, WebRTC's) and Strong (DeepFilterNet, the desktop's model), the kept one selected.
    pub fn noise_choices(&self) -> Vec<NoiseChoice> {
        let chosen = self.noise_mode();
        morpheus_call::NoiseSuppression::ALL
            .into_iter()
            .map(|mode| NoiseChoice { key: mode.key().to_owned(), label: morpheus_view_model::i18n::t(mode.label()), about: morpheus_view_model::i18n::t(mode.about()), selected: mode == chosen })
            .collect()
    }

    /// Choose one (kept; the microphone of the next call is made with it).
    pub fn set_noise_choice(&self, key: String) {
        if let Some(mode) = morpheus_call::NoiseSuppression::from_key(&key) {
            let _ = std::fs::write(self.dir.join(NOISE_FILE), mode.to_json());
        }
    }

    /// What the call going made its microphone with (`off`, `standard` or `strong`); `None` outside a call.
    pub fn applied_noise_choice(&self) -> Option<String> {
        if morpheus_call::applied_noise_suppression() == Some(morpheus_call::NoiseSuppression::Strong) {
            return Some("strong".to_owned());
        }
        morpheus_call::applied_platform_noise_suppression().map(|on| if on { "standard" } else { "off" }.to_owned())
    }

    /// The microphone's and the speaker's volume, as last kept.
    pub fn call_volumes(&self) -> CallVolumes {
        let prefs = read_device_prefs(&self.dir);
        CallVolumes { input: prefs.input_volume, output: prefs.output_volume }
    }

    /// How loud the microphone is (0 to 2); kept.
    pub fn set_call_input_volume(&self, volume: f32) {
        let mut prefs = read_device_prefs(&self.dir);
        prefs.input_volume = volume.clamp(0.0, morpheus_call::MAX_VOLUME);
        morpheus_call::set_input_volume(prefs.input_volume);
        let _ = write_device_prefs(&self.dir, &prefs);
    }

    /// How loud the call plays (0 to 2); kept. A call going follows
    /// ([`crate::calls::CallHandle::set_output_volume`]).
    pub fn set_call_output_volume(&self, volume: f32) {
        let mut prefs = read_device_prefs(&self.dir);
        prefs.output_volume = volume.clamp(0.0, morpheus_call::MAX_VOLUME);
        let _ = write_device_prefs(&self.dir, &prefs);
    }

    /// What this account chose for a person's voice (the defaults when nothing).
    pub fn person_volume(&self, user_id: String) -> PersonVolumeItem {
        let chosen = read_person_volumes(&self.dir, &self.account_id())
            .remove(&user_id)
            .unwrap_or_default();
        PersonVolumeItem { volume: chosen.voice, muted: chosen.voice_muted, stream: chosen.stream, stream_muted: chosen.stream_muted }
    }

    /// Keep what this account chose for a person's voice (their shared screen's sound is left as it was).
    pub fn keep_person_volume(&self, user_id: String, volume: f32, muted: bool) {
        let account = self.account_id();
        let mut chosen: PersonVolume =
            read_person_volumes(&self.dir, &account).remove(&user_id).unwrap_or_default();
        chosen.voice = volume.clamp(0.0, morpheus_call::MAX_VOLUME);
        chosen.voice_muted = muted;
        let _ = write_person_volume(&self.dir, &account, &user_id, chosen);
    }

    /// Keep what this account chose for a person's shared screen's sound (their voice is left as it was).
    pub fn keep_stream_volume(&self, user_id: String, volume: f32, muted: bool) {
        let account = self.account_id();
        let mut chosen: PersonVolume =
            read_person_volumes(&self.dir, &account).remove(&user_id).unwrap_or_default();
        chosen.stream = volume.clamp(0.0, morpheus_call::MAX_VOLUME);
        chosen.stream_muted = muted;
        let _ = write_person_volume(&self.dir, &account, &user_id, chosen);
    }

    /// What a person's voice plays at now (the output volume times theirs), for checking.
    pub fn person_gain(&self, user_id: String) -> f32 {
        morpheus_call::user_track_gain(&user_id, morpheus_call::TrackSource::Microphone)
    }
}

/// What a shared screen's sound is like here now, for checking that it plays.
#[derive(Debug, Clone, uniffi::Record)]
pub struct StreamHeard {
    /// How loud it plays (0 to 1, at its volume; 0 while deafened).
    pub level: f32,
    /// The volume it is played at (1 as sent).
    pub volume: f32,
}

#[uniffi::export]
impl Morpheus {
    /// The sound of `user_id`'s shared screen as it arrives and plays here; `None` while there is none.
    pub fn stream_heard(&self, user_id: String) -> Option<StreamHeard> {
        morpheus_call::screen_audio_of_user(&user_id).map(|h| StreamHeard { level: h.level, volume: h.volume })
    }
}

/// A person this account gave a volume of their own (for the Voice & Video page).
#[derive(Debug, Clone, uniffi::Record)]
pub struct KeptPerson {
    pub user_id: String,
    pub volume: f32,
    pub muted: bool,
    pub stream: f32,
    pub stream_muted: bool,
}

#[uniffi::export]
impl Morpheus {
    /// Everyone this account chose a volume for, by user ID.
    pub fn kept_people(&self) -> Vec<KeptPerson> {
        morpheus_view_model::calls::kept_person_list(&self.dir, &self.account_id())
            .into_iter()
            .map(|(user_id, v)| KeptPerson { user_id, volume: v.voice, muted: v.voice_muted, stream: v.stream, stream_muted: v.stream_muted })
            .collect()
    }
}

/// The microphone test's bar after another stretch of sound (16-bit mono): 0 to 1, rising at once, falling slowly.
#[uniffi::export]
pub fn mic_test_level(previous: f32, samples: Vec<i16>) -> f32 {
    morpheus_view_model::mic_meter::mic_level(previous, &samples)
}

/// One choice of noise suppression.
#[derive(Clone, uniffi::Record)]
pub struct NoiseChoice {
    pub key: String,
    pub label: String,
    pub about: String,
    pub selected: bool,
}

/// A call going wants the phone's microphone frames (Strong noise suppression, #531): the app captures it and gives it to [`push_call_mic`].
#[uniffi::export]
pub fn call_mic_wanted() -> bool {
    #[cfg(target_os = "android")]
    {
        morpheus_call::mic_wanted()
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

/// The phone's microphone: 16-bit samples, 48 kHz, mono, any number at a time; cleaned and sent into the call. False when no call wants them.
#[uniffi::export]
pub fn push_call_mic(samples: Vec<i16>) -> bool {
    #[cfg(target_os = "android")]
    {
        morpheus_call::push_mic(&samples)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = samples;
        false
    }
}

/// How many cleaned 10 ms frames the strong noise model has sent into calls (tests read it).
#[uniffi::export]
pub fn call_mic_frames_cleaned() -> u64 {
    #[cfg(target_os = "android")]
    {
        morpheus_call::frames_cleaned()
    }
    #[cfg(not(target_os = "android"))]
    {
        0
    }
}

const SCREEN_QUALITY_FILE: &str = "screen-quality.json";

/// One choice of the shared screen's size or speed.
#[derive(Clone, uniffi::Record)]
pub struct ShareChoice {
    pub key: String,
    pub label: String,
    pub selected: bool,
}

/// How the phone's screen share is sent: the size (a phone is tall: the box turns with it) and the speed.
#[derive(Clone, uniffi::Record)]
pub struct ShareSetup {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

/// The size a `width` x `height` phone screen is sent at under `quality`: its short side at most 720 or 1080 (what "720p" and "1080p" call the picture), never enlarged,
/// even sizes, the screen's own proportions; `Source` is the screen as it is.
fn fit_for_phone(quality: morpheus_call::ScreenQuality, width: u32, height: u32) -> (u32, u32) {
    let (width, height) = (width.max(2), height.max(2));
    let limit = match quality.resolution {
        morpheus_call::ScreenResolution::P720 => 720,
        morpheus_call::ScreenResolution::P1080 => 1080,
        morpheus_call::ScreenResolution::Source => return (width & !1, height & !1),
    };
    let short = width.min(height);
    if short <= limit {
        return (width & !1, height & !1);
    }
    let scale = f64::from(limit) / f64::from(short);
    let even = |side: u32| (((f64::from(side) * scale).round() as u32).max(2)) & !1;
    (even(width), even(height))
}

impl Morpheus {
    fn screen_quality(&self) -> morpheus_call::ScreenQuality {
        std::fs::read_to_string(self.dir.join(SCREEN_QUALITY_FILE)).map(|json| morpheus_call::ScreenQuality::from_json(&json)).unwrap_or(morpheus_call::ScreenQuality { resolution: morpheus_call::ScreenResolution::P720, ..morpheus_call::ScreenQuality::default() })
    }
}

#[uniffi::export]
impl Morpheus {
    /// The sizes the shared screen can be sent at (the desktop's 720p, 1080p, Source), the kept one selected.
    pub fn share_resolutions(&self) -> Vec<ShareChoice> {
        let q = self.screen_quality();
        morpheus_call::ScreenResolution::ALL.into_iter().map(|r| ShareChoice { key: r.key().to_owned(), label: r.label().to_owned(), selected: r == q.resolution }).collect()
    }

    /// The frame rates offered (15, 30, 60), the kept one selected.
    pub fn share_frame_rates(&self) -> Vec<ShareChoice> {
        let q = self.screen_quality();
        morpheus_call::SCREEN_FPS.into_iter().map(|f| ShareChoice { key: f.to_string(), label: format!("{f} fps"), selected: f == q.fps }).collect()
    }

    /// Choose the size (`720`, `1080`, `source`); kept, and used by the next share.
    pub fn set_share_resolution(&self, key: String) {
        if let Some(resolution) = morpheus_call::ScreenResolution::from_key(&key) {
            let _ = std::fs::write(self.dir.join(SCREEN_QUALITY_FILE), morpheus_call::ScreenQuality { resolution, ..self.screen_quality() }.to_json());
        }
    }

    /// Choose the frame rate; kept, and used by the next share.
    pub fn set_share_frame_rate(&self, fps: u32) {
        let _ = std::fs::write(self.dir.join(SCREEN_QUALITY_FILE), morpheus_call::ScreenQuality { fps, ..self.screen_quality() }.normalized().to_json());
    }

    /// What to capture a `width` x `height` screen at for the share that starts now.
    pub fn share_setup(&self, width: u32, height: u32) -> ShareSetup {
        let q = self.screen_quality();
        let (w, h) = fit_for_phone(q, width, height);
        ShareSetup { width: w, height: h, fps: q.fps }
    }
}

#[cfg(test)]
mod share_tests {
    use super::*;
    use morpheus_call::{ScreenQuality, ScreenResolution};

    fn q(resolution: ScreenResolution) -> ScreenQuality {
        ScreenQuality { resolution, ..ScreenQuality::default() }
    }

    #[test]
    fn a_tall_phone_is_fitted_in_the_box_turned_its_way() {
        assert_eq!(fit_for_phone(q(ScreenResolution::P720), 1080, 2400), (720, 1600));
        assert_eq!(fit_for_phone(q(ScreenResolution::P1080), 1440, 3200), (1080, 2400));
        assert_eq!(fit_for_phone(q(ScreenResolution::Source), 1080, 2400), (1080, 2400));
        assert_eq!(fit_for_phone(q(ScreenResolution::P720), 2400, 1080), (1600, 720), "landscape: the short side is the same");
        assert_eq!(fit_for_phone(q(ScreenResolution::P1080), 700, 1200), (700, 1200), "never enlarged");
    }
}

const SENSITIVITY_FILE: &str = "input-sensitivity.json";

/// Input sensitivity (#545): the voice gate of Voice & Video, as the desktop has it: the threshold is found automatically, or is set, and the
/// mic is let through above it. A phone starts with it off.
#[derive(Clone, uniffi::Record)]
pub struct InputSensitivity {
    pub auto: bool,
    /// The manual threshold, dBFS (-100: off).
    pub threshold_db: f32,
    /// The gate does something (automatic, or a threshold above the bottom of the meter).
    pub active: bool,
}

/// What a call hears of the gate now: the level of the voice going in (dBFS), the threshold, and whether the gate lets it through.
#[derive(Clone, uniffi::Record)]
pub struct GateReading {
    pub level_db: f32,
    pub threshold_db: f32,
    pub open: bool,
}

impl Morpheus {
    fn sensitivity(&self) -> (bool, f32) {
        let kept: serde_json::Value = std::fs::read_to_string(self.dir.join(SENSITIVITY_FILE)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(serde_json::Value::Null);
        (kept.get("auto").and_then(|v| v.as_bool()).unwrap_or(false), kept.get("db").and_then(|v| v.as_f64()).map_or(-100.0, |v| v as f32))
    }

    /// Make the kept sensitivity the one calls use (at the start of a call).
    pub(crate) fn apply_sensitivity(&self) {
        let (auto, db) = self.sensitivity();
        morpheus_call::set_noise_tuning(morpheus_call::NoiseTuning { gate_auto: auto, gate_threshold_db: db, ..morpheus_call::noise_tuning() });
    }
}

#[uniffi::export]
impl Morpheus {
    pub fn input_sensitivity(&self) -> InputSensitivity {
        let (auto, db) = self.sensitivity();
        InputSensitivity { auto, threshold_db: db, active: auto || db > morpheus_call::GATE_MIN_DB + 0.5 }
    }

    /// Choose automatic, or a threshold in dBFS (-100: off); kept, and used by the next call.
    pub fn set_input_sensitivity(&self, automatic: bool, threshold_db: f32) {
        let db = threshold_db.clamp(morpheus_call::GATE_MIN_DB, morpheus_call::GATE_MAX_DB);
        let _ = std::fs::write(self.dir.join(SENSITIVITY_FILE), serde_json::json!({ "auto": automatic, "db": db }).to_string());
    }

    /// What the call going measures: the voice's level, the gate's threshold and whether it is open.
    pub fn gate_reading(&self) -> GateReading {
        let (level_db, threshold_db, open) = morpheus_call::input_sensitivity();
        GateReading { level_db, threshold_db, open }
    }
}

/// The level of 16-bit samples in dBFS (-100 for silence), the unit the sensitivity threshold is in (the meter of the settings page).
#[uniffi::export]
pub fn gate_level_db(samples: Vec<i16>) -> f32 {
    morpheus_call::mic_level_db(&samples)
}
