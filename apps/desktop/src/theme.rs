//! Theme tokens, from `crates/ui/assets/theme.css`. The colours come from the
//! chosen theme's palette (`view_model::theme`: dark, midnight, light); the
//! sizes are the same in all of them. A colour is a function, read each time
//! something is drawn, so choosing a theme changes the next frame.

use std::sync::atomic::{AtomicU8, Ordering};

use crate::view_model::{
    glass::{self, Glass, Support},
    settings::Theme,
    theme::{Palette, swatch},
};

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// The theme and custom colour as last chosen, kept in the data folder
/// (`theme.json`) because the account's copy arrives with the session, after
/// the first frame: read once, before anything is drawn, so the window opens
/// in the saved colours.
const CACHE_FILE: &str = "theme.json";
#[cfg(not(test))]
static LOADED: std::sync::Once = std::sync::Once::new();

/// What `theme.json` holds: the theme and the custom colour, if any.
pub fn parse_cache(text: &str) -> (Theme, Option<u32>) {
    let value: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
    let theme = value
        .get("theme")
        .and_then(|v| v.as_str())
        .map(Theme::from_key)
        .unwrap_or_default();
    let custom = value
        .get("custom_colour")
        .and_then(|v| v.as_str())
        .and_then(crate::view_model::theme::parse_hex);
    (theme, custom)
}

fn cache_text(theme: Theme, custom: Option<u32>) -> String {
    let mut value = serde_json::json!({ "theme": theme.key(), "glass": glass_setting_raw().to_json() });
    if let Some(c) = custom {
        value["custom_colour"] = crate::view_model::theme::hex(c).into();
    }
    value.to_string()
}

/// Install the cached theme from `dir` (synchronously: a small local file).
fn install_cache(dir: &std::path::Path) {
    if let Ok(text) = std::fs::read_to_string(dir.join(CACHE_FILE)) {
        let (theme, custom) = parse_cache(&text);
        if let Ok(mut g) = GLASS.write() {
            *g = Glass::from_json(serde_json::from_str::<serde_json::Value>(&text).ok().as_ref().and_then(|v| v.get("glass")));
        }
        store_theme(theme);
        store_custom(custom);
    }
}

fn ensure_loaded() {
    // Unit tests draw with whatever they set, not the machine's choice.
    #[cfg(not(test))]
    LOADED.call_once(|| install_cache(&crate::data_dir()));
}

fn remember() {
    #[cfg(not(test))]
    {
        let text = cache_text(current(), custom());
        let dir = crate::data_dir();
        let path = dir.join(CACHE_FILE);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            std::fs::create_dir_all(&dir).ok();
            std::fs::write(path, text).ok();
        }
    }
}

fn store_theme(theme: Theme) {
    CURRENT.store(
        Theme::ALL.iter().position(|t| *t == theme).unwrap_or(0) as u8,
        Ordering::Relaxed,
    );
    refresh_glass();
}

fn store_custom(colour: Option<u32>) {
    if let Ok(mut custom) = CUSTOM.write() {
        *custom = colour.map(|c| (c, crate::view_model::theme::derive(c)));
    }
    refresh_glass();
}

/// Draw with `theme` from now on.
pub fn set(theme: Theme) {
    ensure_loaded();
    store_theme(theme);
    remember();
}

pub fn current() -> Theme {
    ensure_loaded();
    Theme::ALL
        .get(usize::from(CURRENT.load(Ordering::Relaxed)))
        .copied()
        .unwrap_or_default()
}

/// The custom colour chosen (its base), and the palette derived from it, if any.
static CUSTOM: std::sync::RwLock<Option<(u32, Palette)>> = std::sync::RwLock::new(None);

/// Draw with a palette derived from `colour` (0xRRGGBB) instead of the theme, or, with
/// `None`, with the theme.
pub fn set_custom(colour: Option<u32>) {
    ensure_loaded();
    store_custom(colour);
    remember();
}

pub fn custom() -> Option<u32> {
    ensure_loaded();
    CUSTOM.read().ok().and_then(|c| c.map(|(base, _)| base))
}

