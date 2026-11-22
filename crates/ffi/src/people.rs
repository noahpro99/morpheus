//! People and ways into rooms: starting a direct message, invites, joining by
//! address, making a room, friends and the public directory. The wording and
//! the parsing are `morpheus-view-model` (`friends`, `invites`,
//! `join_address`, `discover`).

use std::sync::Arc;

use morpheus_core::{MatrixLink, OwnedRoomId, OwnedRoomOrAliasId, OwnedUserId, RoomSummary};
use morpheus_view_model::{discover, friends, invites, join_address::parse_room_address};

use crate::{FfiError, MediaHandle, Morpheus};

fn failed(reason: impl Into<String>) -> FfiError {
    FfiError::Failed { reason: reason.into() }
}

fn room_id(id: &str) -> Result<OwnedRoomId, FfiError> {
    OwnedRoomId::try_from(id).map_err(|_| failed(format!("{id} is not a room id")))
}

/// A room I am invited to.
#[derive(uniffi::Record)]
pub struct InviteItem {
    pub id: String,
    pub name: String,
    pub avatar: Option<Arc<MediaHandle>>,
    pub is_dm: bool,
    /// "Bob invited you to a room."
    pub line: String,
}

pub(crate) fn invite_items(rooms: &[RoomSummary], session: &morpheus_core::Session) -> Vec<InviteItem> {
    invites::pending_invites(rooms)
        .iter()
        .map(|room| InviteItem {
            id: room.id.to_string(),
            name: room.name.clone(),
            avatar: room.avatar.clone().map(|m| MediaHandle::new(m, session)),
            is_dm: room.is_dm,
            line: invites::card_line(room),
        })
        .collect()
}

/// A friend: a name in a private list.
#[derive(uniffi::Record)]
pub struct FriendItem {
    pub user_id: String,
    pub name: String,
    pub subtitle: String,
    pub avatar: Option<Arc<MediaHandle>>,
}

/// A room of a server's public directory.
#[derive(uniffi::Record)]
pub struct DirectoryItem {
    pub id: String,
    pub name: String,
    pub topic: Option<String>,
    pub alias: Option<String>,
    /// "12 members".
    pub members: String,
    pub avatar: Option<Arc<MediaHandle>>,
    pub is_space: bool,
}

#[derive(uniffi::Record)]
pub struct DirectoryPage {
    pub rooms: Vec<DirectoryItem>,
    /// Pass back to get the next page; `None` at the end.
    pub next: Option<String>,
}

#[uniffi::export]
impl Morpheus {
    /// The direct message with whoever `typed` names (a username on this
    /// server, or a full Matrix ID), made if there is none yet; its room id.
    pub async fn start_dm(&self, typed: String) -> Result<String, FfiError> {
        let session = self.session()?;
        let user = session.find_user(typed).await?;
        friends::check_target(session.user_id().as_ref().map(|u| u.as_str()), user.as_str())
            .map_err(failed)?;
        let dm = session.dm_with(user).await?.to_string();
        // Messaging someone brings a closed DM back.
        let _ = session.set_mark(morpheus_core::marks::CLOSED, dm.clone(), None).await;
        Ok(dm)
    }

    /// Join an invited room.
    pub async fn accept_invite(&self, room_id_of: String) -> Result<(), FfiError> {
        Ok(self.session()?.join_room(room_id(&room_id_of)?).await?)
    }

    /// Turn an invite down.
    pub async fn decline_invite(&self, room_id_of: String) -> Result<(), FfiError> {
        Ok(self.session()?.leave_room(room_id(&room_id_of)?).await?)
    }

    /// Join the room an address names (`#alias:server`, `!id:server`, a
    /// matrix.to or matrix: link); its room id.
    pub async fn join_address(&self, text: String) -> Result<String, FfiError> {
        let session = self.session()?;
        let (room, via) = match parse_room_address(&text) {
            Ok(MatrixLink::Room { room, via, .. }) => (room, via),
            Ok(_) => return Err(failed("That is a person, not a room.")),
            Err(err) => return Err(failed(err.message())),
        };
        let room: OwnedRoomOrAliasId = room;
        let found = session
            .linked_room(room, via.clone())
            .await
            .map_err(|err| failed(format!("Could not find that room: {err}")))?;
        if !found.joined {
            session.join_room_via(found.room_id.clone(), via).await?;
        }
        Ok(found.room_id.to_string())
    }

    /// Make a room; its room id.
    pub async fn create_room(&self, name: String, topic: String) -> Result<String, FfiError> {
        if name.trim().is_empty() {
            return Err(failed("A room needs a name."));
        }
        Ok(self.session()?.create_room(name, topic, None).await?.to_string())
    }

    /// My friends.
    pub async fn friends(&self) -> Result<Vec<FriendItem>, FfiError> {
        let session = self.session()?;
        let nicknames = Default::default();
        let mut items = Vec::new();
        for user in session.friends().await? {
            let profile = session.profile(user.clone()).await.ok();
            let row = friends::friend_row(
                &nicknames,
                user.as_str(),
                profile.as_ref().map(|p| (p.name.as_str(), p.status.as_deref())),
            );
            items.push(FriendItem {
                user_id: user.to_string(),
                name: row.name,
                subtitle: row.subtitle,
                avatar: profile.and_then(|p| p.avatar).map(|m| MediaHandle::new(m, &session)),
            });
        }
        Ok(items)
    }

