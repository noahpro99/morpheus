//! The theme row of Settings > Appearance (#490), from `morpheus_view_model::theme_row`: the row
//! (one recent custom theme) and `theme_history_list` (the whole history, for the Custom panel).

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
}
