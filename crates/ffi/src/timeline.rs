//! A room's timeline: rows made by `morpheus-view-model::rows` (grouping under
//! a sender's header, day dividers) from the core's entries, pushed as a
//! whole list whenever they change, and the actions on it.

use std::{collections::HashMap, sync::Arc};

use morpheus_core::{
    MessageId, MessageKind, OwnedEventId, OwnedRoomId, Session, TimelineEntry, TimelineHandle,
    Vector,
};
use morpheus_view_model::rows::{MessageRow, Row, read_receipts, first_unread_after_marker, first_unread_since, group_rows};

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
    /// Who reacted (user IDs), for "View reactions".
    pub senders: Vec<String>,
    /// A custom emoji (its key is the picture's `mxc://` URI): the picture.
    pub emoji: Option<Arc<MediaHandle>>,
}

/// A piece of a line of rich text with one style (#405).
#[derive(uniffi::Record)]
pub struct RichRun {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub code: bool,
    pub faint: bool,
    /// A person or room pill.
    pub mention: bool,
    pub link: Option<String>,
    /// A custom emoji: the text is its `:name:`, this the picture.
    pub emote: Option<Arc<MediaHandle>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum RichKind {
    Para,
    Code,
    Rule,
}

/// A line of a message's formatted text.
#[derive(uniffi::Record)]
pub struct RichLine {
    pub kind: RichKind,
    /// How many block quotes it is in.
    pub quote: u32,
    /// How many lists it is in.
    pub indent: u32,
    /// A bullet or "3." on the first line of a list item.
    pub marker: Option<String>,
    pub runs: Vec<RichRun>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum StateTone {
    Good,
    Bad,
    Quiet,
}

/// How a state line ("bob joined") is drawn (#410): a Lucide icon name, its tone and the kind's name.
#[derive(uniffi::Record)]
pub struct StateLook {
    pub glyph: String,
    pub tone: StateTone,
    pub kind: String,
}

/// The shield on a message from a device that is not verified (#409).
#[derive(uniffi::Record)]
pub struct ShieldItem {
    pub red: bool,
    pub reason: String,
}

fn rich_lines(blocks: &[morpheus_view_model::rich::Block], session: &Session) -> Vec<RichLine> {
    use morpheus_view_model::rich_lines::{self, LineKind};
    rich_lines::lines(blocks)
        .into_iter()
        .map(|line| RichLine {
            kind: match line.kind {
                LineKind::Para => RichKind::Para,
                LineKind::Code => RichKind::Code,
                LineKind::Rule => RichKind::Rule,
            },
            quote: line.quote,
            indent: line.indent,
            marker: line.marker,
            runs: line
                .runs
                .into_iter()
                .map(|run| RichRun {
                    text: run.text,
                    bold: run.style.bold,
                    italic: run.style.italic,
                    underline: run.style.underline,
                    strike: run.style.strike,
                    code: run.style.code,
                    faint: run.style.faint,
                    mention: run.style.mention,
                    link: run.style.link.map(|l| l.to_string()),
                    emote: run.style.emote.map(|uri| MediaHandle::new(morpheus_core::MediaRef::from_mxc_uri(&uri), session)),
                })
                .collect(),
        })
        .collect()
}

fn state_look(kind: Option<morpheus_core::StateKind>) -> StateLook {
    use morpheus_view_model::state_lines::{self, Tone};
    let look = state_lines::icon(kind);
    StateLook {
        glyph: look.glyph.to_owned(),
        tone: match look.tone {
            Tone::Good => StateTone::Good,
            Tone::Bad => StateTone::Bad,
            Tone::Quiet => StateTone::Quiet,
        },
        kind: state_lines::StateIcon::class(kind).to_owned(),
    }
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
    Text {
        text: String,
        /// The text cut into plain runs and links (`url` set), for drawing.
        segments: Vec<TextSegment>,
    },
    Image {
        /// The file name or caption.
        caption: String,
        media: Arc<MediaHandle>,
        width: Option<u32>,
        height: Option<u32>,
        /// A GIF: it moves when drawn (#412).
        animated: bool,
    },
    /// A file, video or audio clip: a row to tap, which fetches it whole.
    File {
        name: String,
        kind: FileKind,
        size: Option<u64>,
        media: Arc<MediaHandle>,
        /// A video's poster picture, size and length, when the message says (#412).
        thumbnail: Option<Arc<MediaHandle>>,
        width: Option<u32>,
        height: Option<u32>,
        duration_ms: Option<u64>,
    },
    /// A deleted or unreadable message, a state change, in one line.
    Other { summary: String },
}

/// A run of a message's text: plain, or a link to open.
#[derive(uniffi::Record)]
pub struct TextSegment {
    pub text: String,
    pub url: Option<String>,
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
    /// The picture's key, kept with the rows of the last run (the app's disk cache has the picture).
    pub sender_avatar_key: Option<String>,
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
    /// The first web link of a text message: the one that may get a card.
    pub preview_url: Option<String>,
    /// Who else has read up to this message (user IDs, not the sender or me).
    pub read_by: Vec<String>,
    /// Someone else's message that mentions me: the row is lit (#407).
    pub mentions_me: bool,
    /// A very long message: how many of its lines stay in view while it is collapsed behind "Show more" (#408).
    pub collapsed_lines: Option<u32>,
    /// A room event (someone joined, the topic changed), not something a person wrote.
    pub is_event: bool,
    /// A text message as formatted lines (bold, quotes, lists, custom emoji); empty for anything else.
    pub lines: Vec<RichLine>,
    /// An event's icon.
    pub state: Option<StateLook>,
    /// A message from a device nobody verified.
    pub shield: Option<ShieldItem>,
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

fn message_item(row: &MessageRow, show_header: bool, read_by: Vec<String>, session: &Session, nicks: &morpheus_view_model::nicknames::NickMap) -> MessageItem {
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
            animated: morpheus_view_model::attachments::is_gif(body),
        },
        MessageKind::File { body, media, size, .. } => MessageBody::File {
            name: body.clone(),
            kind: FileKind::File,
            size: *size,
            media: MediaHandle::new(media.clone(), session),
            thumbnail: None,
            width: None,
            height: None,
            duration_ms: None,
        },
        MessageKind::Audio { body, media, size, .. } => MessageBody::File {
            name: body.clone(),
            kind: FileKind::Audio,
            size: *size,
            media: MediaHandle::new(media.clone(), session),
            thumbnail: None,
            width: None,
            height: None,
            duration_ms: None,
        },
        MessageKind::Video { body, media, thumbnail, width, height, duration } => MessageBody::File {
            name: body.clone(),
            kind: FileKind::Video,
            size: None,
            media: MediaHandle::new(media.clone(), session),
            thumbnail: thumbnail.clone().map(|t| MediaHandle::new(t, session)),
            width: *width,
            height: *height,
            duration_ms: duration.map(|d| d.as_millis() as u64),
        },
        kind => match kind.plain_body() {
            Some(text) => MessageBody::Text {
                text: text.to_owned(),
                segments: morpheus_view_model::linkify::segments(text)
                    .into_iter()
                    .map(|segment| match segment {
                        morpheus_view_model::linkify::Segment::Text(text) => TextSegment { text, url: None },
                        morpheus_view_model::linkify::Segment::Link { text, url } => TextSegment { text, url: Some(url) },
                    })
                    .collect(),
            },
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
        sender_name: morpheus_view_model::nicknames::shown_name(nicks, message.sender.as_str(), &message.sender_name),
        sender_avatar_key: message.sender_avatar.as_ref().map(|m| m.key().to_owned()),
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
                senders: r.senders.iter().map(ToString::to_string).collect(),
                emoji: r.key.starts_with("mxc://").then(|| MediaHandle::new(morpheus_core::MediaRef::from_mxc_uri(&r.key), session)),
            })
            .collect(),
        read_by,
        preview_url: if matches!(message.kind, MessageKind::Text { .. }) {
            morpheus_view_model::link_previews::first_url(&row.blocks)
        } else {
            None
        },
        thread: message.thread.as_ref().map(|t| ThreadChip {
            count: morpheus_view_model::threads::chip_count(t),
            latest: morpheus_view_model::threads::chip_latest(t),
        }),
        mentions_me: message.mentions_me && !message.is_own,
        collapsed_lines: if matches!(message.kind, MessageKind::Text { .. }) {
            morpheus_view_model::long_message::collapse(&row.blocks).map(|c| c.shown as u32)
        } else {
            None
        },
        is_event: matches!(message.kind, MessageKind::State(_)),
        lines: if matches!(message.kind, MessageKind::Text { .. } | MessageKind::Notice { .. } | MessageKind::Emote { .. }) { rich_lines(&row.blocks, session) } else { Vec::new() },
        state: matches!(message.kind, MessageKind::State(_)).then(|| state_look(message.state_kind)),
        shield: message.shield.as_ref().map(|s| ShieldItem { red: s.red, reason: s.reason.clone() }),
    }
}

