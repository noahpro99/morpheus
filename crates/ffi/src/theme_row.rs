//! The theme row of Settings > Appearance (#490), from `morpheus_view_model::theme_row`: the row
//! (one recent custom theme) and `theme_history_list` (the whole history, for the Custom panel).

use crate::Morpheus;
use morpheus_view_model::{
    settings::Theme,
    theme::{self, derive, hex, parse_hex},
    theme_history::{Look, ThemeHistory},
    theme_row::{self, ThemeRowKind},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ThemeRowItemKind {
    MaterialYou,
    Builtin,
    Recent,
    Custom,
}

/// A look the app has used: a theme key (`dark`, `midnight`, `light`) and maybe a custom colour (six hex digits).
#[derive(Debug, Clone, uniffi::Record)]
pub struct ThemeLook {
    pub theme: String,
    pub colour: Option<String>,
}

/// One swatch of the row.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ThemeRowEntry {
    pub kind: ThemeRowItemKind,
    /// The base theme key: what picking it applies (with `colour`, if any).
    pub theme: String,
    /// The custom colour as six hex digits, for a recent custom theme.
    pub colour: Option<String>,
    pub selected: bool,
    /// The swatch's two halves, `0xRRGGBB` (the rail and the chat); Material You and Custom have none worth drawing (0).
    pub sidebar: u32,
    pub chat: u32,
}

fn look(l: &ThemeLook) -> Look {
    Look { theme: Theme::from_key(&l.theme), colour: l.colour.as_deref().and_then(parse_hex) }
}

/// The row, in order: Material You (only when `material_you_available`), Dark, Midnight, Light,
/// the recent custom themes newest first, Custom last. `current` is the look in use (its custom
/// colour is shown and selected even when `history` does not hold it yet); `history` is the looks
/// used, newest first; `material_you_on` makes Material You the selected one.
#[uniffi::export]
pub fn theme_row(
    current: ThemeLook,
    history: Vec<ThemeLook>,
    material_you_available: bool,
    material_you_on: bool,
) -> Vec<ThemeRowEntry> {
    let mut kept = ThemeHistory::default();
    for l in history.iter().rev() {
        kept.record(look(l));
    }
    entries(theme_row::theme_row(look(&current), &kept, material_you_available, material_you_on))
}

/// The recent custom themes for the Custom panel, under its colour picker: newest first, the one
/// in use selected (none while Material You is on). `history` is the looks used, newest first.
#[uniffi::export]
pub fn theme_history_list(
    current: ThemeLook,
    history: Vec<ThemeLook>,
    material_you_on: bool,
) -> Vec<ThemeRowEntry> {
    let mut kept = ThemeHistory::default();
    for l in history.iter().rev() {
        kept.record(look(l));
    }
    entries(theme_row::recent_customs(look(&current), &kept, material_you_on))
}

fn entries(items: Vec<theme_row::ThemeRowItem>) -> Vec<ThemeRowEntry> {
    items
        .into_iter()
        .map(|item| {
            let (sidebar, chat) = match (item.kind, item.colour) {
                (ThemeRowKind::Recent, Some(c)) => {
                    let p = derive(c);
                    (p.bg_sidebar, p.bg_chat)
                }
                (ThemeRowKind::Builtin, _) => theme::swatch(item.theme),
                _ => (0, 0),
            };
            ThemeRowEntry {
                kind: match item.kind {
                    ThemeRowKind::MaterialYou => ThemeRowItemKind::MaterialYou,
                    ThemeRowKind::Builtin => ThemeRowItemKind::Builtin,
                    ThemeRowKind::Recent => ThemeRowItemKind::Recent,
                    ThemeRowKind::Custom => ThemeRowItemKind::Custom,
                },
                theme: item.theme.key().to_owned(),
                colour: item.colour.map(|c| hex(c).trim_start_matches('#').to_owned()),
                selected: item.selected,
                sidebar,
                chat,
            }
        })
        .collect()
}

const HISTORY_FILE: &str = "theme_history.json";

/// The looks used lately, as kept in the app's folder.
pub(crate) fn history_in(dir: &std::path::PathBuf) -> ThemeHistory {
    let kept = std::fs::read_to_string(dir.join(HISTORY_FILE)).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    ThemeHistory::from_json(kept.as_ref())
}

pub(crate) fn keep_history(dir: &std::path::PathBuf, history: &ThemeHistory) {
    let _ = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(dir.join(HISTORY_FILE), history.to_json().to_string()));
}

fn looks_of(history: &ThemeHistory) -> Vec<ThemeLook> {
    history.newest_first().into_iter().map(|e| ThemeLook { theme: e.look.theme.key().to_owned(), colour: e.look.colour.map(|c| format!("{c:06x}")) }).collect()
}

