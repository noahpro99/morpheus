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
            .unwrap_or_default()
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
        for (user, volume) in read_person_volumes(&self.dir, &self.account_id()) {
            morpheus_call::set_user_volume(&user, volume.voice_gain());
        }
    }
}

#[uniffi::export]
impl Morpheus {
    /// Whether the call's noise suppression is on (#434): WebRTC's, the only one a phone has (the desktop's stronger model is #531). On
    /// unless Off was chosen.
    pub fn noise_suppression_on(&self) -> bool {
        self.noise_mode().platform_on()
    }

    /// Turn the call's noise suppression on or off; kept, and applied from the next call (the microphone of one going is made already).
    pub fn set_noise_suppression_on(&self, on: bool) {
        let mode = if on { morpheus_call::NoiseSuppression::Standard } else { morpheus_call::NoiseSuppression::Off };
        let _ = std::fs::write(self.dir.join(NOISE_FILE), mode.to_json());
    }

    /// Whether the call going made its microphone with noise suppression on or off; `None` outside a call.
    pub fn applied_noise_suppression_on(&self) -> Option<bool> {
        morpheus_call::applied_platform_noise_suppression()
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
