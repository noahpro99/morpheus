//! What the message box does besides editing text, as
//! `crates/ui/src/components/composer.rs` decides it, with no UI toolkit in
//! it: the autocomplete (`:code` emoji, `@name` members), the reply and edit
//! modes, the files waiting on the message being written, and what Enter
//! sends. Markdown becomes HTML in the core when text is sent
//! (`TimelineHandle::send_text`), so the plan only carries the plain text.

use std::collections::HashMap;

use morpheus_core::{Member, Message, MessageId, OwnedEventId};

use super::emoji;

/// What the next send does besides sending plain text.
#[derive(Debug, Clone, PartialEq)]
pub enum ComposerMode {
    Reply {
        event_id: OwnedEventId,
        sender_name: String,
        summary: String,
    },
    Edit {
        id: MessageId,
        original: String,
    },
}

/// One row of the autocomplete.
#[derive(Debug, Clone, PartialEq)]
pub enum Suggestion {
    Emoji {
        emoji: &'static str,
        code: &'static str,
    },
    Member {
        /// Their display name: what goes in the message, so they are pinged.
        name: String,
        /// Your nickname for them, if you gave one.
        nickname: Option<String>,
        user_id: String,
    },
}

impl Suggestion {
    /// What replaces the word being typed.
    pub fn insert(&self) -> String {
        match self {
            Self::Emoji { emoji, .. } => format!("{emoji} "),
            Self::Member { name, .. } => format!("@{name} "),
        }
    }
}

/// The word the caret sits in, taken as the draft's last word.
pub fn last_word(draft: &str) -> &str {
    draft.rsplit(char::is_whitespace).next().unwrap_or("")
}

/// Whether the draft's last word asks for members (`@…`).
pub fn wants_members(draft: &str) -> bool {
    last_word(draft).starts_with('@')
}

/// Suggestions for the word being typed: `:code` from the emoji table,
/// `@name` from the room's members, found by display name, ID, or your
/// nickname for them (`nicknames`: user ID to nickname).
pub fn suggest(
    draft: &str,
    members: Option<&[Member]>,
    nicknames: &HashMap<String, String>,
) -> Vec<Suggestion> {
    let word = last_word(draft);
    if let Some(code) = word.strip_prefix(':') {
        if code.len() < 2 {
            return Vec::new();
        }
        return emoji::search(code)
            .into_iter()
            .take(8)
            .map(|(emoji, code)| Suggestion::Emoji { emoji, code })
            .collect();
    }
    if let Some(prefix) = word.strip_prefix('@') {
        let prefix = prefix.to_lowercase();
        return members
            .unwrap_or_default()
            .iter()
            .map(|m| (m, nicknames.get(m.user_id.as_str())))
            .filter(|(m, nickname)| {
                m.name.to_lowercase().starts_with(&prefix)
                    || m.user_id.localpart().to_lowercase().starts_with(&prefix)
                    || nickname.is_some_and(|n| n.to_lowercase().starts_with(&prefix))
            })
            .take(8)
            .map(|(m, nickname)| Suggestion::Member {
                name: m.name.clone(),
                nickname: nickname.cloned(),
                user_id: m.user_id.to_string(),
            })
            .collect();
    }
    Vec::new()
}

/// The draft with its last word replaced by what `choice` inserts.
pub fn accept(draft: &str, choice: &Suggestion) -> String {
    let keep = draft.len() - last_word(draft).len();
    format!("{}{}", &draft[..keep], choice.insert())
}

/// The draft with `glyph` (and a space) added at the end, a space first if
/// the draft does not end in one: what the emoji panel does.
pub fn with_emoji(draft: &str, glyph: &str) -> String {
    let mut text = draft.to_owned();
    if !text.is_empty() && !text.ends_with(' ') {
        text.push(' ');
    }
    text.push_str(glyph);
    text.push(' ');
    text
}

/// The draft with `@name ` added at the end ("Mention" from a member's menu).
pub fn with_mention(draft: &str, name: &str) -> String {
    let mut text = draft.to_owned();
    if !text.is_empty() && !text.ends_with(' ') {
        text.push(' ');
    }
    text.push_str(&format!("@{name} "));
    text
}

/// A file waiting on the message being written (pasted, dropped or picked).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attached {
    pub id: u64,
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// The kind of send the box makes of its draft.
#[derive(Debug, Clone, PartialEq)]
pub enum SendKind {
    Text,
    Reply(OwnedEventId),
    Edit(MessageId),
}

/// What Enter sends: the text (trimmed, if any) in the way the mode says,
/// then the files, after it so they read below it.
#[derive(Debug, Clone, PartialEq)]
pub struct SendPlan {
    pub text: Option<(SendKind, String)>,
    pub files: Vec<Attached>,
}