fn rows_of(rows: &[Row], seen: &std::collections::HashMap<String, Vec<morpheus_core::OwnedUserId>>, session: &Session, nicks: &morpheus_view_model::nicknames::NickMap) -> Vec<TimelineRow> {
    rows.iter()
        .filter_map(|row| match row {
            Row::Message { row, show_header } => Some(TimelineRow::Message {
                message: message_item(row, *show_header, seen.get(&row.message.unique_id).map(|users| users.iter().map(ToString::to_string).collect()).unwrap_or_default(), session, nicks),
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
    pub(crate) handle: TimelineHandle,
    pub(crate) session: Session,
    pub(crate) room_id: OwnedRoomId,
    pub(crate) nicks: Arc<crate::nicknames::Nicknames>,
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
        let emotes = self.emote_pairs(&body).await;
        Ok(self.handle.send_text_with(body, emotes).await?)
    }

    /// Upload a file (a picture, a clip, anything) and send it; the message
    /// type follows `mime`.
    pub async fn send_file(&self, name: String, mime: String, bytes: Vec<u8>) -> Result<(), FfiError> {
        Ok(self.handle.send_attachment_bytes(name, mime, bytes).await?)
    }

    /// Send a message as a reply to the event `reply_to`.
    pub async fn send_reply(&self, body: String, reply_to: String) -> Result<(), FfiError> {
        let emotes = self.emote_pairs(&body).await;
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
        let emotes = self.emote_pairs(&body).await;
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
    /// The newest rows this room had when the app last ran, for drawing at once while the session is
    /// restored (no pictures); `None` when nothing is kept for it.
    pub fn cached_timeline(&self, room_id: String) -> Option<Vec<TimelineRow>> {
        crate::timeline_cache::load(&self.dir, &room_id)
    }

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
        let is_thread = thread_root.is_some();
        let opened = match thread_root {
            Some(root) => session.thread(room_id.clone(), root).await?,
            None => session.timeline(room_id.clone()).await?,
        };
        let (handle, mut updates) = (opened.handle, opened.updates);
        // Where the person had read to when the room opened: the NEW divider goes
        // above the first message after it, once, and stays.
        let read_at = if is_thread { None } else { session.fully_read_at(room_id.clone()).await.ok().flatten() };
        let (dir, cache_room) = (self.dir.clone(), room_id.to_string());
        let nicks = self.nicks.clone();
        let mut nick_changes = nicks.watch();
        let task = runtime().spawn(async move {
            let mut entries: Vector<TimelineEntry> = Vector::new();
            let mut known: HashMap<String, Arc<MessageRow>> = HashMap::new();
            let mut marker: Option<String> = None;
            let mut caught_up = false;
            loop {
                // A new batch of changes, or a nickname that was set (the rows are drawn again with it).
                tokio::select! {
                    batch = updates.next() => {
                        let Some(batch) = batch else { break };
                        for diff in batch {
                            diff.apply(&mut entries);
                        }
                    }
                    _ = nick_changes.changed() => {}
                }
                if marker.is_none() && !caught_up {
                    marker = first_unread_after_marker(&entries)
                        .or_else(|| read_at.and_then(|at| first_unread_since(&entries, at)));
                }
                // Sending something means being caught up.
                if entries.iter().any(|e| matches!(e, TimelineEntry::Message(m) if m.is_own && m.is_local_echo)) {
                    marker = None;
                    caught_up = true;
                }
                let rows = group_rows(&entries, marker.as_deref(), &known);
                known = rows
                    .iter()
                    .filter_map(|row| match row {
                        Row::Message { row, .. } => {
                            Some((row.message.unique_id.clone(), row.clone()))
                        }
                        _ => None,
                    })
                    .collect();
                let items = rows_of(&rows, &read_receipts(&entries), &session, &nicks.snapshot());
                // The open room's rows are kept for the next launch to draw at once (#313).
                if !is_thread {
                    crate::timeline_cache::save(&dir, &cache_room, &items);
                }
                listener.on_rows(items);
            }
        });
        Ok(Arc::new(RoomTimeline {
            handle,
            session: for_timeline,
            room_id,
            nicks: self.nicks.clone(),
            _updates: TaskHandle::new(task),
        }))
    }
}

/// Whether the timeline offers "Jump to present": `rows_above` rows lie between the newest one and the first
/// visible one, which is scrolled `offset_px` past its start (logical pixels). The desktop's rule (#414).
#[uniffi::export]
pub fn away_from_present(rows_above: u32, offset_px: f32) -> bool {
    morpheus_view_model::jump::away_from_present(rows_above, offset_px)
}

/// What "Say hi! 👋" sends (#421).
#[uniffi::export]
pub fn say_hi_greeting() -> String {
    morpheus_view_model::say_hi::GREETING.to_owned()
}

/// Whether the start of a conversation offers "Say hi!": a direct message with only room events in it
/// (`message_is_event` has a flag per message row: true when it is such an event).
#[uniffi::export]
pub fn say_hi_offered(is_direct: bool, at_start: bool, message_is_event: Vec<bool>) -> bool {
    morpheus_view_model::say_hi::offered(is_direct, at_start, message_is_event)
}

/// How long "Undo" is offered after a message is deleted or a room left, in seconds (#415).
#[uniffi::export]
pub fn undo_seconds() -> u64 {
    morpheus_view_model::toast::UNDO_SECS
}
