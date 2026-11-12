//! The colour themes' palettes, from `crates/ui/assets/theme.css`: Dark is
//! the base (`:root`), Midnight and Light override some of its tokens.
//! Colours are `0xRRGGBB`, or `0xRRGGBBAA` where the token has an alpha.

use crate::settings::Theme;

/// Every colour token a screen draws with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub bg_rail: u32,
    pub bg_sidebar: u32,
    pub bg_chat: u32,
    pub bg_members: u32,
    pub bg_elevated: u32,
    pub bg_input: u32,
    /// `--bg-hover` and `--bg-selected`: translucent washes (RRGGBBAA).
    pub bg_hover: u32,
    pub bg_selected: u32,
    pub text: u32,
    pub text_muted: u32,
    pub text_faint: u32,
    pub accent: u32,
    pub accent_strong: u32,
    pub danger: u32,
    /// `--own`: the colour of your own name.
    pub own: u32,
    /// `--moon`: the logo's lockup.
    pub moon: u32,
    /// `.body a`: links in messages (the same in every theme, as in the CSS).
    pub link: u32,
    /// `--green`.
    pub green: u32,
    /// Good and cautionary states: a verified device, encryption on, a warning.
    pub ok: u32,
    pub warn: u32,
    /// The faint line around cards and menus (white on dark, black on light), RRGGBBAA.
    pub edge: u32,
    /// `rgba(124, 140, 255, 0.15)` (RRGGBBAA): a reaction of yours, a jumped-to message.
    pub accent_tint: u32,
}

/// The base palette (`:root`).
pub const DARK: Palette = Palette {
    bg_rail: 0x0d0f14,
    bg_sidebar: 0x141720,
    bg_chat: 0x1a1e29,
    bg_members: 0x141720,
    bg_elevated: 0x222736,
    bg_input: 0x0f1118,
    bg_hover: 0xffffff0a,
    bg_selected: 0xffffff14,
    text: 0xe6e8ee,
    text_muted: 0x8a90a2,
    text_faint: 0x5c6274,
    accent: 0x7c8cff,
    accent_strong: 0x5f72ff,
    danger: 0xff6b6b,
    own: 0xb8c0ff,
    moon: 0xb8c0ff,
    link: 0x8fa0ff,
    green: 0x23a559,
    ok: 0x4fd1a5,
    warn: 0xffb020,
    edge: 0xffffff0f,
    accent_tint: 0x7c8cff26,
};

/// `:root[data-theme="midnight"]`: black surfaces.
pub const MIDNIGHT: Palette = Palette {
    bg_rail: 0x000000,
    bg_sidebar: 0x050506,
    bg_chat: 0x0a0a0c,
    bg_members: 0x050506,
    bg_elevated: 0x17181d,
    bg_input: 0x000000,
    ..DARK
};

/// `:root[data-theme="light"]`.
pub const LIGHT: Palette = Palette {
    bg_rail: 0xe3e6ee,
    bg_sidebar: 0xeef0f5,
    bg_chat: 0xffffff,
    bg_members: 0xeef0f5,
    bg_elevated: 0xe2e5ee,
    bg_input: 0xf2f3f7,
    bg_hover: 0x0000000a,
    bg_selected: 0x00000014,
    edge: 0x0000000f,
    text: 0x1c1f29,
    text_muted: 0x5b6172,
    text_faint: 0x8d93a3,
    accent: 0x4f5fe6,
    accent_strong: 0x3d4fe0,
    own: 0x3d4fe0,
    ..DARK
};

impl Palette {
    pub fn of(theme: Theme) -> Palette {
        match theme {
            Theme::Dark => DARK,
            Theme::Midnight => MIDNIGHT,
            Theme::Light => LIGHT,
        }
    }
}

/// The swatch a theme's option shows in Settings: (sidebar, chat), as the
/// CSS paints `.theme-swatch-side` and `.theme-swatch-chat`.
pub fn swatch(theme: Theme) -> (u32, u32) {
    match theme {
        Theme::Dark => (0x141720, 0x1a1e29),
        Theme::Midnight => (0x000000, 0x0a0a0c),
        Theme::Light => (0xeef0f5, 0xffffff),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(rgb: u32) -> u32 {
        let (r, g, b) = ((rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff);
        r + g + b
    }

    #[test]
    fn dark_is_the_base_and_midnight_only_darkens_surfaces() {
        assert_eq!(Palette::of(Theme::Dark), DARK);
        assert_eq!(MIDNIGHT.text, DARK.text);
        assert_eq!(MIDNIGHT.accent, DARK.accent);
        assert!(luminance(MIDNIGHT.bg_chat) < luminance(DARK.bg_chat));
        assert_eq!(MIDNIGHT.bg_rail, 0x000000);
    }

    #[test]
    fn light_flips_surfaces_text_and_the_washes() {
        assert!(luminance(LIGHT.bg_chat) > luminance(DARK.bg_chat));
        assert!(luminance(LIGHT.text) < luminance(DARK.text));
        // The hover wash is black on light, white on dark.
        assert_eq!(LIGHT.bg_hover >> 8, 0x000000);
        assert_eq!(DARK.bg_hover >> 8, 0xffffff);
        assert_eq!(LIGHT.danger, DARK.danger);
        assert_eq!(LIGHT.link, DARK.link);
    }

    #[test]
    fn the_swatches_match_the_themes_css() {
        assert_eq!(swatch(Theme::Dark), (0x141720, 0x1a1e29));
        assert_eq!(swatch(Theme::Midnight), (0x000000, 0x0a0a0c));
        assert_eq!(swatch(Theme::Light), (0xeef0f5, 0xffffff));
    }
}
