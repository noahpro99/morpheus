//! What a timeline lists, as `crates/ui/src/components/timeline.rs`
//! (`group_rows`) decides it: the core's entries as rows, a sender's
//! consecutive messages grouped under one header, day dividers, the NEW
//! divider and the start of the room. Rows are built once per batch of
//! updates, not per frame; a message that did not change keeps its row (the
//! same `Arc`), so the list can compare old and new rows by pointer and tell
//! the list element only what changed.

use std::{collections::HashMap, sync::Arc};

use morpheus_core::{Message, MessageKind, TimelineEntry, Vector};

use super::{
    format::{calendar_date, clock_time},
    rich::{self, Block, Para, Style},
};

/// Messages from the same sender within this window collapse under one header.
pub const GROUP_WINDOW_MS: u64 = 5 * 60 * 1000;

/// A message and what is drawn from it, made once.
#[derive(Debug, PartialEq)]
pub struct MessageRow {
    pub message: Message,
    /// The text of a text, notice or emote, with " (edited)" when it was.
    pub blocks: Vec<Block>,
    /// `HH:MM`.
    pub time: String,
}

impl MessageRow {
    pub fn new(message: Message) -> Self {
        let blocks = match &message.kind {
            MessageKind::Text { body, html }
            | MessageKind::Notice { body, html }
            | MessageKind::Emote { body, html } => {
                let mut blocks = match html {
                    Some(html) => {
                        let blocks = rich::html(html);
                        if blocks.is_empty() {
                            rich::plain(body)
                        } else {
                            blocks
                        }
                    }
                    None => rich::plain(body),
                };
                if message.is_edited {
                    mark_edited(&mut blocks);
                }
                blocks
            }
            _ => Vec::new(),
        };
        let time = clock_time(message.timestamp_ms);
        Self {
            message,
            blocks,
            time,
        }
    }
}

/// " (edited)" after the last paragraph's text (its own line after anything else).
fn mark_edited(blocks: &mut Vec<Block>) {
    let faint = Style {
        faint: true,
        ..Style::default()
    };
    match blocks.last_mut() {
        Some(Block::Para(para)) => para.push(" (edited)", faint),
        _ => {
            let mut para = Para::default();
            para.push("(edited)", faint);
            blocks.push(Block::Para(para));
        }
    }
}

#[derive(Clone, Debug)]
pub enum Row {
    Message {
        row: Arc<MessageRow>,
        show_header: bool,
    },
    /// The label, and the timestamp that keys it (a label can repeat).
    Date(String, u64),
    /// "This is the beginning of the room."
    Start,
    /// The Discord-style "new messages" divider.
    New,
    /// Grey placeholder rows: `older` is the set above the oldest message
    /// (more history to come), else the one while nothing has loaded yet.
    Skeleton { older: bool },
}

impl PartialEq for Row {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Message { row, show_header },
                Self::Message {
                    row: other_row,
                    show_header: other_header,
                },
            ) => Arc::ptr_eq(row, other_row) && show_header == other_header,
            (Self::Date(_, a), Self::Date(_, b)) => a == b,
            (Self::Start, Self::Start) | (Self::New, Self::New) => true,
            (Self::Skeleton { older: a }, Self::Skeleton { older: b }) => a == b,
            _ => false,
        }
    }
}

