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

/// A colour as hue (degrees, 0 up to 360), saturation and value (0 to 1).
pub fn to_hsv(c: u32) -> (f32, f32, f32) {
    let [r, g, b] = channels(c).map(|v| v / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    if d == 0.0 {
        return (0.0, 0.0, max);
    }
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, d / max, max)
}

/// A colour from hue (degrees; 360 is 0), saturation and value (0 to 1, clamped).
pub fn from_hsv(h: f32, s: f32, v: f32) -> u32 {
    let (s, v) = (s.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = v - c;
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

/// `0xRRGGBBAA` laid over the opaque `bg`: what shows where a translucent wash is drawn.
pub fn over(rgba: u32, bg: u32) -> u32 {
    mix(bg, rgba >> 8, (rgba & 0xff) as f32 / 255.0)
}

/// Black or white, whichever reads better on `bg` (for text on any colour: a swatch,
/// a badge, a button).
pub fn on_colour(bg: u32) -> u32 {
    if contrast(0xffffff, bg) >= contrast(0x000000, bg) { 0xffffff } else { 0x000000 }
}

/// `start` with its lightness moved toward white (`lighter`) or black, hue and saturation
/// kept, until `passes` accepts it; at the end of the scale, the white or black itself.
fn nudge(start: u32, lighter: bool, passes: impl Fn(u32) -> bool) -> u32 {
    let (h, s, l) = to_hsl(start);
    let mut l = l;
    loop {
        let c = from_hsl(h, s, l);
        if passes(c) {
            return c;
        }
        l += if lighter { 0.01 } else { -0.01 };
        if !(0.0..=1.0).contains(&l) {
            return if lighter { 0xffffff } else { 0x000000 };
        }
    }
}

impl Palette {
    /// The foreground (text, icon) on the accent, `accent_strong` and `accent_solid`
    /// fills (a primary button, a selected rail item, a chosen option), on the
    /// danger fill, and on the green one: black or white by contrast.
    pub fn on_accent(&self) -> u32 {
        on_colour(self.accent)
    }
    pub fn on_accent_strong(&self) -> u32 {
        on_colour(self.accent_strong)
    }
    pub fn on_accent_solid(&self) -> u32 {
        on_colour(self.accent_solid)
    }
    pub fn on_danger(&self) -> u32 {
        on_colour(self.danger)
    }
    pub fn on_green(&self) -> u32 {
        on_colour(self.green)
    }
}

/// The Material 3 colour roles of a palette (`0xRRGGBB`): what the Android app's
/// `MaterialTheme` is built from. Every `on_*` is picked by contrast, never
/// hardcoded: black or white by [`on_colour`] on a coloured fill, and the
/// theme's own text colours on the surfaces they read on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterialScheme {
    pub primary: u32,
    pub on_primary: u32,
    pub primary_container: u32,
    pub on_primary_container: u32,
    pub secondary: u32,
    pub on_secondary: u32,
    pub secondary_container: u32,
    pub on_secondary_container: u32,
    pub tertiary: u32,
    pub on_tertiary: u32,
    pub tertiary_container: u32,
    pub on_tertiary_container: u32,
    pub error: u32,
    pub on_error: u32,
    pub error_container: u32,
    pub on_error_container: u32,
    pub background: u32,
    pub on_background: u32,
    pub surface: u32,
    pub on_surface: u32,
    pub surface_variant: u32,
    /// Secondary text: reads on every surface below.
    pub on_surface_variant: u32,
    pub surface_container_lowest: u32,
    pub surface_container_low: u32,
    pub surface_container: u32,
    pub surface_container_high: u32,
    pub surface_container_highest: u32,
    pub inverse_surface: u32,
    pub inverse_on_surface: u32,
    pub inverse_primary: u32,
    pub outline: u32,
    pub outline_variant: u32,
    pub scrim: u32,
}

impl MaterialScheme {
    /// Every (role, fill, foreground drawn on it).
    pub fn pairs(&self) -> Vec<(&'static str, u32, u32)> {
        vec![
            ("primary", self.primary, self.on_primary),
            ("primary_container", self.primary_container, self.on_primary_container),
            ("secondary", self.secondary, self.on_secondary),
            ("secondary_container", self.secondary_container, self.on_secondary_container),
            ("tertiary", self.tertiary, self.on_tertiary),
            ("tertiary_container", self.tertiary_container, self.on_tertiary_container),
            ("error", self.error, self.on_error),
            ("error_container", self.error_container, self.on_error_container),
            ("background", self.background, self.on_background),
            ("surface", self.surface, self.on_surface),
            ("surface_variant", self.surface_variant, self.on_surface_variant),
            ("surface_container_lowest", self.surface_container_lowest, self.on_surface),
            ("surface_container_low", self.surface_container_low, self.on_surface),
            ("surface_container", self.surface_container, self.on_surface),
            ("surface_container_high", self.surface_container_high, self.on_surface),
            ("surface_container_highest", self.surface_container_highest, self.on_surface),
            ("inverse_surface", self.inverse_surface, self.inverse_on_surface),
        ]
    }

    /// The surfaces text sits on.
    pub fn surfaces(&self) -> Vec<(&'static str, u32)> {
        vec![
            ("background", self.background),
            ("surface", self.surface),
            ("surface_variant", self.surface_variant),
            ("surface_container_lowest", self.surface_container_lowest),
            ("surface_container_low", self.surface_container_low),
            ("surface_container", self.surface_container),
            ("surface_container_high", self.surface_container_high),
            ("surface_container_highest", self.surface_container_highest),
        ]
    }
}

impl Palette {
    /// The Material 3 roles of this palette.
    pub fn material(&self) -> MaterialScheme {
        const NEEDED: f32 = 4.5;
        // A tinted container: the elevated surface with a share of `hue` in it.
        let container = |hue: u32| mix(self.bg_elevated, hue, 0.28);
        let primary_container = container(self.accent_solid);
        let secondary_container = container(self.accent);
        let tertiary_container = container(self.ok);
        let error_container = container(self.danger);
        let surface_container_lowest = self.bg_input;
        let surface_container_low = self.bg_sidebar;
        let surface_container = self.bg_sidebar;
        let surface_container_high = self.bg_elevated;
        let surface_container_highest = mix(self.bg_elevated, self.text, 0.06);
        let surface_variant = self.bg_elevated;
        let surfaces = [
            self.bg_rail,
            self.bg_chat,
            surface_variant,
            surface_container_lowest,
            surface_container_low,
            surface_container,
            surface_container_high,
            surface_container_highest,
        ];
        // The theme's own text where it reads on every surface, else black or white by contrast.
        let reads_everywhere = |fg: u32| surfaces.iter().all(|s| contrast(fg, *s) >= NEEDED);
        let on_surface = if reads_everywhere(self.text) { self.text } else { on_colour(self.bg_chat) };
        let on_surface_variant = if reads_everywhere(self.text_muted) { self.text_muted } else { on_surface };
        let inverse_surface = self.text;
        let on_inverse = on_colour(inverse_surface);
        MaterialScheme {
            primary: self.accent_solid,
            on_primary: self.on_accent_solid(),
            primary_container,
            on_primary_container: on_colour(primary_container),
            secondary: self.accent,
            on_secondary: self.on_accent(),
            secondary_container,
            on_secondary_container: on_colour(secondary_container),
            tertiary: self.ok,
            on_tertiary: on_colour(self.ok),
            tertiary_container,
            on_tertiary_container: on_colour(tertiary_container),
            error: self.danger,
            on_error: self.on_danger(),
            error_container,
            on_error_container: on_colour(error_container),
            background: self.bg_rail,
            on_background: on_surface,
            surface: self.bg_chat,
            on_surface,
            surface_variant,
            on_surface_variant,
            surface_container_lowest,
            surface_container_low,
            surface_container,
            surface_container_high,
            surface_container_highest,
            inverse_surface,
            inverse_on_surface: on_inverse,
            inverse_primary: self.accent,
            outline: self.text_muted,
            outline_variant: mix(self.bg_chat, self.text, 0.16),
            scrim: 0x000000,
        }
    }
}

/// A key cap drawn on a coloured fill: the text colour, and the border and
/// background washes (0xRRGGBBAA).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapColours {
    pub text: u32,
    pub border: u32,
    pub bg: u32,
}

