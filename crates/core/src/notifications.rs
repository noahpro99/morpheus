//! Desktop-notification-worthy messages for the current session.
//!
//! A sync event handler sees every `m.room.message` the homeserver sends for
//! a joined room, whichever room is open, with no per-room timeline. Push
//! rules decide whether it notifies and whether it is a highlight (mention),
//! through the room's [`matrix_sdk::room::PushContext`]. Events older than
//! the subscription are dropped: the first sync replays each room's latest
//! message, which is history, not news.

use matrix_sdk::{
    Room, RoomState,
    event_handler::RawEvent,
    ruma::{
        MilliSecondsSinceUnixEpoch, UInt,
        events::room::message::{MessageType, OriginalSyncRoomMessageEvent},
        push::Action,
        serde::Raw,
    },
};
use tokio::sync::mpsc;
use tracing::debug;

use crate::{CoreFuture, OwnedRoomId, Session};

/// Messages older than this at subscription time are history, not news.
const STALE_AFTER_MS: u64 = 60_000;

/// A new, notification-worthy message from someone other than the current user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// The message, so a push for it (which carries ids only) can be matched.
    pub event_id: String,
    pub room_id: OwnedRoomId,
    pub room_name: String,
    pub sender_name: String,
    pub body_summary: String,
    /// Whether the user's push rules flagged this as a highlight (mention).
    pub is_mention: bool,
}

/// A live stream of [`Notification`]s across every joined room.
pub struct NotificationUpdates {
    rx: mpsc::UnboundedReceiver<Notification>,
}

impl NotificationUpdates {
    /// Next notification, or `None` once the session is gone.
    pub async fn next(&mut self) -> Option<Notification> {
        self.rx.recv().await
    }
}

impl Session {
    /// Subscribe to notification-worthy messages across all joined rooms.
    pub fn notifications(&self) -> CoreFuture<NotificationUpdates> {
        let client = self.client.clone();
        let own_user = self.user_id();

        self.core.run(async move {
            let (tx, rx) = mpsc::unbounded_channel();
            let since = MilliSecondsSinceUnixEpoch::now();
            let cutoff = since
                .0
                .checked_sub(UInt::from(STALE_AFTER_MS as u32))
                .unwrap_or(since.0);

            // The handler is dropped with the client; a dropped consumer just
            // makes sends fail, which is fine.
            client.add_event_handler(
                move |event: OriginalSyncRoomMessageEvent, room: Room, raw: RawEvent| {
                    let tx = tx.clone();
                    let own_user = own_user.clone();
                    async move {
                        if room.state() != RoomState::Joined
                            || own_user.as_deref() == Some(event.sender.as_ref())
                            || event.origin_server_ts.0 < cutoff
                        {
                            return;
                        }
                        let Some(notification) = notification_for(&room, &event, &raw).await else {
                            return;
                        };
                        if tx.send(notification).is_err() {
                            debug!(room_id = %room.room_id(), "notification consumer dropped");
                        }
                    }
                },
            );

            Ok(NotificationUpdates { rx })
        })
    }
}

/// Build a [`Notification`] for a message, or `None` when push rules say it
/// should not notify.
async fn notification_for(
    room: &Room,
    event: &OriginalSyncRoomMessageEvent,
    raw: &RawEvent,
) -> Option<Notification> {
    let context = match room.push_context().await {
        Ok(Some(context)) => context,
        Ok(None) => {
            debug!(room_id = %room.room_id(), "no push context yet");
            return None;
        }
        Err(err) => {
            debug!(room_id = %room.room_id(), %err, "push context failed");
            return None;
        }
    };
    let raw: Raw<OriginalSyncRoomMessageEvent> = Raw::from_json(raw.0.clone());
    let actions = context.for_event(&raw).await;
    if !actions.iter().any(Action::should_notify) {
        return None;
    }
    let is_mention = actions.iter().any(Action::is_highlight);

    let sender_name = match room.get_member_no_sync(&event.sender).await {
        Ok(Some(member)) => member.name().to_owned(),
        _ => event.sender.localpart().to_owned(),
    };
    let room_name = room
        .cached_display_name()
        .map(|n| n.to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| room.room_id().to_string());

    Some(Notification {
        event_id: event.event_id.to_string(),
        room_id: room.room_id().to_owned(),
        room_name,
        sender_name,
        body_summary: summarize(&event.content.msgtype),
        is_mention,
    })
}

/// A one-line preview of a message.
fn summarize(msgtype: &MessageType) -> String {
    match msgtype {
        MessageType::Text(text) => text.body.clone(),
        MessageType::Notice(notice) => notice.body.clone(),
        MessageType::Emote(emote) => emote.body.clone(),
        MessageType::Image(image) => format!("🖼 {}", image.body),
        MessageType::File(file) => format!("📎 {}", file.body),
        other => format!("sent a {}", other.msgtype()),
    }
}