/// The rows of `entries`, oldest first. `marker` is the `unique_id` of the
/// message the NEW divider belongs above, if it is still showing. `old`
/// holds the rows now listed: an unchanged message keeps its row.
pub fn group_rows(
    entries: &Vector<TimelineEntry>,
    marker: Option<&str>,
    old: &HashMap<String, Arc<MessageRow>>,
) -> Vec<Row> {
    let mut rows = Vec::with_capacity(entries.len() + 2);
    let mut previous: Option<&Message> = None;
    // A divider meant for a hidden row goes above the next visible one.
    let mut marker_pending = false;

    for entry in entries {
        match entry {
            // Name and avatar changes stay out of the chat, as in Discord, and
            // so do calls' joins, leaves and keys (the sidebar shows calls).
            TimelineEntry::Message(message) if message.kind.is_hidden() => {
                marker_pending |= marker == Some(message.unique_id.as_str());
            }
            TimelineEntry::Message(message) => {
                if std::mem::take(&mut marker_pending) || marker == Some(message.unique_id.as_str())
                {
                    rows.push(Row::New);
                    // Always show a header right after the divider, like Discord does.
                    previous = None;
                }
                let continues = previous.is_some_and(|prev| {
                    prev.sender == message.sender
                        && message.timestamp_ms.saturating_sub(prev.timestamp_ms) < GROUP_WINDOW_MS
                        && !matches!(prev.kind, MessageKind::State(_))
                        && !matches!(message.kind, MessageKind::State(_))
                });
                let row = match old.get(&message.unique_id) {
                    Some(row) if row.message == **message => row.clone(),
                    _ => Arc::new(MessageRow::new((**message).clone())),
                };
                rows.push(Row::Message {
                    row,
                    show_header: !continues,
                });
                previous = Some(message);
            }
            TimelineEntry::DateDivider { timestamp_ms } => {
                rows.push(Row::Date(calendar_date(*timestamp_ms), *timestamp_ms));
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

/// The `unique_id` of the first message after the SDK's read-marker virtual
/// item, i.e. the first message that was still unread when the room opened.
pub fn first_unread_after_marker(entries: &Vector<TimelineEntry>) -> Option<String> {
    let marker_idx = entries
        .iter()
        .position(|entry| matches!(entry, TimelineEntry::ReadMarker))?;
    entries
        .iter()
        .skip(marker_idx + 1)
        .find_map(|entry| match entry {
            TimelineEntry::Message(message) => Some(message.unique_id.clone()),
            _ => None,
        })
}

/// The first message from someone else sent after `read_at` (ms): where the
/// divider goes when the read marker sits on an event without a row.
pub fn first_unread_since(entries: &Vector<TimelineEntry>, read_at: u64) -> Option<String> {
    entries.iter().find_map(|entry| match entry {
        TimelineEntry::Message(message) if !message.is_own && message.timestamp_ms > read_at => {
            Some(message.unique_id.clone())
        }
        _ => None,
    })
}

/// What changed from `old` to `new`, as the list element wants it: the rows
/// from `start` (`removed` old ones) were replaced by `added` new ones.
/// Rows equal at both ends are kept.
pub fn changed_range(old: &[Row], new: &[Row]) -> (usize, usize, usize) {
    let shared = old.len().min(new.len());
    let start = (0..shared).find(|&i| old[i] != new[i]).unwrap_or(shared);
    let tail = (0..shared - start)
        .find(|&i| old[old.len() - 1 - i] != new[new.len() - 1 - i])
        .unwrap_or(shared - start);
    (start, old.len() - start - tail, new.len() - start - tail)
}

impl Row {
    /// What identifies this row across rebuilds (`None` for a placeholder).
    fn key(&self) -> Option<String> {
        match self {
            Self::Message { row, .. } => Some(format!("m{}", row.message.unique_id)),
            Self::Date(_, at) => Some(format!("d{at}")),
            Self::Start => Some("s".into()),
            Self::New => Some("n".into()),
            Self::Skeleton { .. } => None,
        }
    }
}

/// Where the view stays put when the rows change: the first row at or after
/// `from` in `old` that is still in `new`, as (its index in `old`, its index
/// in `new`). A list scrolled to the top shows a placeholder row first, so
/// rows added after it would push what was on screen away; anchoring on a
/// real row keeps the view on it.
pub fn anchor_row(old: &[Row], from: usize, new: &[Row]) -> Option<(usize, usize)> {
    let keys: HashMap<String, usize> = new
        .iter()
        .enumerate()
        .filter_map(|(ix, row)| Some((row.key()?, ix)))
        .collect();
    old.iter()
        .enumerate()
        .skip(from)
        .find_map(|(ix, row)| Some((ix, *keys.get(&row.key()?)?)))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use morpheus_core::{MessageId, OwnedEventId, OwnedUserId};

    fn message_by(id: &str, sender: &str, at: u64, kind: MessageKind) -> TimelineEntry {
        TimelineEntry::Message(Box::new(Message {
            unique_id: id.to_owned(),
            id: MessageId::Event(OwnedEventId::try_from(format!("${id}")).expect("event id")),
            sender: OwnedUserId::try_from(sender).expect("user id"),
            sender_name: sender.trim_start_matches('@').into(),
            sender_avatar: None,
            timestamp_ms: at,
            is_own: false,
            is_local_echo: false,
            is_edited: false,
            can_edit: false,
            reactions: Vec::new(),
            in_reply_to: None,
            thread_root: None,
            thread: None,
            kind,
            shield: None,
        }))
    }

    fn text(body: &str) -> MessageKind {
        MessageKind::Text {
            body: body.into(),
            html: None,
        }
    }

    fn message(id: &str, kind: MessageKind) -> TimelineEntry {
        message_by(id, "@bob:localhost", 0, kind)
    }

    fn shape(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                Row::Message { row, show_header } => {
                    format!(
                        "{}{}",
                        row.message.unique_id,
                        if *show_header { "+" } else { "" }
                    )
                }
                Row::New => "NEW".into(),
                Row::Date(..) => "date".into(),
                Row::Start => "start".into(),
                Row::Skeleton { .. } => "skeleton".into(),
            })
            .collect()
    }

    fn rows(entries: &[TimelineEntry], marker: Option<&str>) -> Vec<Row> {
        let entries: Vector<TimelineEntry> = entries.iter().cloned().collect();
        group_rows(&entries, marker, &HashMap::new())
    }

    #[test]
    fn profile_changes_are_hidden_and_the_new_divider_moves_past_them() {
        let entries = [
            message("a", MessageKind::State("bob joined".into())),
            message("b", MessageKind::ProfileChange),
            message("c", MessageKind::ProfileChange),
            message("d", MessageKind::State("bob left".into())),
        ];
        assert_eq!(shape(&rows(&entries, Some("b"))), ["a+", "NEW", "d+"]);
    }

    #[test]
    fn one_sender_within_five_minutes_shares_a_header() {
        let minute = 60_000;
        let entries = [
            message_by("a", "@bob:localhost", 0, text("1")),
            message_by("b", "@bob:localhost", minute, text("2")),
            message_by("c", "@carol:localhost", 2 * minute, text("3")),
            message_by("d", "@carol:localhost", 9 * minute, text("4")),
            message_by("e", "@carol:localhost", 10 * minute, text("5")),
        ];
        assert_eq!(
            shape(&rows(&entries, None)),
            ["a+", "b", "c+", "d+", "e"],
            "a new header for another sender and after a gap"
        );
    }

    #[test]
    fn dividers_and_state_events_break_a_group() {
        let entries = [
            message_by("a", "@bob:localhost", 0, text("1")),
            TimelineEntry::DateDivider { timestamp_ms: 10 },
            message_by("b", "@bob:localhost", 20, text("2")),
            message_by("c", "@bob:localhost", 30, MessageKind::State("x".into())),
            message_by("d", "@bob:localhost", 40, text("3")),
            TimelineEntry::ReadMarker,
        ];
        assert_eq!(
            shape(&rows(&entries, None)),
            ["a+", "date", "b+", "c+", "d+"]
        );
    }

    #[test]
    fn the_new_divider_sits_above_its_message_and_restarts_the_group() {
        let entries = [
            message("a", text("1")),
            message("b", text("2")),
            message("c", text("3")),
        ];
        assert_eq!(
            shape(&rows(&entries, Some("b"))),
            ["a+", "NEW", "b+", "c"],
            "a header follows the divider"
        );
    }

    #[test]
    fn the_start_of_the_room_is_a_row() {
        let entries = [TimelineEntry::TimelineStart, message("a", text("1"))];
        assert_eq!(shape(&rows(&entries, None)), ["start", "a+"]);
    }

    #[test]
    fn markers_come_from_the_read_marker_or_a_time() {
        let entries: Vector<TimelineEntry> = [
            message_by("a", "@bob:localhost", 10, text("1")),
            TimelineEntry::ReadMarker,
            message_by("b", "@bob:localhost", 20, text("2")),
        ]
        .into_iter()
        .collect();
        assert_eq!(first_unread_after_marker(&entries).as_deref(), Some("b"));
        assert_eq!(first_unread_since(&entries, 10).as_deref(), Some("b"));
        assert_eq!(first_unread_since(&entries, 30), None);
    }

    #[test]
    fn unchanged_messages_keep_their_rows() {
        let entries: Vector<TimelineEntry> =
            [message("a", text("1")), message("b", text("2"))]
                .into_iter()
                .collect();
        let first = group_rows(&entries, None, &HashMap::new());
        let known: HashMap<String, Arc<MessageRow>> = first
            .iter()
            .filter_map(|row| match row {
                Row::Message { row, .. } => Some((row.message.unique_id.clone(), row.clone())),
                _ => None,
            })
            .collect();
        let mut changed = entries.clone();
        changed.set(1, message("b", text("two")));
        let second = group_rows(&changed, None, &known);
        assert_eq!(first[0], second[0], "same message, same row");
        assert_ne!(first[1], second[1], "an edited one is a new row");
        assert_eq!(changed_range(&first, &second), (1, 1, 1));
    }

    #[test]
    fn the_view_anchors_on_a_real_row_when_older_ones_arrive() {
        let rows = |ids: &[&str]| -> Vec<Row> {
            let entries: Vector<TimelineEntry> = ids
                .iter()
                .map(|id| message_by(id, "@bob:localhost", 0, text(id)))
                .collect();
            group_rows(&entries, None, &HashMap::new())
        };
        let skeleton = || Row::Skeleton { older: true };
        let mut old = rows(&["c", "d"]);
        old.insert(0, skeleton());
        // A page of older messages arrives after the placeholder.
        let mut new = rows(&["a", "b", "c", "d"]);
        new.insert(0, skeleton());
        // The view was at the top, on the placeholder: it stays on "c".
        let (from, to) = anchor_row(&old, 0, &new).expect("an anchor");
        assert_eq!((from, to), (1, 3));
        // A row that is gone passes the anchor on to the next one still there.
        let gone = rows(&["d"]);
        assert_eq!(anchor_row(&old, 1, &gone), Some((2, 0)));
        // Nothing left in common: no anchor.
        assert_eq!(anchor_row(&old, 0, &rows(&["x"])), None);
    }

    #[test]
    fn what_changed_is_the_middle() {
        let a = |id: &str| Row::Date(id.into(), id.len() as u64 * 7 + u64::from(id.as_bytes()[0]));
        let (x, y, z, w) = (a("x"), a("yy"), a("zzz"), a("wwww"));
        // Appended.
        assert_eq!(
            changed_range(&[x.clone(), y.clone()], &[x.clone(), y.clone(), z.clone()]),
            (2, 0, 1)
        );
        // Prepended.
        assert_eq!(
            changed_range(&[y.clone()], &[x.clone(), z.clone(), y.clone()]),
            (0, 0, 2)
        );
        // Replaced in the middle.
        assert_eq!(
            changed_range(&[x.clone(), y.clone(), w.clone()], &[x.clone(), z, w]),
            (1, 1, 1)
        );
        // Removed from the end, and nothing.
        assert_eq!(changed_range(&[x.clone(), y.clone()], &[x.clone()]), (1, 1, 0));
        assert_eq!(changed_range(&[x.clone()], &[x]), (1, 0, 0));
        assert_eq!(changed_range(&[], &[]), (0, 0, 0));
    }

    #[test]
    fn edited_text_ends_with_the_edited_tag() {
        let mut message = match message("a", text("hello")) {
            TimelineEntry::Message(m) => *m,
            _ => unreachable!(),
        };
        message.is_edited = true;
        let row = MessageRow::new(message);
        assert_eq!(rich::text_of(&row.blocks), "hello (edited)");
    }
}
