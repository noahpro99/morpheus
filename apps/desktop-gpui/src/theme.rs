//! Theme tokens, from `crates/ui/assets/theme.css`. The colours come from the
//! chosen theme's palette (`view_model::theme`: dark, midnight, light); the
//! sizes are the same in all of them. A colour is a function, read each time
//! something is drawn, so choosing a theme changes the next frame.

use std::sync::atomic::{AtomicU8, Ordering};

use crate::view_model::{
    settings::Theme,
    theme::{Palette, swatch},
};

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Draw with `theme` from now on.
pub fn set(theme: Theme) {
    CURRENT.store(Theme::ALL.iter().position(|t| *t == theme).unwrap_or(0) as u8, Ordering::Relaxed);
}

pub fn current() -> Theme {
    Theme::ALL
        .get(usize::from(CURRENT.load(Ordering::Relaxed)))
        .copied()
        .unwrap_or_default()
}

fn palette() -> Palette {
    Palette::of(current())
}

/// The swatch of a theme's option in Settings: (sidebar, chat).
pub fn swatch_of(theme: Theme) -> (u32, u32) {
    swatch(theme)
}

macro_rules! colours {
    ($($name:ident),* $(,)?) => {
        $(
            #[inline]
            pub fn $name() -> u32 {
                palette().$name
            }
        )*
    };
}

colours!(
    bg_rail, bg_sidebar, bg_chat, bg_input, bg_elevated, bg_hover, bg_selected, edge, ok, warn, text, text_muted,
    text_faint, accent, accent_strong, danger, own, link, green, accent_tint,
);

/// `color` (0xRRGGBB) with alpha `a`, as 0xRRGGBBAA: a tint of a token.
pub fn alpha(color: u32, a: u8) -> u32 {
    (color << 8) | u32::from(a)
}

pub const RADIUS: f32 = 8.0;
/// `html { font-size: 15px }`: what 1rem is.
pub const REM: f32 = 15.0;
pub const RADIUS_SM: f32 = 6.0;
pub const RADIUS_LG: f32 = 12.0;
pub const RADIUS_XL: f32 = 16.0;
pub const RAIL_W: f32 = 72.0;
pub const SIDEBAR_W: f32 = 248.0;
pub const BOTTOM_H: f32 = 50.0;
pub const BOTTOM_GAP: f32 = 12.0;
/// `--mono`.
pub const MONO: &str = "JetBrains Mono";
/// What body text is: Inter at `html { font-size: 15px; line-height: 1.4 }`.
pub const LINE_HEIGHT: f32 = 1.4;
pub const MEMBERS_W: f32 = 232.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choosing_a_theme_changes_the_colours_read_next() {
        set(Theme::Light);
        assert_eq!(bg_chat(), 0xffffff);
        assert_eq!(text(), 0x1c1f29);
        set(Theme::Midnight);
        assert_eq!(bg_chat(), 0x0a0a0c);
        set(Theme::Dark);
        assert_eq!(bg_chat(), 0x1a1e29);
        assert_eq!(current(), Theme::Dark);
    }
}

/// Every colour a screen draws comes from a token: a hex literal in the
/// source is only white or black (text on the accent, scrims and shadows) or
/// the profile banner's gradient end, which no theme changes.
#[cfg(test)]
mod source_tests {
    use std::path::Path;

    fn sources(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("src dir").flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n != "e2e") {
                    sources(&path, found);
                }
            } else if path.extension().is_some_and(|e| e == "rs") && path.file_name().is_some_and(|n| n != "theme.rs") {
                found.push(path);
            }
        }
    }

    #[test]
    fn no_hex_colours_outside_the_theme() {
        let mut files = Vec::new();
        sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
        let mut stray = Vec::new();
        for file in files {
            let text = std::fs::read_to_string(&file).expect("source");
            // Unit tests may name colours; the code above them may not.
            let code = text.split("#[cfg(test)]").next().unwrap_or_default();
            for (n, line) in code.lines().enumerate() {
                let mut rest = line;
                while let Some(at) = rest.find("0x") {
                    let digits: String = rest[at + 2..].chars().take_while(char::is_ascii_hexdigit).collect();
                    rest = &rest[at + 2 + digits.len()..];
                    let colour = matches!(digits.len(), 6 | 8);
                    let fixed = digits.starts_with("ffffff") || digits.starts_with("000000") || digits == "2d3a9e";
                    if colour && !fixed {
                        stray.push(format!("{}:{}: 0x{digits}", file.display(), n + 1));
                    }
                }
            }
        }
        assert!(stray.is_empty(), "colours that should be tokens:\n{}", stray.join("\n"));
    }
}
