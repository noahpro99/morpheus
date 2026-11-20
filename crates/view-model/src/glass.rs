//! Frosted-glass themes: the window is see-through (and, where the system can, blurred
//! behind) and the surfaces are translucent, so the desktop shows through the panels.
//!
//! What GPUI's window background appearance does on each platform:
//! - macOS: `Blurred` is an NSVisualEffectView-style blur behind the window, `Transparent`
//!   plain alpha.
//! - Windows: `Blurred` is the DWM acrylic accent (Windows 10 1803 and later; Windows 11
//!   draws it too), `Transparent` a transparent gradient accent.
//! - Linux on Wayland: the window gets an alpha surface, and with `Blurred` it asks the
//!   compositor for blur (KDE's blur protocol); compositors such as Hyprland blur
//!   translucent windows by their own rules (`decoration:blur`), so there the strength
//!   is the compositor's, not ours.
//! - Linux on X11: the surface can have alpha but whether anything shows through depends
//!   on a compositing manager being there, which cannot be told from the window, so glass
//!   is not offered (it would be a black window without one).
//!
//! Where glass is not supported the setting stays but draws opaque. Text must stay
//! readable over a backdrop we cannot see, so the strength has a floor per palette,
//! computed against the worst backdrops (near white, black and mid grey).

use crate::theme::{Palette, contrast, mix, over};

/// How much of the window the glass setting leaves see-through, in percent opaque.
pub const STRENGTHS: [u8; 5] = [90, 80, 70, 60, 50];
pub const DEFAULT_OPACITY: u8 = 80;

/// The contrast the main text must keep over any backdrop, and muted text.
pub const TEXT_CONTRAST: f32 = 4.5;
pub const MUTED_CONTRAST: f32 = 3.0;

/// The backdrops text must read over: black, a bright window (a blur never gives a pure
/// white, but a page of white text is as bright as a backdrop gets), and mid grey.
pub const BRIGHT: u32 = 0xdcdcdc;
pub const BACKDROPS: [u32; 3] = [0x000000, BRIGHT, 0x808080];
// Muted text is secondary: it must hold 3.0 over every backdrop but a bright one, where
// only the main text is held to 4.5.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glass {
    pub on: bool,
    /// Percent opaque of the main surfaces (rail, sidebar, chat, members).
    pub opacity: u8,
}

impl Default for Glass {
    fn default() -> Self {
        Glass { on: false, opacity: DEFAULT_OPACITY }
    }
}

/// What the platform can do for a see-through window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// Translucent with a blur drawn by the system (macOS, Windows).
    Blur,
    /// Translucent; whether the compositor blurs is its own rule (Wayland).
    Translucent,
    /// Opaque only.
    None,
}

impl Support {
    /// `os` is `std::env::consts::OS`; `wayland` is whether the session is Wayland.
    pub fn of(os: &str, wayland: bool) -> Support {
        match os {
            "macos" | "windows" => Support::Blur,
            "linux" | "freebsd" if wayland => Support::Translucent,
            _ => Support::None,
        }
    }

    pub fn detect() -> Support {
        // MORPHEUS_GLASS_SUPPORT=blur|translucent|none forces it (tests, or a system that
        // is wrong about itself).
        if let Ok(forced) = std::env::var("MORPHEUS_GLASS_SUPPORT") {
            match forced.as_str() {
                "blur" => return Support::Blur,
                "translucent" => return Support::Translucent,
                "none" => return Support::None,
                _ => {}
            }
        }
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
        Support::of(std::env::consts::OS, wayland)
    }

    /// The note shown under the setting.
    pub fn note(self) -> &'static str {
        match self {
            Support::Blur => "Any blur behind the window comes from your system, not from Morpheus.",
            Support::Translucent => {
                "Your desktop shows through. Whether it is blurred is up to your compositor (on Hyprland, its blur settings)."
            }
            Support::None => {
                "This system cannot draw a see-through window (X11 needs a compositor we cannot detect), so the theme stays opaque."
            }
        }
    }
}

