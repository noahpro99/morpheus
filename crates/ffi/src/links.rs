//! Matrix links (`https://matrix.to/#/...` and `matrix:` URIs) opened in the app, as the desktop's `links.rs` does: a person's profile
//! (or their chat), a room you are in (at the message), or a room you are not in, previewed with Join or Ask to join.

use std::sync::Arc;

use morpheus_core::{MatrixLink, OwnedRoomOrAliasId};
use morpheus_view_model::links as words;

use crate::{FfiError, MediaHandle, Morpheus};

/// What a link opens.
#[derive(uniffi::Record)]
pub struct LinkTarget {
    /// `none` (not a Matrix link), `user`, `chat` (a person's chat), `room` (one you are in) or `preview` (one you are not in).
    pub kind: String,
    /// The user's ID, or the room's.
    pub id: String,
    /// The message the link names, when it does.
    pub event_id: Option<String>,
    /// The room's name (a preview), its topic and size.
    pub name: String,
    pub topic: Option<String>,
    pub members: String,
    pub avatar: Option<Arc<MediaHandle>>,
    /// Joining needs the room's say-so: ask to join.
    pub knockable: bool,
    /// The servers to ask.
    pub via: Vec<String>,
    /// The link's room as written (an ID or an alias), for asking to join.
    pub address: String,
}

fn bare(kind: &str, id: String) -> LinkTarget {
    LinkTarget { kind: kind.into(), id, event_id: None, name: String::new(), topic: None, members: String::new(), avatar: None, knockable: false, via: Vec::new(), address: String::new() }
}

#[uniffi::export]
impl Morpheus {
    /// Whether `url` is a Matrix link the app opens itself.
    pub fn is_matrix_link(&self, url: String) -> bool {
        morpheus_core::parse_matrix_link(&url).is_some()
    }

    /// What `url` opens (asking the server about a room).
    pub async fn resolve_link(&self, url: String) -> Result<LinkTarget, FfiError> {
        let session = self.session()?;
        Ok(match morpheus_core::parse_matrix_link(&url) {
            None => bare("none", String::new()),
            Some(MatrixLink::User(user)) => bare(if morpheus_core::link_wants_chat(&url) { "chat" } else { "user" }, user.to_string()),
            Some(MatrixLink::Room { room, event, via }) => {
                let address = room.to_string();
                let linked = session.linked_room(room, via.clone()).await?;
                let mut target = bare(if linked.joined { "room" } else { "preview" }, linked.room_id.to_string());
                target.event_id = event.map(|e| e.to_string());
                target.name = words::preview_name(&linked);
                target.topic = words::preview_topic(&linked);
                target.members = words::members_line(linked.members);
                target.avatar = linked.avatar.map(|m| MediaHandle::new(m, &session));
                target.knockable = linked.knockable;
                target.via = via;
                target.address = address;
                target
            }
        })
    }

    /// Ask to join a room that takes knocks.
    pub async fn knock_link(&self, address: String, via: Vec<String>) -> Result<(), FfiError> {
        let room = OwnedRoomOrAliasId::try_from(address.as_str()).map_err(|_| FfiError::Failed { reason: format!("{address} is not a room") })?;
        Ok(self.session()?.knock_room(room, via).await?)
    }
}
