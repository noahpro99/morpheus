use std::sync::Arc;

use futures_util::{StreamExt, pin_mut};
use matrix_sdk::ruma::events::{
    StateEventType,
    room::message::{MessageType, RoomMessageEventContent},
};
use matrix_sdk_ui::timeline::{
    EventTimelineItem, MembershipChange, MsgLikeKind, RoomExt, Timeline, TimelineDetails,
    TimelineItem, TimelineItemContent, TimelineItemKind, VirtualTimelineItem,
};
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{debug, warn};

use crate::{Core, CoreFuture, Error, OwnedRoomId, OwnedUserId, Session, VectorDiff};

/// Messages fetched when a room is opened for the first time.
const INITIAL_BACKFILL: u16 = 40;

/// One row of a room timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimelineEntry {
    Message(Message),
    DateDivider { timestamp_ms: u64 },
    ReadMarker,
    TimelineStart,
}

/// A rendered-ready event. Owned and cheap to compare so UIs can diff it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Stable identity across local echo → remote echo transitions.
    pub id: String,
    pub event_id: Option<String>,
    pub sender: OwnedUserId,
    pub sender_name: String,
    pub timestamp_ms: u64,
    pub is_own: bool,
    pub is_local_echo: bool,
    pub is_edited: bool,
    pub kind: MessageKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageKind {
    Text(String),
    Notice(String),
    Emote(String),
    Image {
        body: String,
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
    fn from_item(item: &TimelineItem) -> Self {
        match item.kind() {
            TimelineItemKind::Virtual(VirtualTimelineItem::DateDivider(ts)) => Self::DateDivider {
                timestamp_ms: ts.0.into(),
            },
            TimelineItemKind::Virtual(VirtualTimelineItem::ReadMarker) => Self::ReadMarker,
            TimelineItemKind::Virtual(VirtualTimelineItem::TimelineStart) => Self::TimelineStart,
            TimelineItemKind::Event(event) => {
                Self::Message(Message::from_event(item.unique_id().0.clone(), event))
            }
        }
    }
}

impl Message {
    fn from_event(id: String, event: &EventTimelineItem) -> Self {
        let sender_name = match event.sender_profile() {
            TimelineDetails::Ready(profile) => profile.display_name.clone(),
            _ => None,
        }
        .unwrap_or_else(|| event.sender().localpart().to_owned());

        let kind = MessageKind::from_content(event.content(), &sender_name);

        Self {
            id,
            event_id: event.event_id().map(ToString::to_string),
            sender: event.sender().to_owned(),
            sender_name,
            timestamp_ms: event.timestamp().0.into(),
            is_own: event.is_own(),
            is_local_echo: event.is_local_echo(),
            is_edited: event.content().as_message().is_some_and(|m| m.is_edited()),
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
                    MessageType::Text(text) => Self::Text(text.body.clone()),
                    MessageType::Notice(notice) => Self::Notice(notice.body.clone()),
                    MessageType::Emote(emote) => Self::Emote(emote.body.clone()),
                    MessageType::Image(image) => Self::Image {
                        body: image.body.clone(),
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
    /// Send a plain-text message. It shows up in the stream as a local echo first.
    pub fn send_text(&self, body: String) -> CoreFuture<()> {
        let timeline = self.timeline.clone();
        let content = RoomMessageEventContent::text_plain(body);
        self.core.run(async move {
            timeline.send(content.into()).await?;
            Ok(())
        })
    }

    /// Load older history. Resolves to `true` once the start of the room is reached.
    pub fn paginate_backwards(&self, count: u16) -> CoreFuture<bool> {
        let timeline = self.timeline.clone();
        self.core
            .run(async move { Ok(timeline.paginate_backwards(count).await?) })
    }
}

impl Session {
    /// Open a room's timeline. Subscribes the room in sliding sync and kicks
    /// off an initial back-fill so it is not empty on first render.
    pub fn timeline(&self, room_id: OwnedRoomId) -> CoreFuture<OpenTimeline> {
        let core = self.core.clone();
        let client = self.client.clone();
        let sync = self.sync.clone();

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
                    .map(|item| TimelineEntry::from_item(item))
                    .collect(),
            };
            let _ = tx.send(vec![reset]);

            let forwarder = core.spawn(async move {
                pin_mut!(stream);
                while let Some(diffs) = stream.next().await {
                    let diffs: Vec<_> = diffs
                        .into_iter()
                        .map(|diff| diff.map(|item| TimelineEntry::from_item(&item)))
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
