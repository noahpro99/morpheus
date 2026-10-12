//! Per-message actions that live on the room rather than the timeline:
//! pinning, reporting, forwarding, and the unread flag.

use matrix_sdk::ruma::{OwnedEventId, OwnedUserId};
use serde_json::Value;

use crate::{CoreFuture, Error, OwnedRoomId, Session};

/// A pinned message, rendered from the stored event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedMessage {
    pub event_id: OwnedEventId,
    pub sender: OwnedUserId,
    /// Plain body, or a short description for non-text events.
    pub body: String,
}

impl Session {
    pub(crate) fn room(&self, room_id: &OwnedRoomId) -> crate::Result<matrix_sdk::Room> {
        self.client
            .get_room(room_id)
            .ok_or_else(|| Error::UnknownRoom(room_id.clone()))
    }

    /// Event IDs pinned in a room, in pin order.
    pub fn pinned_event_ids(&self, room_id: OwnedRoomId) -> CoreFuture<Vec<OwnedEventId>> {
        let room = self.room(&room_id);
        self.core
            .run(async move { Ok(room?.pinned_event_ids().unwrap_or_default()) })
    }

    /// Pinned messages with their text, newest pin last.
    pub fn pinned_messages(&self, room_id: OwnedRoomId) -> CoreFuture<Vec<PinnedMessage>> {
        let room = self.room(&room_id);
        self.core.run(async move {
            let room = room?;
            let mut out = Vec::new();
            for event_id in room.pinned_event_ids().unwrap_or_default() {
                let Ok(event) = room.event(&event_id, None).await else {
                    continue;
                };
                let Ok(json) = event.raw().deserialize_as::<Value>() else {
                    continue;
                };
                let sender = json
                    .get("sender")
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse().ok());
                let Some(sender) = sender else { continue };
                let content = json.get("content").cloned().unwrap_or(Value::Null);
                let body = content
                    .get("body")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        json.get("type")
                            .and_then(Value::as_str)
                            .map(|t| format!("({t})"))
                            .unwrap_or_else(|| "(event)".into())
                    });
                out.push(PinnedMessage {
                    event_id,
                    sender,
                    body,
                });
            }
            Ok(out)
        })
    }

    pub fn pin(
        &self,
        room_id: OwnedRoomId,
        event_id: OwnedEventId,
        pinned: bool,
    ) -> CoreFuture<()> {
        let room = self.room(&room_id);
        self.core.run(async move {
            let room = room?;
            if pinned {
                room.pin_event(&event_id).await?;
            } else {
                room.unpin_event(&event_id).await?;
            }
            // The pinned list is room state and only updates locally on the
            // next sync; wait for it so callers can refetch right away.
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(8);
            while tokio::time::Instant::now() < deadline {
                let now = room
                    .pinned_event_ids()
                    .unwrap_or_default()
                    .contains(&event_id);
                if now == pinned {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            Ok(())
        })
    }

    /// Report a message to the homeserver admins.
    pub fn report(
        &self,
        room_id: OwnedRoomId,
        event_id: OwnedEventId,
        reason: String,
    ) -> CoreFuture<()> {
        let room = self.room(&room_id);
        self.core.run(async move {
            let reason = reason.trim().to_owned();
            room?
                .report_content(event_id, (!reason.is_empty()).then_some(reason))
                .await?;
            Ok(())
        })
    }

    /// Flag a room as unread (or clear the flag) via `m.marked_unread`.
    pub fn mark_unread(&self, room_id: OwnedRoomId, unread: bool) -> CoreFuture<()> {
        let room = self.room(&room_id);
        self.core.run(async move {
            room?.set_unread_flag(unread).await?;
            Ok(())
        })
    }

    /// Send a copy of a message to another room: the same content, minus any
    /// reply or thread relation.
    pub fn forward(
        &self,
        from: OwnedRoomId,
        event_id: OwnedEventId,
        to: OwnedRoomId,
    ) -> CoreFuture<()> {
        let source = self.room(&from);
        let target = self.room(&to);
        self.core.run(async move {
            let event = source?.event(&event_id, None).await?;
            let json = event.raw().deserialize_as::<Value>()?;
            let event_type = json
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("m.room.message")
                .to_owned();
            let mut content = json.get("content").cloned().unwrap_or(Value::Null);
            if let Some(map) = content.as_object_mut() {
                map.remove("m.relates_to");
            }
            if event_type != "m.room.message" || content.get("msgtype").is_none() {
                return Err(Error::NotForwardable);
            }
            target?.send_raw(&event_type, content).await?;
            Ok(())
        })
    }
}
