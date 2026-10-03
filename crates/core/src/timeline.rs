use std::sync::Arc;

use futures_util::{StreamExt, pin_mut};
use matrix_sdk::ruma::{
    OwnedEventId, OwnedTransactionId,
    api::client::receipt::create_receipt::v3::ReceiptType,
    events::{
        StateEventType,
        room::message::{FormattedBody, MessageFormat, MessageType, RoomMessageEventContent},
    },
};
use matrix_sdk_ui::timeline::{
    EventTimelineItem, MembershipChange, MsgLikeKind, RoomExt, Timeline, TimelineDetails,
    TimelineEventItemId, TimelineItem, TimelineItemContent, TimelineItemKind, VirtualTimelineItem,
};
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{debug, warn};

use crate::{Core, CoreFuture, Error, MediaRef, OwnedRoomId, OwnedUserId, Session, VectorDiff};

/// Messages fetched when a room is opened for the first time.
const INITIAL_BACKFILL: u16 = 40;

/// One row of a room timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimelineEntry {
    /// Boxed: messages are large and the timeline is a persistent vector of entries.
    Message(Box<Message>),
    DateDivider {
        timestamp_ms: u64,
    },
    ReadMarker,
    TimelineStart,
}

/// Addressable identity of a message: remote events by event id, unsent
/// local echoes by transaction id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MessageId {
    Event(OwnedEventId),
    Transaction(OwnedTransactionId),
}

impl From<&MessageId> for TimelineEventItemId {
    fn from(id: &MessageId) -> Self {
        match id {
            MessageId::Event(event_id) => Self::EventId(event_id.clone()),
            MessageId::Transaction(txn_id) => Self::TransactionId(txn_id.clone()),
        }
    }
}

