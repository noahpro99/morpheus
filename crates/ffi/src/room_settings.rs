//! Edit room's Permissions (who may send, react, pin and call, #175) and the room's custom emoji pack (#193), as the desktop's room
//! dialog has them. The rules and words are `view_model::permissions` and `core::emotes`.

use std::sync::Arc;

use morpheus_core::{MediaRef, OwnedRoomId};
use morpheus_view_model::{i18n, permissions as perm, room_dialogs};

use crate::{FfiError, MediaHandle, Morpheus};

fn failed(reason: impl Into<String>) -> FfiError {
    FfiError::Failed { reason: reason.into() }
}

fn room(id: &str) -> Result<OwnedRoomId, FfiError> {
    OwnedRoomId::try_from(id).map_err(|_| failed(format!("{id} is not a room id")))
}

/// One choice of who may do a thing.
#[derive(Clone, uniffi::Record)]
pub struct PermissionChoice {
    /// The power level it asks (what `set_room_permission` takes).
    pub power: i64,
    pub label: String,
    pub selected: bool,
}

/// One thing a room restricts (send, react, pin, call).
#[derive(Clone, uniffi::Record)]
pub struct PermissionRow {
    /// `send`, `react`, `pin` or `call`.
    pub id: String,
    pub label: String,
    pub choices: Vec<PermissionChoice>,
}

#[derive(Clone, uniffi::Record)]
pub struct PermissionsInfo {
    /// The signed-in person may change them; when not, `note` says so and `rows` still show what is set.
    pub can_change: bool,
    pub note: Option<String>,
    pub rows: Vec<PermissionRow>,
}

/// A custom emoji of the room's pack.
#[derive(Clone, uniffi::Record)]
pub struct EmojiEntry {
    pub shortcode: String,
    pub media: Arc<MediaHandle>,
}

#[derive(Clone, uniffi::Record)]
pub struct EmojiPackInfo {
    pub editable: bool,
    pub emotes: Vec<EmojiEntry>,
}

fn action_of(id: &str) -> Result<perm::Action, FfiError> {
    perm::Action::ALL.into_iter().find(|a| a.id() == id).ok_or_else(|| failed("That is not something a room restricts."))
}

fn info_of(powers: &perm::Powers) -> PermissionsInfo {
    PermissionsInfo {
        can_change: powers.can_change,
        note: (!powers.can_change).then(|| i18n::t(perm::CANNOT_CHANGE)),
        rows: perm::Action::ALL
            .into_iter()
            .map(|action| {
                let chosen = powers.level(action);
                PermissionRow {
                    id: action.id().to_owned(),
                    label: i18n::t(action.label()),
                    choices: perm::Level::ALL.into_iter().map(|level| PermissionChoice { power: level.power(), label: i18n::t(level.label()), selected: level == chosen }).collect(),
                }
            })
            .collect(),
    }
}

#[uniffi::export]
impl Morpheus {
    /// Who may send, react, pin and start calls in a room.
    pub async fn room_permissions(&self, room_id: String) -> Result<PermissionsInfo, FfiError> {
        let now = self.session()?.room_powers(room(&room_id)?).await?;
        Ok(info_of(&perm::Powers { content: now.content, own: now.own, can_change: now.can_change }))
    }

    /// Choose who may `action_id` (the choice's `power`); the new state.
    pub async fn set_room_permission(&self, room_id: String, action_id: String, power: i64) -> Result<PermissionsInfo, FfiError> {
        let session = self.session()?;
        let id = room(&room_id)?;
        let action = action_of(&action_id)?;
        let now = session.room_powers(id.clone()).await?;
        let powers = perm::Powers { content: now.content, own: now.own, can_change: now.can_change };
        if !powers.can_change {
            return Err(failed(i18n::t(perm::CANNOT_CHANGE)));
        }
        session.set_room_powers(id.clone(), powers.with_level(action, perm::Level::from_required(power))).await?;
        let now = session.room_powers(id).await?;
        Ok(info_of(&perm::Powers { content: now.content, own: now.own, can_change: now.can_change }))
    }

    /// The room's own custom emoji pack.
    pub async fn room_emoji_pack(&self, room_id: String) -> Result<EmojiPackInfo, FfiError> {
        let session = self.session()?;
        let pack = session.room_emote_pack(room(&room_id)?).await?;
        Ok(EmojiPackInfo {
            editable: pack.editable,
            emotes: pack.emotes.into_iter().map(|e| EmojiEntry { media: MediaHandle::new(MediaRef::from_mxc_uri(&e.url), &session), shortcode: e.shortcode }).collect(),
        })
    }

    /// Add an emoji to the room's pack: a picture (the bytes of a file, named for its type) under `name` (the file's name when blank).
    pub async fn add_room_emoji(&self, room_id: String, name: String, file_name: String, bytes: Vec<u8>) -> Result<(), FfiError> {
        let mime = room_dialogs::picture_mime(&file_name).ok_or_else(|| failed(i18n::t(room_dialogs::NOT_A_PICTURE)))?;
        let code = morpheus_core::clean_shortcode(&name)
            .or_else(|| morpheus_core::shortcode_from_file_name(&file_name))
            .ok_or_else(|| failed("An emoji needs a name."))?;
        Ok(self.session()?.add_emote(Some(room(&room_id)?), String::new(), code, mime.to_owned(), bytes).await?)
    }

    pub async fn rename_room_emoji(&self, room_id: String, old: String, new: String) -> Result<(), FfiError> {
        Ok(self.session()?.rename_emote(Some(room(&room_id)?), String::new(), old, new).await?)
    }

    pub async fn remove_room_emoji(&self, room_id: String, shortcode: String) -> Result<(), FfiError> {
        Ok(self.session()?.remove_emote(Some(room(&room_id)?), String::new(), shortcode).await?)
    }
}

#[uniffi::export]
impl Morpheus {
    /// The link that opens a room or server (matrix.to, through its own server), for sharing.
    pub fn room_invite_link(&self, room_id: String) -> Result<String, FfiError> {
        let id = room(&room_id)?;
        // (A new room's ID has no server in it: ask through the signed-in server.)
        let server = id.server_name().map(|s| s.to_string()).or_else(|| self.session().ok().and_then(|s| s.user_id()).map(|u| u.server_name().to_string()));
        Ok(match server {
            Some(server) => format!("https://matrix.to/#/{id}?via={server}"),
            None => format!("https://matrix.to/#/{id}"),
        })
    }

}