/// The cap on `fill`: text black or white by contrast, with a wash of the
/// opposite colour behind it (so the wash only raises the contrast) and an
/// edge of the text colour.
pub fn cap_on(fill: u32) -> CapColours {
    let text = on_colour(fill);
    let opposite = text ^ 0xffffff;
    CapColours { text, border: (text << 8) | 0x66, bg: (opposite << 8) | 0x26 }
}

impl Palette {
    /// The Jump to present pill's (fill, text) at rest and hovered: each state's
    /// text is picked against that state's own fill.
    pub fn jump_pill(&self, hovered: bool) -> (u32, u32) {
        if hovered {
            (self.accent, self.on_accent())
        } else {
            (self.accent_strong, self.on_accent_strong())
        }
    }

    /// Every coloured fill that carries text or an icon, as (name, fill,
    /// foreground drawn on it).
    pub fn fills(&self) -> Vec<(&'static str, u32, u32)> {
        let on = on_colour;
        vec![
            ("accent", self.accent, self.on_accent()),
            ("accent_strong", self.accent_strong, self.on_accent_strong()),
            ("accent_solid", self.accent_solid, self.on_accent_solid()),
            ("danger", self.danger, self.on_danger()),
            ("green", self.green, self.on_green()),
            ("ok", self.ok, on(self.ok)),
            ("warn", self.warn, on(self.warn)),
            ("own", self.own, on(self.own)),
            ("link", self.link, on(self.link)),
        ]
    }
}

