//! Input and output volume control for calls.

use std::sync::atomic::{AtomicU32, Ordering};

pub const MAX_VOLUME: f32 = 2.0;

static INPUT_VOLUME: AtomicU32 = AtomicU32::new(1.0_f32.to_bits());
static OUTPUT_VOLUME: AtomicU32 = AtomicU32::new(1.0_f32.to_bits());

/// The current microphone input volume multiplier (1.0 = 100%, up to 2.0).
pub fn input_volume() -> f32 {
    f32::from_bits(INPUT_VOLUME.load(Ordering::Relaxed))
}

pub fn set_input_volume(volume: f32) {
    let clamped = volume.clamp(0.0, MAX_VOLUME);
    INPUT_VOLUME.store(clamped.to_bits(), Ordering::Relaxed);
}

/// The current speaker output volume multiplier (1.0 = 100%, up to 2.0).
pub fn output_volume() -> f32 {
    f32::from_bits(OUTPUT_VOLUME.load(Ordering::Relaxed))
}

pub fn set_output_volume(volume: f32) {
    let clamped = volume.clamp(0.0, MAX_VOLUME);
    OUTPUT_VOLUME.store(clamped.to_bits(), Ordering::Relaxed);
}

use std::{collections::HashMap, sync::Mutex};

static USER_VOLUMES: Mutex<Option<HashMap<(String, TrackSource), f32>>> = Mutex::new(None);

/// Which of a person's audio tracks a gain is for: their microphone or the
/// sound of their shared screen. Each is kept apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TrackSource {
    Microphone,
    ScreenShare,
}

/// How loud one person's voice plays for you on top of the output volume
/// (1 as sent; 0 while muted for you), by their Matrix user ID.
pub fn set_user_volume(user: &str, volume: f32) {
    set_user_track_volume(user, TrackSource::Microphone, volume);
}

pub fn user_volume(user: &str) -> f32 {
    user_track_volume(user, TrackSource::Microphone)
}

/// How loud one of a person's tracks plays for you (1 as sent, 0 while muted).
pub fn set_user_track_volume(user: &str, source: TrackSource, volume: f32) {
    let volume = volume.clamp(0.0, MAX_VOLUME);
    if let Ok(mut all) = USER_VOLUMES.lock() {
        all.get_or_insert_with(HashMap::new)
            .insert((user.to_owned(), source), volume);
    }
}

pub fn user_track_volume(user: &str, source: TrackSource) -> f32 {
    USER_VOLUMES
        .lock()
        .ok()
        .and_then(|all| all.as_ref()?.get(&(user.to_owned(), source)).copied())
        .unwrap_or(1.0)
}

/// What `user`'s track of `source` plays at: the microphone at the output
/// volume times theirs, a shared screen's sound at its own volume alone.
pub fn user_track_gain(user: &str, source: TrackSource) -> f32 {
    let own = user_track_volume(user, source);
    match source {
        TrackSource::Microphone => output_volume() * own,
        TrackSource::ScreenShare => own,
    }
}

static VOICE_OWNERS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// Records that the LiveKit identity `identity` is a device of `user`, as the
/// call's memberships say (an identity may be a hash with no user ID in it).
pub fn note_voice_identity(identity: &str, user: &str) {
    if let Ok(mut all) = VOICE_OWNERS.lock() {
        all.get_or_insert_with(HashMap::new)
            .insert(identity.to_owned(), user.to_owned());
    }
}

/// Whether `identity` has been noted as a device of `user`.
pub fn voice_belongs_to(identity: &str, user: &str) -> bool {
    VOICE_OWNERS
        .lock()
        .ok()
        .and_then(|all| all.as_ref()?.get(identity).map(|owner| owner == user))
        .unwrap_or(false)
}

/// The person a LiveKit identity belongs to, once noted.
pub fn voice_owner(identity: &str) -> Option<String> {
    VOICE_OWNERS
        .lock()
        .ok()
        .and_then(|all| all.as_ref()?.get(identity).cloned())
}

/// What a track (of a LiveKit identity) plays at, by its source.
pub fn track_gain(identity: &str, source: TrackSource) -> f32 {
    match voice_owner(identity) {
        Some(user) => user_track_gain(&user, source),
        None => match source {
            TrackSource::Microphone => output_volume(),
            TrackSource::ScreenShare => 1.0,
        },
    }
}

/// What a voice (a LiveKit identity) plays at: the output volume times the
/// volume given to the person it belongs to.
pub fn voice_gain(identity: &str) -> f32 {
    track_gain(identity, TrackSource::Microphone)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_clamps_between_zero_and_max() {
        set_input_volume(1.5);
        assert!((input_volume() - 1.5).abs() < 1e-4);
        set_input_volume(-0.5);
        assert_eq!(input_volume(), 0.0);
        set_input_volume(3.5);
        assert_eq!(input_volume(), MAX_VOLUME);
        set_input_volume(1.0);

        set_output_volume(0.75);
        assert!((output_volume() - 0.75).abs() < 1e-4);
        set_output_volume(1.0);
    }

    #[test]
    fn a_voice_plays_at_the_volume_of_the_person_its_identity_belongs_to() {
        // The identity is the token service's hash of user and device, with no
        // user ID in it: it is matched through the call's memberships.
        let hashed = "mZs3Bdzfyu9aKeiedvidIcX5EmK9NtRPyMb9gc3ISa8";
        set_output_volume(1.0);
        set_user_volume("@vol-test:localhost", 0.5);
        assert_eq!(voice_gain(hashed), 1.0, "nobody knows whose it is yet");
        note_voice_identity(hashed, "@vol-test:localhost");
        assert!((voice_gain(hashed) - 0.5).abs() < 1e-4);
        assert!(voice_belongs_to(hashed, "@vol-test:localhost"));
        assert!(!voice_belongs_to(hashed, "@other:localhost"));
        // A name that merely starts like a user ID is not theirs.
        assert!(!voice_belongs_to(
            "@vol-test:localhost:DEV",
            "@vol-test:localhost"
        ));
    }

    #[test]
    fn a_streams_gain_applies_only_to_the_screen_share_track_and_the_mics_only_to_the_mic() {
        let who = "@gain-test:localhost";
        let identity = "hashed-gain-test-identity";
        note_voice_identity(identity, who);
        // (The output volume is shared with other tests: gains are only
        // compared with zero and with each other.)
        set_user_track_volume(who, TrackSource::ScreenShare, 0.0);
        assert_eq!(track_gain(identity, TrackSource::ScreenShare), 0.0);
        assert!(
            track_gain(identity, TrackSource::Microphone) > 0.0,
            "mic untouched"
        );
        set_user_track_volume(who, TrackSource::ScreenShare, 1.0);
        set_user_volume(who, 0.0);
        assert_eq!(
            track_gain(identity, TrackSource::ScreenShare),
            1.0,
            "stream untouched"
        );
        assert_eq!(track_gain(identity, TrackSource::Microphone), 0.0);
        assert_eq!(voice_gain(identity), 0.0);
    }
}
