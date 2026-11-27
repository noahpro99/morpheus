//! The theme row of Settings > Appearance (#490), one list for both apps: Material You
//! (only where the platform offers it), the built-in themes, the ONE most recent custom theme
//! and a Custom item last, so the row fits without scrolling. The Custom panel lists the
//! whole history under its colour picker (`recent_customs`, newest first). The current custom
//! theme is the newest and the selected one.

use crate::{
    settings::Theme,
    theme_history::{Look, ThemeHistory},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeRowKind {
    /// The wallpaper's colours (Android 12 and up).
    MaterialYou,
    Builtin,
    /// A custom colour used lately.
    Recent,
    /// Opens the colour picker and the theme-code entry; never selected itself.
    Custom,
}

/// One swatch of the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeRowItem {
    pub kind: ThemeRowKind,
    /// What picking it applies: the base theme (Dark for Material You and Custom, which apply
    /// nothing themselves) ...
    pub theme: Theme,
    /// ... and the custom colour, for a recent one.
    pub colour: Option<u32>,
    pub selected: bool,
}

/// The recent custom themes, newest first: the current custom colour first (so it is always
/// there and selected, even when `history` does not hold it yet), then the history without
/// repeats. The list under the colour picker of the Custom panel.
pub fn recent_customs(
    current: Look,
    history: &ThemeHistory,
    material_you_on: bool,
) -> Vec<ThemeRowItem> {
    let mut colours: Vec<(Theme, u32)> = Vec::new();
    if let Some(c) = current.colour {
        colours.push((current.theme, c));
    }
    for entry in history.newest_first() {
        if let Some(c) = entry.look.colour {
            if !colours.iter().any(|(_, seen)| *seen == c) {
                colours.push((entry.look.theme, c));
            }
        }
    }
    colours
        .into_iter()
        .map(|(theme, colour)| ThemeRowItem {
            kind: ThemeRowKind::Recent,
            theme,
            colour: Some(colour),
            selected: !material_you_on && current.colour == Some(colour),
        })
        .collect()
}