/// How opaque each kind of surface is, 0 to 255, for a main-surface opacity of `percent`:
/// menus, dialogs and inputs sit half way to opaque so they read as frosted panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Alphas {
    pub main: u8,
    pub raised: u8,
}

pub fn alphas(percent: u8) -> Alphas {
    let main = (u32::from(percent.min(100)) * 255 / 100) as u8;
    let raised = main + ((255 - u32::from(main)) / 2) as u8;
    Alphas { main, raised }
}

fn over_backdrop(surface: u32, alpha: u8, backdrop: u32) -> u32 {
    mix(backdrop, surface, f32::from(alpha) / 255.0)
}

/// Whether the palette's text reads at `percent`, with any backdrop behind the window.
pub fn reads_at(palette: &Palette, percent: u8) -> bool {
    first_failure(palette, percent).is_none()
}

/// The first (name, text, flattened background) that fails at `percent`, for tests.
pub fn first_failure(palette: &Palette, percent: u8) -> Option<(&'static str, u32, u32)> {
    let a = alphas(percent);
    let surfaces = [
        (palette.bg_rail, a.main),
        (palette.bg_sidebar, a.main),
        (palette.bg_chat, a.main),
        (palette.bg_members, a.main),
        (palette.bg_elevated, a.raised),
        (palette.bg_input, a.raised),
    ];
    for backdrop in BACKDROPS {
        for (surface, alpha) in surfaces {
            let base = over_backdrop(surface, alpha, backdrop);
            for bg in [
                base,
                over(palette.bg_hover, base),
                over(palette.bg_selected, base),
                over(palette.accent_tint, base),
            ] {
                if contrast(palette.text, bg) < TEXT_CONTRAST {
                    return Some(("text", palette.text, bg));
                }
                if backdrop != BRIGHT && contrast(palette.text_muted, bg) < MUTED_CONTRAST {
                    return Some(("text_muted", palette.text_muted, bg));
                }
            }
        }
    }
    None
}

/// The least opacity (percent) at which the palette's text reads over any backdrop.
pub fn floor(palette: &Palette) -> u8 {
    (0..=100u8).find(|p| reads_at(palette, *p)).unwrap_or(100)
}

/// The strengths on offer for `palette`: those at or above its floor.
pub fn strengths(palette: &Palette) -> Vec<u8> {
    let floor = floor(palette);
    STRENGTHS.into_iter().filter(|s| *s >= floor).collect()
}

impl Glass {
    /// The opacity a slider at `fraction` (0 to 1) of its track picks: from the palette's
    /// floor (the least that still reads) up to fully opaque.
    pub fn opacity_at(fraction: f32, floor: u8) -> u8 {
        let floor = floor.min(100);
        let span = f32::from(100 - floor);
        (f32::from(floor) + fraction.clamp(0.0, 1.0) * span).round() as u8
    }

    /// Where on the slider's track `percent` sits, 0 to 1.
    pub fn fraction_of(percent: u8, floor: u8) -> f32 {
        let floor = floor.min(99);
        (f32::from(percent.clamp(floor, 100) - floor) / f32::from(100 - floor)).clamp(0.0, 1.0)
    }

    /// The opacity actually drawn for `palette`: the chosen one, never below the floor.
    pub fn opacity_for(&self, palette: &Palette) -> u8 {
        self.opacity.clamp(1, 100).max(floor(palette))
    }

    /// The percent opaque to draw with: `None` is opaque (glass off, or unsupported).
    pub fn effective(&self, palette: &Palette, support: Support) -> Option<u8> {
        (self.on && support != Support::None).then(|| self.opacity_for(palette))
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({ "on": self.on, "opacity": self.opacity })
    }