/// The palette in use.
pub fn palette() -> Palette {
    ensure_loaded();
    if let Ok(custom) = CUSTOM.read()
        && let Some((_, palette)) = *custom
    {
        return palette;
    }
    Palette::of(current())
}


// ---- frosted glass ---------------------------------------------------------------------

static GLASS: std::sync::RwLock<Glass> = std::sync::RwLock::new(Glass { on: false, opacity: glass::DEFAULT_OPACITY });
/// The alphas drawn now: main surface in the high byte, raised in the low; 0 is opaque.
static ALPHAS: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);

/// What this system can do for a see-through window (everything in tests, so they
/// do not depend on the machine).
pub fn support() -> Support {
    #[cfg(test)]
    {
        Support::Blur
    }
    #[cfg(not(test))]
    {
        static SUPPORT: std::sync::OnceLock<Support> = std::sync::OnceLock::new();
        *SUPPORT.get_or_init(Support::detect)
    }
}

fn glass_setting_raw() -> Glass {
    GLASS.read().map(|g| *g).unwrap_or_default()
}

/// The glass setting as chosen.
pub fn glass_setting() -> Glass {
    ensure_loaded();
    GLASS.read().map(|g| *g).unwrap_or_default()
}

/// The percent opaque actually drawn (None: opaque), after the contrast floor and
/// what the system supports.
pub fn glass_percent() -> Option<u8> {
    ensure_loaded();
    glass_setting().effective(&palette_unglassed(), support())
}

fn refresh_glass() {
    let a = glass_percent_now().map(glass::alphas);
    let packed = a.map(|a| (u16::from(a.main) << 8) | u16::from(a.raised)).unwrap_or(0);
    ALPHAS.store(packed, Ordering::Relaxed);
}

fn glass_percent_now() -> Option<u8> {
    let g = GLASS.read().map(|g| *g).unwrap_or_default();
    g.effective(&palette_unglassed(), support())
}

fn palette_unglassed() -> Palette {
    if let Ok(custom) = CUSTOM.read()
        && let Some((_, palette)) = *custom
    {
        return palette;
    }
    Palette::of(Theme::ALL.get(usize::from(CURRENT.load(Ordering::Relaxed))).copied().unwrap_or_default())
}

/// Draw with `glass` from now on.
pub fn set_glass(glass: Glass) {
    ensure_loaded();
    if let Ok(mut g) = GLASS.write() {
        *g = glass;
    }
    refresh_glass();
    remember();
}

/// The window's background for what is drawn: blurred where glass is on, else opaque.
pub fn window_background() -> gpui::WindowBackgroundAppearance {
    if glass_percent().is_some() {
        gpui::WindowBackgroundAppearance::Blurred
    } else {
        gpui::WindowBackgroundAppearance::Opaque
    }
}

/// Tell every open window how see-through it is now.
pub fn sync_windows(cx: &mut gpui::App) {
    let background = window_background();
    for handle in cx.windows() {
        handle
            .update(cx, |_, window, _| window.set_background_appearance(background))
            .ok();
    }
}

fn surface(colour: u32, raised: bool) -> gpui::Rgba {
    ensure_loaded();
    let packed = ALPHAS.load(Ordering::Relaxed);
    let alpha = match packed {
        0 => 0xff,
        p if raised => (p & 0xff) as u8,
        p => (p >> 8) as u8,
    };
    gpui::rgba((colour << 8) | u32::from(alpha))
}

macro_rules! fills {
    ($($name:ident: $token:ident, $raised:expr);* $(;)?) => {
        $(
            /// The surface colour as a fill: translucent under a glass theme.
            #[inline]
            pub fn $name() -> gpui::Rgba {
                surface(palette().$token, $raised)
            }
        )*
    };
}

fills!(
    fill_rail: bg_rail, false;
    fill_sidebar: bg_sidebar, false;
    fill_chat: bg_chat, false;
    fill_input: bg_input, true;
    fill_elevated: bg_elevated, true;
);

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
    bg_rail,
    bg_sidebar,
    bg_chat,
    bg_input,
    bg_elevated,
    bg_hover,
    bg_selected,
    edge,
    ok,
    warn,
    text,
    text_muted,
    text_faint,
    accent,
    accent_strong,
    accent_solid,
    danger,
    own,
    link,
    green,
    accent_tint,
);

