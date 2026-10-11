use dioxus::prelude::*;
use morpheus_core::{
    MediaRef, Message, MessageId, MessageKind, OpenTimeline, OwnedRoomId, Reaction, ReplyPreview,
    Session, TimelineEntry, Vector,
};
use tracing::{error, warn};

use super::{
    composer::{ActiveTimeline, ComposerMode, Draft, Mode},
    lightbox::{LightboxImage, LightboxImages, LightboxIndex},
};
use crate::{
    format::{calendar_date, clock_time},
    media::{Avatar, use_thumbnail},
};

/// Messages from the same sender within this window collapse under one header.
const GROUP_WINDOW_MS: u64 = 5 * 60 * 1000;
/// Start loading older history when the view is this close to the top.
const PAGINATE_THRESHOLD_PX: f64 = 400.0;
const PAGE_SIZE: u16 = 40;
/// One-click reactions offered on hover.
const QUICK_REACTIONS: [&str; 3] = ["👍", "❤️", "😂"];

/// One room's messages. The parent sets `key` to the room id, so switching
/// rooms remounts this component and drops the previous subscription.
#[component]
pub fn TimelineView(room_id: OwnedRoomId) -> Element {
    let session = use_context::<Session>();
    let mut entries = use_signal(Vector::<TimelineEntry>::new);
    let mut handle = use_context::<ActiveTimeline>();
    let mut load_error = use_signal(|| None::<String>);
    let mut reached_start = use_signal(|| false);
    let mut paginating = use_signal(|| false);
    let mut gallery = use_context::<LightboxImages>();

    // Keep the lightbox's gallery in step with the images currently loaded
    // in this room's timeline, so the arrow keys can walk between them.
    let gallery_images = use_memo(move || {
        entries
            .read()
            .iter()
            .filter_map(|entry| match entry {
                TimelineEntry::Message(message) => match &message.kind {
                    MessageKind::Image {
                        body,
                        media,
                        width,
                        height,
                    } => Some(LightboxImage {
                        media: media.clone(),
                        body: body.clone(),
                        width: *width,
                        height: *height,
                    }),
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>()
    });
    use_effect(move || gallery.set(gallery_images()));

    use_future(move || {
        let session = session.clone();
        let room_id = room_id.clone();
        async move {
            match session.timeline(room_id).await {
                Ok(OpenTimeline {
                    handle: opened,
                    mut updates,
                }) => {
                    handle.set(Some(opened.clone()));
                    while let Some(diffs) = updates.next().await {
                        entries
                            .with_mut(|list| diffs.into_iter().for_each(|diff| diff.apply(list)));
                        // The SDK only sends a receipt when the latest event moved forward.
                        let receipt = opened.clone();
                        spawn(async move {
                            if let Err(err) = receipt.mark_as_read().await {
                                warn!(%err, "read receipt failed");
                            }
                        });
                    }
                }
                Err(err) => {
                    error!(%err, "opening timeline failed");
                    load_error.set(Some(err.to_string()));
                }
            }
        }
    });

    let mut load_older = move || {
        let Some(active) = handle() else { return };
        if paginating() || reached_start() {
            return;
        }
        paginating.set(true);
        spawn(async move {
            match active.paginate_backwards(PAGE_SIZE).await {
                Ok(hit_start) => reached_start.set(hit_start),
                Err(err) => load_error.set(Some(err.to_string())),
            }
            paginating.set(false);
        });
    };

    // `column-reverse` makes scrollTop run from 0 (bottom) to negative (top).
    let on_scroll = move |evt: Event<ScrollData>| {
        let distance_to_top =
            f64::from(evt.scroll_height()) - f64::from(evt.client_height()) + evt.scroll_top();
        if distance_to_top < PAGINATE_THRESHOLD_PX {
            load_older();
        }
    };

    let at_start = reached_start()
        || entries
            .read()
            .iter()
            .any(|entry| matches!(entry, TimelineEntry::TimelineStart));
    let entries_now = entries.read();
    let rows = group_rows(&entries_now);

    rsx! {
        // Rendered newest-first; CSS flips it and keeps the view pinned to the bottom.
        section { class: "timeline", onscroll: on_scroll,
            for row in rows.into_iter().rev() {
                match row {
                    Row::Message { message, show_header } => rsx! {
                        MessageRow { key: "{message.unique_id}", message: message.clone(), show_header }
                    },
                    Row::Date(label) => rsx! {
                        div { key: "date-{label}", class: "date-divider", span { "{label}" } }
                    },
                    Row::Start => rsx! {
                        div { class: "timeline-start", "This is the beginning of the room." }
                    },
                }
            }

            if let Some(message) = load_error() {
                p { class: "error", "{message}" }
            }
            if paginating() {
                p { class: "muted loading", "Loading older messages…" }
            } else if !at_start && handle().is_some() {
                button { class: "ghost load-older", onclick: move |_| load_older(), "Load older messages" }
            }
        }
    }
}

enum Row<'a> {
    Message {
        message: &'a Message,
        show_header: bool,
    },
    Date(String),
    Start,
}

/// Decide which messages get a sender header and turn dividers into labels.
fn group_rows(entries: &Vector<TimelineEntry>) -> Vec<Row<'_>> {
    let mut rows = Vec::with_capacity(entries.len());
    let mut previous: Option<&Message> = None;

    for entry in entries {
        match entry {
            TimelineEntry::Message(message) => {
                let continues = previous.is_some_and(|prev| {
                    prev.sender == message.sender
                        && message.timestamp_ms.saturating_sub(prev.timestamp_ms) < GROUP_WINDOW_MS
                        && !matches!(prev.kind, MessageKind::State(_))
                        && !matches!(message.kind, MessageKind::State(_))
                });
                rows.push(Row::Message {
                    message,
                    show_header: !continues,
                });
                previous = Some(message);
            }
            TimelineEntry::DateDivider { timestamp_ms } => {
                rows.push(Row::Date(calendar_date(*timestamp_ms)));
                previous = None;
            }
            TimelineEntry::TimelineStart => {
                rows.push(Row::Start);
                previous = None;
            }
            TimelineEntry::ReadMarker => {}
        }
    }
    rows
}

#[component]
fn MessageRow(message: Message, show_header: bool) -> Element {
    let timeline = use_context::<ActiveTimeline>();
    let mut mode = use_context::<Mode>();
    let mut draft = use_context::<Draft>();
    let time = clock_time(message.timestamp_ms);

    let start_reply = {
        let message = message.clone();
        move |_| {
            if let MessageId::Event(event_id) = &message.id {
                mode.set(Some(ComposerMode::Reply {
                    event_id: event_id.clone(),
                    sender_name: message.sender_name.clone(),
                    summary: message.kind.summary(),
                }));
            }
        }
    };
    let start_edit = {
        let message = message.clone();
        move |_| {
            let original = message.kind.plain_body().unwrap_or_default().to_owned();
            draft.set(original.clone());
            mode.set(Some(ComposerMode::Edit {
                id: message.id.clone(),
                original,
            }));
        }
    };
    let delete = {
        let id = message.id.clone();
        move |_| {
            let Some(active) = timeline() else { return };
            let id = id.clone();
            spawn(async move {
                if let Err(err) = active.redact(id).await {
                    warn!(%err, "delete failed");
                }
            });
        }
    };
    let can_reply = matches!(message.id, MessageId::Event(_)) && !message.is_local_echo;

    if let MessageKind::State(text) = &message.kind {
        return rsx! {
            div { class: "message system",
                span { class: "time", "{time}" }
                span { class: "system-text", "{text}" }
            }
        };
    }

    let react = {
        let id = message.id.clone();
        move |key: &str| {
            let Some(active) = timeline() else { return };
            let (id, key) = (id.clone(), key.to_owned());
            spawn(async move {
                if let Err(err) = active.toggle_reaction(id, key).await {
                    warn!(%err, "reaction failed");
                }
            });
        }
    };

    rsx! {
        div {
            class: "message",
            class: if show_header { "with-header" },
            class: if message.is_local_echo { "pending" },
            class: if message.is_own { "own" },
            if show_header {
                Avatar { media: message.sender_avatar.clone(), fallback: message.sender_name.clone(), size: 40 }
                div { class: "message-main",
                    div { class: "message-meta",
                        span { class: "sender", "{message.sender_name}" }
                        span { class: "time", "{time}" }
                    }
                    ReplyQuote { reply: message.in_reply_to.clone() }
                    MessageBody { kind: message.kind.clone(), edited: message.is_edited }
                    Reactions { reactions: message.reactions.clone(), on_toggle: react.clone() }
                }
            } else {
                span { class: "time gutter", "{time}" }
                div { class: "message-main",
                    ReplyQuote { reply: message.in_reply_to.clone() }
                    MessageBody { kind: message.kind.clone(), edited: message.is_edited }
                    Reactions { reactions: message.reactions.clone(), on_toggle: react.clone() }
                }
            }
            if !message.is_local_echo {
                div { class: "actions",
                    for key in QUICK_REACTIONS {
                        button {
                            class: "action",
                            title: "React with {key}",
                            onclick: {
                                let react = react.clone();
                                move |_| react(key)
                            },
                            "{key}"
                        }
                    }
                    if can_reply {
                        button { class: "action reply", title: "Reply", onclick: start_reply, "↩" }
                    }
                    if message.can_edit {
                        button { class: "action edit", title: "Edit", onclick: start_edit, "✏️" }
                    }
                    if message.is_own && can_reply {
                        button { class: "action delete", title: "Delete", onclick: delete, "🗑" }
                    }
                }
            }
        }
    }
}

#[component]
fn Reactions(reactions: Vec<Reaction>, on_toggle: Callback<&'static str>) -> Element {
    if reactions.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { class: "reactions",
            for reaction in reactions {
                button {
                    key: "{reaction.key}",
                    class: "reaction",
                    class: if reaction.mine { "mine" },
                    onclick: {
                        // Keys outlive the click handler; leaking the few distinct emoji is fine.
                        let key: &'static str = Box::leak(reaction.key.clone().into_boxed_str());
                        move |_| on_toggle(key)
                    },
                    span { class: "reaction-key", "{reaction.key}" }
                    span { class: "reaction-count", "{reaction.count}" }
                }
            }
        }
    }
}