    pub fn from_json(value: Option<&serde_json::Value>) -> Glass {
        let Some(v) = value else { return Glass::default() };
        Glass {
            on: v.get("on").and_then(|v| v.as_bool()).unwrap_or(false),
            opacity: v
                .get("opacity")
                .and_then(|v| v.as_u64())
                .map(|p| p.clamp(1, 100) as u8)
                .unwrap_or(DEFAULT_OPACITY),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{DARK, LIGHT, MIDNIGHT, derive};

    fn palettes() -> Vec<(String, Palette)> {
        let mut all = vec![("dark".into(), DARK), ("midnight".into(), MIDNIGHT), ("light".into(), LIGHT)];
        for c in [0x2a4d8f, 0xf2e9c9, 0xff0000, 0x00ff00, 0x808080, 0x101010, 0xfafafa, 0x7c8cff] {
            all.push((format!("{c:06x}"), derive(c)));
        }
        all
    }

    #[test]
    fn fully_opaque_always_reads_and_the_floor_is_where_it_starts_to() {
        for (name, p) in palettes() {
            let f = floor(&p);
            assert!(reads_at(&p, 100), "{name}: opaque must read");
            assert!(reads_at(&p, f), "{name}: reads at its floor {f}");
            if f > 0 {
                assert!(!reads_at(&p, f - 1), "{name}: {f} is the least");
            }
        }
    }

    #[test]
    fn text_reads_over_the_worst_backdrop_at_every_strength_offered() {
        for (name, p) in palettes() {
            let offered = strengths(&p);
            assert!(!offered.is_empty() || floor(&p) > 50, "{name}");
            for s in offered {
                assert_eq!(first_failure(&p, s), None, "{name} at {s}%");
            }
        }
    }

    #[test]
    fn a_strength_below_the_floor_is_raised_to_it_not_drawn_unreadable() {
        let g = Glass { on: true, opacity: 1 };
        for (name, p) in palettes() {
            let drawn = g.effective(&p, Support::Blur).expect("glass");
            assert!(reads_at(&p, drawn), "{name} drawn at {drawn}%");
        }
    }

    #[test]
    fn a_translucent_window_is_not_readable_for_a_palette_with_text_near_the_backdrop() {
        // Mid-grey chat with white text: see-through reads on black but not on white.
        let p = derive(0x808080);
        assert!(floor(&p) > 50, "floor {}", floor(&p));
    }

    #[test]
    fn glass_degrades_to_opaque_where_the_system_cannot() {
        let g = Glass { on: true, opacity: 80 };
        assert_eq!(g.effective(&DARK, Support::None), None);
        assert!(g.effective(&DARK, Support::Blur).is_some());
        assert!(g.effective(&DARK, Support::Translucent).is_some());
        assert_eq!(Glass::default().effective(&DARK, Support::Blur), None, "off by default");
    }

    #[test]
    fn support_by_platform() {
        assert_eq!(Support::of("macos", false), Support::Blur);
        assert_eq!(Support::of("windows", false), Support::Blur);
        assert_eq!(Support::of("linux", true), Support::Translucent);
        assert_eq!(Support::of("linux", false), Support::None);
        assert_eq!(Support::of("android", false), Support::None);
    }

    #[test]
    fn raised_surfaces_are_more_opaque_than_the_main_ones() {
        let a = alphas(60);
        assert!(a.raised > a.main && a.main > 0);
        assert_eq!(alphas(100), Alphas { main: 255, raised: 255 });
    }

    #[test]
    fn the_slider_runs_from_the_floor_to_opaque() {
        assert_eq!(Glass::opacity_at(0.0, 70), 70);
        assert_eq!(Glass::opacity_at(1.0, 70), 100);
        assert_eq!(Glass::opacity_at(0.5, 70), 85);
        assert_eq!(Glass::opacity_at(-3.0, 70), 70);
        assert_eq!(Glass::opacity_at(9.0, 70), 100);
        for floor in [0u8, 1, 50, 72, 99, 100] {
            for percent in floor.max(1)..=100 {
                let back = Glass::opacity_at(Glass::fraction_of(percent, floor), floor);
                assert_eq!(back, percent, "floor {floor}");
            }
        }
    }

    #[test]
    fn saved_settings_round_trip_and_bad_ones_fall_back() {
        let g = Glass { on: true, opacity: 65 };
        assert_eq!(Glass::from_json(Some(&g.to_json())), g);
        assert_eq!(Glass::from_json(None), Glass::default());
        assert_eq!(Glass::from_json(Some(&serde_json::json!({"on": "yes", "opacity": 500}))).opacity, 100);
    }
}
