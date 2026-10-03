use dioxus::prelude::*;
use morpheus_core::{
    Message, MessageKind, OpenTimeline, OwnedRoomId, Session, TimelineEntry, Vector,
};
use tracing::error;

use super::composer::ActiveTimeline;
use crate::format::{calendar_date, clock_time};

/// Messages from the same sender within this window collapse under one header.
const GROUP_WINDOW_MS: u64 = 5 * 60 * 1000;

/// One room's messages. The parent sets `key` to the room id, so switching
/// rooms remounts this component and drops the previous subscription.
#[component]
pub fn TimelineView(room_id: OwnedRoomId) -> Element {
    let session = use_context::<Session>();
    let mut entries = use_signal(Vector::<TimelineEntry>::new);
    let mut handle = use_context::<ActiveTimeline>();
    let mut load_error = use_signal(|| None::<String>);
    let mut reached_start = use_signal(|| false);

    use_future(move || {
        let session = session.clone();
        let room_id = room_id.clone();
        async move {
            match session.timeline(room_id).await {
                Ok(OpenTimeline {
                    handle: opened,
                    mut updates,
                }) => {
                    handle.set(Some(opened));
                    while let Some(diffs) = updates.next().await {
                        entries
                            .with_mut(|list| diffs.into_iter().for_each(|diff| diff.apply(list)));
                    }
                }
                Err(err) => {
                    error!(%err, "opening timeline failed");
                    load_error.set(Some(err.to_string()));
                }
            }
        }
    });

    let load_older = move |_| {
        let Some(active) = handle() else { return };
        spawn(async move {
            match active.paginate_backwards(40).await {
                Ok(hit_start) => reached_start.set(hit_start),
                Err(err) => load_error.set(Some(err.to_string())),
            }
        });
    };

    let rows = group_rows(&entries.read());
    let at_start = reached_start()
        || entries
            .read()
            .iter()
            .any(|entry| matches!(entry, TimelineEntry::TimelineStart));

    rsx! {
        // `column-reverse` keeps the view pinned to the newest message, so we
        // render newest-first and let CSS flip it.
        section { class: "timeline",
            for row in rows.into_iter().rev() {
                match row {
                    Row::Message { message, show_header } => rsx! {
                        MessageRow { key: "{message.id}", message, show_header }
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
            if !at_start && handle().is_some() {
                button { class: "ghost load-older", onclick: load_older, "Load older messages" }
            }
        }
    }
}

enum Row {
    Message { message: Message, show_header: bool },
    Date(String),
    Start,
}

/// Decide which messages get a sender header and turn dividers into labels.
fn group_rows(entries: &Vector<TimelineEntry>) -> Vec<Row> {
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
                    message: message.clone(),
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
    let time = clock_time(message.timestamp_ms);

    if let MessageKind::State(text) = &message.kind {
        return rsx! {
            div { class: "message system",
                span { class: "time", "{time}" }
                span { class: "system-text", "{text}" }
            }
        };
    }

    rsx! {
        div {
            class: "message",
            class: if show_header { "with-header" },
            class: if message.is_local_echo { "pending" },
            class: if message.is_own { "own" },
            if show_header {
                div { class: "avatar", "{message.sender_name.chars().next().unwrap_or('?')}" }
                div { class: "message-main",
                    div { class: "message-meta",
                        span { class: "sender", "{message.sender_name}" }
                        span { class: "time", "{time}" }
                    }
                    MessageBody { kind: message.kind.clone(), edited: message.is_edited }
                }
            } else {
                span { class: "time gutter", "{time}" }
                div { class: "message-main",
                    MessageBody { kind: message.kind.clone(), edited: message.is_edited }
                }
            }
        }
    }
}

#[component]
fn MessageBody(kind: MessageKind, edited: bool) -> Element {
    let edited_tag = if edited { " (edited)" } else { "" };
    match kind {
        MessageKind::Text(body) => {
            rsx! { p { class: "body", "{body}" span { class: "edited", "{edited_tag}" } } }
        }
        MessageKind::Notice(body) => rsx! { p { class: "body notice", "{body}" } },
        MessageKind::Emote(body) => rsx! { p { class: "body emote", "{body}" } },
        MessageKind::Image { body } => rsx! { p { class: "body attachment", "🖼 {body}" } },
        MessageKind::File { body } => rsx! { p { class: "body attachment", "📎 {body}" } },
        MessageKind::Redacted => rsx! { p { class: "body muted", "Message deleted" } },
        MessageKind::UnableToDecrypt => rsx! { p { class: "body muted", "Unable to decrypt" } },
        MessageKind::State(text) => rsx! { p { class: "body muted", "{text}" } },
        MessageKind::Unsupported(what) => rsx! { p { class: "body muted", "Unsupported: {what}" } },
    }
}
