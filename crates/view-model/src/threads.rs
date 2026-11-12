//! Threads without a toolkit: the labels on a thread's chip and the panel's
//! divider, and which messages can open a thread.

use morpheus_core::{Message, MessageId, ThreadSummary};

/// "1 reply" or "N replies": the chip's count.
pub fn chip_count(thread: &ThreadSummary) -> String {
    plural_replies(thread.reply_count)
}

/// The chip's "last from" note, when the latest sender is known.
pub fn chip_latest(thread: &ThreadSummary) -> Option<String> {
    thread
        .latest_sender_name
        .as_ref()
        .map(|name| format!("· last from {name}"))
}

fn plural_replies(count: u32) -> String {
    if count == 1 {
        "1 reply".to_owned()
    } else {
        format!("{count} replies")
    }
}

/// The line under a thread's root in its panel.
pub fn reply_count_label(root: &Message) -> String {
    match root.thread.as_ref().map(|t| t.reply_count) {
        None | Some(0) => "No replies yet".to_owned(),
        Some(n) => plural_replies(n),
    }
}

/// Whether a thread can be opened on this message (local echoes cannot be
/// roots: they have no event ID yet).
pub fn can_open(message: &Message) -> bool {
    matches!(message.id, MessageId::Event(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use morpheus_core::{MessageKind, OwnedEventId, OwnedUserId};

    fn message(id: MessageId, thread: Option<u32>) -> Message {
        Message {
            unique_id: "u".into(),
            id,
            sender: OwnedUserId::try_from("@a:x").expect("user"),
            sender_name: "a".into(),
            sender_avatar: None,
            timestamp_ms: 0,
            is_own: false,
            is_local_echo: false,
            is_edited: false,
            can_edit: false,
            reactions: Vec::new(),
            in_reply_to: None,
            thread_root: None,
            thread: thread.map(|n| summary(n, Some("bob"))),
            kind: MessageKind::Text {
                body: "x".into(),
                html: None,
            },
            shield: None,
            state_kind: None,
        }
    }

    fn summary(count: u32, name: Option<&str>) -> ThreadSummary {
        ThreadSummary {
            root_event_id: OwnedEventId::try_from("$r").expect("event"),
            reply_count: count,
            latest_sender_name: name.map(Into::into),
            latest_summary: None,
        }
    }

    fn event() -> MessageId {
        MessageId::Event(OwnedEventId::try_from("$e").expect("event"))
    }

    #[test]
    fn counts_are_singular_and_plural() {
        assert_eq!(chip_count(&summary(1, None)), "1 reply");
        assert_eq!(chip_count(&summary(2, None)), "2 replies");
        assert_eq!(chip_count(&summary(0, None)), "0 replies");
    }

    #[test]
    fn the_latest_sender_is_noted_when_known() {
        assert_eq!(
            chip_latest(&summary(2, Some("bob"))).as_deref(),
            Some("· last from bob")
        );
        assert_eq!(chip_latest(&summary(2, None)), None);
    }

    #[test]
    fn the_divider_says_when_there_are_no_replies() {
        assert_eq!(reply_count_label(&message(event(), None)), "No replies yet");
        assert_eq!(reply_count_label(&message(event(), Some(0))), "No replies yet");
        assert_eq!(reply_count_label(&message(event(), Some(1))), "1 reply");
        assert_eq!(reply_count_label(&message(event(), Some(7))), "7 replies");
    }

    #[test]
    fn only_sent_messages_open_threads() {
        assert!(can_open(&message(event(), None)));
        let echo = MessageId::Transaction("t".try_into().expect("txn"));
        assert!(!can_open(&message(echo, None)));
    }
}
