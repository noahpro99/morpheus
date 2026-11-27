//! What the phone's Edit room page adds (#422, #423) and the invite page (#425): who may send, react, pin and call in a channel
//! (`view_model::permissions`), the room's or server's custom emoji pack, and a room's link. All of it is the desktop's logic.

use std::sync::Arc;

use morpheus_core::OwnedRoomId;
use morpheus_view_model::{i18n, permissions as perm, room_dialogs};

use crate::{FfiError, MediaHandle, Morpheus};

fn failed(reason: impl Into<String>) -> FfiError {
    FfiError::Failed { reason: reason.into() }
}

fn room_id(id: &str) -> Result<OwnedRoomId, FfiError> {
    OwnedRoomId::try_from(id).map_err(|_| failed(format!("{id} is not a room id")))
}

/// One thing a channel can restrict, and who may do it now.
#[derive(uniffi::Record)]
pub struct PermissionRow {
    /// `send`, `react`, `pin` or `call`.
    pub action: String,
    pub label: String,
    /// The chosen level's power: 0, 50 or 100.
    pub level: i64,
    /// What each level is called, in the order of the powers 0, 50, 100.
    pub options: Vec<PermissionOption>,
}

#[derive(uniffi::Record)]
pub struct PermissionOption {
    pub power: i64,
    pub label: String,
}

/// The Permissions section: its rows when the person may change them, else only the note.
#[derive(uniffi::Record)]
pub struct PermissionsInfo {
    pub can_change: bool,
    pub note: String,
    pub rows: Vec<PermissionRow>,
}

fn action_named(name: &str) -> Result<perm::Action, FfiError> {
    perm::Action::ALL.into_iter().find(|a| a.id() == name).ok_or_else(|| failed(format!("{name} is not a permission")))
}

/// One emoji of a pack.
#[derive(uniffi::Record)]
pub struct PackEmoji {
    pub shortcode: String,
    pub media: Arc<MediaHandle>,
}

/// A room's (or server's) own emoji pack and whether the person may change it.
#[derive(uniffi::Record)]
pub struct EmojiPackInfo {
    pub editable: bool,
    pub emotes: Vec<PackEmoji>,
}

#[uniffi::export]
impl Morpheus {
    /// Who may send, react, pin and start calls in this room, as the Edit room page lists it.
    pub async fn room_permissions(&self, room_id_of: String) -> Result<PermissionsInfo, FfiError> {
        let powers = self.session()?.room_powers(room_id(&room_id_of)?).await?;
        let known = perm::Powers { content: powers.content, own: powers.own, can_change: powers.can_change };
        if !known.can_change {
            return Ok(PermissionsInfo { can_change: false, note: i18n::t(perm::CANNOT_CHANGE), rows: Vec::new() });
        }
        let rows = perm::Action::ALL
            .into_iter()
            .map(|action| PermissionRow {
                action: action.id().to_owned(),
                label: i18n::t(action.label()),
                level: known.level(action).power(),
                options: perm::Level::ALL.into_iter().map(|l| PermissionOption { power: l.power(), label: i18n::t(l.label()) }).collect(),
            })
            .collect();
        Ok(PermissionsInfo { can_change: true, note: String::new(), rows })
    }

    /// Choose who may do `action` (`send`, `react`, `pin`, `call`): the power `power` (0, 50 or 100).
    pub async fn set_room_permission(&self, room_id_of: String, action: String, power: i64) -> Result<(), FfiError> {
        let session = self.session()?;
        let id = room_id(&room_id_of)?;
        let action = action_named(&action)?;
        let level = perm::Level::ALL.into_iter().find(|l| l.power() == power).ok_or_else(|| failed("not a level"))?;
        let powers = session.room_powers(id.clone()).await?;
        let known = perm::Powers { content: powers.content, own: powers.own, can_change: powers.can_change };
        if !known.can_change {
            return Err(failed(i18n::t(perm::CANNOT_CHANGE)));
        }
        Ok(session.set_room_powers(id, known.with_level(action, level)).await?)
    }

    /// The room's or server's emoji pack.
    pub async fn room_emoji_pack(&self, room_id_of: String) -> Result<EmojiPackInfo, FfiError> {
        let session = self.session()?;
        let pack = session.room_emote_pack(room_id(&room_id_of)?).await?;
        Ok(EmojiPackInfo {
            editable: pack.editable,
            emotes: pack
                .emotes
                .into_iter()
                .map(|e| PackEmoji { shortcode: e.shortcode, media: MediaHandle::new(morpheus_core::MediaRef::from_mxc_uri(&e.url), &session) })
                .collect(),
        })
    }

    /// Add a picture to the pack under `typed` (or a name made from the file's), replacing one of that name.
    pub async fn add_room_emoji(&self, room_id_of: String, typed: String, file_name: String, bytes: Vec<u8>) -> Result<(), FfiError> {
        let code = morpheus_core::clean_shortcode(&typed).or_else(|| morpheus_core::shortcode_from_file_name(&file_name));
        let (Some(code), Some(mime)) = (code, room_dialogs::picture_mime(&file_name)) else {
            return Err(failed(i18n::t("Choose a picture (PNG, JPEG, GIF or WebP) and give it a name.")));
        };
        Ok(self.session()?.add_emote(Some(room_id(&room_id_of)?), String::new(), code, mime.to_owned(), bytes).await?)
    }

    pub async fn rename_room_emoji(&self, room_id_of: String, old: String, new: String) -> Result<(), FfiError> {
        Ok(self.session()?.rename_emote(Some(room_id(&room_id_of)?), String::new(), old, new).await?)
    }

    pub async fn remove_room_emoji(&self, room_id_of: String, shortcode: String) -> Result<(), FfiError> {
        Ok(self.session()?.remove_emote(Some(room_id(&room_id_of)?), String::new(), shortcode).await?)
    }
}
