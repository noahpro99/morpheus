//! The full emoji picker (`crates/ui/src/components/emoji_picker.rs`):
//! every emoji by category, a search, a skin tone and the recently used
//! ones first. The hover bar's "more" button opens it to react. This is
//! what it lists (search ranking, tones) and what it remembers on this
//! device; the front end draws it.

use std::path::Path;

use emojis::{Emoji, Group, SkinTone};

/// The categories, in order: group, name (a tab's title), the tab's icon.
pub const GROUPS: [(Group, &str, &str); 9] = [
    (Group::SmileysAndEmotion, "Smileys & Emotion", "😀"),
    (Group::PeopleAndBody, "People & Body", "👋"),
    (Group::AnimalsAndNature, "Animals & Nature", "🐶"),
    (Group::FoodAndDrink, "Food & Drink", "🍔"),
    (Group::TravelAndPlaces, "Travel & Places", "✈️"),
    (Group::Activities, "Activities", "⚽"),
    (Group::Objects, "Objects", "💡"),
    (Group::Symbols, "Symbols", "❤️"),
    (Group::Flags, "Flags", "🏁"),
];

/// The tones offered: (stored number, name, tone).
pub const TONES: [(u8, &str, SkinTone); 6] = [
    (0, "Default", SkinTone::Default),
    (1, "Light", SkinTone::Light),
    (2, "MediumLight", SkinTone::MediumLight),
    (3, "Medium", SkinTone::Medium),
    (4, "MediumDark", SkinTone::MediumDark),
    (5, "Dark", SkinTone::Dark),
];

/// The tone for a stored number (none: the default).
pub fn tone_of(stored: Option<u8>) -> SkinTone {
    TONES
        .iter()
        .find(|(n, _, _)| Some(*n) == stored)
        .map_or(SkinTone::Default, |(_, _, tone)| *tone)
}

/// `emoji` as shown and sent: in the chosen tone when it has tones.
pub fn toned(emoji: &'static Emoji, tone: SkinTone) -> &'static str {
    if tone == SkinTone::Default {
        return emoji.as_str();
    }
    emoji
        .with_skin_tone(tone)
        .map_or(emoji.as_str(), Emoji::as_str)
}

/// A raised hand in `tone`, standing for the tone.
pub fn hand(tone: SkinTone) -> &'static str {
    emojis::get("✋").map_or("✋", |e| toned(e, tone))
}

/// Emoji whose name or shortcodes match `query`: shortcodes starting with
/// it first, then names starting with it, then a word of the name, then
/// anything containing it.
pub fn search(query: &str) -> Vec<&'static Emoji> {
    let query = query.trim().to_lowercase().replace(' ', "_");
    let spaced = query.replace('_', " ");
    let mut found: Vec<(u8, &'static Emoji)> = emojis::iter()
        .filter_map(|emoji| {
            let name = emoji.name().to_lowercase();
            let codes: Vec<&str> = emoji.shortcodes().collect();
            let score = if codes.iter().any(|c| c.starts_with(&query)) {
                0
            } else if name.starts_with(&spaced) {
                1
            } else if name.split(' ').any(|w| w.starts_with(&spaced)) {
                2
            } else if codes.iter().any(|c| c.contains(&query)) || name.contains(&spaced) {
                3
            } else {
                return None;
            };
            Some((score, emoji))
        })
        .collect();
    found.sort_by_key(|(score, _)| *score);
    found.into_iter().map(|(_, emoji)| emoji).collect()
}

/// One section of the grid: its group (`data-group`), heading and emoji.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub group: &'static str,
    /// Empty for the search results.
    pub heading: &'static str,
    pub cells: Vec<String>,
}

/// What the grid lists: with a query, the matches in one section (none:
/// empty); without, the recently used ones (if any) and every category.
pub fn sections(query: &str, tone: SkinTone, recent: &[String]) -> Vec<Section> {
    if !query.trim().is_empty() {
        let cells: Vec<String> = search(query)
            .into_iter()
            .map(|e| toned(e, tone).to_owned())
            .collect();
        return if cells.is_empty() {
            Vec::new()
        } else {
            vec![Section {
                group: "Results",
                heading: "",
                cells,
            }]
        };
    }
    let mut sections = Vec::new();
    if !recent.is_empty() {
        sections.push(Section {
            group: "Recent",
            heading: "Recently used",
            cells: recent.to_vec(),
        });
    }
    for (group, name, _) in GROUPS {
        sections.push(Section {
            group: name,
            heading: name,
            cells: group.emojis().map(|e| toned(e, tone).to_owned()).collect(),
        });
    }
    sections
}

/// What the picker keeps on this device: the skin tone and the emoji used
/// lately, newest first (`emoji.json` in the data directory, as the
/// Dioxus app's).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Remembered {
    pub tone: Option<u8>,
    pub recent: Vec<String>,
}

