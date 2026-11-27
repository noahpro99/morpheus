//! The mod view (the desktop's `mod_view.rs`): a person across the rooms you share, where they stand in each and the moderation you may
//! do there. What each row offers is `view_model::mod_view`'s.

use morpheus_core::{MemberRole, OwnedRoomId, OwnedUserId};
use morpheus_view_model::mod_view;

use crate::{FfiError, Morpheus};

fn failed(reason: impl Into<String>) -> FfiError {
    FfiError::Failed { reason: reason.into() }
}

fn user(id: &str) -> Result<OwnedUserId, FfiError> {
    OwnedUserId::try_from(id).map_err(|_| failed(format!("{id} is not a user id")))
}

fn room(id: &str) -> Result<OwnedRoomId, FfiError> {
    OwnedRoomId::try_from(id).map_err(|_| failed(format!("{id} is not a room id")))
}

/// One role to choose.
#[derive(Clone, uniffi::Record)]
pub struct ModRole {
    pub key: String,
    pub label: String,
    pub current: bool,
}

/// One room of the mod view.
#[derive(Clone, uniffi::Record)]
pub struct ModRow {
    pub room_id: String,
    pub name: String,
    /// Where they stand: a role, Invited, Banned or Not in the room.
    pub state: String,
    /// The roles you may give (empty when you may not change roles here).
    pub roles: Vec<ModRole>,
    pub kick: bool,
    pub ban: bool,
    pub unban: bool,
}

fn role_of(key: &str) -> Result<MemberRole, FfiError> {
    mod_view::ROLES.iter().find(|(_, name)| name.to_lowercase() == key).map(|(role, _)| *role).ok_or_else(|| failed("That is not a role."))
}

#[uniffi::export]
impl Morpheus {
    /// Every room shared with `user_id` that they can be moderated in or stand in.
    pub async fn mod_view_rows(&self, user_id: String) -> Result<Vec<ModRow>, FfiError> {
        let rooms = self.session()?.mod_overview(user(&user_id)?).await?;
        Ok(rooms
            .iter()
            .map(|r| {
                let view = mod_view::row_view(r);
                ModRow {
                    room_id: r.room_id.to_string(),
                    name: r.name.clone(),
                    state: morpheus_view_model::i18n::t(view.state),
                    roles: view
                        .roles
                        .map(|(list, theirs)| list.into_iter().map(|(role, name)| ModRole { key: name.to_lowercase(), label: morpheus_view_model::i18n::t(name), current: role == theirs }).collect())
                        .unwrap_or_default(),
                    kick: view.kick,
                    ban: view.ban,
                    unban: view.unban,
                }
            })
            .collect())
    }

    /// Remove them from a room, with a reason (spaces only: none).
    pub async fn mod_kick(&self, room_id: String, user_id: String, reason: String) -> Result<(), FfiError> {
        Ok(self.session()?.kick(room(&room_id)?, user(&user_id)?, mod_view::given_reason(&reason)).await?)
    }

    pub async fn mod_ban(&self, room_id: String, user_id: String, reason: String) -> Result<(), FfiError> {
        Ok(self.session()?.ban(room(&room_id)?, user(&user_id)?, mod_view::given_reason(&reason)).await?)
    }

    pub async fn mod_unban(&self, room_id: String, user_id: String) -> Result<(), FfiError> {
        Ok(self.session()?.unban(room(&room_id)?, user(&user_id)?).await?)
    }

    pub async fn mod_set_role(&self, room_id: String, user_id: String, role_key: String) -> Result<(), FfiError> {
        Ok(self.session()?.set_role(room(&room_id)?, user(&user_id)?, role_of(&role_key)?).await?)
    }
}
