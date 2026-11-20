//! Shareable themes: the current look (the theme, the custom colour that replaces it, the
//! frosted-glass setting) as one short code, to paste, save in a file or send in a chat
//! message, and the same code read back, validated, before it is applied.
//!
//! The format is one token with no spaces, so it survives being pasted into a message:
//!
//! ```text
//! morpheus-theme/1;theme=dark;colour=2a4d8f;glass=80
//! ```
//!
//! `theme` is `dark`, `midnight` or `light`; `colour` (optional) is six hex digits and
//! replaces the theme's palette; `glass` (optional) is how opaque the frosted glass is,
//! 1 to 100 percent, and is left out when glass is off. Unknown fields, repeated fields
//! and anything out of range are refused, not guessed at, so what is previewed is
//! exactly what is applied; a newer version number is refused with its own message.

use crate::glass::Glass;
use crate::settings::Theme;
use crate::theme::{Palette, derive};

/// What every code starts with.
pub const PREFIX: &str = "morpheus-theme/";
/// The version this build writes and reads.
pub const VERSION: u32 = 1;
/// The longest code read (a real one is under 60 characters).
pub const MAX_LEN: usize = 120;
/// The extension of a saved theme file.
pub const FILE_EXTENSION: &str = "morpheus-theme";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SharedTheme {
    pub theme: Theme,
    pub colour: Option<u32>,
    pub glass: Glass,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeCodeError {
    Empty,
    TooLong,
    NotATheme,
    NewerVersion(u32),
    Malformed(String),
    UnknownField(String),
    Repeated(String),
    BadTheme(String),
    BadColour(String),
    BadGlass(String),
    MissingTheme,
}

impl ThemeCodeError {
    /// What to tell the person.
    pub fn message(&self) -> String {
        match self {
            ThemeCodeError::Empty => "Paste a theme code.".into(),
            ThemeCodeError::TooLong => "That is too long to be a theme code.".into(),
            ThemeCodeError::NotATheme => {
                format!("That is not a Morpheus theme code (they start with {PREFIX}{VERSION}).")
            }
            ThemeCodeError::NewerVersion(v) => {
                format!("That theme was made by a newer Morpheus (code version {v}); update to use it.")
            }
            ThemeCodeError::Malformed(part) => format!("\"{part}\" is not a field of the form name=value."),
            ThemeCodeError::UnknownField(name) => format!("The theme code has a field Morpheus does not know: {name}."),
            ThemeCodeError::Repeated(name) => format!("The theme code gives {name} twice."),
            ThemeCodeError::BadTheme(v) => format!("\"{v}\" is not a theme (dark, midnight or light)."),
            ThemeCodeError::BadColour(v) => format!("\"{v}\" is not a colour (six hex digits, like 2a4d8f)."),
            ThemeCodeError::BadGlass(v) => format!("\"{v}\" is not a glass strength (a number from 1 to 100)."),
            ThemeCodeError::MissingTheme => "The theme code does not say which theme it is based on.".into(),
        }
    }
}

impl SharedTheme {
    /// The look as it is now: the code a person copies.
    pub fn encode(&self) -> String {
        let mut code = format!("{PREFIX}{VERSION};theme={}", self.theme.key());
        if let Some(colour) = self.colour {
            code.push_str(&format!(";colour={colour:06x}"));
        }
        if self.glass.on {
            code.push_str(&format!(";glass={}", self.glass.opacity.clamp(1, 100)));
        }
        code
    }

    /// Reads a code, as typed or pasted: surrounding space and the backticks of a code
    /// block are ignored.
    pub fn decode(text: &str) -> Result<SharedTheme, ThemeCodeError> {
        let text = text.trim().trim_matches('`').trim();
        if text.is_empty() {
            return Err(ThemeCodeError::Empty);
        }
        if text.len() > MAX_LEN {
            return Err(ThemeCodeError::TooLong);
        }
        let rest = text.strip_prefix(PREFIX).ok_or(ThemeCodeError::NotATheme)?;
        let mut parts = rest.split(';');
        let version: u32 = parts
            .next()
            .and_then(|v| v.parse().ok())
            .ok_or(ThemeCodeError::NotATheme)?;
        if version > VERSION {
            return Err(ThemeCodeError::NewerVersion(version));
        }
        if version != VERSION {
            return Err(ThemeCodeError::NotATheme);
        }
        let (mut theme, mut colour, mut glass) = (None, None, None);
        for part in parts {
            let (name, value) = part
                .split_once('=')
                .ok_or_else(|| ThemeCodeError::Malformed(part.to_owned()))?;
            let slot_taken = match name {
                "theme" => theme.is_some(),
                "colour" => colour.is_some(),
                "glass" => glass.is_some(),
                _ => return Err(ThemeCodeError::UnknownField(name.to_owned())),
            };
            if slot_taken {
                return Err(ThemeCodeError::Repeated(name.to_owned()));
            }
            match name {
                "theme" => {
                    theme = Some(
                        Theme::ALL
                            .into_iter()
                            .find(|t| t.key() == value)
                            .ok_or_else(|| ThemeCodeError::BadTheme(value.to_owned()))?,
                    )
                }
                "colour" => {
                    let valid = value.len() == 6 && value.chars().all(|c| c.is_ascii_hexdigit());
                    colour = Some(
                        valid
                            .then(|| u32::from_str_radix(value, 16).ok())
                            .flatten()
                            .ok_or_else(|| ThemeCodeError::BadColour(value.to_owned()))?,
                    )
                }
                _ => {
                    glass = Some(
                        value
                            .parse::<u8>()
                            .ok()
                            .filter(|p| (1..=100).contains(p))
                            .ok_or_else(|| ThemeCodeError::BadGlass(value.to_owned()))?,
                    )
                }
            }
        }
        Ok(SharedTheme {
            theme: theme.ok_or(ThemeCodeError::MissingTheme)?,
            colour,
            glass: match glass {
                Some(opacity) => Glass { on: true, opacity },
                None => Glass { on: false, ..Glass::default() },
            },
        })
    }

    /// The palette this look draws with (before glass).
    pub fn palette(&self) -> Palette {
        match self.colour {
            Some(c) => derive(c),
            None => Palette::of(self.theme),
        }
    }

    /// A line saying what the code holds, for the preview.
    pub fn summary(&self) -> String {
        let base = match self.colour {
            Some(c) => format!("Custom colour {}", crate::theme::hex(c)),
            None => format!("{} theme", self.theme.label()),
        };
        if self.glass.on {
            format!("{base}, frosted glass at {}% opaque", self.glass.opacity_for(&self.palette()))
        } else {
            base
        }
    }
}

/// The first theme code in a message (alone, or in a code block, or inside a sentence),
/// if it reads. A message that merely mentions the prefix does not count.
pub fn find_in_message(body: &str) -> Option<SharedTheme> {
    let mut rest = body;
    while let Some(at) = rest.find(PREFIX) {
        let tail = &rest[at..];
        let end = tail
            .find(|c: char| c.is_whitespace() || c == '`')
            .unwrap_or(tail.len());
        if let Ok(theme) = SharedTheme::decode(&tail[..end]) {
            return Some(theme);
        }
        rest = &tail[PREFIX.len()..];
    }
    None
}

/// The text of a theme file: the code and a newline.
pub fn file_text(theme: &SharedTheme) -> String {
    format!("{}\n", theme.encode())
}

/// A theme file read back (its first non-empty line is the code).
pub fn from_file_text(text: &str) -> Result<SharedTheme, ThemeCodeError> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    SharedTheme::decode(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dark() -> SharedTheme {
        SharedTheme { theme: Theme::Dark, colour: None, glass: Glass::default() }
    }

    #[test]
    fn a_plain_theme_is_a_short_code() {
        assert_eq!(dark().encode(), "morpheus-theme/1;theme=dark");
    }

    #[test]
    fn a_colour_and_glass_are_in_the_code_and_read_back() {
        let t = SharedTheme { theme: Theme::Light, colour: Some(0x2a4d8f), glass: Glass { on: true, opacity: 85 } };
        assert_eq!(t.encode(), "morpheus-theme/1;theme=light;colour=2a4d8f;glass=85");
        assert_eq!(SharedTheme::decode(&t.encode()), Ok(t));
        assert_eq!(SharedTheme::decode(&dark().encode()), Ok(dark()));
    }

    #[test]
    fn every_theme_and_a_sweep_of_colours_round_trip() {
        for theme in Theme::ALL {
            for colour in [None, Some(0x000000), Some(0xffffff), Some(0x0a0b0c), Some(0xf2e9c9)] {
                for glass in [Glass::default(), Glass { on: true, opacity: 1 }, Glass { on: true, opacity: 100 }] {
                    let t = SharedTheme { theme, colour, glass };
                    assert_eq!(SharedTheme::decode(&t.encode()), Ok(t));
                }
            }
        }
    }

    #[test]
    fn pasted_codes_may_have_space_or_backticks_round_them() {
        let want = Ok(dark());
        assert_eq!(SharedTheme::decode("  morpheus-theme/1;theme=dark \n"), want);
        assert_eq!(SharedTheme::decode("`morpheus-theme/1;theme=dark`"), want);
        assert_eq!(SharedTheme::decode("```\nmorpheus-theme/1;theme=dark\n```"), want);
    }

    #[test]
    fn bad_codes_are_refused_with_a_reason() {
        use ThemeCodeError::*;
        let bad = |s: &str| SharedTheme::decode(s).expect_err(s);
        assert_eq!(bad(""), Empty);
        assert_eq!(bad("   "), Empty);
        assert_eq!(bad("hello"), NotATheme);
        assert_eq!(bad("morpheus-theme/x;theme=dark"), NotATheme);
        assert_eq!(bad("morpheus-theme/2;theme=dark"), NewerVersion(2));
        assert_eq!(bad("morpheus-theme/0;theme=dark"), NotATheme);
        assert_eq!(bad("morpheus-theme/1"), MissingTheme);
        assert_eq!(bad("morpheus-theme/1;colour=2a4d8f"), MissingTheme);
        assert_eq!(bad("morpheus-theme/1;theme=pink"), BadTheme("pink".into()));
        assert_eq!(bad("morpheus-theme/1;theme=dark;colour=12345"), BadColour("12345".into()));
        assert_eq!(bad("morpheus-theme/1;theme=dark;colour=#2a4d8f"), BadColour("#2a4d8f".into()));
        assert_eq!(bad("morpheus-theme/1;theme=dark;colour=gggggg"), BadColour("gggggg".into()));
        assert_eq!(bad("morpheus-theme/1;theme=dark;glass=0"), BadGlass("0".into()));
        assert_eq!(bad("morpheus-theme/1;theme=dark;glass=101"), BadGlass("101".into()));
        assert_eq!(bad("morpheus-theme/1;theme=dark;glass=-5"), BadGlass("-5".into()));
        assert_eq!(bad("morpheus-theme/1;theme=dark;glass=99999999999"), BadGlass("99999999999".into()));
        assert_eq!(bad("morpheus-theme/1;theme=dark;size=big"), UnknownField("size".into()));
        assert_eq!(bad("morpheus-theme/1;theme=dark;theme=light"), Repeated("theme".into()));
        assert_eq!(bad("morpheus-theme/1;theme=dark;glass"), Malformed("glass".into()));
        assert_eq!(bad(&format!("morpheus-theme/1;theme=dark;colour={}", "a".repeat(200))), TooLong);
        for e in [Empty, TooLong, NotATheme, NewerVersion(3), MissingTheme] {
            assert!(!e.message().is_empty());
        }
    }

    #[test]
    fn the_preview_palette_is_what_applying_gives() {
        let t = SharedTheme::decode("morpheus-theme/1;theme=midnight").expect("code");
        assert_eq!(t.palette(), Palette::of(Theme::Midnight));
        let c = SharedTheme::decode("morpheus-theme/1;theme=dark;colour=2a4d8f").expect("code");
        assert_eq!(c.palette(), derive(0x2a4d8f));
        assert!(c.summary().contains("#2a4d8f"));
    }

    #[test]
    fn a_summary_shows_the_glass_strength_that_will_be_drawn() {
        // 10% is under the lowest opacity, so it is drawn at that and says so.
        let t = SharedTheme::decode("morpheus-theme/1;theme=dark;glass=10").expect("code");
        let drawn = t.glass.opacity_for(&t.palette());
        assert!(drawn > 10);
        assert!(t.summary().contains(&format!("{drawn}%")), "{}", t.summary());
    }

    #[test]
    fn a_code_is_found_in_a_message_alone_in_a_block_or_in_a_sentence() {
        let want = Some(SharedTheme { theme: Theme::Dark, colour: Some(0x336699), glass: Glass::default() });
        let code = "morpheus-theme/1;theme=dark;colour=336699";
        assert_eq!(find_in_message(code), want);
        assert_eq!(find_in_message(&format!("try mine!\n```\n{code}\n```")), want);
        assert_eq!(find_in_message(&format!("my theme {code} is nice")), want);
        assert_eq!(find_in_message(&format!("`{code}`")), want);
    }

    #[test]
    fn a_message_that_only_mentions_the_prefix_is_not_a_theme() {
        assert_eq!(find_in_message("codes start with morpheus-theme/1;"), None);
        assert_eq!(find_in_message("morpheus-theme/1;theme=nope"), None);
        assert_eq!(find_in_message("just chatting"), None);
        // A broken first mention does not hide a good second one.
        assert!(find_in_message("morpheus-theme/ then morpheus-theme/1;theme=light").is_some());
    }

    #[test]
    fn a_theme_file_is_the_code_and_reads_back() {
        let t = SharedTheme { theme: Theme::Midnight, colour: Some(0x123456), glass: Glass { on: true, opacity: 70 } };
        assert_eq!(from_file_text(&file_text(&t)), Ok(t));
        assert_eq!(from_file_text("\n\n  morpheus-theme/1;theme=light  \nignored"), Ok(SharedTheme { theme: Theme::Light, ..dark() }));
        assert_eq!(from_file_text(""), Err(ThemeCodeError::Empty));
        assert!(from_file_text("not a theme").is_err());
    }
}
