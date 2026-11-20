//! Whether windows keep drawing while the system says they are hidden.
//!
//! macOS reports a window as occluded while Mission Control, another full
//! screen app or a covering window hides it, and the platform layer stops the
//! display link then. An app showing live video (a call) turns this on so what
//! the system shows of the window (Mission Control's thumbnail) keeps moving.

use std::sync::atomic::{AtomicBool, Ordering};

static KEEP: AtomicBool = AtomicBool::new(false);

/// Keep drawing windows the system reports as hidden (or stop doing so).
pub fn set_keep_rendering(on: bool) {
    KEEP.store(on, Ordering::Relaxed);
}

/// Whether hidden windows keep drawing.
pub fn keep_rendering() -> bool {
    KEEP.load(Ordering::Relaxed)
}

/// Whether the frame clock runs for a window: when it is visible, or when
/// drawing is kept going.
pub fn frame_clock_runs(visible: bool, keep: bool) -> bool {
    visible || keep
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_visible_window_always_draws() {
        assert!(frame_clock_runs(true, false));
        assert!(frame_clock_runs(true, true));
    }

    #[test]
    fn a_hidden_window_draws_only_when_kept_going() {
        assert!(!frame_clock_runs(false, false));
        assert!(frame_clock_runs(false, true));
    }

    #[test]
    fn the_switch_is_read_back() {
        set_keep_rendering(true);
        assert!(keep_rendering());
        set_keep_rendering(false);
        assert!(!keep_rendering());
    }
}