/// A mime type from a file name, for files dropped or picked from disk.
pub fn mime_of(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map_or("", |(_, e)| e).to_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "txt" | "md" | "log" => "text/plain",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "ogg" | "oga" => "audio/ogg",
        "wav" => "audio/wav",
        "json" => "application/json",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

/// The composer's state apart from the text and the suggestions' rows.
#[derive(Debug, Default)]
pub struct ComposerState {
    pub mode: Option<ComposerMode>,
    pub attachments: Vec<Attached>,
    next_id: u64,
}

impl ComposerState {
    /// A reply to `message`; not possible to one that is still sending.
    pub fn start_reply(&mut self, message: &Message) -> bool {
        let MessageId::Event(event_id) = &message.id else {
            return false;
        };
        if message.is_local_echo {
            return false;
        }
        self.mode = Some(ComposerMode::Reply {
            event_id: event_id.clone(),
            sender_name: message.sender_name.clone(),
            summary: message.kind.summary(),
        });
        true
    }

    /// Editing `message`: the text to put in the box.
    pub fn start_edit(&mut self, message: &Message) -> String {
        let original = message.kind.plain_body().unwrap_or_default().to_owned();
        self.mode = Some(ComposerMode::Edit {
            id: message.id.clone(),
            original: original.clone(),
        });
        original
    }

    /// Esc or the banner's ✕: leaves reply or edit mode. Whether there was
    /// one (the draft is cleared then, as the Dioxus app does).
    pub fn cancel(&mut self) -> bool {
        self.mode.take().is_some()
    }

    pub fn editing(&self) -> bool {
        matches!(self.mode, Some(ComposerMode::Edit { .. }))
    }

    /// A file waits on the next message. Its tile's ID.
    pub fn attach(&mut self, name: String, mime: String, bytes: Vec<u8>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.attachments.push(Attached {
            id,
            name,
            mime,
            bytes,
        });
        id
    }

    /// The tile's ✕.
    pub fn remove(&mut self, id: u64) {
        self.attachments.retain(|a| a.id != id);
    }

    /// Whether Enter would send anything for this draft.
    pub fn can_send(&self, draft: &str) -> bool {
        !draft.trim().is_empty() || (!self.attachments.is_empty() && !self.editing())
    }

    /// Enter: what goes out, and the box is spent (mode and files taken). An
    /// edit changes the text only; files wait for a message of their own.
    pub fn take_send(&mut self, draft: &str) -> Option<SendPlan> {
        let body = draft.trim();
        let editing = self.editing();
        if body.is_empty() && (self.attachments.is_empty() || editing) {
            return None;
        }
        let files = if editing {
            Vec::new()
        } else {
            std::mem::take(&mut self.attachments)
        };
        let mode = self.mode.take();
        let text = (!body.is_empty()).then(|| {
            let kind = match mode {
                None => SendKind::Text,
                Some(ComposerMode::Reply { event_id, .. }) => SendKind::Reply(event_id),
                Some(ComposerMode::Edit { id, .. }) => SendKind::Edit(id),
            };
            (kind, body.to_owned())
        });
        Some(SendPlan { text, files })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use morpheus_core::{MessageKind, OwnedUserId};

    fn member(user: &str, name: &str) -> Member {
        Member {
            user_id: OwnedUserId::try_from(user).expect("user id"),
            name: name.into(),
            avatar: None,
            role: morpheus_core::MemberRole::Member,
            power_level: 0,
            is_ignored: false,
        }
    }

    fn message(id: &str, echo: bool, kind: MessageKind) -> Message {
        Message {
            unique_id: id.into(),
            id: if echo {
                MessageId::Transaction(id.into())
            } else {
                MessageId::Event(OwnedEventId::try_from(format!("${id}")).expect("event id"))
            },
            sender: OwnedUserId::try_from("@carol:localhost").expect("user id"),
            sender_name: "carol".into(),
            sender_avatar: None,
            timestamp_ms: 0,
            is_own: true,
            is_local_echo: echo,
            is_edited: false,
            can_edit: true,
            reactions: Vec::new(),
            in_reply_to: None,
            thread_root: None,
            thread: None,
            kind,
            shield: None,
        }
    }

    fn text(body: &str) -> MessageKind {
        MessageKind::Text {
            body: body.into(),
            html: None,
        }
    }

    fn people() -> Vec<Member> {
        vec![
            member("@alice:localhost", "Alice"),
            member("@bob:localhost", "Bob Builder"),
            member("@carol:localhost", "carol"),
        ]
    }

    #[test]
    fn the_last_word_is_what_completes() {
        assert_eq!(last_word("hot :fir"), ":fir");
        assert_eq!(last_word("hot "), "");
        assert_eq!(last_word("two\nlines @b"), "@b");
        assert!(wants_members("hey @b"));
        assert!(!wants_members("hey @b "));
    }

    #[test]
    fn emoji_need_two_letters_and_match_codes() {
        let none = HashMap::new();
        assert!(suggest(":f", None, &none).is_empty(), "one letter is too few");
        assert!(suggest(":", None, &none).is_empty());
        assert!(suggest("a:fi", None, &none).is_empty(), "a colon inside a word");
        let rows = suggest("hot :fir", None, &none);
        assert_eq!(
            rows.first(),
            Some(&Suggestion::Emoji {
                emoji: "🔥",
                code: "fire"
            })
        );
        assert!(rows.len() <= 8);
        assert_eq!(accept("hot :fir", &rows[0]), "hot 🔥 ");
    }

    #[test]
    fn members_match_by_name_id_or_nickname() {
        let people = people();
        let none = HashMap::new();
        let names = |draft: &str, nicks: &HashMap<String, String>| -> Vec<String> {
            suggest(draft, Some(&people), nicks)
                .into_iter()
                .map(|s| match s {
                    Suggestion::Member { name, .. } => name,
                    other => panic!("{other:?}"),
                })
                .collect()
        };
        assert_eq!(names("hey @bo", &none), ["Bob Builder"]);
        assert_eq!(names("@AL", &none), ["Alice"]);
        assert_eq!(names("@", &none).len(), 3);
        assert!(names("@zed", &none).is_empty());
        // Not until the members are known.
        assert!(suggest("@bo", None, &none).is_empty());
        let nick = HashMap::from([("@alice:localhost".to_owned(), "boss".to_owned())]);
        assert_eq!(names("@bo", &nick), ["Alice", "Bob Builder"]);
        let rows = suggest("hey @bo", Some(&people), &none);
        assert_eq!(accept("hey @bo", &rows[0]), "hey @Bob Builder ");
    }

    #[test]
    fn the_emoji_panel_and_mention_append_with_spacing() {
        assert_eq!(with_emoji("", "🚀"), "🚀 ");
        assert_eq!(with_emoji("go", "🚀"), "go 🚀 ");
        assert_eq!(with_emoji("go ", "🚀"), "go 🚀 ");
        assert_eq!(with_mention("hi", "bob"), "hi @bob ");
        assert_eq!(with_mention("", "bob"), "@bob ");
    }

    #[test]
    fn a_reply_needs_a_sent_message() {
        let mut state = ComposerState::default();
        assert!(!state.start_reply(&message("a", true, text("x"))));
        assert!(state.mode.is_none());
        let mut echoed = message("b", false, text("x"));
        echoed.is_local_echo = true;
        assert!(!state.start_reply(&echoed));
        assert!(state.start_reply(&message("c", false, text("hello there"))));
        match &state.mode {
            Some(ComposerMode::Reply {
                sender_name,
                summary,
                ..
            }) => assert_eq!((sender_name.as_str(), summary.as_str()), ("carol", "hello there")),
            other => panic!("{other:?}"),
        }
        assert!(state.cancel());
        assert!(!state.cancel());
    }

    #[test]
    fn sending_follows_the_mode() {
        let mut state = ComposerState::default();
        assert_eq!(state.take_send("   "), None);
        let plan = state.take_send("  hello \n").expect("plan");
        assert_eq!(plan.text, Some((SendKind::Text, "hello".into())));

        let target = message("t", false, text("original"));
        state.start_reply(&target);
        let plan = state.take_send("answer").expect("plan");
        let MessageId::Event(id) = &target.id else {
            panic!()
        };
        assert_eq!(plan.text, Some((SendKind::Reply(id.clone()), "answer".into())));
        assert!(state.mode.is_none(), "sending leaves the mode");

        assert_eq!(state.start_edit(&target), "original");
        assert!(state.editing());
        let plan = state.take_send("changed").expect("plan");
        assert_eq!(
            plan.text,
            Some((SendKind::Edit(target.id.clone()), "changed".into()))
        );
    }

    #[test]
    fn files_go_with_the_message_but_not_with_an_edit() {
        let mut state = ComposerState::default();
        let first = state.attach("a.png".into(), "image/png".into(), vec![1]);
        let second = state.attach("b.txt".into(), "text/plain".into(), vec![2]);
        assert_ne!(first, second);
        state.remove(first);
        assert_eq!(state.attachments.len(), 1);
        assert!(state.can_send(""), "files alone can be sent");
        let plan = state.take_send("").expect("plan");
        assert_eq!(plan.text, None);
        assert_eq!(plan.files.len(), 1);
        assert_eq!(plan.files[0].name, "b.txt");
        assert!(state.attachments.is_empty());

        state.attach("c.txt".into(), "text/plain".into(), vec![3]);
        state.start_edit(&message("t", false, text("old")));
        assert!(!state.can_send(""), "an edit needs text");
        let plan = state.take_send("new").expect("plan");
        assert!(plan.files.is_empty(), "files wait while editing");
        assert_eq!(state.attachments.len(), 1);
    }

    #[test]
    fn mime_types_come_from_the_extension() {
        assert_eq!(mime_of("Photo.JPG"), "image/jpeg");
        assert_eq!(mime_of("notes.txt"), "text/plain");
        assert_eq!(mime_of("noext"), "application/octet-stream");
    }
}