#[component]
fn MessageBody(kind: MessageKind, edited: bool) -> Element {
    let edited_tag = if edited { " (edited)" } else { "" };
    match kind {
        MessageKind::Text { body, html } => {
            rsx! { RichText { class: "body", body, html, edited_tag } }
        }
        MessageKind::Notice { body, html } => {
            rsx! { RichText { class: "body notice", body, html, edited_tag } }
        }
        MessageKind::Emote { body, html } => {
            rsx! { RichText { class: "body emote", body, html, edited_tag } }
        }
        MessageKind::Image {
            body,
            media,
            width,
            height,
        } => rsx! { ImageBody { body, media, width, height } },
        MessageKind::File { body } => rsx! { p { class: "body attachment", "📎 {body}" } },
        MessageKind::Redacted => rsx! { p { class: "body muted", "Message deleted" } },
        MessageKind::UnableToDecrypt => rsx! { p { class: "body muted", "Unable to decrypt" } },
        MessageKind::State(text) => rsx! { p { class: "body muted", "{text}" } },
        MessageKind::Unsupported(what) => rsx! { p { class: "body muted", "Unsupported: {what}" } },
    }
}

/// Sanitised HTML when the sender provided it, plain text otherwise.
#[component]
fn RichText(
    class: &'static str,
    body: String,
    html: Option<String>,
    edited_tag: &'static str,
) -> Element {
    match html {
        Some(html) => rsx! {
            div { class: "{class} rich",
                div { dangerous_inner_html: "{html}" }
                span { class: "edited", "{edited_tag}" }
            }
        },
        None => rsx! { p { class: "{class}", "{body}" span { class: "edited", "{edited_tag}" } } },
    }
}

