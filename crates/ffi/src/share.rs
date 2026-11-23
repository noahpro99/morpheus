//! The phone as a share target: where shared content can be sent. The ranking
//! is view-model's (`share::rank`, on the palette's fuzzy match), as on desktop.

use std::sync::Arc;

use morpheus_view_model::share::{rank, Candidate};

use crate::{MediaHandle, Morpheus};

/// One place to send a share to.
#[derive(uniffi::Record)]
pub struct ShareTarget {
    pub id: String,
    pub name: String,
    /// The server a channel is in; none for a DM or a room outside any server.
    pub server: Option<String>,
    pub is_dm: bool,
    pub avatar: Option<Arc<MediaHandle>>,
    pub avatar_key: Option<String>,
}

#[uniffi::export]
impl Morpheus {
    /// The rooms, channels and DMs to offer for what is typed, best first; with nothing typed the recent
    /// conversations, the most recent first (the room list is in recency order). Servers, voice channels and invites are not places to send to.
    pub fn share_targets(&self, query: String) -> Vec<ShareTarget> {
        let Ok(session) = self.session() else { return Vec::new() };
        let rooms = self.rooms_now.lock().map(|r| r.clone()).unwrap_or_default();
        let sendable: Vec<&morpheus_core::RoomSummary> = rooms.iter().filter(|r| !r.is_space && !r.invited && !r.is_voice).collect();
        let place = |room: &morpheus_core::RoomSummary| -> Option<String> {
            room.parent_spaces
                .iter()
                .chain(rooms.iter().filter(|s| s.is_space && s.space_children.contains(&room.id)).map(|s| &s.id))
                .find_map(|id| rooms.iter().find(|s| &s.id == id && s.is_space))
                .map(|s| s.name.clone())
        };
        let candidates: Vec<Candidate> = sendable
            .iter()
            .map(|r| Candidate { name: r.name.clone(), place: place(r).unwrap_or_default() })
            .collect();
        rank(&candidates, &query)
            .into_iter()
            .map(|i| {
                let room = sendable[i];
                ShareTarget {
                    id: room.id.to_string(),
                    name: room.name.clone(),
                    server: if room.is_dm { None } else { place(room) },
                    is_dm: room.is_dm,
                    avatar: room.avatar.clone().map(|m| MediaHandle::new(m, &session)),
                    avatar_key: room.avatar.as_ref().map(|m| m.key().to_owned()),
                }
            })
            .collect()
    }
}
