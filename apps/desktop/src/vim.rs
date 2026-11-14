//! Vim mode's switch (Settings > Appearance > Keyboard), readable from
//! anywhere that handles a key: the message box and the shell's hotkeys. Off
//! unless turned on; kept in the data directory (`view_model::vim`).

use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Read the choice (at startup).
pub fn load(dir: &std::path::Path) {
    ENABLED.store(crate::view_model::vim::load_enabled(dir), Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Turn it on or off and keep the choice.
pub fn set(dir: &std::path::Path, on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
    if let Err(err) = crate::view_model::vim::save_enabled(dir, on) {
        eprintln!("morpheus-desktop: saving the vim mode setting: {err}");
    }
}
