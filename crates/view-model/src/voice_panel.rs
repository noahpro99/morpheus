//! The voice channel chat panel's width: dragged at its left edge within limits,
//! reset by a double click, kept per device in the data directory.

use std::path::Path;

/// The panel is narrower than this never.
pub const MIN_WIDTH: f32 = 248.0;
/// ... and wider than this never, so the call keeps room.
pub const MAX_WIDTH: f32 = 640.0;
/// The call keeps at least this much beside the panel.
pub const MIN_CALL: f32 = 320.0;

const FILE: &str = "voice_chat_panel.json";

/// The width it opens at: 40% of the page, between the limits it started with.
pub fn default_width(page: f32) -> f32 {
    (page * 0.4).clamp(MIN_WIDTH, 388.0)
}

/// `width` kept within the limits, and so that the call keeps its minimum; never below
/// the lower limit.
pub fn clamp_width(width: f32, page: f32) -> f32 {
    let most = MAX_WIDTH.min(page - MIN_CALL).max(MIN_WIDTH);
    width.clamp(MIN_WIDTH, most)
}

/// The width after dragging the left edge `delta` px (to the right is positive) from
/// where it began at `start`: dragging left widens it.
pub fn dragged_width(start: f32, delta: f32, page: f32) -> f32 {
    clamp_width(start - delta, page)
}

pub fn load(dir: &Path) -> Option<f32> {
    let text = std::fs::read_to_string(dir.join(FILE)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let width = value.get("width")?.as_f64()? as f32;
    width.is_finite().then_some(width)
}

pub fn save(dir: &Path, width: Option<f32>) -> std::io::Result<()> {
    match width {
        Some(width) => std::fs::write(dir.join(FILE), serde_json::json!({ "width": width }).to_string()),
        None => match std::fs::remove_file(dir.join(FILE)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_opens_at_forty_percent_within_the_old_limits() {
        assert_eq!(default_width(800.0), 320.0);
        assert_eq!(default_width(300.0), MIN_WIDTH);
        assert_eq!(default_width(2000.0), 388.0);
    }

    #[test]
    fn dragging_left_widens_within_limits_and_leaves_the_call_its_room() {
        assert_eq!(dragged_width(300.0, -60.0, 1200.0), 360.0);
        assert_eq!(dragged_width(300.0, 500.0, 1200.0), MIN_WIDTH);
        assert_eq!(dragged_width(300.0, -900.0, 2000.0), MAX_WIDTH);
        assert_eq!(dragged_width(300.0, -900.0, 800.0), 480.0);
        assert_eq!(dragged_width(300.0, -900.0, 400.0), MIN_WIDTH);
    }

    #[test]
    fn the_width_is_kept_and_a_reset_forgets_it() {
        let dir = std::env::temp_dir().join(format!("morpheus-vp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        assert_eq!(load(&dir), None);
        save(&dir, Some(333.0)).expect("save");
        assert_eq!(load(&dir), Some(333.0));
        save(&dir, None).expect("reset");
        assert_eq!(load(&dir), None);
        std::fs::remove_dir_all(&dir).ok();
    }
}
