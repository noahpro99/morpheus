use super::toast::report;
use dioxus::prelude::*;
use morpheus_core::{Member, MessageId, OwnedEventId, OwnedRoomId, Session, TimelineHandle};

use super::emoji;
use super::hotkeys::{FileDrops, Hotkey, HotkeyRequest};
use super::shell::{Mention, MentionRequest};
use super::timeline::LastOwnMessage;
use crate::attach::pick_attachment;
use crate::icons::{Icon, PLUS, SEND, SMILE};

/// One row of the composer's autocomplete.
#[derive(Debug, Clone, PartialEq)]
enum Suggestion {
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
    fn insert(&self) -> String {
        match self {
            Self::Emoji { emoji, .. } => format!("{emoji} "),
            Self::Member { name, .. } => format!("@{name} "),
        }
    }
}

/// The word the caret sits in, taken as the draft's last word.
fn last_word(draft: &str) -> &str {
    draft.rsplit(char::is_whitespace).next().unwrap_or("")
}

/// Suggestions for the word being typed: `:code` from the emoji table,
/// `@name` from the room's members, found by display name, ID, or your
/// nickname for them.
fn suggest(
    draft: &str,
    members: Option<&[Member]>,
    nicknames: &super::nicknames::NickMap,
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
            .map(|m| (m, nicknames.0.get(m.user_id.as_str())))
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

/// Shared between the timeline (which owns the handle) and the composer.
pub type ActiveTimeline = Signal<Option<TimelineHandle>>;

/// What the next Enter in the composer does besides sending plain text.
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

/// Set by message hover actions; cleared by Esc, cancel, or send.
pub type Mode = Signal<Option<ComposerMode>>;
/// The draft text, exposed so "edit" can pre-fill it.
pub type Draft = Signal<String>;

#[component]
pub fn Composer(
    room_name: String,
    #[props(default)] placeholder: Option<String>,
    /// Only the room's main composer sends dropped and pasted files; a
    /// thread's composer leaves them alone so nothing is sent twice.
    #[props(default)]
    accepts_drops: bool,
    /// The room whose members `@` completes; `None` disables mentions.
    #[props(default)]
    room_id: Option<OwnedRoomId>,
) -> Element {
    let timeline = use_context::<ActiveTimeline>();
    let mut mode = use_context::<Mode>();
    let mut draft = use_context::<Draft>();
    let session = use_context::<Session>();
    // Autocomplete: the rows on offer and which one is highlighted. Members
    // are fetched the first time an `@` is typed.
    let mut suggestions = use_signal(Vec::<Suggestion>::new);
    let mut active = use_signal(|| 0usize);
    let mut members = use_signal(|| None::<Vec<Member>>);
    let mut emoji_open = use_signal(|| false);
    // Your nicknames for people: `@` finds them by those too.
    let nicknames = move || {
        try_consume_context::<super::nicknames::Nicknames>()
            .map(|map| map.peek().clone())
            .unwrap_or_default()
    };
    let mut recompute = {
        let room_id = room_id.clone();
        let session = session.clone();
        move || {
            let text = draft.peek().clone();
            if last_word(&text).starts_with('@')
                && members.peek().is_none()
                && let Some(room_id) = room_id.clone()
            {
                let session = session.clone();
                spawn(async move {
                    match session.members(room_id).await {
                        Ok(list) => {
                            members.set(Some(list));
                            let text = draft.peek().clone();
                            suggestions.set(suggest(
                                &text,
                                members.peek().as_deref(),
                                &nicknames(),
                            ));
                            active.set(0);
                        }
                        Err(err) => report("members for mention autocomplete", &err),
                    }
                });
            }
            suggestions.set(suggest(&text, members.peek().as_deref(), &nicknames()));
            active.set(0);
        }
    };
    // Replace the word being typed with the chosen row.
    let mut accept = move |index: usize| {
        let Some(choice) = suggestions.peek().get(index).cloned() else {
            return;
        };
        let mut text = draft.peek().clone();
        let keep = text.len() - last_word(&text).len();
        text.truncate(keep);
        text.push_str(&choice.insert());
        draft.set(text);
        suggestions.set(Vec::new());
    };
    // Dropped and pasted files go out through this room's timeline.
    let mut drops = use_context::<FileDrops>();
    use_effect(move || {
        if !accepts_drops || drops().0.is_empty() {
            return;
        }
        let Some(handle) = timeline.peek().clone() else {
            return;
        };
        let files = std::mem::take(&mut drops.write().0);
        spawn(async move {
            for file in files {
                if let Err(err) = handle
                    .send_attachment_bytes(file.name, file.mime, file.bytes)
                    .await
                {
                    report("Upload", &err);
                }
            }
        });
    });
    // Focus the box as soon as the room is ready to send, so navigating to a
    // room (or opening a thread) lets you type straight away.
    let mut input_el = use_signal(|| None::<std::rc::Rc<MountedData>>);
    // Starting a reply or an edit puts the caret in the box.
    use_effect(move || {
        if mode().is_some()
            && let Some(el) = input_el.peek().clone()
        {
            spawn(async move {
                let _ = el.set_focus(true).await;
            });
        }
    });
    // "Mention" from a member's menu drops their name into the box.
    let mut mention = use_context::<MentionRequest>();
    use_effect(move || {
        if let Some(name) = mention().0 {
            mention.set(Mention::default());
            let mut text = draft.peek().clone();
            if !text.is_empty() && !text.ends_with(' ') {
                text.push(' ');
            }
            text.push_str(&format!("@{name} "));
            draft.set(text);
            if let Some(el) = input_el.peek().clone() {
                spawn(async move {
                    let _ = el.set_focus(true).await;
                });
            }
        }
    });
    // Not on a phone: focusing raises the keyboard over the room just opened.
    let phone = try_use_context::<super::shell::NarrowWindow>();
    use_effect(move || {
        if timeline().is_some()
            && !phone.is_some_and(|narrow| narrow.peek().0)
            && let Some(el) = input_el()
        {
            spawn(async move {
                let _ = el.set_focus(true).await;
            });
        }
    });

    let mut send = move || {
        let body = draft().trim().to_owned();
        let Some(handle) = timeline() else { return };
        if body.is_empty() {
            return;
        }
        draft.set(String::new());
        let action = mode.take();
        // Sending first: the message shows (greyed) the moment it is queued.
        // Waiting on the typing notice before it held it back a round trip.
        let sending = match action {
            None => handle.send_text(body),
            Some(ComposerMode::Reply { event_id, .. }) => handle.send_reply(body, event_id),
            Some(ComposerMode::Edit { id, .. }) => handle.edit(id, body),
        };
        let typing = handle.stop_typing();
        spawn(async move {
            let _ = typing.await;
            if let Err(err) = sending.await {
                report("Sending", &err);
            }
        });
    };

    let on_input = move |e: FormEvent| {
        draft.set(e.value());
        recompute();
        if let Some(handle) = timeline()
            && !e.value().is_empty()
        {
            spawn(async move {
                if let Err(err) = handle.renew_typing().await {
                    report("typing notice", &err);
                }
            });
        }
    };

    let mut cancel = move || {
        if mode.take().is_some() {
            draft.set(String::new());
        }
    };

    let attach = move |_: ()| {
        let Some(handle) = timeline() else { return };
        spawn(async move {
            let Some(path) = pick_attachment().await else {
                return;
            };
            if let Err(err) = handle.send_attachment(path).await {
                report("Attaching", &err);
            }
        });
    };

    // Shortcuts the shell hands over: attach a file, edit the last message.
    let mut hotkey = use_context::<HotkeyRequest>();
    let last_own = use_hook(try_consume_context::<LastOwnMessage>);
    use_effect(move || match hotkey() {
        Some(Hotkey::Attach) => {
            hotkey.set(None);
            attach(());
        }
        Some(Hotkey::EditLast) => {
            hotkey.set(None);
            if let Some(message) = last_own.and_then(|m| m.peek().0.clone()) {
                let original = message.kind.plain_body().unwrap_or_default().to_owned();
                draft.set(original.clone());
                mode.set(Some(ComposerMode::Edit {
                    id: message.id.clone(),
                    original,
                }));
            }
        }
        _ => {}
    });

    // A phone (not a narrow desktop window): its keyboard's Enter is a new line.
    let on_phone = cfg!(any(target_os = "android", target_os = "ios"));
    let placeholder_text = placeholder.unwrap_or_else(|| format!("Message #{room_name}"));

    rsx! {
        footer { class: "composer",
            if let Some(active) = mode() {
                div { class: "composer-banner",
                    match &active {
                        ComposerMode::Reply { sender_name, summary, .. } => rsx! {
                            span { class: "banner-label", "Replying to " b { "{sender_name}" } }
                            span { class: "banner-preview", "{summary}" }
                        },
                        ComposerMode::Edit { .. } => rsx! {
                            span { class: "banner-label", "Editing message" }
                        },
                    }
                    button { class: "ghost", title: "Cancel (Esc)", onclick: move |_| cancel(), "✕" }
                }
            }
            div { class: "composer-row",
                button {
                    class: "attach-button",
                    title: "Attach a file (Ctrl+Shift+U)",
                    r#type: "button",
                    disabled: timeline().is_none(),
                    onclick: move |_| attach(()),
                    Icon { glyph: PLUS, size: 18 }
                }
                // Grows with its lines (a few, then it scrolls).
                textarea {
                    class: "composer-input",
                    rows: "{draft().lines().count().clamp(1, 8)}",
                    onmounted: move |e| input_el.set(Some(e.data())),
                    placeholder: "{placeholder_text}",
                    value: "{draft}",
                    disabled: timeline().is_none(),
                    oninput: on_input,
                    onkeydown: move |e| {
                        let open = !suggestions.peek().is_empty();
                        let count = suggestions.peek().len();
                        match e.key() {
                            Key::ArrowDown if open => {
                                e.prevent_default();
                                active.set((active() + 1) % count);
                            }
                            Key::ArrowUp if open => {
                                e.prevent_default();
                                active.set((active() + count - 1) % count);
                            }
                            Key::Enter | Key::Tab if open => {
                                e.prevent_default();
                                accept(active());
                            }
                            Key::Escape if open => {
                                e.prevent_default();
                                suggestions.set(Vec::new());
                            }
                            Key::Escape if emoji_open() => {
                                e.prevent_default();
                                emoji_open.set(false);
                            }
                            // On a phone Enter is a new line (the send button
                            // sends), as in Discord's app; elsewhere it sends,
                            // and Shift+Enter is the new line. A keyboard
                            // plugged into a phone sends with Ctrl+Enter.
                            Key::Enter if !e.modifiers().shift()
                                && (!on_phone || e.modifiers().ctrl()) =>
                            {
                                e.prevent_default();
                                send();
                            }
                            Key::Escape => cancel(),
                            _ => {}
                        }
                    },
                }
                button {
                    class: "emoji-button",
                    class: if emoji_open() { "active" },
                    title: "Emoji",
                    r#type: "button",
                    disabled: timeline().is_none(),
                    onclick: move |_| emoji_open.toggle(),
                    Icon { glyph: SMILE, size: 20 }
                }
                if on_phone {
                    button {
                        class: "send-button",
                        class: if !draft().trim().is_empty() { "ready" },
                        title: "Send",
                        r#type: "button",
                        disabled: timeline().is_none() || draft().trim().is_empty(),
                        // The keyboard stays up: tapping does not take focus.
                        onmousedown: move |e| e.prevent_default(),
                        onclick: move |_| send(),
                        Icon { glyph: SEND, size: 18 }
                    }
                }
            }
            if !suggestions().is_empty() {
                div { class: "autocomplete", role: "listbox",
                    for (index, row) in suggestions().iter().enumerate() {
                        button {
                            key: "{index}",
                            class: "autocomplete-item",
                            class: if index == active() { "active" },
                            r#type: "button",
                            // Keep the input focused while picking with the mouse.
                            onmousedown: move |e| e.prevent_default(),
                            onclick: move |_| accept(index),
                            match row {
                                Suggestion::Emoji { emoji, code } => rsx! {
                                    span { class: "emoji", "{emoji}" }
                                    span { ":{code}:" }
                                },
                                Suggestion::Member { name, nickname: Some(nickname), user_id } => rsx! {
                                    span { class: "emoji", "@" }
                                    span { "{nickname}" }
                                    span { class: "muted", "{name} · {user_id}" }
                                },
                                Suggestion::Member { name, nickname: None, user_id } => rsx! {
                                    span { class: "emoji", "@" }
                                    span { "{name}" }
                                    span { class: "muted", "{user_id}" }
                                },
                            }
                        }
                    }
                }
            }
            if emoji_open() {
                div { class: "emoji-panel", role: "dialog",
                    for (glyph, code) in emoji::EMOJI.iter().copied() {
                        button {
                            key: "{code}",
                            title: "{code}",
                            r#type: "button",
                            onmousedown: move |e| e.prevent_default(),
                            onclick: move |_| {
                                let mut text = draft.peek().clone();
                                if !text.is_empty() && !text.ends_with(' ') {
                                    text.push(' ');
                                }
                                text.push_str(glyph);
                                text.push(' ');
                                draft.set(text);
                                emoji_open.set(false);
                            },
                            "{glyph}"
                        }
                    }
                }
            }
        }
    }
}
