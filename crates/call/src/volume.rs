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
