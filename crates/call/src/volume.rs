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

static USER_VOLUMES: Mutex<Option<HashMap<String, f32>>> = Mutex::new(None);

/// How loud one person's voice plays for you on top of the output volume
/// (1 as sent; 0 while muted for you), by their Matrix user ID.
pub fn set_user_volume(user: &str, volume: f32) {
    let volume = volume.clamp(0.0, MAX_VOLUME);
    if let Ok(mut all) = USER_VOLUMES.lock() {
        all.get_or_insert_with(HashMap::new).insert(user.to_owned(), volume);
    }
}

pub fn user_volume(user: &str) -> f32 {
    USER_VOLUMES
        .lock()
        .ok()
        .and_then(|all| all.as_ref()?.get(user).copied())
        .unwrap_or(1.0)
}

/// What a voice (a LiveKit identity, "@user:server:DEVICE") plays at: the
/// output volume times the volume given to that person.
pub fn voice_gain(identity: &str) -> f32 {
    let own = USER_VOLUMES.lock().ok().and_then(|all| {
        all.as_ref()?
            .iter()
            .find(|(user, _)| identity == user.as_str() || identity.starts_with(&format!("{user}:")))
            .map(|(_, v)| *v)
    });
    output_volume() * own.unwrap_or(1.0)
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
}