/// `start` mixed toward `fg` until `passes` accepts it (muted and faint text: grey that
/// gets as strong as it needs to).
fn strengthen(start: u32, fg: u32, passes: impl Fn(u32) -> bool) -> u32 {
    let mut t = 0.0;
    loop {
        let c = mix(start, fg, t);
        if passes(c) || t >= 1.0 {
            return c;
        }
        t += 0.02;
    }
}

impl Palette {
    /// Every background text is drawn on: each surface, bare and under the hover and
    /// selected washes and the accent tint (flattened, as they show).
    pub fn text_backgrounds(&self) -> Vec<u32> {
        [self.bg_rail, self.bg_sidebar, self.bg_chat, self.bg_members, self.bg_elevated, self.bg_input]
            .into_iter()
            .flat_map(|s| [s, over(self.bg_hover, s), over(self.bg_selected, s), over(self.accent_tint, s)])
            .collect()
    }

    /// Every (name, text, background) pair of a custom palette that must read (a contrast
    /// of 4.5 or more): each kind of text on every surface, the text on a selected rail
    /// item (`accent_strong`), and white on `accent_solid`, a button's fill.
    pub fn text_pairs(&self) -> Vec<(&'static str, u32, u32)> {
        let mut pairs = Vec::new();
        for (name, fg) in [
            ("text", self.text),
            ("text_muted", self.text_muted),
            ("text_faint", self.text_faint),
            ("accent", self.accent),
            ("link", self.link),
            ("own", self.own),
            ("moon", self.moon),
            ("danger", self.danger),
            ("ok", self.ok),
            ("warn", self.warn),
        ] {
            pairs.extend(self.text_backgrounds().into_iter().map(|bg| (name, fg, bg)));
        }
        pairs.push(("text on accent_strong", self.text, self.accent_strong));
        pairs.push(("white on accent_solid", 0xffffff, self.accent_solid));
        pairs
    }
}

