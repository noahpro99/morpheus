//! Settings > Appearance > Graphics: whether to draw with the dedicated
//! graphics card (the discrete GPU of a laptop with two) or the integrated
//! one. On unless turned off; kept in the data directory, read at start
//! (the GPU is chosen once, when the window system starts).

const FILE: &str = "graphics.json";

/// The kept choice: draw with the dedicated card (true unless turned off).
pub fn load_dedicated(dir: &std::path::Path) -> bool {
    std::fs::read_to_string(dir.join(FILE))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|v| v.get("dedicated").and_then(serde_json::Value::as_bool))
        .unwrap_or(true)
}

pub fn save_dedicated(dir: &std::path::Path, on: bool) -> std::io::Result<()> {
    std::fs::write(
        dir.join(FILE),
        serde_json::json!({ "dedicated": on }).to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choice_is_kept_and_on_by_default() {
        let dir = std::env::temp_dir().join(format!("morpheus-gpu-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        assert!(load_dedicated(&dir));
        save_dedicated(&dir, false).expect("save");
        assert!(!load_dedicated(&dir));
        save_dedicated(&dir, true).expect("save");
        assert!(load_dedicated(&dir));
        std::fs::remove_dir_all(&dir).ok();
    }
}
