//! Searching messages: a local index the SDK keeps of every message this
//! device has loaded, decrypted ones included (so it works in encrypted
//! rooms, but finds only what the device has seen).

#[cfg(not(target_arch = "wasm32"))]
use futures_util::StreamExt;
#[cfg(not(target_arch = "wasm32"))]
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
            #[cfg(target_arch = "wasm32")]
            return server_search(&client, &room, query, limit).await;
            #[cfg(not(target_arch = "wasm32"))]
            local_search(&room, query, limit).await
        })
    }
}

/// The SDK's index of what this device has loaded.
#[cfg(not(target_arch = "wasm32"))]
async fn local_search(
    room: &matrix_sdk::Room,
    query: String,
    limit: usize,
) -> crate::Result<Vec<SearchHit>> {
    let mut pages = Box::pin(room.search_messages_events(query));
    let mut hits = Vec::new();
    while hits.len() < limit {
        let Some(page) = pages.next().await else {
            break;
        };
        let page = page.map_err(|err| crate::Error::Other(format!("search: {err}")))?;
        for event in page {
            let Ok(AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomMessage(
                SyncMessageLikeEvent::Original(message),
            ))) = event.raw().deserialize()
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
}

/// A browser keeps no local index: the server searches (rooms it can read,
/// so not encrypted ones).
#[cfg(target_arch = "wasm32")]
async fn server_search(
    client: &matrix_sdk::Client,
    room: &matrix_sdk::Room,
    query: String,
    limit: usize,
) -> crate::Result<Vec<SearchHit>> {
    use matrix_sdk::ruma::{
        api::client::{
            filter::RoomEventFilter,
            search::search_events::v3::{Categories, Criteria, Request},
        },
        events::{AnyMessageLikeEvent, AnyTimelineEvent, MessageLikeEvent},
    };
    let mut filter = RoomEventFilter::default();
    filter.rooms = Some(vec![room.room_id().to_owned()]);
    let mut criteria = Criteria::new(query);
    criteria.filter = filter;
    let mut categories = Categories::new();
    categories.room_events = Some(criteria);
    let response = client.send(Request::new(categories)).await?;
    let mut hits = Vec::new();
    for found in response.search_categories.room_events.results {
        let Some(Ok(AnyTimelineEvent::MessageLike(AnyMessageLikeEvent::RoomMessage(
            MessageLikeEvent::Original(message),
        )))) = found.result.map(|raw| raw.deserialize())
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
    Ok(hits)
}