/// A render-ready event. Owned and cheap to compare so UIs can diff it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Stable render key across local echo → remote echo transitions.
    pub unique_id: String,
    pub id: MessageId,
    pub sender: OwnedUserId,
    pub sender_name: String,
    pub sender_avatar: Option<MediaRef>,
    pub timestamp_ms: u64,
    pub is_own: bool,
    pub is_local_echo: bool,
    pub is_edited: bool,
    pub reactions: Vec<Reaction>,
    pub kind: MessageKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reaction {
    pub key: String,
    pub count: usize,
    /// Whether the current user reacted with this key.
    pub mine: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageKind {
    /// `html` is sanitised and safe to inject; `body` is the plain fallback.
    Text {
        body: String,
        html: Option<String>,
    },
    Notice {
        body: String,
        html: Option<String>,
    },
    Emote {
        body: String,
        html: Option<String>,
    },
    Image {
        body: String,
        media: MediaRef,
        width: Option<u32>,
        height: Option<u32>,
    },
    File {
        body: String,
    },
    Redacted,
    UnableToDecrypt,
    /// A membership or state change, already phrased as a sentence.
    State(String),
    Unsupported(String),
}

impl TimelineEntry {
    fn from_item(item: &TimelineItem, own_user: Option<&OwnedUserId>) -> Self {
        match item.kind() {
            TimelineItemKind::Virtual(VirtualTimelineItem::DateDivider(ts)) => Self::DateDivider {
                timestamp_ms: ts.0.into(),
            },
            TimelineItemKind::Virtual(VirtualTimelineItem::ReadMarker) => Self::ReadMarker,
            TimelineItemKind::Virtual(VirtualTimelineItem::TimelineStart) => Self::TimelineStart,
            TimelineItemKind::Event(event) => Self::Message(Box::new(Message::from_event(
                item.unique_id().0.clone(),
                event,
                own_user,
            ))),
        }
    }
}

impl Message {
    fn from_event(
        unique_id: String,
        event: &EventTimelineItem,
        own_user: Option<&OwnedUserId>,
    ) -> Self {
        let profile = match event.sender_profile() {
            TimelineDetails::Ready(profile) => Some(profile),
            _ => None,
        };
        let sender_name = profile
            .and_then(|p| p.display_name.clone())
            .unwrap_or_else(|| event.sender().localpart().to_owned());
        let sender_avatar = profile
            .and_then(|p| p.avatar_url.clone())
            .map(MediaRef::from_mxc);

        let id = match event.identifier() {
            TimelineEventItemId::EventId(event_id) => MessageId::Event(event_id),
            TimelineEventItemId::TransactionId(txn_id) => MessageId::Transaction(txn_id),
        };

        let reactions = event
            .content()
            .reactions()
            .map(|by_key| {
                by_key
                    .iter()
                    .map(|(key, senders)| Reaction {
                        key: key.clone(),
                        count: senders.len(),
                        mine: own_user.is_some_and(|me| senders.contains_key(me)),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let kind = MessageKind::from_content(event.content(), &sender_name);

        Self {
            unique_id,
            id,
            sender: event.sender().to_owned(),
            sender_name,
            sender_avatar,
            timestamp_ms: event.timestamp().0.into(),
            is_own: event.is_own(),
            is_local_echo: event.is_local_echo(),
            is_edited: event.content().as_message().is_some_and(|m| m.is_edited()),
            reactions,
            kind,
        }
    }
}

impl MessageKind {
    /// `sender_name` is the subject for state changes made by the sender.
    fn from_content(content: &TimelineItemContent, sender_name: &str) -> Self {
        match content {
            TimelineItemContent::MsgLike(msg_like) => match &msg_like.kind {
                MsgLikeKind::Message(message) => match message.msgtype() {
                    MessageType::Text(text) => Self::Text {
                        body: text.body.clone(),
                        html: sanitized_html(text.formatted.as_ref()),
                    },
                    MessageType::Notice(notice) => Self::Notice {
                        body: notice.body.clone(),
                        html: sanitized_html(notice.formatted.as_ref()),
                    },
                    MessageType::Emote(emote) => Self::Emote {
                        body: emote.body.clone(),
                        html: sanitized_html(emote.formatted.as_ref()),
                    },
                    MessageType::Image(image) => Self::Image {
                        body: image.body.clone(),
                        media: MediaRef::from_source(image.source.clone()),
                        width: image
                            .info
                            .as_ref()
                            .and_then(|i| i.width)
                            .and_then(uint_to_u32),
                        height: image
                            .info
                            .as_ref()
                            .and_then(|i| i.height)
                            .and_then(uint_to_u32),
                    },
                    MessageType::File(file) => Self::File {
                        body: file.body.clone(),
                    },
                    other => Self::Unsupported(other.msgtype().to_owned()),
                },
                MsgLikeKind::Redacted => Self::Redacted,
                MsgLikeKind::UnableToDecrypt(_) => Self::UnableToDecrypt,
                MsgLikeKind::Sticker(_) => Self::Unsupported("sticker".into()),
                MsgLikeKind::Poll(_) => Self::Unsupported("poll".into()),
                MsgLikeKind::LiveLocation(_) => Self::Unsupported("live location".into()),
                MsgLikeKind::Other(_) => Self::Unsupported("message".into()),
            },
            TimelineItemContent::MembershipChange(change) => {
                let who = change
                    .display_name()
                    .unwrap_or_else(|| change.user_id().to_string());
                Self::State(format!("{who} {}", describe_membership(change.change())))
            }
            TimelineItemContent::ProfileChange(_) => {
                Self::State(format!("{sender_name} updated their profile"))
            }
            TimelineItemContent::OtherState(state) => Self::State(format!(
                "{sender_name} {}",
                describe_state(&state.content().event_type())
            )),
            TimelineItemContent::FailedToParseMessageLike { event_type, .. } => {
                Self::Unsupported(event_type.to_string())
            }
            TimelineItemContent::FailedToParseState { event_type, .. } => {
                Self::Unsupported(event_type.to_string())
            }
            TimelineItemContent::CallInvite => Self::Unsupported("call".into()),
            TimelineItemContent::RtcNotification { .. } => {
                Self::Unsupported("call notification".into())
            }
        }
    }
}

/// Strip everything the Matrix spec does not allow in formatted bodies.
fn sanitized_html(formatted: Option<&FormattedBody>) -> Option<String> {
    let formatted = formatted.filter(|f| f.format == MessageFormat::Html)?;
    let html = ruma_html::Html::parse(&formatted.body);
    html.sanitize();
    Some(html.to_string())
}

fn describe_membership(change: Option<MembershipChange>) -> &'static str {
    match change {
        Some(MembershipChange::Joined) => "joined the room",
        Some(MembershipChange::Left) => "left the room",
        Some(MembershipChange::Invited) => "was invited",
        Some(MembershipChange::InvitationAccepted) => "accepted the invite",
        Some(MembershipChange::InvitationRejected) => "declined the invite",
        Some(MembershipChange::InvitationRevoked) => "had their invite revoked",
        Some(MembershipChange::Banned) => "was banned",
        Some(MembershipChange::Unbanned) => "was unbanned",
        Some(MembershipChange::Kicked) => "was kicked",
        Some(MembershipChange::KickedAndBanned) => "was kicked and banned",
        Some(MembershipChange::Knocked) => "knocked",
        Some(MembershipChange::KnockAccepted) => "had their knock accepted",
        Some(MembershipChange::KnockRetracted) => "retracted their knock",
        Some(MembershipChange::KnockDenied) => "had their knock denied",
        _ => "changed their membership",
    }
}

fn describe_state(event_type: &StateEventType) -> String {
    match event_type {
        StateEventType::RoomCreate => "created the room".into(),
        StateEventType::RoomName => "changed the room name".into(),
        StateEventType::RoomTopic => "changed the topic".into(),
        StateEventType::RoomAvatar => "changed the room icon".into(),
        StateEventType::RoomCanonicalAlias => "changed the room address".into(),
        StateEventType::RoomJoinRules => "changed who can join".into(),
        StateEventType::RoomHistoryVisibility => "changed history visibility".into(),
        StateEventType::RoomPowerLevels => "changed permissions".into(),
        StateEventType::RoomEncryption => "enabled encryption".into(),
        StateEventType::RoomPinnedEvents => "changed pinned messages".into(),
        other => format!("changed {other}"),
    }
}

/// Actions on an open timeline. Clone freely; all clones share the timeline.
#[derive(Clone)]
pub struct TimelineHandle {
    core: Core,
    timeline: Arc<Timeline>,
    /// Aborts the diff-forwarding task when the last handle is dropped.
    _forwarder: Arc<AbortOnDrop>,
}

/// Incremental updates for one timeline. The first batch is always a
/// [`VectorDiff::Reset`] with the items known at open time.
pub struct TimelineUpdates {
    rx: mpsc::UnboundedReceiver<Vec<VectorDiff<TimelineEntry>>>,
}

/// Result of opening a room: the action handle and the update stream.
pub struct OpenTimeline {
    pub handle: TimelineHandle,
    pub updates: TimelineUpdates,
}

struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl TimelineUpdates {
    pub async fn next(&mut self) -> Option<Vec<VectorDiff<TimelineEntry>>> {
        self.rx.recv().await
    }
}

impl TimelineHandle {
    /// Send a message. Markdown is rendered to HTML when it contains any
    /// formatting; plain text otherwise. Shows up as a local echo first.
    pub fn send_text(&self, body: String) -> CoreFuture<()> {
        let timeline = self.timeline.clone();
        let content = RoomMessageEventContent::text_markdown(body);
        self.core.run(async move {
            timeline.send(content.into()).await?;
            Ok(())
        })
    }

    /// Add or remove the current user's reaction with `key` on a message.
    pub fn toggle_reaction(&self, message: MessageId, key: String) -> CoreFuture<()> {
        let timeline = self.timeline.clone();
        self.core.run(async move {
            timeline
                .toggle_reaction(&TimelineEventItemId::from(&message), &key)
                .await?;
            Ok(())
        })
    }

    /// Load older history. Resolves to `true` once the start of the room is reached.
    pub fn paginate_backwards(&self, count: u16) -> CoreFuture<bool> {
        let timeline = self.timeline.clone();
        self.core
            .run(async move { Ok(timeline.paginate_backwards(count).await?) })
    }

    /// Send a public read receipt for the latest event, if it moved forward.
    pub fn mark_as_read(&self) -> CoreFuture<bool> {
        let timeline = self.timeline.clone();
        self.core
            .run(async move { Ok(timeline.mark_as_read(ReceiptType::Read).await?) })
    }
}

impl Session {
    /// Open a room's timeline. Subscribes the room in sliding sync and kicks
    /// off an initial back-fill so it is not empty on first render.
    pub fn timeline(&self, room_id: OwnedRoomId) -> CoreFuture<OpenTimeline> {
        let core = self.core.clone();
        let client = self.client.clone();
        let sync = self.sync.clone();
        let own_user = self.user_id();

        self.core.run(async move {
            let room = client
                .get_room(&room_id)
                .ok_or_else(|| Error::UnknownRoom(room_id.clone()))?;
            sync.room_list_service()
                .set_room_subscriptions(&[&room_id])
                .await;

            let timeline = Arc::new(room.timeline().await?);
            let (initial, stream) = timeline.subscribe().await;

            let (tx, rx) = mpsc::unbounded_channel();
            let reset = VectorDiff::Reset {
                values: initial
                    .iter()
                    .map(|item| TimelineEntry::from_item(item, own_user.as_ref()))
                    .collect(),
            };
            let _ = tx.send(vec![reset]);

            let forwarder = core.spawn(async move {
                pin_mut!(stream);
                while let Some(diffs) = stream.next().await {
                    let diffs: Vec<_> = diffs
                        .into_iter()
                        .map(|diff| {
                            diff.map(|item| TimelineEntry::from_item(&item, own_user.as_ref()))
                        })
                        .collect();
                    if tx.send(diffs).is_err() {
                        debug!(%room_id, "timeline consumer dropped, stopping");
                        break;
                    }
                }
            });

            let backfill = timeline.clone();
            core.spawn(async move {
                if let Err(err) = backfill.paginate_backwards(INITIAL_BACKFILL).await {
                    warn!(%err, "initial back-pagination failed");
                }
            });

            Ok(OpenTimeline {
                handle: TimelineHandle {
                    core,
                    timeline,
                    _forwarder: Arc::new(AbortOnDrop(forwarder)),
                },
                updates: TimelineUpdates { rx },
            })
        })
    }
}

fn uint_to_u32(value: matrix_sdk::ruma::UInt) -> Option<u32> {
    u32::try_from(u64::from(value)).ok()
}
