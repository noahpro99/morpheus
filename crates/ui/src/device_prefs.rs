//! Settings that belong to this device rather than the account, kept as
//! marker files in the data directory so every part of the app (the desktop
//! notifier, the Android push service in Kotlin) can read them cheaply.

use std::path::Path;

/// Present when notifications are off on this device.
pub const NOTIFICATIONS_OFF: &str = "notifications_off";

/// Whether this device shows notifications (on unless turned off).
pub fn notifications_enabled(data_dir: &Path) -> bool {
    !data_dir.join(NOTIFICATIONS_OFF).exists()
}

pub fn set_notifications_enabled(data_dir: &Path, enabled: bool) -> std::io::Result<()> {
    let marker = data_dir.join(NOTIFICATIONS_OFF);
    if enabled {
        match std::fs::remove_file(marker) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        }
    } else {
        std::fs::write(marker, "")
    }
}
