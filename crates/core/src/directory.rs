//! A server's public room directory (`/publicRooms`): what "Discover" lists.

use matrix_sdk::ruma::{
    api::client::directory::get_public_rooms_filtered, directory::Filter, room::RoomType,
};

use crate::{CoreFuture, MediaRef, OwnedRoomId, Session};

/// One room (or space) of a server's directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicRoom {
    pub room_id: OwnedRoomId,
    pub name: Option<String>,
    pub topic: Option<String>,
    /// `#alias:server`, when it has one.
    pub alias: Option<String>,
    pub avatar: Option<MediaRef>,
    pub members: u64,
    pub is_space: bool,
}

/// One page of a directory.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PublicRooms {
    pub rooms: Vec<PublicRoom>,
    /// Pass back as `since` for the next page; `None` at the end.
    pub next: Option<String>,
}

impl Session {
    /// A page of `server`'s public rooms (this account's own server when
    /// `None`) matching `term` (everything when blank), the biggest first as
    /// the server orders them. `since`: the previous page's `next`.
    pub fn public_rooms(
        &self,
        server: Option<String>,
        term: String,
        since: Option<String>,
    ) -> CoreFuture<PublicRooms> {
        let client = self.client.clone();
        self.core.run(async move {
            let mut request = get_public_rooms_filtered::v3::Request::new();
            request.server = match server.filter(|s| !s.trim().is_empty()) {
                Some(server) => Some(
                    server
                        .trim()
                        .try_into()
                        .map_err(|_| crate::Error::Other(format!("not a server name: {server}")))?,
                ),
                None => None,
            };
            request.limit = Some(30u32.into());
            request.since = since;
            let mut filter = Filter::new();
            let term = term.trim();
            if !term.is_empty() {
                filter.generic_search_term = Some(term.to_owned());
            }
            request.filter = filter;
            let response = client.public_rooms_filtered(request).await?;
            Ok(PublicRooms {
                rooms: response
                    .chunk
                    .into_iter()
                    .map(|room| PublicRoom {
                        room_id: room.room_id,
                        name: room.name,
                        topic: room.topic,
                        alias: room.canonical_alias.map(|a| a.to_string()),
                        avatar: room.avatar_url.map(MediaRef::from_mxc),
                        members: room.num_joined_members.into(),
                        is_space: room.room_type == Some(RoomType::Space),
                    })
                    .collect(),
                next: response.next_batch,
            })
        })
    }
}