/// The whole palette from one colour: the chat is the colour, the rail, sidebar and inputs
/// darker shades of it, panels a step lighter (darker, on a light colour). Text is white
/// unless white would fail WCAG contrast (4.5) against the colour, then black. Everything
/// drawn as text (muted and faint text, the accent, links, names, warnings) is nudged in
/// lightness, its hue kept, until it reads (4.5) on every surface it can be on, washes and
/// tint included (see [`Palette::text_pairs`]); surfaces that would not hold the text
/// are moved away from it.
pub fn derive(base: u32) -> Palette {
    const BLACK: u32 = 0x000000;
    const WHITE: u32 = 0xffffff;
    const NEEDED: f32 = 4.5;
    let reads = |fg: u32, bg: u32| contrast(fg, bg) >= NEEDED;
    let white_text = reads(WHITE, base);
    let (fg, away) = if white_text { (WHITE, BLACK) } else { (BLACK, WHITE) };
    // The washes go the usual way (white on dark, black on light) unless that would
    // put the text under its minimum on the colour itself.
    let mut wash = if white_text { WHITE } else { BLACK };
    if !reads(fg, over(wash << 8 | 0x14, base)) {
        wash = away;
    }
    let (hover, selected) = (wash << 8 | 0x0a, wash << 8 | 0x14);
    // A surface the text reads on, bare and under both washes.
    let holds = |s: u32| reads(fg, s) && reads(fg, over(hover, s)) && reads(fg, over(selected, s));
    let fit = |mut s: u32| {
        let from = s;
        let mut t = 0.0;
        while !holds(s) && t < 1.0 {
            t += 0.02;
            s = mix(from, away, t);
        }
        s
    };
    let bg_rail = fit(mix(base, BLACK, 0.45));
    let bg_sidebar = fit(mix(base, BLACK, 0.22));
    let bg_input = fit(mix(base, BLACK, 0.32));
    let bg_elevated = fit(if white_text { mix(base, WHITE, 0.08) } else { mix(base, BLACK, 0.10) });
    let surfaces = [bg_rail, bg_sidebar, base, bg_input, bg_elevated];

    let (hue, sat, _) = to_hsl(base);
    let sat = sat.clamp(0.55, 0.9);
    let edge = if white_text { 0xffffff0f } else { 0x0000000f };
    // What a colour must read on, with a tint of `alpha` of `tint_colour` under it.
    let backgrounds = |tint_colour: u32, alpha: u32| -> Vec<u32> {
        surfaces
            .into_iter()
            .flat_map(|s| [s, over(hover, s), over(selected, s), over(tint_colour << 8 | alpha, s)])
            .collect()
    };
    let readable = |c: u32, bgs: &[u32]| bgs.iter().all(|bg| reads(c, *bg));
    let accent = nudge(from_hsl(hue, sat, if white_text { 0.70 } else { 0.38 }), white_text, |c| {
        readable(c, &backgrounds(c, 0x26))
    });
    // The tint is as strong as it can be while the text still reads on it.
    let mut alpha = 0x26;
    while alpha > 0 && !readable(fg, &backgrounds(accent, alpha)) {
        alpha -= 2;
    }
    let bgs = backgrounds(accent, alpha);
    let text_muted = strengthen(if white_text { 0xc2c6d2 } else { 0x444444 }, fg, |c| readable(c, &bgs));
    let text_faint = strengthen(if white_text { 0x9096a8 } else { 0x666666 }, fg, |c| readable(c, &bgs));
    let tone = |c: u32| nudge(c, white_text, |n| readable(n, &bgs));
    let own = nudge(from_hsl(hue, sat, if white_text { 0.82 } else { 0.30 }), white_text, |c| readable(c, &bgs));
    // The fill of a selected rail item: the text reads on it.
    let accent_strong = nudge(accent, !white_text, |c| reads(fg, c));
    // A chosen option's fill: white text on it must pass too.
    let accent_solid = nudge(from_hsl(hue, sat, 0.45), false, |c| reads(WHITE, c));
    Palette {
        bg_rail,
        bg_sidebar,
        bg_chat: base,
        bg_members: bg_sidebar,
        bg_elevated,
        bg_input,
        bg_hover: hover,
        bg_selected: selected,
        text: fg,
        text_muted,
        text_faint,
        accent,
        accent_strong,
        accent_solid,
        danger: tone(DARK.danger),
        own,
        moon: own,
        link: accent,
        ok: tone(DARK.ok),
        warn: tone(DARK.warn),
        edge,
        accent_tint: (accent << 8) | alpha,
        ..DARK
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every palette a screen can draw with: the themes and colours derived from
    /// the extremes and the middle (a very light accent included).
    fn every_palette() -> Vec<(String, Palette)> {
        let mut all: Vec<(String, Palette)> = [Theme::Dark, Theme::Midnight, Theme::Light]
            .into_iter()
            .map(|t| (format!("{t:?}"), Palette::of(t)))
            .collect();
        for base in [
            0xffffff, 0xfffde0, 0xf5f5f5, 0xffe0e0, 0xe0ffe0, 0xe0e8ff, 0xffff00, 0x00ffff, 0x808080, 0x7c8cff,
            0x23a559, 0xff6b6b, 0x4b5ce6, 0x203040, 0x000000, 0x101010,
        ] {
            all.push((format!("custom {base:06x}"), derive(base)));
        }
        all
    }

    #[test]
    fn the_text_on_every_coloured_fill_reads_in_every_palette() {
        for (name, p) in every_palette() {
            for (fill_name, fill, fg) in p.fills() {
                let ratio = contrast(fg, fill);
                assert!(
                    ratio >= 4.5,
                    "{name}: the text on {fill_name} ({fill:06x}) is {fg:06x}, {ratio:.2}:1 (under 4.5)"
                );
                // An icon needs 3:1; text 4.5 covers it, and the pair is what is drawn.
                assert!(ratio >= 3.0, "{name}: the icon on {fill_name} is {ratio:.2}:1");
            }
        }
    }

    #[test]
    fn every_material_on_colour_reads_on_its_fill_in_every_palette() {
        for (name, p) in every_palette() {
            let m = p.material();
            for (role, fill, on) in m.pairs() {
                let ratio = contrast(on, fill);
                assert!(ratio >= 4.5, "{name}: {role} {on:06x} on {fill:06x} is {ratio:.2}:1 (under 4.5)");
            }
            // The secondary text reads on every surface a list or a sheet can sit on, not only on `surface`.
            for (role, fill) in m.surfaces() {
                let ratio = contrast(m.on_surface_variant, fill);
                assert!(ratio >= 4.5, "{name}: on_surface_variant on {role} {fill:06x} is {ratio:.2}:1");
            }
        }
    }

    #[test]
    fn the_material_scheme_keeps_the_themes_own_text_and_surfaces_where_they_read() {
        let p = Palette::of(Theme::Dark);
        let m = p.material();
        assert_eq!(m.primary, p.accent_solid);
        assert_eq!(m.on_primary, p.on_accent_solid());
        assert_eq!(m.surface, p.bg_chat);
        assert_eq!(m.background, p.bg_rail);
        assert_eq!(m.on_surface, p.text, "the theme's own text colour is kept where it reads");
        assert_eq!(m.error, p.danger);
        assert_eq!(m.on_error, p.on_danger());
    }

    fn luminance(rgb: u32) -> u32 {
        let (r, g, b) = ((rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff);
        r + g + b
    }

    #[test]
    fn hsv_converts_primaries_greys_and_the_ends_of_the_hue() {
        assert_eq!(from_hsv(0.0, 1.0, 1.0), 0xff0000);
        assert_eq!(from_hsv(120.0, 1.0, 1.0), 0x00ff00);
        assert_eq!(from_hsv(240.0, 1.0, 1.0), 0x0000ff);
        assert_eq!(from_hsv(360.0, 1.0, 1.0), from_hsv(0.0, 1.0, 1.0));
        assert_eq!(from_hsv(77.0, 0.0, 0.5), 0x808080);
        assert_eq!(from_hsv(10.0, 1.0, 0.0), 0x000000);
        assert_eq!(to_hsv(0xff0000), (0.0, 1.0, 1.0));
        assert_eq!(to_hsv(0x808080).1, 0.0);
        assert_eq!(to_hsv(0x000000), (0.0, 0.0, 0.0));
        assert_eq!(to_hsv(0xffffff), (0.0, 0.0, 1.0));
        assert_eq!(to_hsv(0x0000ff), (240.0, 1.0, 1.0));
        // Out of range is clamped.
        assert_eq!(from_hsv(0.0, 2.0, 3.0), 0xff0000);
    }

    #[test]
    fn hsv_round_trips_every_colour_on_a_sweep() {
        for r in (0..=255u32).step_by(15) {
            for g in (0..=255u32).step_by(15) {
                for b in (0..=255u32).step_by(15) {
                    let c = (r << 16) | (g << 8) | b;
                    let (h, s, v) = to_hsv(c);
                    assert!((0.0..360.0).contains(&h), "{c:06x} hue {h}");
                    assert_eq!(from_hsv(h, s, v), c, "{c:06x}");
                }
            }
        }
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
            assert!(contrast(p.text_muted, p.bg_chat) >= 4.5, "{light:06x}");
            assert!(contrast(p.text, p.bg_chat) >= 4.5, "{light:06x}");
            assert_eq!(p.bg_hover >> 8, 0x000000);
        }
    }


    /// Hues, lightnesses and the awkward colours: pure yellow, cyan, white, black, greys.
    fn sweep() -> Vec<u32> {
        let mut colours = vec![
            0xffff00, 0x00ffff, 0xffffff, 0x000000, 0x808080, 0x777777, 0x767676, 0x7f7f7f,
            0x999999, 0x555555, 0xff00ff, 0x00ff00, 0xff0000, 0x0000ff,
        ];
        for h in (0..360).step_by(15) {
            for l in [0.05, 0.15, 0.25, 0.35, 0.45, 0.5, 0.55, 0.65, 0.75, 0.85, 0.95] {
                for s in [0.2, 0.6, 1.0] {
                    colours.push(from_hsl(h as f32, s, l));
                }
            }
        }
        colours
    }

    #[test]
    fn every_text_reads_on_every_surface_of_a_custom_palette() {
        for base in sweep() {
            let p = derive(base);
            for (name, fg, bg) in p.text_pairs() {
                let ratio = contrast(fg, bg);
                assert!(
                    ratio >= 4.5,
                    "{base:06x}: {name} {fg:06x} on {bg:06x} is {ratio:.2}"
                );
            }
        }
    }

    #[test]
    fn coloured_text_keeps_its_hue_and_translucent_washes_are_flattened() {
        let p = derive(0x2a4d8f);
        assert!((to_hsl(p.own).0 - to_hsl(0x2a4d8f).0).abs() < 6.0);
        // 10% white over black is #1a1a1a, not black.
        assert_eq!(over(0xffffff1a, 0x000000), 0x1a1a1a);
        assert_eq!(on_colour(0xffff00), 0x000000);
        assert_eq!(on_colour(0x000080), 0xffffff);
    }
    #[test]
    fn the_jump_pill_and_its_key_cap_read_at_rest_and_hovered_in_every_palette() {
        let mut palettes = every_palette();
        for base in sweep() {
            palettes.push((format!("sweep {base:06x}"), derive(base)));
        }
        for (name, p) in palettes {
            for hovered in [false, true] {
                let (fill, text) = p.jump_pill(hovered);
                let ratio = contrast(text, fill);
                assert!(ratio >= 4.5, "{name} hovered={hovered}: label {text:06x} on {fill:06x} is {ratio:.2}");
                let cap = cap_on(fill);
                let flat = over(cap.bg, fill);
                let ratio = contrast(cap.text, flat);
                assert!(ratio >= 4.5, "{name} hovered={hovered}: key {:06x} on {flat:06x} is {ratio:.2}", cap.text);
            }
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
