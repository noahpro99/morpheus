//! Matrix links (`https://matrix.to/#/…` and `matrix:` URIs): who or what
//! they point at, and a look at a room before joining it.

use matrix_sdk::ruma::{
    MatrixToUri, MatrixUri, OwnedEventId, OwnedRoomId, OwnedRoomOrAliasId, OwnedUserId,
    matrix_uri::MatrixId,
};

use crate::{CoreFuture, MediaRef, Session};

/// What a Matrix link points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatrixLink {
    User(OwnedUserId),
    /// A room (by id or alias), or a message in it; `via`: servers to ask.
    Room {
        room: OwnedRoomOrAliasId,
        event: Option<OwnedEventId>,
        via: Vec<String>,
    },
}

/// The Matrix link `url` is, if it is one. Query parameters other than
/// `via` (a client's own, such as Element's) are ignored.
pub fn parse_matrix_link(url: &str) -> Option<MatrixLink> {
    let url = url.trim();
    let (id, via) = if url.starts_with("matrix:") {
        let uri = MatrixUri::parse(url).ok()?;
        (uri.id().clone(), uri.via().to_vec())
    } else {
        let rest = url
            .strip_prefix("https://matrix.to/")
            .or_else(|| url.strip_prefix("http://matrix.to/"))?;
        // `#/<id>[/<event>][?query]`: only `via` is kept.
        let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
        let via: Vec<&str> = query
            .split('&')
            .filter(|pair| pair.starts_with("via="))
            .collect();
        let clean = if via.is_empty() {
            format!("https://matrix.to/{path}")
        } else {
            format!("https://matrix.to/{path}?{}", via.join("&"))
        };
        let uri = MatrixToUri::parse(&clean).ok()?;
        (uri.id().clone(), uri.via().to_vec())
    };
    let via = via.iter().map(ToString::to_string).collect();
    Some(match id {
        MatrixId::User(user) => MatrixLink::User(user),
        MatrixId::Room(room) => MatrixLink::Room {
            room: room.into(),
            event: None,
            via,
        },
        MatrixId::RoomAlias(alias) => MatrixLink::Room {
            room: alias.into(),
            event: None,
            via,
        },
        MatrixId::Event(room, event) => MatrixLink::Room {
            room,
            event: Some(event),
            via,
        },
        _ => return None,
    })
}

/// A room seen through a link, before (or after) joining it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedRoom {
    pub room_id: OwnedRoomId,
    pub name: Option<String>,
    pub topic: Option<String>,
    pub avatar: Option<MediaRef>,
    pub members: u64,
    /// This account is in it already.
    pub joined: bool,
}

impl Session {
    /// What the room behind a link is: from this device if it is joined,
    /// else asked of the server (`via` first).
    pub fn linked_room(
        &self,
        room: OwnedRoomOrAliasId,
        via: Vec<String>,
    ) -> CoreFuture<LinkedRoom> {
        let client = self.client.clone();
        self.core.run(async move {
            let via = via
                .iter()
                .filter_map(|s| s.as_str().try_into().ok())
                .collect();
            let preview = client.get_room_preview(&room, via).await?;
            Ok(LinkedRoom {
                joined: preview.state == Some(matrix_sdk::RoomState::Joined),
                room_id: preview.room_id,
                name: preview.name,
                topic: preview.topic,
                avatar: preview.avatar_url.map(MediaRef::from_mxc),
                members: preview.num_joined_members,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_matrix_to_links() {
        assert!(matches!(
            parse_matrix_link("https://matrix.to/#/@bob:example.org?n=1"),
            Some(MatrixLink::User(user)) if user == "@bob:example.org"
        ));
        assert!(matches!(
            parse_matrix_link("https://matrix.to/#/%23room:example.org"),
            Some(MatrixLink::Room { room, event: None, .. }) if room == "#room:example.org"
        ));
        let Some(MatrixLink::Room { room, event, via }) = parse_matrix_link(
            "https://matrix.to/#/%21abc:example.org/%24ev?via=example.org&via=other.org",
        ) else {
            panic!("an event link");
        };
        assert_eq!(room, "!abc:example.org");
        assert_eq!(event.as_deref().map(|e| e.as_str()), Some("$ev"));
        assert_eq!(via, ["example.org", "other.org"]);
        assert_eq!(
            parse_matrix_link("https://example.org/#/@bob:example.org"),
            None
        );
        assert!(matches!(
            parse_matrix_link("matrix:u/bob:example.org"),
            Some(MatrixLink::User(_))
        ));
    }
}
