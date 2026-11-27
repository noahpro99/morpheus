//! The open room's last rows, kept in the app's folder so the next launch can draw them at once,
//! before the session is back (#313). One room is kept (the one that was open); pictures, files and
//! avatars are not (a file or picture shows as its name, the letter stands in for an avatar) and the
//! live rows replace all of it as soon as they arrive.

use serde::{Deserialize, Serialize};

use crate::timeline::{MessageBody, MessageItem, ReactionItem, ReplyItem, TextSegment, ThreadChip, TimelineRow};

const FILE: &str = "timeline-cache.json";
/// How many of the newest rows are kept: a screenful and some.
const KEEP: usize = 40;

#[derive(Serialize, Deserialize)]
struct Segment {
    text: String,
    url: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Reply {
    event_id: String,
    sender_name: Option<String>,
    summary: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Reaction {
    key: String,
    count: u32,
    mine: bool,
}

#[derive(Serialize, Deserialize)]
struct Chip {
    count: String,
    latest: Option<String>,
}

#[derive(Serialize, Deserialize)]
enum Body {
    Text { text: String, segments: Vec<Segment> },
    Other { summary: String },
}

#[derive(Serialize, Deserialize)]
struct Message {
    key: String,
    event_id: Option<String>,
    sender_id: String,
    sender_name: String,
    #[serde(default)]
    sender_avatar_key: Option<String>,
    time: String,
    timestamp_ms: u64,
    show_header: bool,
    is_own: bool,
    edited: bool,
    can_edit: bool,
    can_delete: bool,
    body: Body,
    reply_to: Option<Reply>,
    reactions: Vec<Reaction>,
    thread: Option<Chip>,
    preview_url: Option<String>,
    read_by: Vec<String>,
}

#[derive(Serialize, Deserialize)]
enum Row {
    Message(Message),
    Date { label: String, timestamp_ms: u64 },
    Start,
    New,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    room_id: String,
    rows: Vec<Row>,
}

fn row_of(row: &TimelineRow) -> Row {
    match row {
        TimelineRow::Message { message: m } => Row::Message(Message {
            key: m.key.clone(),
            event_id: m.event_id.clone(),
            sender_id: m.sender_id.clone(),
            sender_name: m.sender_name.clone(),
            sender_avatar_key: m.sender_avatar_key.clone(),
            time: m.time.clone(),
            timestamp_ms: m.timestamp_ms,
            show_header: m.show_header,
            is_own: m.is_own,
            edited: m.edited,
            can_edit: m.can_edit,
            can_delete: m.can_delete,
            body: match &m.body {
                MessageBody::Text { text, segments } => Body::Text {
                    text: text.clone(),
                    segments: segments.iter().map(|s| Segment { text: s.text.clone(), url: s.url.clone() }).collect(),
                },
                MessageBody::Image { caption, .. } => Body::Other { summary: caption.clone() },
                MessageBody::File { name, .. } => Body::Other { summary: name.clone() },
                MessageBody::Other { summary } => Body::Other { summary: summary.clone() },
            },
            reply_to: m.reply_to.as_ref().map(|r| Reply { event_id: r.event_id.clone(), sender_name: r.sender_name.clone(), summary: r.summary.clone() }),
            reactions: m.reactions.iter().map(|r| Reaction { key: r.key.clone(), count: r.count, mine: r.mine }).collect(),
            thread: m.thread.as_ref().map(|t| Chip { count: t.count.clone(), latest: t.latest.clone() }),
            preview_url: m.preview_url.clone(),
            read_by: m.read_by.clone(),
        }),
        TimelineRow::Date { label, timestamp_ms } => Row::Date { label: label.clone(), timestamp_ms: *timestamp_ms },
        TimelineRow::Start => Row::Start,
        TimelineRow::New => Row::New,
    }
}

fn back(row: Row) -> TimelineRow {
    match row {
        Row::Message(m) => TimelineRow::Message {
            message: MessageItem {
                key: m.key,
                event_id: m.event_id,
                sender_id: m.sender_id,
                sender_name: m.sender_name,
                sender_avatar: None,
                sender_avatar_key: m.sender_avatar_key,
                time: m.time,
                timestamp_ms: m.timestamp_ms,
                show_header: m.show_header,
                is_own: m.is_own,
                sending: false,
                edited: m.edited,
                can_edit: m.can_edit,
                can_delete: m.can_delete,
                body: match m.body {
                    Body::Text { text, segments } => MessageBody::Text {
                        text,
                        segments: segments.into_iter().map(|s| TextSegment { text: s.text, url: s.url }).collect(),
                    },
                    Body::Other { summary } => MessageBody::Other { summary },
                },
                reply_to: m.reply_to.map(|r| ReplyItem { event_id: r.event_id, sender_name: r.sender_name, summary: r.summary }),
                reactions: m.reactions.into_iter().map(|r| ReactionItem { key: r.key, count: r.count, mine: r.mine, senders: Vec::new(), emoji: None }).collect(),
                thread: m.thread.map(|t| ThreadChip { count: t.count, latest: t.latest }),
                preview_url: m.preview_url,
                read_by: m.read_by,
                mentions_me: false,
                collapsed_lines: None,
                is_event: false,
                lines: Vec::new(),
                state: None,
                shield: None,
            },
        },
        Row::Date { label, timestamp_ms } => TimelineRow::Date { label, timestamp_ms },
        Row::Start => TimelineRow::Start,
        Row::New => TimelineRow::New,
    }
}

/// The newest rows of a room as JSON text. Messages still sending are left out, and the cache starts at a
/// message (a day heading at the cut would be a heading over nothing).
pub(crate) fn encode(room_id: &str, rows: &[TimelineRow]) -> String {
    let from = rows.len().saturating_sub(KEEP);
    let kept: Vec<Row> = rows[from..]
        .iter()
        .filter(|row| !matches!(row, TimelineRow::Message { message } if message.sending))
        .map(row_of)
        .collect();
    serde_json::to_string(&Saved { room_id: room_id.to_owned(), rows: kept }).unwrap_or_default()
}

pub(crate) fn decode(room_id: &str, text: &str) -> Option<Vec<TimelineRow>> {
    let saved: Saved = serde_json::from_str(text).ok()?;
    (saved.room_id == room_id).then(|| saved.rows.into_iter().map(back).collect())
}

/// Keep the rows of `room_id` (nothing is written for a list with no message: the first push of a room
/// still catching up must not replace a good cache).
pub(crate) fn save(dir: &std::path::Path, room_id: &str, rows: &[TimelineRow]) {
    if rows.iter().any(|row| matches!(row, TimelineRow::Message { .. })) {
        let _ = std::fs::write(dir.join(FILE), encode(room_id, rows));
    }
}

pub(crate) fn load(dir: &std::path::Path, room_id: &str) -> Option<Vec<TimelineRow>> {
    decode(room_id, &std::fs::read_to_string(dir.join(FILE)).ok()?)
}

pub(crate) fn forget(dir: &std::path::Path) {
    let _ = std::fs::remove_file(dir.join(FILE));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(n: usize, body: MessageBody) -> TimelineRow {
        TimelineRow::Message {
            message: MessageItem {
                key: format!("k{n}"), event_id: Some(format!("$e{n}")), sender_id: "@a:s".into(), sender_name: "alice".into(), sender_avatar: None, sender_avatar_key: Some("mxc://s/b".into()),
                time: "12:00".into(), timestamp_ms: n as u64, show_header: n == 0, is_own: false, sending: false, edited: true, can_edit: false,
                can_delete: false, body, reply_to: Some(ReplyItem { event_id: "$r".into(), sender_name: Some("bob".into()), summary: Some("hi".into()) }),
                reactions: vec![ReactionItem { key: "+1".into(), count: 2, mine: true, senders: Vec::new(), emoji: None }], thread: None, preview_url: None, read_by: vec!["@b:s".into()], mentions_me: false, collapsed_lines: None, is_event: false, lines: Vec::new(), state: None, shield: None,
            },
        }
    }

    fn text(s: &str) -> MessageBody {
        MessageBody::Text { text: s.into(), segments: vec![TextSegment { text: s.into(), url: None }] }
    }

    #[test]
    fn the_last_rows_come_back_for_their_room_only() {
        let rows = vec![TimelineRow::Date { label: "Today".into(), timestamp_ms: 1 }, message(0, text("hello")), TimelineRow::New, message(1, MessageBody::Other { summary: "gone".into() })];
        let back = decode("!a:s", &encode("!a:s", &rows)).expect("decodes");
        assert_eq!(back.len(), 4);
        let TimelineRow::Message { message } = &back[1] else { panic!("a message") };
        assert_eq!((message.sender_name.as_str(), message.edited, message.reactions[0].count), ("alice", true, 2));
        assert_eq!(message.sender_avatar_key.as_deref(), Some("mxc://s/b"), "the picture's key is kept with the rows");
        assert!(matches!(&message.body, MessageBody::Text { text, .. } if text == "hello"));
        assert!(decode("!other:s", &encode("!a:s", &rows)).is_none(), "another room's rows are not this room's");
        assert!(decode("!a:s", "not json").is_none());
    }

    #[test]
    fn only_the_newest_rows_are_kept_and_a_list_without_messages_writes_nothing() {
        let rows: Vec<TimelineRow> = (0..100).map(|n| message(n, text("m"))).collect();
        assert_eq!(decode("!a:s", &encode("!a:s", &rows)).expect("decodes").len(), KEEP);
        let dir = std::env::temp_dir().join(format!("morpheus-tl-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        save(&dir, "!a:s", &rows[..3]);
        save(&dir, "!a:s", &[TimelineRow::Start]);
        assert_eq!(load(&dir, "!a:s").expect("kept").len(), 3, "an empty first push did not replace it");
        forget(&dir);
        assert!(load(&dir, "!a:s").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