    /// Add or remove a friend.
    pub async fn set_friend(&self, user_id: String, friend: bool) -> Result<(), FfiError> {
        let session = self.session()?;
        // Adding takes what a person types (a username too); removing a listed ID.
        let user = if friend {
            session.find_user(user_id).await?
        } else {
            OwnedUserId::try_from(user_id.as_str()).map_err(|_| failed(format!("{user_id} is not a Matrix ID")))?
        };
        Ok(session.set_friend(user, friend).await?)
    }

    /// A page of this server's (or `server`'s) public rooms matching `term`.
    pub async fn public_rooms(
        &self,
        server: Option<String>,
        term: String,
        since: Option<String>,
    ) -> Result<DirectoryPage, FfiError> {
        let session = self.session()?;
        let page = session.public_rooms(server, term, since).await?;
        Ok(DirectoryPage {
            rooms: page
                .rooms
                .iter()
                .map(|room| DirectoryItem {
                    id: room.room_id.to_string(),
                    name: discover::name(room),
                    topic: discover::topic(room),
                    alias: room.alias.clone(),
                    members: discover::members_line(room),
                    avatar: room.avatar.clone().map(|m| MediaHandle::new(m, &session)),
                    is_space: room.is_space,
                })
                .collect(),
            next: page.next,
        })
    }

    /// Join a room found in the directory (`via`: the server it was found on).
    pub async fn join_public(&self, room_id_of: String, via: Option<String>) -> Result<(), FfiError> {
        let id = room_id(&room_id_of)?;
        Ok(self.session()?.join_room_via(id, via.into_iter().collect()).await?)
    }
}

#[uniffi::export]
impl Morpheus {
    /// Pin a DM to the top of the list, or close it (hidden until a new message), on every device:
    /// `kind` is `pinned` or `closed`.
    pub async fn set_dm_mark(&self, room_id_of: String, kind: String, on: bool) -> Result<(), FfiError> {
        let kind = match kind.as_str() {
            "pinned" => morpheus_core::marks::PINNED,
            "closed" => morpheus_core::marks::CLOSED,
            other => return Err(failed(format!("{other} is not a mark"))),
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or_default();
        self.session()?.set_mark(kind, room_id_of, on.then_some(now)).await?;
        self.layout_changed.notify_one();
        Ok(())
    }

    /// Put the servers in this order (their room IDs, first to last) on every device. What else the
    /// account keeps with the layout (the desktop's folders, hidden muted rooms) is left as it is.
    pub async fn set_rail_order(&self, space_ids: Vec<String>) -> Result<(), FfiError> {
        self.change_rail(|prefs| morpheus_view_model::rail_folders::reorder(prefs, &space_ids)).await
    }

    /// The rail's entries as the account keeps them: servers and folders of servers, in order.
    pub async fn rail_rows(&self) -> Result<Vec<RailRowItem>, FfiError> {
        use morpheus_view_model::rail_folders::{RailRow, rows};
        let prefs = self.session()?.ui_prefs().await?;
        Ok(rows(&prefs)
            .into_iter()
            .map(|row| match row {
                RailRow::Server(id) => RailRowItem { key: id.clone(), folder_id: None, name: String::new(), spaces: vec![id] },
                RailRow::Folder { id, name, spaces } => RailRowItem { key: format!("folder:{id}"), folder_id: Some(id), name, spaces },
            })
            .collect())
    }

    /// Put a server in a folder of its own (named "New folder"), or in an existing folder.
    pub async fn move_server_to_folder(&self, space_id: String, folder_id: Option<String>) -> Result<(), FfiError> {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or_default();
        self.change_rail(|prefs| match &folder_id {
            Some(folder) => morpheus_view_model::rail_folders::move_to_folder(prefs, &space_id, folder),
            None => morpheus_view_model::rail_folders::move_to_new_folder(prefs, &space_id, now),
        })
        .await
    }

    /// Take a server out of its folder.
    pub async fn move_server_out_of_folder(&self, space_id: String) -> Result<(), FfiError> {
        self.change_rail(|prefs| morpheus_view_model::rail_folders::move_out_of_folders(prefs, &space_id)).await
    }

    pub async fn rename_folder(&self, folder_id: String, name: String) -> Result<(), FfiError> {
        self.change_rail(|prefs| morpheus_view_model::rail_folders::rename_folder(prefs, &folder_id, &name)).await
    }
}

#[uniffi::export]
impl Morpheus {
    /// The people this account ignores (their messages are folded away), as the desktop keeps them.
    pub async fn ignored_people(&self) -> Result<Vec<String>, FfiError> {
        Ok(self.session()?.marks(morpheus_core::marks::IGNORED).await?.into_keys().collect())
    }

    /// Ignore someone, or stop, on every device.
    pub async fn set_ignored(&self, user_id: String, ignored: bool) -> Result<(), FfiError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or_default();
        Ok(self.session()?.set_mark(morpheus_core::marks::IGNORED, user_id, ignored.then_some(now)).await?)
    }
}

/// One entry of the rail: a server, or a folder of servers.
#[derive(Debug, Clone, uniffi::Record)]
pub struct RailRowItem {
    /// What `set_rail_order` takes for it: the server's ID, or `folder:<id>`.
    pub key: String,
    /// Set for a folder.
    pub folder_id: Option<String>,
    /// The folder's name.
    pub name: String,
    /// The server (or the folder's servers), by room ID.
    pub spaces: Vec<String>,
}

impl Morpheus {
    pub(crate) async fn change_rail(&self, change: impl FnOnce(&mut serde_json::Value)) -> Result<(), FfiError> {
        let session = self.session()?;
        let mut prefs = session.ui_prefs().await?;
        change(&mut prefs);
        session.set_ui_prefs(prefs).await?;
        self.layout_changed.notify_one();
        Ok(())
    }
}
