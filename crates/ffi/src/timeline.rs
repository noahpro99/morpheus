//! A room's timeline: rows made by `morpheus-view-model::rows` (grouping under
//! a sender's header, day dividers) from the core's entries, pushed as a
//! whole list whenever they change, and the actions on it.

use std::{collections::HashMap, sync::Arc};

use morpheus_core::{
    MessageId, MessageKind, OwnedEventId, OwnedRoomId, Session, TimelineEntry, TimelineHandle,
    Vector,
};
use morpheus_view_model::rows::{MessageRow, Row, group_rows};

use crate::{FfiError, MediaHandle, Morpheus, TaskHandle, runtime};

/// A message to reply to, as far as it is known.
#[derive(uniffi::Record)]
pub struct ReplyItem {
    pub event_id: String,
    pub sender_name: Option<String>,
    pub summary: Option<String>,
}

#[derive(uniffi::Record)]
pub struct ReactionItem {
    pub key: String,
    pub count: u32,
    pub mine: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FileKind {
    File,
    Video,
    Audio,
}

/// What a message says.
#[derive(uniffi::Enum)]
pub enum MessageBody {
    /// Plain text (a notice or an emote too).
    Text { text: String },
    Image {
        /// The file name or caption.
        caption: String,
        media: Arc<MediaHandle>,
        width: Option<u32>,
        height: Option<u32>,
    },
    /// A file, video or audio clip: a row to tap, which fetches it whole.
    File {
        name: String,
        kind: FileKind,
        size: Option<u64>,
        media: Arc<MediaHandle>,
    },
    /// A deleted or unreadable message, a state change, in one line.
    Other { summary: String },
}

/// A message row.
#[derive(uniffi::Record)]
pub struct MessageItem {
    /// Stable while the message lives, also while it is still sending.
    pub key: String,
    /// `None` while it is still sending.
    pub event_id: Option<String>,
    pub sender_id: String,
    pub sender_name: String,
    pub sender_avatar: Option<Arc<MediaHandle>>,
    /// `HH:MM`.
    pub time: String,
    pub timestamp_ms: u64,
    /// The name, picture and time go above it (a new sender, or a pause).
    pub show_header: bool,
    pub is_own: bool,
    pub sending: bool,
    pub edited: bool,
    /// An own text message: its text can be replaced.
    pub can_edit: bool,
    /// An own message: it can be deleted.
    pub can_delete: bool,
    pub body: MessageBody,
    pub reply_to: Option<ReplyItem>,
    pub reactions: Vec<ReactionItem>,
    /// Replies to this message live in a thread: the chip under it.
    pub thread: Option<ThreadChip>,
}

/// The chip on a thread's root: how many replies, and from whom last.
#[derive(uniffi::Record)]
pub struct ThreadChip {
    /// "1 reply", "3 replies".
    pub count: String,
    /// "· last from Bob", once known.
    pub latest: Option<String>,
}

#[derive(uniffi::Enum)]
pub enum TimelineRow {
    Message { message: MessageItem },
    /// A day heading, e.g. "Tuesday, March 4".
    Date { label: String, timestamp_ms: u64 },
    /// "This is the beginning of the room."
    Start,
    /// The unread divider.
    New,
}

/// Gets the whole list of rows, oldest first, whenever one changed.
#[uniffi::export(with_foreign)]
pub trait TimelineListener: Send + Sync {
    fn on_rows(&self, rows: Vec<TimelineRow>);
}

fn message_item(row: &MessageRow, show_header: bool, session: &Session) -> MessageItem {
    let message = &row.message;
    let body = match &message.kind {
        MessageKind::Image {
            body,
            media,
            width,
            height,
            ..
        } => MessageBody::Image {
            caption: body.clone(),
            media: MediaHandle::new(media.clone(), session),
            width: *width,
            height: *height,
        },
        MessageKind::File { body, media, size, .. } => MessageBody::File {
            name: body.clone(),
            kind: FileKind::File,
            size: *size,
            media: MediaHandle::new(media.clone(), session),
        },
        MessageKind::Audio { body, media, size, .. } => MessageBody::File {
            name: body.clone(),
            kind: FileKind::Audio,
            size: *size,
            media: MediaHandle::new(media.clone(), session),
        },
        MessageKind::Video { body, media, .. } => MessageBody::File {
            name: body.clone(),
            kind: FileKind::Video,
            size: None,
            media: MediaHandle::new(media.clone(), session),
        },
        kind => match kind.plain_body() {
            Some(text) => MessageBody::Text { text: text.to_owned() },
            None => MessageBody::Other {
                summary: kind.summary(),
            },
        },
    };
    MessageItem {
        key: message.unique_id.clone(),
        event_id: match &message.id {
            MessageId::Event(id) => Some(id.to_string()),
            MessageId::Transaction(_) => None,
        },
        sender_id: message.sender.to_string(),
        sender_name: message.sender_name.clone(),
        sender_avatar: message
            .sender_avatar
            .clone()
            .map(|media| MediaHandle::new(media, session)),
        time: row.time.clone(),
        timestamp_ms: message.timestamp_ms,
        show_header,
        is_own: message.is_own,
        sending: message.is_local_echo,
        edited: message.is_edited,
        can_edit: message.is_own && message.can_edit,
        can_delete: message.is_own && matches!(message.id, MessageId::Event(_)),
        body,
        reply_to: message.in_reply_to.as_ref().map(|reply| ReplyItem {
            event_id: reply.event_id.to_string(),
            sender_name: reply.sender_name.clone(),
            summary: reply.summary.clone(),
        }),
        reactions: message
            .reactions
            .iter()
            .map(|r| ReactionItem {
                key: r.key.clone(),
                count: r.count as u32,
                mine: r.mine,
            })
            .collect(),
        thread: message.thread.as_ref().map(|t| ThreadChip {
            count: morpheus_view_model::threads::chip_count(t),
            latest: morpheus_view_model::threads::chip_latest(t),
        }),
    }
}

fn rows_of(rows: &[Row], session: &Session) -> Vec<TimelineRow> {
    rows.iter()
        .filter_map(|row| match row {
            Row::Message { row, show_header } => Some(TimelineRow::Message {
                message: message_item(row, *show_header, session),
            }),
            Row::Date(label, at) => Some(TimelineRow::Date {
                label: label.clone(),
                timestamp_ms: *at,
            }),
            Row::Start => Some(TimelineRow::Start),
            Row::New => Some(TimelineRow::New),
            Row::Skeleton { .. } => None,
        })
        .collect()
}

/// An open room: send, reply, react, load older messages. Dropping it (or
/// the handle of its updates) closes the timeline.
#[derive(uniffi::Object)]
pub struct RoomTimeline {
    handle: TimelineHandle,
    pub(crate) session: Session,
    pub(crate) room_id: OwnedRoomId,
    _updates: Arc<TaskHandle>,
}

fn event_id(id: &str) -> Result<OwnedEventId, FfiError> {
    OwnedEventId::try_from(id).map_err(|_| FfiError::Failed {
        reason: format!("{id} is not an event id"),
    })
}

#[uniffi::export]
impl RoomTimeline {
    /// Send a message (Markdown is formatted).
    pub async fn send_text(&self, body: String) -> Result<(), FfiError> {
        let emotes = self.emote_pairs().await;
        Ok(self.handle.send_text_with(body, emotes).await?)
    }

