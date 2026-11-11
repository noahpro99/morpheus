//! Lucide glyphs (ISC licence), the ones the shell draws, from
//! `crates/ui/src/icons.rs`. [`svg_for`] wraps one into a whole SVG file for
//! the asset source; [`icon`] draws it in the current text colour.

use gpui::{Styled as _, Svg, px, svg};

pub const PLUS: &str = r#"<path d="M5 12h14" /> <path d="M12 5v14" />"#;
pub const CHEVRON_DOWN: &str = r#"<path d="m6 9 6 6 6-6" />"#;
pub const CHEVRON_RIGHT: &str = r#"<path d="m9 18 6-6-6-6" />"#;
pub const FOLDER: &str = r#"<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />"#;
pub const LOCK: &str = r#"<rect width="18" height="11" x="3" y="11" rx="2" ry="2" /> <path d="M7 11V7a5 5 0 0 1 10 0v4" />"#;
pub const LOCK_OPEN: &str = r#"<rect width="18" height="11" x="3" y="11" rx="2" ry="2" /> <path d="M7 11V7a5 5 0 0 1 9.9-1" />"#;
pub const SETTINGS: &str = r#"<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" /> <circle cx="12" cy="12" r="3" />"#;
pub const ELLIPSIS: &str = r#"<circle cx="12" cy="12" r="1" /> <circle cx="19" cy="12" r="1" /> <circle cx="5" cy="12" r="1" />"#;
pub const PIN: &str = r#"<path d="M12 17v5" /> <path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z" />"#;
pub const MIC: &str = r#"<path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" /> <path d="M19 10v2a7 7 0 0 1-14 0v-2" /> <line x1="12" x2="12" y1="19" y2="22" />"#;
pub const MIC_OFF: &str = r#"<line x1="2" x2="22" y1="2" y2="22" /> <path d="M18.89 13.23A7.12 7.12 0 0 0 19 12v-2" /> <path d="M5 10v2a7 7 0 0 0 12 5" /> <path d="M15 9.34V5a3 3 0 0 0-5.68-1.33" /> <path d="M9 9v3a3 3 0 0 0 5.12 2.12" /> <line x1="12" x2="12" y1="19" y2="22" />"#;
pub const HEADPHONES: &str = r#"<path d="M3 14h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a9 9 0 0 1 18 0v7a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3" />"#;
pub const HEADPHONE_OFF: &str = r#"<path d="M21 14h-1.343" /> <path d="M9.128 3.47A9 9 0 0 1 21 12v3.343" /> <path d="m2 2 20 20" /> <path d="M20.414 20.414A2 2 0 0 1 19 21h-1a2 2 0 0 1-2-2v-3" /> <path d="M3 14h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a9 9 0 0 1 2.636-6.364" />"#;
pub const VOLUME_2: &str = r#"<path d="M11 4.702a.705.705 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.705.705 0 0 0 11 19.298z" /> <path d="M16 9a5 5 0 0 1 0 6" /> <path d="M19.364 18.364a9 9 0 0 0 0-12.728" />"#;
pub const STAR: &str = r#"<path d="M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.396 21.01a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.16 9.795a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z" />"#;
pub const PAPERCLIP: &str = r#"<path d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48" />"#;
pub const PENCIL: &str = r#"<path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z" /> <path d="m15 5 4 4" />"#;
pub const REPLY: &str =
    r#"<polyline points="9 17 4 12 9 7" /> <path d="M20 18v-2a4 4 0 0 0-4-4H4" />"#;
pub const SMILE: &str = r#"<circle cx="12" cy="12" r="10" /> <path d="M8 14s1.5 2 4 2 4-2 4-2" /> <line x1="9" x2="9.01" y1="9" y2="9" /> <line x1="15" x2="15.01" y1="9" y2="9" />"#;
pub const X: &str = r#"<path d="M18 6 6 18" /> <path d="m6 6 12 12" />"#;

/// The glyph named by an asset path `icons/<name>.svg`.
fn glyph(name: &str) -> Option<&'static str> {
    Some(match name {
        "plus" => PLUS,
        "chevron-down" => CHEVRON_DOWN,
        "chevron-right" => CHEVRON_RIGHT,
        "folder" => FOLDER,
        "lock" => LOCK,
        "lock-open" => LOCK_OPEN,
        "settings" => SETTINGS,
        "ellipsis" => ELLIPSIS,
        "pin" => PIN,
        "mic" => MIC,
        "mic-off" => MIC_OFF,
        "headphones" => HEADPHONES,
        "headphone-off" => HEADPHONE_OFF,
        "volume-2" => VOLUME_2,
        "star" => STAR,
        "paperclip" => PAPERCLIP,
        "pencil" => PENCIL,
        "reply" => REPLY,
        "smile" => SMILE,
        "x" => X,
        _ => return None,
    })
}

/// The SVG file for an asset path like `icons/plus.svg`.
pub fn svg_for(path: &str) -> Option<String> {
    let name = path.strip_prefix("icons/")?.strip_suffix(".svg")?;
    Some(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{}</svg>"#,
        glyph(name)?
    ))
}

/// A glyph (`"plus"`, `"lock"`, ...) at `size` pixels, in the text colour.
pub fn icon(name: &str, size: f32) -> Svg {
    svg()
        .path(format!("icons/{name}.svg"))
        .size(px(size))
        .flex_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_makes_an_svg() {
        assert!(svg_for("icons/plus.svg").is_some_and(|s| s.contains("M5 12h14")));
        assert!(svg_for("icons/nope.svg").is_none());
        assert!(svg_for("logo.svg").is_none());
    }
}