/// The foreground on each coloured fill (a primary button, a selected item, a danger
/// or call button, a badge): black or white by contrast, whatever the theme or the
/// custom colour. Anything drawn on a theme fill uses these, never a fixed white.
macro_rules! on_fills {
    ($($name:ident),* $(,)?) => {
        $(
            #[inline]
            pub fn $name() -> u32 {
                palette().$name()
            }
        )*
    };
}

on_fills!(on_accent, on_accent_strong, on_accent_solid, on_danger, on_green);

/// Black or white, whichever reads on any `fill` (a swatch, a user colour, a toggle's track).
pub fn on(fill: u32) -> u32 {
    crate::view_model::theme::on_colour(fill)
}

/// `color` (0xRRGGBB) with alpha `a`, as 0xRRGGBBAA: a tint of a token.
pub fn alpha(color: u32, a: u8) -> u32 {
    (color << 8) | u32::from(a)
}

/// The gold of a mention's bar and tint (Discord's); the same in every theme.
pub const MENTION_GOLD: u32 = 0xfaa81a;
pub const RADIUS: f32 = 8.0;
/// `html { font-size: 15px }`: what 1rem is.
pub const REM: f32 = 15.0;
pub const RADIUS_SM: f32 = 6.0;
pub const RADIUS_LG: f32 = 12.0;
pub const RADIUS_XL: f32 = 16.0;
pub const RAIL_W: f32 = 72.0;
pub const SIDEBAR_W: f32 = 248.0;
pub const BOTTOM_H: f32 = 50.0;
/// The gap under the message box, the user panel and the member list: one
/// shared bottom edge.
pub const BOTTOM_GAP: f32 = 8.0;
/// `--mono`.
/// Text on the F3 stats overlay, which always sits on a dark translucent panel
/// whatever the theme.
pub const OVERLAY_TEXT: u32 = 0xf2f3f5;

pub const MONO: &str = "JetBrains Mono";
/// What body text is: Inter at `html { font-size: 15px; line-height: 1.4 }`.
pub const LINE_HEIGHT: f32 = 1.4;
pub const MEMBERS_W: f32 = 232.0;
/// The search results, over the member list: wider than it, as Discord's.
pub const SEARCH_W: f32 = 440.0;

#[cfg(test)]
mod tests {
    use super::*;

    /// The theme is global state: the tests that set it take turns.
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    fn turn() -> std::sync::MutexGuard<'static, ()> {
        TURN.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn the_saved_theme_is_installed_from_the_data_folder_before_drawing() {
        let _turn = turn();
        let dir = std::env::temp_dir().join(format!("morpheus-theme-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(CACHE_FILE),
            cache_text(Theme::Midnight, Some(0x336699)),
        )
        .unwrap();
        install_cache(&dir);
        assert_eq!(current(), Theme::Midnight);
        assert_eq!(custom(), Some(0x336699));
        assert_eq!(parse_cache("garbage"), (Theme::default(), None));
        set_custom(None);
        set(Theme::Dark);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn glass_makes_surfaces_translucent_and_off_is_opaque() {
        let _turn = turn();
        set(Theme::Dark);
        set_glass(Glass::default());
        assert_eq!(fill_chat().a, 1.0);
        set_glass(Glass { on: true, opacity: 80 });
        assert!(fill_chat().a < 1.0 && fill_chat().a > 0.5);
        assert!(fill_elevated().a > fill_chat().a, "menus and dialogs are more opaque");
        set_glass(Glass::default());
        assert_eq!(fill_chat().a, 1.0);
    }

    #[test]
    fn choosing_a_theme_changes_the_colours_read_next() {
        let _turn = turn();
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
            } else if path.extension().is_some_and(|e| e == "rs")
                && path.file_name().is_some_and(|n| n != "theme.rs")
            {
                found.push(path);
            }
        }
    }