/// How many recent emoji are kept.
pub const RECENT_KEPT: usize = 24;
const FILE: &str = "emoji.json";

impl Remembered {
    pub fn load(data_dir: &Path) -> Self {
        let saved: serde_json::Value = std::fs::read(data_dir.join(FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            tone: saved
                .get("tone")
                .and_then(|v| v.as_u64())
                .and_then(|t| u8::try_from(t).ok()),
            recent: saved
                .get("recent")
                .and_then(|v| v.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// Writes it back, keeping whatever else the file holds.
    pub fn save(&self, data_dir: &Path) -> std::io::Result<()> {
        let mut saved: serde_json::Value = std::fs::read(data_dir.join(FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        saved["tone"] = serde_json::json!(self.tone);
        saved["recent"] = serde_json::json!(self.recent);
        std::fs::create_dir_all(data_dir)?;
        std::fs::write(data_dir.join(FILE), saved.to_string())
    }

    /// `emoji` used just now: first among the recent ones.
    pub fn note_used(&mut self, emoji: &str) {
        self.recent.retain(|e| e != emoji);
        self.recent.insert(0, emoji.to_owned());
        self.recent.truncate(RECENT_KEPT);
    }

    /// The tone chosen: 0 (default) is stored as none.
    pub fn set_tone(&mut self, stored: u8) {
        self.tone = (stored > 0).then_some(stored);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_finds_by_shortcode_and_name() {
        assert_eq!(search("rocket").first().map(|e| e.as_str()), Some("🚀"));
        assert_eq!(search("thumbsup").first().map(|e| e.as_str()), Some("👍"));
        assert_eq!(search("thumbs up").first().map(|e| e.as_str()), Some("👍"));
        assert!(search("zzqqxx").is_empty());
        assert!(search("rocket").len() < 10);
    }

    #[test]
    fn tones_apply_where_there_are_tones() {
        let thumbs = emojis::get("👍").expect("thumbs up");
        assert_eq!(toned(thumbs, SkinTone::Medium), "👍🏽");
        let rocket = emojis::get("🚀").expect("rocket");
        assert_eq!(toned(rocket, SkinTone::Medium), "🚀");
        assert_eq!(tone_of(Some(3)), SkinTone::Medium);
        assert_eq!(tone_of(None), SkinTone::Default);
        assert_eq!(tone_of(Some(99)), SkinTone::Default);
    }

    #[test]
    fn the_grid_lists_recent_first_then_every_category() {
        let all = sections("", SkinTone::Default, &[]);
        let names: Vec<_> = all.iter().map(|s| s.group).collect();
        assert_eq!(names.len(), 9);
        assert_eq!(names[0], "Smileys & Emotion");
        assert!(all.iter().map(|s| s.cells.len()).sum::<usize>() > 1000);
        let with_recent = sections("  ", SkinTone::Default, &["🚀".to_owned()]);
        assert_eq!(with_recent[0].group, "Recent");
        assert_eq!(with_recent[0].heading, "Recently used");
        assert_eq!(with_recent[0].cells, ["🚀"]);
        assert_eq!(with_recent.len(), 10);
    }

    #[test]
    fn a_search_is_one_section_in_the_chosen_tone_or_nothing() {
        let found = sections("thumbsup", SkinTone::Medium, &["🚀".to_owned()]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].group, "Results");
        assert_eq!(found[0].cells[0], "👍🏽");
        assert!(sections("zzqqxx", SkinTone::Default, &[]).is_empty());
    }

    #[test]
    fn what_was_used_is_remembered_newest_first_without_repeats_and_capped() {
        let mut kept = Remembered::default();
        kept.note_used("🚀");
        kept.note_used("👍🏽");
        kept.note_used("🚀");
        assert_eq!(kept.recent, ["🚀", "👍🏽"]);
        for i in 0..40 {
            kept.note_used(&format!("{i}"));
        }
        assert_eq!(kept.recent.len(), RECENT_KEPT);
        assert_eq!(kept.recent[0], "39");
        kept.set_tone(3);
        assert_eq!(kept.tone, Some(3));
        kept.set_tone(0);
        assert_eq!(kept.tone, None);
    }

    #[test]
    fn it_round_trips_through_the_data_directory_keeping_other_settings() {
        let dir = std::env::temp_dir().join(format!("morpheus-emoji-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join(FILE), r#"{"other": 1}"#).expect("write");
        let mut kept = Remembered::load(&dir);
        assert_eq!(kept, Remembered::default());
        kept.note_used("🚀");
        kept.set_tone(2);
        kept.save(&dir).expect("save");
        assert_eq!(Remembered::load(&dir), kept);
        let raw: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join(FILE)).expect("read")).expect("json");
        assert_eq!(raw["other"], 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
