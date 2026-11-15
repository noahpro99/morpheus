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
    /// A chosen option's fill, dark enough for white text (a contrast of 4.5 or more).
    pub accent_solid: u32,
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
    accent_solid: 0x4b5ce6,
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
    accent_solid: 0x4f5fe6,
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


// ---- a custom colour -----------------------------------------------------------------

/// A colour typed as `#rgb`, `#rrggbb` (the `#` is optional), as 0xRRGGBB.
pub fn parse_hex(text: &str) -> Option<u32> {
    let t = text.trim().trim_start_matches('#');
    if !t.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match t.len() {
        6 => u32::from_str_radix(t, 16).ok(),
        3 => {
            let v = u32::from_str_radix(t, 16).ok()?;
            let (r, g, b) = ((v >> 8) & 0xf, (v >> 4) & 0xf, v & 0xf);
            Some(((r * 17) << 16) | ((g * 17) << 8) | (b * 17))
        }
        _ => None,
    }
}

/// `0xRRGGBB` as `#rrggbb`.
pub fn hex(color: u32) -> String {
    format!("#{color:06x}")
}

fn channels(c: u32) -> [f32; 3] {
    [((c >> 16) & 0xff) as f32, ((c >> 8) & 0xff) as f32, (c & 0xff) as f32]
}

fn pack(c: [f32; 3]) -> u32 {
    let b = |v: f32| v.round().clamp(0.0, 255.0) as u32;
    (b(c[0]) << 16) | (b(c[1]) << 8) | b(c[2])
}

/// `a` moved `t` (0 to 1) of the way to `b`.
pub fn mix(a: u32, b: u32, t: f32) -> u32 {
    let (a, b) = (channels(a), channels(b));
    pack([0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t))
}