#[uniffi::export]
impl Morpheus {
    /// The theme row of Settings > Appearance for the look in use and the looks kept (#490).
    pub fn theme_row_now(&self, material_you_available: bool, material_you_on: bool) -> Vec<ThemeRowEntry> {
        let (current, history) = self.look_and_history();
        theme_row(current, history, material_you_available, material_you_on)
    }

    /// The custom themes for the Custom panel, newest first.
    pub fn theme_history_now(&self, material_you_on: bool) -> Vec<ThemeRowEntry> {
        let (current, history) = self.look_and_history();
        theme_history_list(current, history, material_you_on)
    }

    /// Use a theme with a custom colour or without (picking a swatch or a history entry).
    pub fn use_theme_look(&self, theme: String, colour: Option<String>) -> Result<crate::settings::Appearance, crate::FfiError> {
        let colour = match colour {
            Some(text) => Some(theme::parse_hex(&text).ok_or_else(|| crate::FfiError::Failed {
                reason: format!("\"{}\" is not a colour (six hex digits, like 2a4d8f).", text.trim()),
            })?),
            None => None,
        };
        crate::settings::use_look(&self.dir, Theme::from_key(&theme), colour)
    }

    /// The look a theme code holds, without using it (the Custom panel previews it first).
    pub fn theme_look_of_code(&self, text: String) -> Result<ThemeLook, crate::FfiError> {
        let shared = morpheus_view_model::theme_share::SharedTheme::decode(&text).map_err(|err| crate::FfiError::Failed { reason: err.message() })?;
        Ok(ThemeLook { theme: shared.theme.key().to_owned(), colour: shared.colour.map(|c| format!("{c:06x}")) })
    }
}

impl Morpheus {
    fn look_and_history(&self) -> (ThemeLook, Vec<ThemeLook>) {
        let shared = crate::settings::shared_in(&self.dir);
        (ThemeLook { theme: shared.theme.key().to_owned(), colour: shared.colour.map(|c| format!("{c:06x}")) }, looks_of(&history_in(&self.dir)))
    }
}

/// The two halves of a swatch for a look (a typed colour, an opened code), for the Custom panel's preview: `0xRRGGBB` each.
#[uniffi::export]
pub fn theme_look_swatch(look: ThemeLook) -> ThemeRowEntry {
    let l = self::look(&look);
    let (sidebar, chat) = match l.colour {
        Some(c) => {
            let p = derive(c);
            (p.bg_sidebar, p.bg_chat)
        }
        None => theme::swatch(l.theme),
    };
    ThemeRowEntry { kind: ThemeRowItemKind::Recent, theme: look.theme, colour: look.colour, selected: false, sidebar, chat }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(theme: &str, colour: Option<&str>) -> ThemeLook {
        ThemeLook { theme: theme.into(), colour: colour.map(Into::into) }
    }

    #[test]
    fn the_row_follows_the_view_model_through_the_ffi() {
        let row = theme_row(l("dark", Some("2a4d8f")), vec![l("dark", Some("2a4d8f")), l("dark", Some("8f2a4d"))], true, false);
        let kinds: Vec<_> = row.iter().map(|e| e.kind).collect();
        use ThemeRowItemKind::*;
        assert_eq!(kinds, [MaterialYou, Builtin, Builtin, Builtin, Recent, Custom]);
        assert_eq!(row[4].colour.as_deref(), Some("2a4d8f"));
        assert!(row[4].selected);
        let all = theme_history_list(l("dark", Some("2a4d8f")), vec![l("dark", Some("2a4d8f")), l("dark", Some("8f2a4d"))], false);
        assert_eq!(all.iter().map(|e| e.colour.clone().unwrap()).collect::<Vec<_>>(), ["2a4d8f", "8f2a4d"]);
        assert!(all[0].selected && !all[1].selected);
        let none = theme_row(l("light", None), vec![], false, false);
        assert_eq!(none.len(), 4);
        assert!(none[2].selected);
    }

    #[test]
    fn a_custom_colour_in_use_is_kept_and_the_row_shows_only_the_newest() {
        let dir = std::env::temp_dir().join(format!("morpheus-theme-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for colour in ["2a4d8f", "8f2a4d", "4d8f2a"] {
            crate::settings::use_look(&dir, Theme::from_key("dark"), theme::parse_hex(colour)).unwrap();
        }
        let history = history_in(&dir);
        assert_eq!(looks_of(&history).iter().map(|l| l.colour.clone().unwrap()).collect::<Vec<_>>(), ["4d8f2a", "8f2a4d", "2a4d8f"]);
        let (current, looks) = (ThemeLook { theme: "dark".into(), colour: Some("4d8f2a".into()) }, looks_of(&history));
        let row = theme_row(current.clone(), looks.clone(), false, false);
        assert_eq!(row.iter().filter(|e| e.kind == ThemeRowItemKind::Recent).count(), 1, "one custom theme in the row");
        assert_eq!(theme_history_list(current, looks, false).len(), 3, "the whole history below the picker");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
