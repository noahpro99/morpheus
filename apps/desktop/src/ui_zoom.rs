//! The zoom of the whole window (Ctrl+Plus, Ctrl+Minus, Ctrl+0; Settings >
//! Appearance). The steps, limits and what is kept are
//! `view_model::ui_zoom`; the scale itself is gpui's `Window::set_ui_zoom`,
//! which scales layout, text, images and input of the window together.

use std::{path::Path, sync::atomic::{AtomicU16, Ordering}};

use gpui::{App, Window};

use crate::view_model::ui_zoom as steps;

static PERCENT: AtomicU16 = AtomicU16::new(steps::DEFAULT);

/// Reads what this device kept; call before the window opens.
pub fn init(dir: &Path) {
    PERCENT.store(steps::load(dir), Ordering::Relaxed);
}

/// The zoom now, in percent.
pub fn percent() -> u16 {
    PERCENT.load(Ordering::Relaxed)
}

/// Puts the zoom in force on a window (when it opens).
pub fn apply(window: &mut Window, cx: &mut App) {
    window.set_ui_zoom(steps::factor(percent()), cx);
}

/// Zooms to `pct` (kept within the limits), keeps it, and applies it.
pub fn set(pct: u16, dir: &Path, window: &mut Window, cx: &mut App) -> u16 {
    let pct = steps::clamp(pct);
    PERCENT.store(pct, Ordering::Relaxed);
    if let Err(err) = steps::save(dir, pct) {
        eprintln!("morpheus-desktop: saving the zoom: {err}");
    }
    apply(window, cx);
    pct
}
