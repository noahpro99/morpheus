//! What the message box offers: suggestions for the word being typed (`@`
//! members, `:` emoji, custom emoji) and the emoji panel. The matching and
//! the rewriting of the draft are `morpheus_view_model::composer` and
//! `emoji_picker`, the desktop's.

use std::{collections::HashMap, sync::Arc};

use morpheus_core::{Emote, MediaRef};
use morpheus_view_model::{
    composer::{self, Suggestion},
    emoji_picker,
};

use crate::{MediaHandle, Morpheus, RoomTimeline};

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SuggestionKind {
    Emoji,
    Custom,
    Member,
}

/// One row of the suggestions above the message box.
#[derive(uniffi::Record)]
pub struct SuggestionItem {
    pub kind: SuggestionKind,
    /// The emoji, the `shortcode` or the person's name.
    pub label: String,
    /// The emoji's code, or the person's ID.
    pub detail: Option<String>,
    /// The picture of a custom emoji.
    pub media: Option<Arc<MediaHandle>>,
    /// The whole draft with this chosen.
    pub draft_after: String,
}

#[derive(uniffi::Record)]
pub struct EmojiSection {
    pub heading: String,
    pub cells: Vec<String>,
}

#[derive(uniffi::Record)]
pub struct CustomEmojiItem {
    pub shortcode: String,
    pub media: Arc<MediaHandle>,
}

#[derive(uniffi::Record)]
pub struct CustomEmojiSection {
    pub name: String,
    pub emotes: Vec<CustomEmojiItem>,
}

/// The emoji panel: every category, or the matches of `query` in one section.
#[uniffi::export]
pub fn emoji_sections(query: String) -> Vec<EmojiSection> {
    emoji_picker::sections(&query, emoji_picker::tone_of(None), &[])
        .into_iter()
        .map(|section| EmojiSection {
            heading: if section.heading.is_empty() {
                section.group.to_owned()
            } else {
                section.heading.to_owned()
            },
            cells: section.cells,
        })
        .collect()
}

/// One skin tone the panel offers: its number and a raised hand in it.
#[derive(uniffi::Record)]
pub struct ToneChoice {
    pub number: u8,
    pub hand: String,
}

/// The emoji panel as this device has it: the tone chosen, the tones offered and the sections (recently used first).
#[derive(uniffi::Record)]
pub struct EmojiPanelInfo {
    pub tone: u8,
    pub tones: Vec<ToneChoice>,
    pub sections: Vec<EmojiSection>,
}

#[uniffi::export]
impl Morpheus {
    /// The emoji panel for `query` in the kept skin tone, with the emoji used lately in front (#420).
    pub fn emoji_panel(&self, query: String) -> EmojiPanelInfo {
        let kept = emoji_picker::Remembered::load(&self.dir);
        let sections = emoji_picker::sections(&query, emoji_picker::tone_of(kept.tone), &kept.recent)
            .into_iter()
            .map(|section| EmojiSection {
                heading: if section.heading.is_empty() { section.group.to_owned() } else { section.heading.to_owned() },
                cells: section.cells,
            })
            .collect();
        EmojiPanelInfo {
            tone: kept.tone.unwrap_or(0),
            tones: emoji_picker::TONES.iter().map(|(number, _, tone)| ToneChoice { number: *number, hand: emoji_picker::hand(*tone).to_owned() }).collect(),
            sections,
        }
    }

    /// `glyph` was used: it comes first among the recent ones (a custom emoji, `:name:`, is not kept here).
    pub fn note_emoji_used(&self, glyph: String) {
        if glyph.starts_with(':') {
            return;
        }
        let mut kept = emoji_picker::Remembered::load(&self.dir);
        kept.note_used(&glyph);
        let _ = kept.save(&self.dir);
    }

    /// Keep skin tone `number` (0 is the default).
    pub fn set_emoji_tone(&self, number: u8) {
        let mut kept = emoji_picker::Remembered::load(&self.dir);
        kept.set_tone(number);
        let _ = kept.save(&self.dir);
    }
}

/// The draft with `glyph` added after it.
#[uniffi::export]
pub fn with_emoji(draft: String, glyph: String) -> String {
    composer::with_emoji(&draft, &glyph)
}

impl RoomTimeline {
    /// The custom emoji in reach in this room: the person's own pack and the
    /// packs of their rooms.
    pub(crate) async fn emotes(&self) -> Vec<Emote> {
        self.packs()
            .await
            .into_iter()
            .flat_map(|pack| pack.emotes)
            .collect()
    }

    async fn packs(&self) -> Vec<morpheus_core::EmotePack> {
        self.session
            .emote_packs(Some(self.room_id.clone()))
            .await
            .unwrap_or_default()
    }

    /// `(shortcode, mxc)` pairs a send turns into pictures.
    pub(crate) async fn emote_pairs(&self, body: &str) -> Vec<(String, String)> {
        if !morpheus_core::may_name_emote(body) {
            return Vec::new();
        }
        self.emotes()
            .await
            .into_iter()
            .map(|e| (e.shortcode, e.url))
            .collect()
    }
}

#[uniffi::export]
impl RoomTimeline {
    /// What the word being typed at the end of `draft` could become.
    pub async fn suggest(&self, draft: String) -> Vec<SuggestionItem> {
        let members = if composer::wants_members(&draft) {
            self.session.members(self.room_id.clone()).await.ok()
        } else {
            None
        };
        let custom = if composer::last_word(&draft).starts_with(':') {
            self.emotes().await
        } else {
            Vec::new()
        };
        composer::suggest_with(&draft, members.as_deref(), &HashMap::new(), &custom)
            .into_iter()
            .map(|choice| {
                let draft_after = composer::accept(&draft, &choice);
                match choice {
                    Suggestion::Emoji { emoji, code } => SuggestionItem {
                        kind: SuggestionKind::Emoji,
                        label: emoji.to_owned(),
                        detail: Some(code.to_owned()),
                        media: None,
                        draft_after,
                    },
                    Suggestion::Custom { shortcode, url } => SuggestionItem {
                        kind: SuggestionKind::Custom,
                        label: shortcode,
                        detail: None,
                        media: Some(MediaHandle::new(MediaRef::from_mxc_uri(&url), &self.session)),
                        draft_after,
                    },
                    Suggestion::Member { name, user_id, .. } => SuggestionItem {
                        kind: SuggestionKind::Member,
                        label: name,
                        detail: Some(user_id),
                        media: None,
                        draft_after,
                    },
                }
            })
            .collect()
    }

    /// The custom emoji for the panel, by pack; `query` narrows by name.
    pub async fn custom_emoji(&self, query: String) -> Vec<CustomEmojiSection> {
        let packs = self.packs().await;
        emoji_picker::custom_sections(&packs, &query)
            .into_iter()
            .map(|section| CustomEmojiSection {
                name: section.name,
                emotes: section
                    .emotes
                    .into_iter()
                    .map(|e| CustomEmojiItem {
                        media: MediaHandle::new(MediaRef::from_mxc_uri(&e.url), &self.session),
                        shortcode: e.shortcode,
                    })
                    .collect(),
            })
            .collect()
    }
}
