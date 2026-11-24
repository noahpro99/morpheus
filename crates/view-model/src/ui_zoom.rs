//! The zoom of the whole desktop UI (Ctrl+Plus, Ctrl+Minus, Ctrl+0), as in a
//! browser: percentages in browser-like steps from 50 to 200, kept per device
//! in the data directory. The front end applies `factor` as the window's scale.

use std::path::Path;

/// The zoom it starts at and returns to.
pub const DEFAULT: u16 = 100;
pub const MIN: u16 = 50;
pub const MAX: u16 = 200;
/// The stops, as a browser has them.
pub const STEPS: [u16; 11] = [50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200];

const FILE: &str = "ui_zoom";

/// `pct` held within the limits.
pub fn clamp(pct: u16) -> u16 {
    pct.clamp(MIN, MAX)
}

/// The next stop above `pct` (the top one stays).
pub fn zoom_in(pct: u16) -> u16 {
    let pct = clamp(pct);
    STEPS.iter().copied().find(|&s| s > pct).unwrap_or(MAX)
}

/// The next stop below `pct` (the lowest one stays).
pub fn zoom_out(pct: u16) -> u16 {
    let pct = clamp(pct);
    STEPS.iter().rev().copied().find(|&s| s < pct).unwrap_or(MIN)
}

/// The scale the window is drawn at, 1.0 at 100%.
pub fn factor(pct: u16) -> f32 {
    f32::from(clamp(pct)) / 100.0
}

/// "125%".
pub fn label(pct: u16) -> String {
    format!("{}%", clamp(pct))
}

/// The toast shown when the zoom changes.
pub fn toast(pct: u16) -> String {
    format!("Zoom {}", label(pct))
}

/// Whether `pct` is the default.
pub fn is_default(pct: u16) -> bool {
    clamp(pct) == DEFAULT
}

/// What this device keeps (100% when nothing, or nonsense, is kept).
pub fn load(dir: &Path) -> u16 {
    std::fs::read_to_string(dir.join(FILE))
        .ok()
        .and_then(|text| text.trim().parse::<u16>().ok())
        .map(clamp)
        .unwrap_or(DEFAULT)
}

/// Keeps `pct` (the default is kept as no file).
pub fn save(dir: &Path, pct: u16) -> std::io::Result<()> {
    let path = dir.join(FILE);
    if is_default(pct) {
        return match std::fs::remove_file(path) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        };
    }
    std::fs::write(path, clamp(pct).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zooming_in_walks_the_steps_and_stops_at_the_top() {
        assert_eq!(zoom_in(100), 110);
        assert_eq!(zoom_in(110), 125);
        assert_eq!(zoom_in(175), 200);
        assert_eq!(zoom_in(200), 200);
    }

    #[test]
    fn zooming_out_walks_the_steps_and_stops_at_the_bottom() {
        assert_eq!(zoom_out(100), 90);
        assert_eq!(zoom_out(67), 50);
        assert_eq!(zoom_out(50), 50);
    }

    #[test]
    fn a_value_between_steps_goes_to_the_next_step_either_way() {
        assert_eq!(zoom_in(105), 110);
        assert_eq!(zoom_out(105), 100);
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        assert_eq!(clamp(5), MIN);
        assert_eq!(clamp(900), MAX);
        assert_eq!(zoom_in(900), MAX);
        assert_eq!(zoom_out(1), MIN);
    }

    #[test]
    fn the_factor_and_words() {
        assert_eq!(factor(100), 1.0);
        assert_eq!(factor(150), 1.5);
        assert_eq!(factor(10), 0.5);
        assert_eq!(label(125), "125%");
        assert_eq!(toast(90), "Zoom 90%");
    }

    #[test]
    fn the_zoom_is_kept_and_reset_to_nothing_kept() {
        let dir = std::env::temp_dir().join(format!("morpheus-zoom-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(load(&dir), DEFAULT, "nothing kept");
        save(&dir, 150).unwrap();
        assert_eq!(load(&dir), 150);
        save(&dir, DEFAULT).unwrap();
        assert!(!dir.join(FILE).exists());
        assert_eq!(load(&dir), DEFAULT);
        std::fs::write(dir.join(FILE), "banana").unwrap();
        assert_eq!(load(&dir), DEFAULT, "nonsense is ignored");
        std::fs::write(dir.join(FILE), "9000").unwrap();
        assert_eq!(load(&dir), MAX);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_zoom_keys_are_recognised() {
        use crate::hotkeys::{Hotkey, classify};
        for key in ["+", "="] {
            assert_eq!(classify(key, true, false, false), Some(Hotkey::ZoomIn), "{key}");
            assert_eq!(classify(key, true, false, true), Some(Hotkey::ZoomIn), "{key}+shift");
        }
        for key in ["-", "_"] {
            assert_eq!(classify(key, true, false, false), Some(Hotkey::ZoomOut), "{key}");
            assert_eq!(classify(key, true, false, true), Some(Hotkey::ZoomOut), "{key}+shift");
        }
        assert_eq!(classify("0", true, false, false), Some(Hotkey::ZoomReset));
        assert_eq!(classify("-", false, false, false), None, "plain minus types");
    }
}
