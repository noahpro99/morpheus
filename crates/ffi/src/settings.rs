//! Settings: the look (a theme or a custom colour, shared as a code), the
//! language, and what notifies by default. The choices of the look and the
//! language are this device's, kept in the app's folder; the palettes, the
//! theme codes and the wording are `morpheus-view-model` (`theme`,
//! `theme_share`, `i18n`, `settings`).

use std::path::PathBuf;

use morpheus_core::NotifyMode;
use morpheus_view_model::{
    i18n,
    settings::{Theme, notify_default_kind},
    theme::{self, Palette},
    theme_share::SharedTheme,
};

use crate::{FfiError, Morpheus};

const FILE: &str = "appearance.txt";

/// The colours a screen draws with, `0xRRGGBB`.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PaletteItem {
    /// The rail and the bars.
    pub background: u32,
    /// The chat.
    pub surface: u32,
    /// Cards, menus, the composer.
    pub raised: u32,
    pub text: u32,
    pub muted: u32,
    pub accent: u32,
    /// Text on the accent.
    pub on_accent: u32,
    pub danger: u32,
    /// Text on the danger colour (a mention pill, an unread badge).
    pub on_danger: u32,
    /// Dark text on light surfaces: the system bars flip with it.
    pub is_light: bool,
}

/// A look: a base theme and maybe a custom colour over it.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Appearance {
    /// `dark`, `midnight` or `light`.
    pub theme: String,
    /// A custom colour as six hex digits.
    pub colour: Option<String>,
    pub palette: PaletteItem,
}

/// A theme to pick.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ThemeChoice {
    pub key: String,
    pub label: String,
    pub sidebar: u32,
    pub chat: u32,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct LanguageChoice {
    /// `system` or a language code.
    pub code: String,
    pub name: String,
}

/// What notifies by default: `all` or `mentions`.
#[derive(Debug, Clone, uniffi::Record)]
pub struct NotifyDefaults {
    pub dms: String,
    pub rooms: String,
}

/// Black or white, whichever reads on `rgb` (`theme::on_colour`): the text on any fill a screen draws.
#[uniffi::export]
pub fn on_colour(rgb: u32) -> u32 {
    theme::on_colour(rgb)
}

fn palette_item(p: &Palette) -> PaletteItem {
    PaletteItem {
        background: p.bg_rail,
        surface: p.bg_chat,
        raised: p.bg_elevated,
        text: p.text,
        muted: p.text_muted,
        accent: p.accent_solid,
        on_accent: p.on_accent_solid(),
        danger: p.danger,
        on_danger: p.on_danger(),
        is_light: theme::contrast(p.bg_chat, 0x000000) > theme::contrast(p.bg_chat, 0xffffff),
    }
}

fn appearance_of(shared: SharedTheme) -> Appearance {
    Appearance {
        theme: shared.theme.key().to_owned(),
        colour: shared.colour.map(|c| format!("{c:06x}")),
        palette: palette_item(&shared.palette()),
    }
}

fn shared_in(dir: &PathBuf) -> SharedTheme {
    let kept = std::fs::read_to_string(dir.join(FILE)).unwrap_or_default();
    let value = |name: &str| {
        kept.lines().find_map(|line| line.strip_prefix(name).and_then(|rest| rest.strip_prefix('=')))
    };
    SharedTheme {
        theme: Theme::from_key(value("theme").unwrap_or_default()),
        colour: value("colour").and_then(theme::parse_hex),
        glass: Default::default(),
    }
}

fn save(dir: &PathBuf, shared: &SharedTheme) -> Result<(), FfiError> {
    let mut text = format!("theme={}\n", shared.theme.key());
    if let Some(colour) = shared.colour {
        text.push_str(&format!("colour={colour:06x}\n"));
    }
    std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(dir.join(FILE), text))
        .map_err(|err| FfiError::Failed { reason: format!("could not save the look: {err}") })
}

fn mode_key(mode: NotifyMode) -> &'static str {
    match mode {
        NotifyMode::AllMessages => "all",
        _ => "mentions",
    }
}

#[uniffi::export]
impl Morpheus {
    /// The look in use.
    pub fn appearance(&self) -> Appearance {
        appearance_of(shared_in(&self.dir))
    }

    /// The themes to pick from.
    pub fn themes(&self) -> Vec<ThemeChoice> {
        Theme::ALL
            .into_iter()
            .map(|t| {
                let (sidebar, chat) = theme::swatch(t);
                ThemeChoice { key: t.key().into(), label: t.label().into(), sidebar, chat }
            })
            .collect()
    }

    /// Use a base theme (the custom colour goes).
    pub fn set_theme(&self, key: String) -> Result<Appearance, FfiError> {
        let shared = SharedTheme { theme: Theme::from_key(&key), colour: None, glass: Default::default() };
        save(&self.dir, &shared)?;
        Ok(appearance_of(shared))
    }

    /// Colour everything from one colour (six hex digits, `#` optional).
    pub fn set_custom_colour(&self, hex: String) -> Result<Appearance, FfiError> {
        let colour = theme::parse_hex(&hex).ok_or_else(|| FfiError::Failed {
            reason: format!("\"{}\" is not a colour (six hex digits, like 2a4d8f).", hex.trim()),
        })?;
        let shared = SharedTheme { colour: Some(colour), ..shared_in(&self.dir) };
        save(&self.dir, &shared)?;
        Ok(appearance_of(shared))
    }

    /// The code to give someone to get this look.
    pub fn theme_code(&self) -> String {
        shared_in(&self.dir).encode()
    }