#[component]
fn ImageBody(body: String, media: MediaRef, width: Option<u32>, height: Option<u32>) -> Element {
    let mut lightbox_index = use_context::<LightboxIndex>();
    let gallery = use_context::<LightboxImages>();
    let url = use_thumbnail(Some(media.clone()), 800, 600);
    let aspect = match (width, height) {
        (Some(w), Some(h)) if h > 0 => format!("aspect-ratio: {w} / {h};"),
        _ => String::new(),
    };

    let open = move |_| {
        let key = media.key().to_owned();
        let index = gallery.read().iter().position(|img| img.media.key() == key);
        lightbox_index.set(index);
    };

    rsx! {
        match url() {
            Some(src) => rsx! {
                img {
                    class: "image",
                    style: "{aspect}",
                    src: "{src}",
                    alt: "{body}",
                    title: "{body}",
                    onclick: open,
                }
            },
            None => rsx! { div { class: "image placeholder", style: "{aspect}", "{body}" } },
        }
    }
}

#[component]
fn ReplyQuote(reply: Option<ReplyPreview>) -> Element {
    let Some(reply) = reply else { return rsx! {} };
    rsx! {
        div { class: "reply-quote", title: "{reply.event_id}",
            span { "↩" }
            b { "{reply.sender_name.as_deref().unwrap_or(\"…\")}" }
            span { "{reply.summary.as_deref().unwrap_or(\"Loading…\")}" }
        }
    }
}
