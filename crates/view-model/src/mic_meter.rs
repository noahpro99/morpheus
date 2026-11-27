//! The microphone test's level bar: how loud the last stretch of microphone
//! sound is (0 to 1), rising at once and falling slowly, the same on every
//! platform that draws one (the desktop's own meter is in the call crate).

/// How much of the last level is kept each time a quieter stretch comes.
const FALL: f32 = 0.85;

/// The level of `samples` (16-bit mono) as a bar: the loudness (RMS, scaled up
/// so ordinary speech fills about half of it), never below `previous` falling
/// off slowly.
pub fn mic_level(previous: f32, samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return (previous * FALL).clamp(0.0, 1.0);
    }
    let sum: f64 = samples.iter().map(|s| f64::from(*s).powi(2)).sum();
    let rms = (sum / samples.len() as f64).sqrt() as f32 / 32_768.0;
    // Speech is around 0.05 to 0.15 of full scale: a square root curve gives it a bar to see.
    let now = (rms.sqrt() * 1.4).clamp(0.0, 1.0);
    now.max(previous * FALL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_is_zero_and_a_tone_moves_the_bar() {
        assert_eq!(mic_level(0.0, &[0; 480]), 0.0);
        let tone: Vec<i16> = (0..480).map(|i| ((i as f32 * 0.3).sin() * 8000.0) as i16).collect();
        let level = mic_level(0.0, &tone);
        assert!(level > 0.3 && level < 1.0, "{level}");
    }

    #[test]
    fn the_bar_rises_at_once_and_falls_slowly() {
        let loud = vec![i16::MAX; 480];
        assert_eq!(mic_level(0.0, &loud), 1.0);
        let after = mic_level(1.0, &[0; 480]);
        assert!(after > 0.8 && after < 1.0, "{after}");
        assert!(mic_level(after, &[]) < after, "falls with nothing too");
    }
}
