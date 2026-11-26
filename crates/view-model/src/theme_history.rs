//! The looks (a theme, or a custom colour) you have used, so a tweak does not lose
//! one you liked: the last few, most recent first, plus the pinned ones, which stay.

use crate::settings::Theme;

/// Unpinned looks kept.
pub const KEPT: usize = 10;

/// A look: the theme, and the custom colour that replaces it, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    pub theme: Theme,
    pub colour: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub look: Look,
    pub pinned: bool,
}

/// Recency order, newest first (pinned or not).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeHistory {
    entries: Vec<Entry>,
}

impl ThemeHistory {
    /// `look` was in use: it becomes the newest (keeping its pin), and the oldest
    /// unpinned looks beyond [`KEPT`] go. Returns whether anything changed.
    pub fn record(&mut self, look: Look) -> bool {
        let before = self.entries.clone();
        let pinned = self
            .entries
            .iter()
            .position(|e| e.look == look)
            .map(|at| self.entries.remove(at).pinned)
            .unwrap_or(false);
        self.entries.insert(0, Entry { look, pinned });
        let mut unpinned = 0;
        self.entries.retain(|e| {
            if e.pinned {
                return true;
            }
            unpinned += 1;
            unpinned <= KEPT
        });
        self.entries != before
    }

    /// Pins `look` if it is kept and not pinned, else unpins it.
    pub fn toggle_pin(&mut self, look: Look) -> bool {
        match self.entries.iter_mut().find(|e| e.look == look) {
            Some(e) => {
                e.pinned = !e.pinned;
                true
            }
            None => false,
        }
    }

    /// Recency order, newest first, pins not moved (the theme row, #490).
    pub fn newest_first(&self) -> Vec<Entry> {
        self.entries.clone()
    }

    /// For showing: pinned first, each group newest first.
    pub fn shown(&self) -> Vec<Entry> {
        let (pinned, rest): (Vec<Entry>, Vec<Entry>) =
            self.entries.iter().copied().partition(|e| e.pinned);
        pinned.into_iter().chain(rest).collect()
    }

    /// As kept in the account: a list of `{theme, colour?, pinned?}`.
    pub fn to_json(&self) -> serde_json::Value {
        self.entries
            .iter()
            .map(|e| {
                let mut v = serde_json::json!({ "theme": e.look.theme.key() });
                if let Some(c) = e.look.colour {
                    v["colour"] = crate::theme::hex(c).into();
                }
                if e.pinned {
                    v["pinned"] = true.into();
                }
                v
            })
            .collect::<Vec<_>>()
            .into()
    }

    /// Reads [`Self::to_json`]; anything else (absent, malformed) is an empty history.
    pub fn from_json(value: Option<&serde_json::Value>) -> Self {
        let mut history = Self::default();
        let Some(items) = value.and_then(|v| v.as_array()) else {
            return history;
        };
        for item in items {
            let Some(theme) = item.get("theme").and_then(|v| v.as_str()) else {
                continue;
            };
            let look = Look {
                theme: Theme::from_key(theme),
                colour: item
                    .get("colour")
                    .and_then(|v| v.as_str())
                    .and_then(crate::theme::parse_hex),
            };
            if history.entries.iter().any(|e| e.look == look) {
                continue;
            }
            history.entries.push(Entry {
                look,
                pinned: item.get("pinned").and_then(|v| v.as_bool()).unwrap_or(false),
            });
        }
        history
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colour(c: u32) -> Look {
        Look { theme: Theme::Dark, colour: Some(c) }
    }

    #[test]
    fn the_newest_look_is_first_and_a_repeat_moves_to_the_front() {
        let mut h = ThemeHistory::default();
        assert!(h.record(colour(1)));
        assert!(h.record(colour(2)));
        assert!(h.record(colour(1)));
        let looks: Vec<Look> = h.shown().iter().map(|e| e.look).collect();
        assert_eq!(looks, vec![colour(1), colour(2)]);
        assert!(!h.record(colour(1)), "already first: nothing changed");
    }

    #[test]
    fn a_theme_and_a_colour_on_it_are_different_looks() {
        let mut h = ThemeHistory::default();
        h.record(Look { theme: Theme::Dark, colour: None });
        h.record(colour(5));
        h.record(Look { theme: Theme::Light, colour: Some(5) });
        assert_eq!(h.shown().len(), 3);
    }

    #[test]
    fn only_the_last_ten_unpinned_are_kept_and_pinned_ones_stay_first() {
        let mut h = ThemeHistory::default();
        h.record(colour(100));
        h.toggle_pin(colour(100));
        for c in 0..15 {
            h.record(colour(c));
        }
        let shown = h.shown();
        assert_eq!(shown.len(), 11);
        assert_eq!(shown[0].look, colour(100));
        assert!(shown[0].pinned);
        assert_eq!(shown[1].look, colour(14));
        assert_eq!(shown[10].look, colour(5));
    }

    #[test]
    fn recording_a_pinned_look_keeps_its_pin_and_unpinning_toggles() {
        let mut h = ThemeHistory::default();
        h.record(colour(1));
        h.record(colour(2));
        assert!(h.toggle_pin(colour(1)));
        h.record(colour(1));
        assert!(h.shown()[0].pinned);
        assert!(h.toggle_pin(colour(1)));
        assert!(!h.shown()[0].pinned);
        assert!(!h.toggle_pin(colour(9)), "unknown look");
    }

    #[test]
    fn it_round_trips_and_tolerates_junk() {
        let mut h = ThemeHistory::default();
        h.record(colour(0x2a4d8f));
        h.record(Look { theme: Theme::Midnight, colour: None });
        h.toggle_pin(colour(0x2a4d8f));
        assert_eq!(ThemeHistory::from_json(Some(&h.to_json())), h);
        assert_eq!(ThemeHistory::from_json(None), ThemeHistory::default());
        assert_eq!(
            ThemeHistory::from_json(Some(&serde_json::json!("x"))),
            ThemeHistory::default()
        );
        let odd = serde_json::json!([{"colour": "zz"}, {"theme": "dark"}, {"theme": "dark"}]);
        assert_eq!(ThemeHistory::from_json(Some(&odd)).shown().len(), 1);
    }
}