/// WCAG relative luminance.
fn luminance(c: u32) -> f32 {
    let [r, g, b] = channels(c).map(|v| {
        let v = v / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    });
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// WCAG contrast ratio of two colours (1 to 21).
pub fn contrast(a: u32, b: u32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn to_hsl(c: u32) -> (f32, f32, f32) {
    let [r, g, b] = channels(c).map(|v| v / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s, l)
}

/// A colour from hue (degrees), saturation and lightness (0 to 1).
pub fn from_hsl(h: f32, s: f32, l: f32) -> u32 {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h.rem_euclid(360.0) / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    pack([(r + m) * 255.0, (g + m) * 255.0, (b + m) * 255.0])
}

/// The whole palette from one colour: the chat is the colour, the rail, sidebar and inputs
/// darker shades of it, panels a step lighter (darker, on a light colour). Text is white
/// unless white would fail WCAG contrast (4.5) against a background, then black (muted
/// text dark grey). Washes, borders and the accent follow the text's side.
pub fn derive(base: u32) -> Palette {
    const BLACK: u32 = 0x000000;
    const WHITE: u32 = 0xffffff;
    let (bg_rail, bg_sidebar, bg_input) = (
        mix(base, BLACK, 0.45),
        mix(base, BLACK, 0.22),
        mix(base, BLACK, 0.32),
    );
    let lighter = mix(base, WHITE, 0.08);
    let white_text = [bg_rail, bg_sidebar, bg_input, base, lighter]
        .into_iter()
        .all(|bg| contrast(WHITE, bg) >= 4.5);
    let bg_elevated = if white_text { lighter } else { mix(base, BLACK, 0.10) };
    let (hue, sat, _) = to_hsl(base);
    let sat = sat.clamp(0.55, 0.9);
    let (wash, edge) = if white_text { (0xffffff00, 0xffffff0f) } else { (0x00000000, 0x0000000f) };
    let accent = from_hsl(hue, sat, if white_text { 0.70 } else { 0.38 });
    // A chosen option's fill: white text on it must pass too.
    let mut solid_l = 0.45;
    let mut accent_solid = from_hsl(hue, sat, solid_l);
    while contrast(WHITE, accent_solid) < 4.5 && solid_l > 0.05 {
        solid_l -= 0.02;
        accent_solid = from_hsl(hue, sat, solid_l);
    }
    let (_, _, accent_l) = to_hsl(accent);
    Palette {
        bg_rail,
        bg_sidebar,
        bg_chat: base,
        bg_members: bg_sidebar,
        bg_elevated,
        bg_input,
        bg_hover: wash | 0x0a,
        bg_selected: wash | 0x14,
        text: if white_text { WHITE } else { BLACK },
        text_muted: if white_text { 0xc2c6d2 } else { 0x444444 },
        text_faint: if white_text { 0x9096a8 } else { 0x666666 },
        accent,
        accent_strong: from_hsl(hue, sat, if white_text { accent_l - 0.08 } else { accent_l - 0.06 }),
        accent_solid,
        own: from_hsl(hue, sat, if white_text { 0.82 } else { 0.30 }),
        link: accent,
        edge,
        accent_tint: (accent << 8) | 0x26,
        ..DARK
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

    #[test]
    fn hex_colours_parse_with_or_without_hash_and_short_form() {
        assert_eq!(parse_hex("#2a4d8f"), Some(0x2a4d8f));
        assert_eq!(parse_hex("2A4D8F"), Some(0x2a4d8f));
        assert_eq!(parse_hex(" #fa0 "), Some(0xffaa00));
        for bad in ["", "#", "#12", "#12345", "#1234567", "#gggggg", "red"] {
            assert_eq!(parse_hex(bad), None, "{bad}");
        }
        assert_eq!(hex(0x0a0b0c), "#0a0b0c");
    }

    #[test]
    fn a_derived_palette_has_the_colour_as_chat_and_darker_shades_around_it() {
        let p = derive(0x2a4d8f);
        assert_eq!(p.bg_chat, 0x2a4d8f);
        assert!(luminance(p.bg_rail) < luminance(p.bg_sidebar));
        assert!(luminance(p.bg_sidebar) < luminance(p.bg_chat));
        assert!(luminance(p.bg_input) < luminance(p.bg_chat));
        assert!(luminance(p.bg_elevated) > luminance(p.bg_chat));
        assert_eq!(p.bg_members, p.bg_sidebar);
        // Hover and edge are white washes on a dark colour, as in the dark theme.
        assert_eq!(p.bg_hover, 0xffffff0a);
        assert_eq!(p.edge >> 8, 0xffffff);
        assert_eq!(p.accent_tint & 0xff, 0x26);
        // The accent keeps the colour's hue.
        assert!((to_hsl(p.accent).0 - to_hsl(0x2a4d8f).0).abs() < 6.0);
    }

    #[test]
    fn text_is_white_unless_white_fails_contrast_and_then_black_with_dark_grey_muted() {
        for dark in [0x000000, 0x2a4d8f, 0x5b2a86, 0x1a1e29, 0x7a1f1f] {
            let p = derive(dark);
            assert_eq!(p.text, 0xffffff, "{dark:06x}");
            for bg in [p.bg_rail, p.bg_sidebar, p.bg_chat, p.bg_input, p.bg_elevated] {
                assert!(contrast(p.text, bg) >= 4.5, "{dark:06x} on {bg:06x}");
            }
        }
        for light in [0xffffff, 0xf2e9c9, 0xcfe8ff, 0xffe066] {
            let p = derive(light);
            assert_eq!(p.text, 0x000000, "{light:06x}");
            assert_eq!(p.text_muted, 0x444444);
            assert!(contrast(p.text, p.bg_chat) >= 4.5, "{light:06x}");
            assert_eq!(p.bg_hover >> 8, 0x000000);
        }
    }

    #[test]
    fn the_accent_fill_always_holds_white_text() {
        for c in [0x000000, 0xffffff, 0xffff00, 0x00ff00, 0x2a4d8f, 0xf2e9c9, 0x808080] {
            let p = derive(c);
            assert!(contrast(0xffffff, p.accent_solid) >= 4.5, "{c:06x}");
        }
    }
}
