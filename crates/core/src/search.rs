//! Searching messages: a local index the SDK keeps of every message this
//! device has loaded, decrypted ones included (so it works in encrypted
//! rooms, but finds only what the device has seen).

use futures_util::StreamExt;
use matrix_sdk::ruma::events::{
    AnySyncMessageLikeEvent, AnySyncTimelineEvent, SyncMessageLikeEvent,
};

use crate::{CoreFuture, OwnedRoomId, Session};

/// One search result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub event_id: String,
    pub sender_name: String,
    pub body: String,
    pub timestamp_ms: u64,
}

impl Session {
    /// Up to `limit` messages in a room matching `query`, best first.
    pub fn search_room(
        &self,
        room_id: OwnedRoomId,
        query: String,
        limit: usize,
    ) -> CoreFuture<Vec<SearchHit>> {
        let client = self.client.clone();
        self.core.run(async move {
            let room = client
                .get_room(&room_id)
                .ok_or_else(|| crate::Error::UnknownRoom(room_id.clone()))?;
            if query.trim().is_empty() {
                return Ok(Vec::new());
            }
            let mut pages = Box::pin(room.search_messages_events(query));
            let mut hits = Vec::new();
            while hits.len() < limit {
                let Some(page) = pages.next().await else {
                    break;
                };
                let page = page.map_err(|err| crate::Error::Other(format!("search: {err}")))?;
                for event in page {
                    let Ok(AnySyncTimelineEvent::MessageLike(
                        AnySyncMessageLikeEvent::RoomMessage(SyncMessageLikeEvent::Original(
                            message,
                        )),
                    )) = event.raw().deserialize()
                    else {
                        continue;
                    };
                    let sender_name = match room.get_member_no_sync(&message.sender).await {
                        Ok(Some(member)) => member.name().to_owned(),
                        _ => message.sender.localpart().to_owned(),
                    };
                    hits.push(SearchHit {
                        event_id: message.event_id.to_string(),
                        sender_name,
                        body: message.content.body().to_owned(),
                        timestamp_ms: u64::from(message.origin_server_ts.0),
                    });
                    if hits.len() >= limit {
                        break;
                    }
                }
            }
            Ok(hits)
        })
    }
}
