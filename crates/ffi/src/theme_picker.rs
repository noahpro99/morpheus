//! The colour picker of Settings > Appearance (#446): hue, saturation and value sliders, the theme a colour would make drawn as a small
//! preview, and the colours used lately. The sums and the palette are `morpheus_view_model::theme`, the desktop's.

use morpheus_view_model::theme::{self, derive, from_hsv, parse_hex, to_hsv};

use crate::Morpheus;

#[derive(Debug, Clone, uniffi::Record)]
pub struct Hsv {
    /// Degrees, 0 up to 360.
    pub h: f32,
    pub s: f32,
    pub v: f32,
}

/// What a custom colour makes of the app, as `0xRRGGBB` fills: enough to draw a small chat in it.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ThemePreview {
    pub rail: u32,
    pub sidebar: u32,
    pub chat: u32,
    pub elevated: u32,
    pub text: u32,
    pub muted: u32,
    pub accent: u32,
    pub accent_solid: u32,
}

/// The sliders' positions for a colour typed as `#rgb` or `#rrggbb`; `None` when it is not one.
#[uniffi::export]
pub fn colour_hsv(text: String) -> Option<Hsv> {
    parse_hex(&text).map(|c| {
        let (h, s, v) = to_hsv(c);
        Hsv { h, s, v }
    })
}

/// The colour (as six hex digits, no `#`) at these slider positions.
#[uniffi::export]
pub fn colour_from_hsv(h: f32, s: f32, v: f32) -> String {
    format!("{:06x}", from_hsv(h, s, v))
}

/// The palette a custom colour derives, for the preview; `None` when `text` is not a colour.
#[uniffi::export]
pub fn theme_preview(text: String) -> Option<ThemePreview> {
    parse_hex(&text).map(|c| {
        let p = derive(c);
        ThemePreview {
            rail: p.bg_rail,
            sidebar: p.bg_sidebar,
            chat: p.bg_chat,
            elevated: p.bg_elevated,
            text: p.text,
            muted: p.text_muted,
            accent: p.accent,
            accent_solid: p.accent_solid,
        }
    })
}

const FILE: &str = "recent-colours.json";
const KEPT: usize = 8;

#[uniffi::export]
impl Morpheus {
    /// The custom colours used lately, newest first (six hex digits each).
    pub fn recent_colours(&self) -> Vec<String> {
        std::fs::read(self.dir.join(FILE)).ok().and_then(|b| serde_json::from_slice::<Vec<String>>(&b).ok()).unwrap_or_default()
    }

    /// `text` was applied as the custom colour: first among the recent ones.
    pub fn note_recent_colour(&self, text: String) {
        let Some(c) = parse_hex(&text) else { return };
        let mine = format!("{c:06x}");
        let mut kept = self.recent_colours();
        kept.retain(|k| *k != mine);
        kept.insert(0, mine);
        kept.truncate(KEPT);
        let _ = std::fs::write(self.dir.join(FILE), serde_json::to_vec(&kept).unwrap_or_default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sliders_and_hex_agree() {
        let h = colour_hsv("#2a4d8f".into()).expect("a colour");
        assert_eq!(colour_from_hsv(h.h, h.s, h.v), "2a4d8f");
        assert!(colour_hsv("blue-ish".into()).is_none());
        assert_eq!(colour_from_hsv(0.0, 1.0, 1.0), "ff0000");
    }

    #[test]
    fn the_preview_is_the_derived_theme() {
        let p = theme_preview("2a4d8f".into()).expect("a preview");
        assert_eq!(p.chat, 0x2a4d8f);
        assert!(theme_preview("nope".into()).is_none());
    }
}