    /// The translucent tokens are RRGGBBAA: through `rgb()` their alpha byte
    /// reads as blue (white at 8% came out yellow), so they go through `rgba()`.
    #[test]
    fn translucent_tokens_are_never_drawn_with_rgb() {
        let mut files = Vec::new();
        sources(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        );
        let mut wrong = Vec::new();
        for file in files {
            let text = std::fs::read_to_string(&file).expect("source");
            for (n, line) in text.lines().enumerate() {
                for token in ["bg_hover", "bg_selected", "edge", "accent_tint"] {
                    if line.contains(&format!("rgb(theme::{token}())")) {
                        wrong.push(format!("{}:{}: {token}", file.display(), n + 1));
                    }
                }
            }
        }
        assert!(wrong.is_empty(), "translucent tokens through rgb():\n{}", wrong.join("\n"));
    }

    /// A foreground on a coloured fill is picked by contrast (`theme::on_accent_solid()`,
    /// `on_danger()`, `on(fill)`: black or white as reads), never a fixed white: white on
    /// a light accent or a light danger red is unreadable. An opaque white or black in
    /// the code is only for what no theme changes (the video's black, the lightbox's
    /// scrim), and says so with `// fixed:` on the line.
    #[test]
    fn no_fixed_white_or_black_on_a_theme_fill() {
        let mut files = Vec::new();
        sources(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        );
        let mut fixed = Vec::new();
        for file in files {
            let text = std::fs::read_to_string(&file).expect("source");
            let code = text.split("#[cfg(test)]").next().unwrap_or_default();
            for (n, line) in code.lines().enumerate() {
                if line.contains("// fixed:") {
                    continue;
                }
                let mut rest = line;
                while let Some(at) = rest.find("0x") {
                    let digits: String = rest[at + 2..]
                        .chars()
                        .take_while(char::is_ascii_hexdigit)
                        .collect();
                    rest = &rest[at + 2 + digits.len()..];
                    if digits == "ffffff" || digits == "000000" {
                        fixed.push(format!("{}:{}: 0x{digits}", file.display(), n + 1));
                    }
                }
            }
        }
        assert!(
            fixed.is_empty(),
            "fixed white or black (use theme::on_* or mark it `// fixed:`):\n{}",
            fixed.join("\n")
        );
    }

    #[test]
    fn no_hex_colours_outside_the_theme() {
        let mut files = Vec::new();
        sources(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        );
        let mut stray = Vec::new();
        for file in files {
            let text = std::fs::read_to_string(&file).expect("source");
            // Unit tests may name colours; the code above them may not.
            let code = text.split("#[cfg(test)]").next().unwrap_or_default();
            for (n, line) in code.lines().enumerate() {
                let mut rest = line;
                while let Some(at) = rest.find("0x") {
                    let digits: String = rest[at + 2..]
                        .chars()
                        .take_while(char::is_ascii_hexdigit)
                        .collect();
                    rest = &rest[at + 2 + digits.len()..];
                    let colour = matches!(digits.len(), 6 | 8);
                    let fixed = digits.starts_with("ffffff")
                        || digits.starts_with("000000")
                        || digits == "2d3a9e";
                    if colour && !fixed {
                        stray.push(format!("{}:{}: 0x{digits}", file.display(), n + 1));
                    }
                }
            }
        }
        assert!(
            stray.is_empty(),
            "colours that should be tokens:\n{}",
            stray.join("\n")
        );
    }
}

/// How far a smaller run of text must be lifted to share the baseline of a
/// bigger one after it in a row aligned by box bottoms (the layout has no
/// text baselines, so `items_baseline` lines up the boxes' bottoms). Inter's
/// baseline sits its descent plus half the leading above a line box's
/// bottom, which grows with the font size (sizes in rem).
pub fn baseline_lift(big_rem: f32, small_rem: f32) -> f32 {
    const ASCENT: f32 = 1984.0 / 2048.0;
    const DESCENT: f32 = 494.0 / 2048.0;
    let above_bottom = (LINE_HEIGHT - (ASCENT + DESCENT)) / 2.0 + DESCENT;
    above_bottom * (big_rem - small_rem) * REM
}

#[cfg(test)]
mod baseline_tests {
    use super::*;

    #[test]
    fn a_smaller_time_is_lifted_onto_the_names_baseline() {
        let lift = baseline_lift(1.0, 0.75);
        assert!((lift - 1.26).abs() < 0.01, "{lift}");
        assert_eq!(baseline_lift(0.8, 0.8), 0.0);
    }
}