/// The row. `current` is the look in use; `material_you_on` whether Material You is in use
/// (it then is the selected item and the rest are not). A current custom colour missing
/// from `history` (or not newest) is shown first among the recent ones, so it is always
/// the selected one.
pub fn theme_row(
    current: Look,
    history: &ThemeHistory,
    material_you_available: bool,
    material_you_on: bool,
) -> Vec<ThemeRowItem> {
    let material_on = material_you_available && material_you_on;
    let mut items = Vec::new();
    if material_you_available {
        items.push(ThemeRowItem {
            kind: ThemeRowKind::MaterialYou,
            theme: Theme::Dark,
            colour: None,
            selected: material_on,
        });
    }
    for theme in Theme::ALL {
        items.push(ThemeRowItem {
            kind: ThemeRowKind::Builtin,
            theme,
            colour: None,
            selected: !material_on && current.colour.is_none() && current.theme == theme,
        });
    }
    // Only the newest one: the rest are in the Custom panel.
    items.extend(recent_customs(current, history, material_on).into_iter().take(1));
    items.push(ThemeRowItem {
        kind: ThemeRowKind::Custom,
        theme: Theme::Dark,
        colour: None,
        selected: false,
    });
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custom(c: u32) -> Look {
        Look { theme: Theme::Dark, colour: Some(c) }
    }
    fn builtin(theme: Theme) -> Look {
        Look { theme, colour: None }
    }
    fn history(colours: &[u32]) -> ThemeHistory {
        let mut h = ThemeHistory::default();
        for c in colours {
            h.record(custom(*c));
        }
        h
    }
    fn kinds(row: &[ThemeRowItem]) -> Vec<ThemeRowKind> {
        row.iter().map(|i| i.kind).collect()
    }
    fn selected(row: &[ThemeRowItem]) -> Vec<usize> {
        row.iter().enumerate().filter(|(_, i)| i.selected).map(|(at, _)| at).collect()
    }

    #[test]
    fn the_order_is_material_you_built_ins_the_newest_custom_then_custom() {
        use ThemeRowKind::*;
        let row = theme_row(builtin(Theme::Dark), &history(&[1, 2]), true, false);
        assert_eq!(kinds(&row), [MaterialYou, Builtin, Builtin, Builtin, Recent, Custom]);
        let colours: Vec<_> = row.iter().filter_map(|i| i.colour).collect();
        assert_eq!(colours, [2], "only the newest");
        assert_eq!(
            row[1..4].iter().map(|i| i.theme).collect::<Vec<_>>(),
            Theme::ALL.to_vec()
        );
    }

    #[test]
    fn material_you_is_there_only_when_available() {
        let row = theme_row(builtin(Theme::Dark), &history(&[]), false, true);
        assert!(row.iter().all(|i| i.kind != ThemeRowKind::MaterialYou));
        assert_eq!(row.len(), 4, "three built-ins and Custom");
        assert_eq!(selected(&row), [0], "Dark: Material You on is ignored when unavailable");
    }

    #[test]
    fn a_built_in_is_the_one_selected() {
        let row = theme_row(builtin(Theme::Midnight), &history(&[7]), true, false);
        assert_eq!(selected(&row), [2]);
        assert_eq!(row[2].theme, Theme::Midnight);
    }

    #[test]
    fn the_current_custom_colour_is_the_newest_history_entry_and_selected() {
        let row = theme_row(custom(2), &history(&[1, 2]), false, false);
        let at = selected(&row);
        assert_eq!(at.len(), 1);
        assert_eq!(row[at[0]].colour, Some(2));
        assert_eq!(row[at[0]].kind, ThemeRowKind::Recent);
        assert_eq!(at[0], 3, "the newest custom");
        assert!(!row.last().expect("custom").selected, "Custom is a button, never selected");
    }

    #[test]
    fn a_history_pick_becomes_the_one_shown_and_stays_selected() {
        // Picked the older one (1): it is current, so it is the one in the row; the Custom
        // panel still lists both, newest first.
        let row = theme_row(custom(1), &history(&[1, 2]), false, false);
        let colours: Vec<_> = row.iter().filter_map(|i| i.colour).collect();
        assert_eq!(colours, [1]);
        assert_eq!(selected(&row), [3]);
        let all = recent_customs(custom(1), &history(&[1, 2]), false);
        assert_eq!(all.iter().filter_map(|i| i.colour).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(all.iter().filter(|i| i.selected).count(), 1);
    }

    #[test]
    fn a_custom_colour_not_yet_in_the_history_is_shown_first_and_selected() {
        let row = theme_row(custom(9), &history(&[1]), false, false);
        let colours: Vec<_> = row.iter().filter_map(|i| i.colour).collect();
        assert_eq!(colours, [9], "the row holds the one in use");
        let all = recent_customs(custom(9), &history(&[1]), false);
        assert_eq!(all.iter().filter_map(|i| i.colour).collect::<Vec<_>>(), [9, 1]);
        assert_eq!(selected(&row), [3]);
    }

    #[test]
    fn a_colour_appears_once_however_many_looks_hold_it() {
        let mut h = history(&[5]);
        h.record(Look { theme: Theme::Light, colour: Some(5) });
        h.record(builtin(Theme::Light));
        let row = theme_row(custom(5), &h, false, false);
        assert_eq!(row.iter().filter(|i| i.colour == Some(5)).count(), 1);
        assert_eq!(row.iter().filter(|i| i.kind == ThemeRowKind::Recent).count(), 1);
        assert_eq!(recent_customs(custom(5), &h, false).len(), 1);
    }

    #[test]
    fn material_you_on_is_the_selected_one_and_nothing_else_is() {
        let row = theme_row(custom(3), &history(&[3]), true, true);
        assert_eq!(selected(&row), [0]);
    }

    #[test]
    fn the_panel_lists_only_the_last_ten_customs_and_the_row_one() {
        let colours: Vec<u32> = (1..=15).collect();
        let row = theme_row(builtin(Theme::Dark), &history(&colours), false, false);
        assert_eq!(row.iter().filter(|i| i.kind == ThemeRowKind::Recent).count(), 1);
        let all = recent_customs(builtin(Theme::Dark), &history(&colours), false);
        assert_eq!(all.len(), crate::theme_history::KEPT);
    }
}
