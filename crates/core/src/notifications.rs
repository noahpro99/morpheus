//! Desktop-notification-worthy events for the current session.
//!
//! # Approach
//!
//! Re-implementing push rule evaluation, or opening a full
//! [`matrix_sdk_ui::timeline::Timeline`] per joined room just to notice new
//! messages, would be wasteful (each `Timeline` triggers an initial
//! back-fill). Instead this uses the SDK's `Client::latest_events()` API:
//! it is fed by the same event cache that sliding sync already populates for
//! every room in the room list (the room list's recency sort and unread
//! counts rely on it too), and each event it reports already carries the
//! push actions the SDK computed for it while processing the sync response
//! (see `matrix_sdk_base::response_processors::timeline`).
//!
//! Subscribing to a room's latest event (via `listen_and_subscribe_to_room`)
//! only ever resolves on *future* changes: the returned `Subscriber`'s
//! `next()` never replays the value that was current at subscribe time. That
//! gives us the "not historical backfill" requirement for free: whatever a
//! room's latest event already is when we start watching it (including on
//! first login) is never reported; only a genuinely new one is. Scrolling up
//! in an open room to load history never touches a room's "latest event"
//! either, so that can't produce a spurious notification.
//!
//! We still have to track which rooms exist (to start/stop watching them) by
//! mirroring the room list's `VectorDiff` stream, the same way [`crate::rooms`]
//! and [`crate::timeline`] do.

use std::collections::{HashMap, HashSet};

use futures_util::{StreamExt, pin_mut};
use matrix_sdk::{
    Room,
    latest_events::LatestEventValue,
    ruma::events::{AnyMessageLikeEventContent, AnySyncTimelineEvent, room::message::MessageType},
};
use matrix_sdk_ui::room_list_service::filters::new_filter_non_left;
use tokio::{sync::mpsc, task::AbortHandle};
use tracing::debug;

use crate::{CoreFuture, OwnedRoomId, OwnedUserId, Session, Vector};

/// How many rooms the underlying room list fetches before the UI asks for more.
const PAGE_SIZE: usize = 200;

/// A new, notification-worthy message from someone other than the current user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
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
    ///
    /// See the module docs for why this doesn't need a per-room `Timeline`
    /// or its own push-rule evaluation.
    pub fn notifications(&self) -> CoreFuture<NotificationUpdates> {
        let core = self.core.clone();
        let sync = self.sync.clone();
        let client = self.client.clone();
        let own_user = self.user_id();

        self.core.run(async move {
            let list = sync.room_list_service().all_rooms().await?;
            let (tx, rx) = mpsc::unbounded_channel();

            let watcher_core = core.clone();
            core.spawn(async move {
                let core = watcher_core;
                let latest_events = client.latest_events().await.clone();
                let (stream, controller) = list.entries_with_dynamic_adapters(PAGE_SIZE);
                controller.set_filter(Box::new(new_filter_non_left()));
                pin_mut!(stream);

                let mut rooms: Vector<Room> = Vector::new();
                let mut watchers: HashMap<OwnedRoomId, AbortHandle> = HashMap::new();

                while let Some(diffs) = stream.next().await {
                    for diff in diffs {
                        diff.map(|item| item.into_inner()).apply(&mut rooms);
                    }

                    let current: HashSet<OwnedRoomId> =
                        rooms.iter().map(|room| room.room_id().to_owned()).collect();

                    watchers.retain(|room_id, handle| {
                        if current.contains(room_id) {
                            true
                        } else {
                            handle.abort();
                            false
                        }
                    });

                    for room in rooms.iter() {
                        let room_id = room.room_id().to_owned();
                        if watchers.contains_key(&room_id) {
                            continue;
                        }

                        let Ok(Some(mut subscriber)) =
                            latest_events.listen_and_subscribe_to_room(&room_id).await
                        else {
                            continue;
                        };

                        let tx = tx.clone();
                        let own_user = own_user.clone();
                        let room = room.clone();
                        let watch_room_id = room_id.clone();
                        let handle = core.spawn(async move {
                            while let Some(value) = subscriber.next().await {
                                let Some(notification) =
                                    notification_from_latest_event(&room, value, own_user.as_ref())
                                        .await
                                else {
                                    continue;
                                };
                                if tx.send(notification).is_err() {
                                    debug!(
                                        room_id = %watch_room_id,
                                        "notification consumer dropped, stopping"
                                    );
                                    break;
                                }
                            }
                        });
                        watchers.insert(room_id, handle.abort_handle());
                    }
                }
            });

            Ok(NotificationUpdates { rx })
        })
    }
}

/// Build a [`Notification`] from a room's new latest event, or `None` if it
/// shouldn't notify (not a message, our own message, or push rules say no).
async fn notification_from_latest_event(
    room: &Room,
    value: LatestEventValue,
    own_user: Option<&OwnedUserId>,
) -> Option<Notification> {
    let LatestEventValue::Remote(event) = value else {
        return None;
    };

    let deserialized: AnySyncTimelineEvent = event.raw().deserialize().ok()?;
    let sender = deserialized.sender().to_owned();
    if own_user.is_some_and(|me| *me == sender) {
        return None;
    }

    let actions = event.push_actions()?;
    if !actions
        .iter()
        .any(matrix_sdk::ruma::push::Action::should_notify)
    {
        return None;
    }
    let is_mention = actions
        .iter()
        .any(matrix_sdk::ruma::push::Action::is_highlight);

    let AnySyncTimelineEvent::MessageLike(message) = &deserialized else {
        return None;
    };
    let body_summary = summarize(&message.original_content()?)?;

    let sender_name = match room.get_member_no_sync(&sender).await {
        Ok(Some(member)) => member.name().to_owned(),
        _ => sender.localpart().to_owned(),
    };
    let room_name = room
        .cached_display_name()
        .map(|n| n.to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| room.room_id().to_string());

    Some(Notification {
        room_id: room.room_id().to_owned(),
        room_name,
        sender_name,
        body_summary,
        is_mention,
    })
}

/// A one-line preview of a message-like content, or `None` for content types
/// that never generate a notification preview (reactions, polls, etc. are
/// filtered out upstream already by `should_notify`, so this only needs to
/// handle `m.room.message`).
fn summarize(content: &AnyMessageLikeEventContent) -> Option<String> {
    let AnyMessageLikeEventContent::RoomMessage(message) = content else {
        return None;
    };
    Some(match &message.msgtype {
        MessageType::Text(text) => text.body.clone(),
        MessageType::Notice(notice) => notice.body.clone(),
        MessageType::Emote(emote) => emote.body.clone(),
        MessageType::Image(image) => format!("🖼 {}", image.body),
        MessageType::File(file) => format!("📎 {}", file.body),
        other => format!("sent a {}", other.msgtype()),
    })
}