    /// Upload a file (a picture, a clip, anything) and send it; the message
    /// type follows `mime`.
    pub async fn send_file(&self, name: String, mime: String, bytes: Vec<u8>) -> Result<(), FfiError> {
        Ok(self.handle.send_attachment_bytes(name, mime, bytes).await?)
    }

    /// Send a message as a reply to the event `reply_to`.
    pub async fn send_reply(&self, body: String, reply_to: String) -> Result<(), FfiError> {
        let emotes = self.emote_pairs().await;
        Ok(self.handle.send_reply_with(body, event_id(&reply_to)?, emotes).await?)
    }

    /// React to a message with `key` (an emoji), or take the reaction back.
    pub async fn toggle_reaction(&self, event_id_of: String, key: String) -> Result<(), FfiError> {
        Ok(self
            .handle
            .toggle_reaction(MessageId::Event(event_id(&event_id_of)?), key)
            .await?)
    }

    /// Load older messages; `true` once the start of the room is reached.
    pub async fn load_older(&self) -> Result<bool, FfiError> {
        Ok(self.handle.paginate_backwards(30).await?)
    }

    /// Replace the text of one of my messages.
    pub async fn edit_message(&self, event_id_of: String, body: String) -> Result<(), FfiError> {
        let emotes = self.emote_pairs().await;
        Ok(self.handle.edit_with(MessageId::Event(event_id(&event_id_of)?), body, emotes).await?)
    }

