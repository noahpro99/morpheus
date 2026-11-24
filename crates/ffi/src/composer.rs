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

use crate::{MediaHandle, RoomTimeline};

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