    /// Use the look a code (as pasted) holds.
    pub fn apply_theme_code(&self, text: String) -> Result<Appearance, FfiError> {
        let shared = SharedTheme::decode(&text).map_err(|err| FfiError::Failed { reason: err.message() })?;
        save(&self.dir, &shared)?;
        Ok(appearance_of(shared))
    }

    /// The languages to pick from, the system's first.
    pub fn languages(&self) -> Vec<LanguageChoice> {
        let mut all = vec![LanguageChoice { code: "system".into(), name: i18n::t("System default") }];
        all.extend(i18n::LANGUAGES.iter().map(|l| LanguageChoice { code: l.code.into(), name: l.name.into() }));
        all
    }

    /// What was picked: `system` or a code.
    pub fn language_choice(&self) -> String {
        i18n::load_choice(&self.dir).key().to_owned()
    }

    /// Show the words in this language (`system`: the phone's); kept.
    pub fn set_language(&self, code: String) -> Result<(), FfiError> {
        let choice = i18n::Choice::from_key(&code);
        i18n::save_choice(&self.dir, choice)
            .map_err(|err| FfiError::Failed { reason: format!("could not save the language: {err}") })?;
        i18n::set_language(choice.resolve(&i18n::system_languages()));
        Ok(())
    }

    /// Use the saved language (at start).
    pub fn start_language(&self) {
        i18n::init(&self.dir);
    }

    /// The words for `key` in the language in use.
    pub fn translate(&self, key: String) -> String {
        i18n::t(&key)
    }

    /// What notifies by default, in DMs and in rooms.
    pub async fn notify_defaults(&self) -> Result<NotifyDefaults, FfiError> {
        let session = self.session()?;
        Ok(NotifyDefaults {
            dms: mode_key(session.default_notify_mode(true).await?).into(),
            rooms: mode_key(session.default_notify_mode(false).await?).into(),
        })
    }

    /// Choose what notifies by default (`dm`: direct messages, else rooms): `all` or `mentions`.
    pub async fn set_notify_default(&self, dm: bool, mode: String) -> Result<(), FfiError> {
        let mode = if mode == "all" { NotifyMode::AllMessages } else { NotifyMode::MentionsOnly };
        let _ = notify_default_kind(dm);
        Ok(self.session()?.set_default_notify_mode(dm, mode).await?)
    }
}

/// A group on the Settings list (the desktop's pages, as the phone shows them).
#[derive(uniffi::Record)]
pub struct SettingsGroup {
    /// Stable: `Account`, `Notifications`, `Appearance`, ...
    pub key: String,
    /// English; the app translates it.
    pub label: String,
    /// The desktop's icon name (`user`, `bell`, ...).
    pub glyph: String,
}

/// One setting the search found: its name and the group and anchor it lives under.
#[derive(uniffi::Record)]
pub struct SettingHit {
    pub label: String,
    pub group: String,
    pub anchor: String,
}

/// The desktop's settings pages, in its order (no Startup: that is for a desktop window).
#[uniffi::export]
pub fn settings_groups() -> Vec<SettingsGroup> {
    morpheus_view_model::settings::SettingsPage::listed(false)
        .into_iter()
        .map(|page| SettingsGroup { key: format!("{page:?}"), label: page.label().to_owned(), glyph: page.glyph().to_owned() })
        .collect()
}

/// The settings whose name or words match every word of `query` (the desktop's search, one index).
#[uniffi::export]
pub fn search_settings(query: String) -> Vec<SettingHit> {
    morpheus_view_model::settings::find_settings(&query, false)
        .into_iter()
        .map(|entry| SettingHit { label: entry.label.to_owned(), group: format!("{:?}", entry.page), anchor: entry.anchor.to_owned() })
        .collect()
}

#[cfg(test)]
mod search_tests {
    use super::*;

    #[test]
    fn groups_are_the_desktops_pages_and_theme_is_found_under_appearance() {
        let keys: Vec<String> = settings_groups().into_iter().map(|g| g.key).collect();
        assert!(keys.starts_with(&["Account".to_owned(), "Security".to_owned()]), "{keys:?}");
        assert!(!keys.contains(&"Startup".to_owned()));
        let hits = search_settings("theme".into());
        assert!(hits.iter().any(|h| h.group == "Appearance" && h.anchor == "theme"), "{}", hits.len());
        assert!(search_settings("   ".into()).is_empty());
    }
}

/// The two lists the phone's swipes move between.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SwipeList {
    Servers,
    Dms,
}

/// What a swipe along a list does (`view_model::list_swipes`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SwipeStep {
    Show { list: SwipeList },
    Open { room: String },
    Nothing,
}

/// The chats-opened-last record after `room` was opened (`kept` is the record as stored, empty for none).
#[uniffi::export]
pub fn last_chats_opened(kept: String, room: String, is_dm: bool) -> String {
    let mut last = morpheus_view_model::list_swipes::LastChats::parse(&kept);
    last.opened(&room, is_dm);
    last.encode()
}

/// What a swipe on `list` does; `finger_left` is the direction the finger moved.
#[uniffi::export]
pub fn list_swipe(kept: String, list: SwipeList, finger_left: bool) -> SwipeStep {
    use morpheus_view_model::list_swipes::{Finger, List, Step};
    let list = match list {
        SwipeList::Servers => List::Servers,
        SwipeList::Dms => List::Dms,
    };
    let finger = if finger_left { Finger::Left } else { Finger::Right };
    match morpheus_view_model::list_swipes::LastChats::parse(&kept).swipe(list, finger) {
        Step::Show(List::Servers) => SwipeStep::Show { list: SwipeList::Servers },
        Step::Show(List::Dms) => SwipeStep::Show { list: SwipeList::Dms },
        Step::Open(room) => SwipeStep::Open { room },
        Step::Nothing => SwipeStep::Nothing,
    }
}
