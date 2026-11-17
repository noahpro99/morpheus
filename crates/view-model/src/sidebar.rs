//! The channel list's width: dragged at its right edge within limits, reset
//! by a double click, kept per device in the data directory.

use std::path::Path;

/// The width it starts at and returns to (the theme's `SIDEBAR_W`).
pub const DEFAULT_WIDTH: f32 = 248.0;
pub const MIN_WIDTH: f32 = 200.0;
pub const MAX_WIDTH: f32 = 400.0;
/// The chat keeps at least this much beside it.
pub const MIN_CHAT: f32 = 400.0;

const FILE: &str = "sidebar.json";

/// `width` kept within the limits, and so that the chat (the window less the
/// rail and the list) keeps its minimum; never below the lower limit.
pub fn clamp_width(width: f32, window: f32, rail: f32) -> f32 {
    let most = MAX_WIDTH.min(window - rail - MIN_CHAT).max(MIN_WIDTH);
    width.clamp(MIN_WIDTH, most)
}

/// The width after dragging the edge `delta` px from where it began at `start`.
pub fn dragged_width(start: f32, delta: f32, window: f32, rail: f32) -> f32 {
    clamp_width(start + delta, window, rail)
}

pub fn load(dir: &Path) -> Option<f32> {
    let text = std::fs::read_to_string(dir.join(FILE)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let width = value.get("width")?.as_f64()? as f32;
    width.is_finite().then_some(width)
}

pub fn save(dir: &Path, width: f32) -> std::io::Result<()> {
    std::fs::write(dir.join(FILE), serde_json::json!({ "width": width }).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_width_stays_between_the_limits() {
        assert_eq!(clamp_width(300.0, 1600.0, 72.0), 300.0);
        assert_eq!(clamp_width(50.0, 1600.0, 72.0), MIN_WIDTH);
        assert_eq!(clamp_width(900.0, 1600.0, 72.0), MAX_WIDTH);
    }

    #[test]
    fn the_chat_keeps_its_room_in_a_narrow_window() {
        // 800 - 72 - 400 = 328 left for the list.
        assert_eq!(clamp_width(400.0, 800.0, 72.0), 328.0);
        // Too narrow for anything better than the lower limit.
        assert_eq!(clamp_width(300.0, 600.0, 72.0), MIN_WIDTH);
    }

    #[test]
    fn dragging_moves_the_edge_with_the_pointer() {
        assert_eq!(dragged_width(248.0, 40.0, 1600.0, 72.0), 288.0);
        assert_eq!(dragged_width(248.0, -500.0, 1600.0, 72.0), MIN_WIDTH);
        assert_eq!(dragged_width(248.0, 500.0, 1600.0, 72.0), MAX_WIDTH);
    }

    #[test]
    fn the_width_is_kept_in_the_data_dir() {
        let dir = std::env::temp_dir().join(format!("morpheus-sidebar-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        assert_eq!(load(&dir), None);
        save(&dir, 312.0).expect("save");
        assert_eq!(load(&dir), Some(312.0));
        std::fs::write(dir.join(FILE), "garbage").expect("write");
        assert_eq!(load(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
