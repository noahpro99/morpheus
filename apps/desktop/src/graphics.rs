//! The dedicated graphics card switch (Settings > Appearance > Graphics,
//! Linux): on unless turned off, kept in the data directory
//! (`view_model::gpu`). gpui reads it when the window system starts, so a
//! change takes effect at the next start.

use std::sync::atomic::{AtomicBool, Ordering};

static DEDICATED: AtomicBool = AtomicBool::new(true);

/// Read the choice (at startup) and hand it to gpui.
pub fn load(dir: &std::path::Path) {
    let on = crate::view_model::gpu::load_dedicated(dir);
    DEDICATED.store(on, Ordering::Relaxed);
    gpui::PREFER_INTEGRATED_GPU.store(!on, Ordering::Relaxed);
}

pub fn dedicated() -> bool {
    DEDICATED.load(Ordering::Relaxed)
}

/// Turn it on or off and keep the choice (for the next start).
pub fn set(dir: &std::path::Path, on: bool) {
    DEDICATED.store(on, Ordering::Relaxed);
    if let Err(err) = crate::view_model::gpu::save_dedicated(dir, on) {
        eprintln!("morpheus-desktop: saving the graphics card setting: {err}");
    }
}