    /// Delete one of my messages.
    pub async fn delete_message(&self, event_id_of: String) -> Result<(), FfiError> {
        Ok(self.handle.redact(MessageId::Event(event_id(&event_id_of)?)).await?)
    }

    /// Tell the others the newest message is read.
    pub async fn mark_read(&self) -> Result<(), FfiError> {
        self.handle.send_read_receipt().await?;
        self.handle.mark_as_fully_read().await?;
        Ok(())
    }
}

#[uniffi::export]
impl Morpheus {
    /// Open a room's timeline; `listener` hears of every change, the current
    /// rows first.
    pub async fn open_room(
        &self,
        room_id: String,
        listener: Arc<dyn TimelineListener>,
    ) -> Result<Arc<RoomTimeline>, FfiError> {
        self.open(room_id, None, listener).await
    }

    /// Open the thread on the message `root_event_id`: the root, then its
    /// replies. What is sent through it goes into the thread.
    pub async fn open_thread(
        &self,
        room_id: String,
        root_event_id: String,
        listener: Arc<dyn TimelineListener>,
    ) -> Result<Arc<RoomTimeline>, FfiError> {
        self.open(room_id, Some(event_id(&root_event_id)?), listener).await
    }
}

impl Morpheus {
    async fn open(
        &self,
        room_id: String,
        thread_root: Option<OwnedEventId>,
        listener: Arc<dyn TimelineListener>,
    ) -> Result<Arc<RoomTimeline>, FfiError> {
        let session = self.session()?;
        let for_timeline = session.clone();
        let room_id = OwnedRoomId::try_from(room_id.as_str()).map_err(|_| FfiError::Failed {
            reason: format!("{room_id} is not a room id"),
        })?;
        let opened = match thread_root {
            Some(root) => session.thread(room_id.clone(), root).await?,
            None => session.timeline(room_id.clone()).await?,
        };
        let (handle, mut updates) = (opened.handle, opened.updates);
        let task = runtime().spawn(async move {
            let mut entries: Vector<TimelineEntry> = Vector::new();
            let mut known: HashMap<String, Arc<MessageRow>> = HashMap::new();
            while let Some(batch) = updates.next().await {
                for diff in batch {
                    diff.apply(&mut entries);
                }
                let rows = group_rows(&entries, None, &known);
                known = rows
                    .iter()
                    .filter_map(|row| match row {
                        Row::Message { row, .. } => {
                            Some((row.message.unique_id.clone(), row.clone()))
                        }
                        _ => None,
                    })
                    .collect();
                listener.on_rows(rows_of(&rows, &session));
            }
        });
        Ok(Arc::new(RoomTimeline {
            handle,
            session: for_timeline,
            room_id,
            _updates: TaskHandle::new(task),
        }))
    }
}
