//! What the phone's pages for the long-press sheet's items do: invite people, create and edit a room, a room's privacy and
//! encryption, a server's overview, reporting a message. What each form accepts and says is `view_model::room_dialogs`,
//! as on the desktop.

use std::sync::Arc;

use morpheus_core::{History, OwnedEventId, OwnedRoomId, Privacy};
use morpheus_view_model::{i18n, room_dialogs};

use crate::{FfiError, MediaHandle, Morpheus};

fn failed(reason: impl Into<String>) -> FfiError {
    FfiError::Failed { reason: reason.into() }
}

fn room_id(id: &str) -> Result<OwnedRoomId, FfiError> {
    OwnedRoomId::try_from(id).map_err(|_| failed(format!("{id} is not a room id")))
}

fn note_error(note: room_dialogs::Note) -> FfiError {
    failed(i18n::t(note.text()))
}

/// A room's name, topic and picture, as the edit page starts.
#[derive(uniffi::Record)]
pub struct RoomDetails {
    pub name: String,
    pub topic: String,
    pub avatar: Option<Arc<MediaHandle>>,
    pub is_space: bool,
}

/// One choice of a form: what it is called in code and what it says.
#[derive(uniffi::Record)]
pub struct Choice {
    pub key: String,
    pub label: String,
}

/// Who can join, how much history newcomers see, and whether the room is encrypted.
#[derive(uniffi::Record)]
pub struct PrivacyInfo {
    pub public: bool,
    /// `shared`, `invited` or `joined`.
    pub history: String,
    /// `None` while it is not known.
    pub encrypted: Option<bool>,
    /// A space has no encryption.
    pub has_encryption: bool,
    /// A DM has no join rule or history to choose.
    pub has_join_and_history: bool,
    /// What the encryption line says (and the warning before turning it on).
    pub encryption_note: String,
    pub encryption_warning: String,
    pub join_choices: Vec<Choice>,
    pub history_choices: Vec<Choice>,
}

/// A channel of a server, as its overview lists it.
#[derive(uniffi::Record)]
pub struct OverviewChannel {
    pub id: String,
    pub name: String,
    pub topic: Option<String>,
    pub voice: bool,
    /// You are in it (it can be opened); else it can be joined.
    pub joined: bool,
    pub via: Vec<String>,
}

/// A server's overview: its picture, name, topic, how many are in it and its channels.
#[derive(uniffi::Record)]
pub struct Overview {
    pub name: String,
    pub topic: Option<String>,
    pub avatar: Option<Arc<MediaHandle>>,
    pub members: u64,
    pub channels: Vec<OverviewChannel>,
}

fn history_key(history: History) -> &'static str {
    match history {
        History::Shared => "shared",
        History::Invited => "invited",
        History::Joined => "joined",
    }
}

#[uniffi::export]
impl Morpheus {
    /// Invite whoever `typed` names to a room or server; says so in words, or says why not.
    pub async fn invite_people(&self, room_id_of: String, typed: String) -> Result<String, FfiError> {
        let user = room_dialogs::parse_invitee(&typed).map_err(note_error)?;
        self.session()?.invite(room_id(&room_id_of)?, user.clone()).await?;
        Ok(room_dialogs::invited_note(&user).text().to_owned())
    }

    /// Make a room (a channel) with this name and topic, in a server when `space` names one. Gives its ID.
    pub async fn create_channel_in(&self, name: String, topic: String, space: Option<String>) -> Result<String, FfiError> {
        if !room_dialogs::can_create(&name) {
            return Err(failed(i18n::t("A room needs a name.")));
        }
        let space = space.filter(|s| !s.is_empty()).map(|s| room_id(&s)).transpose()?;
        Ok(self.session()?.create_room(name, topic, space).await?.to_string())
    }

    pub async fn room_details(&self, room_id_of: String) -> Result<RoomDetails, FfiError> {
        let session = self.session()?;
        let summary = self.rooms_now.lock().ok().and_then(|rooms| rooms.iter().find(|r| r.id.as_str() == room_id_of).cloned());
        let summary = summary.ok_or_else(|| failed("that room is not in the list"))?;
        Ok(RoomDetails {
            name: summary.name,
            topic: summary.topic.unwrap_or_default(),
            avatar: summary.avatar.map(|m| MediaHandle::new(m, &session)),
            is_space: summary.is_space,
        })
    }

    /// Rename a room or server and change its topic.
    pub async fn edit_room(&self, room_id_of: String, name: String, topic: String) -> Result<(), FfiError> {
        if !room_dialogs::can_create(&name) {
            return Err(failed(i18n::t("A room needs a name.")));
        }
        Ok(self.session()?.edit_room(room_id(&room_id_of)?, Some(name), Some(topic)).await?)
    }

    /// Make a picture (a file's bytes, named for its type) the room's or server's picture.
    pub async fn set_room_picture(&self, room_id_of: String, file_name: String, bytes: Vec<u8>) -> Result<(), FfiError> {
        if room_dialogs::picture_mime(&file_name).is_none() {
            return Err(failed(i18n::t(room_dialogs::NOT_A_PICTURE)));
        }
        let path = self.dir.join(format!("room-picture-{}", file_name.replace('/', "_")));
        std::fs::write(&path, bytes).map_err(|e| failed(e.to_string()))?;
        let done = self.session()?.set_room_avatar(room_id(&room_id_of)?, path.clone()).await;
        let _ = std::fs::remove_file(&path);
        Ok(done?)
    }

    pub async fn privacy_info(&self, room_id_of: String) -> Result<PrivacyInfo, FfiError> {
        let session = self.session()?;
        let id = room_id(&room_id_of)?;
        let rooms = self.rooms_now.lock().map(|r| r.clone()).unwrap_or_default();
        let parts = room_dialogs::privacy_parts(&rooms, &id);
        let privacy = session.privacy(id.clone()).await?;
        let encrypted = if parts.encryption { session.is_encrypted(id).await.ok() } else { None };
        Ok(PrivacyInfo {
            public: privacy.public,
            history: history_key(privacy.history).to_owned(),
            encrypted,
            has_encryption: parts.encryption,
            has_join_and_history: parts.join_and_history,
            encryption_note: match encrypted {
                Some(true) => format!("{} {}", i18n::t(room_dialogs::ENCRYPTION_ON), i18n::t(room_dialogs::ENCRYPTION_PERMANENT)),
                Some(false) => i18n::t(room_dialogs::ENCRYPTION_OFF),
                None => String::new(),
            },
            encryption_warning: i18n::t(room_dialogs::ENCRYPTION_WARNING),
            join_choices: room_dialogs::JOIN_CHOICES
                .iter()
                .map(|(public, label)| Choice { key: if *public { "public" } else { "invite" }.to_owned(), label: i18n::t(label) })
                .collect(),
            history_choices: room_dialogs::HISTORY_CHOICES
                .iter()
                .map(|(history, label)| Choice { key: history_key(*history).to_owned(), label: i18n::t(label) })
                .collect(),
        })
    }

    /// Who can join (`public` or `invite`) and how much history they see (`shared`, `invited`, `joined`).
    pub async fn set_privacy(&self, room_id_of: String, join: String, history: String) -> Result<(), FfiError> {
        let history = match history.as_str() {
            "shared" => History::Shared,
            "invited" => History::Invited,
            "joined" => History::Joined,
            other => return Err(failed(format!("{other} is not a history choice"))),
        };
        Ok(self.session()?.set_privacy(room_id(&room_id_of)?, Privacy { public: join == "public", history }).await?)
    }

    /// Turn encryption on (it cannot be turned off again).
    pub async fn turn_on_encryption(&self, room_id_of: String) -> Result<(), FfiError> {
        Ok(self.session()?.enable_encryption(room_id(&room_id_of)?).await?)
    }

    /// Tell the homeserver's admins about a message.
    pub async fn report_message(&self, room_id_of: String, event_id: String, reason: String) -> Result<(), FfiError> {
        let event = OwnedEventId::try_from(event_id.as_str()).map_err(|_| failed(format!("{event_id} is not an event id")))?;
        Ok(self.session()?.report(room_id(&room_id_of)?, event, reason).await?)
    }

    /// A server's overview (its welcome screen on the desktop).
    pub async fn server_overview(&self, space_id: String) -> Result<Overview, FfiError> {
        let session = self.session()?;
        let id = room_id(&space_id)?;
        let (summary, joined) = {
            let rooms = self.rooms_now.lock().map_err(|_| failed("busy"))?;
            (
                rooms.iter().find(|r| r.id == id).cloned().ok_or_else(|| failed("that server is not in the list"))?,
                rooms.iter().filter(|r| !r.invited).map(|r| r.id.clone()).collect::<Vec<_>>(),
            )
        };
        let channels = session
            .space_rooms(id)
            .await?
            .into_iter()
            .filter(|room| !room.is_space)
            .map(|room| OverviewChannel {
                joined: joined.contains(&room.id),
                id: room.id.to_string(),
                name: room.name,
                topic: room.topic,
                voice: room.is_voice,
                via: room.via,
            })
            .collect();
        Ok(Overview {
            name: summary.name,
            topic: summary.topic,
            avatar: summary.avatar.map(|m| MediaHandle::new(m, &session)),
            members: summary.member_count,
            channels,
        })
    }

    /// Join a channel of a server from its overview.
    pub async fn join_channel(&self, room_id_of: String, via: Vec<String>) -> Result<(), FfiError> {
        Ok(self.session()?.join_room_via(room_id(&room_id_of)?, via).await?)
    }

    /// Break a rail folder up: its servers stay, in its place.
    pub async fn ungroup_folder(&self, folder_id: String) -> Result<(), FfiError> {
        self.change_rail(|prefs| morpheus_view_model::rail_folders::ungroup(prefs, &folder_id)).await
    }
}

/// What leaving a server says, and how many rooms go with it.
#[derive(uniffi::Record)]
pub struct LeaveInfo {
    pub title: String,
    pub body: String,
}

impl Morpheus {
    /// The server and the rooms listed in it: what leaving or a per-server profile applies to.
    fn server_rooms(&self, space: &OwnedRoomId) -> Vec<OwnedRoomId> {
        let rooms = self.rooms_now.lock().map(|r| r.clone()).unwrap_or_default();
        let mut ids = vec![space.clone()];
        ids.extend(
            morpheus_view_model::scopes::rooms_in_scope(&rooms, &morpheus_view_model::scopes::Scope::Space(space.clone())).into_iter().map(|room| room.id),
        );
        ids
    }
}

#[uniffi::export]
impl Morpheus {
    /// The question of "Leave server".
    pub fn leave_server_info(&self, space_id: String) -> Result<LeaveInfo, FfiError> {
        let id = room_id(&space_id)?;
        let name = self.rooms_now.lock().ok().and_then(|rooms| rooms.iter().find(|r| r.id == id).map(|r| r.name.clone()));
        let count = self.server_rooms(&id).len().saturating_sub(1);
        Ok(LeaveInfo { title: room_dialogs::leave_space_title(name.as_deref()), body: room_dialogs::leave_space_body(count) })
    }

    /// Leave a server and every room listed in it.
    pub async fn leave_server(&self, space_id: String) -> Result<(), FfiError> {
        let id = room_id(&space_id)?;
        let children: Vec<OwnedRoomId> = self.server_rooms(&id).into_iter().filter(|room| room != &id).collect();
        Ok(self.session()?.leave_space(id, children).await?)
    }

    /// What a server is, in words.
    pub fn create_server_blurb(&self) -> String {
        i18n::t(room_dialogs::create_space_blurb(false))
    }

    /// Make a server (a space of its own, in no other) with this name. Gives its ID.
    pub async fn create_server(&self, name: String) -> Result<String, FfiError> {
        if !room_dialogs::can_create(&name) {
            return Err(failed(i18n::t("A server needs a name.")));
        }
        Ok(self.session()?.create_space(name, None).await?.to_string())
    }

    /// What a category is, in words.
    pub fn create_category_blurb(&self) -> String {
        i18n::t(room_dialogs::create_space_blurb(true))
    }

    /// Make a category (a space inside the server) with this name. Gives its ID.
    pub async fn create_category(&self, space_id: String, name: String) -> Result<String, FfiError> {
        if !room_dialogs::can_create(&name) {
            return Err(failed(i18n::t("A room needs a name.")));
        }
        Ok(self.session()?.create_space(name, Some(room_id(&space_id)?)).await?.to_string())
    }

    /// How a per-server profile is explained, and the name you have in the server now.
    pub async fn server_profile(&self, space_id: String) -> Result<ServerProfile, FfiError> {
        let id = room_id(&space_id)?;
        let blurb = room_dialogs::room_profile_blurb(self.server_rooms(&id).len());
        let profile = self.session()?.room_profile(id).await?;
        Ok(ServerProfile { blurb, name: profile.name })
    }

    /// Save your name (and, when `picture_bytes` is not empty, a picture, cut already) for a server and its rooms.
    pub async fn save_server_profile(&self, space_id: String, name: String, picture_name: String, picture_bytes: Vec<u8>) -> Result<(), FfiError> {
        let id = room_id(&space_id)?;
        let targets = self.server_rooms(&id);
        let path = if picture_bytes.is_empty() {
            None
        } else {
            let path = self.dir.join(format!("server-profile-{}", picture_name.replace('/', "_")));
            std::fs::write(&path, picture_bytes).map_err(|e| failed(e.to_string()))?;
            Some(path)
        };
        let done = self.session()?.set_room_profile(targets, name, path.clone()).await;
        if let Some(path) = path {
            let _ = std::fs::remove_file(path);
        }
        Ok(done?)
    }
}

/// The per-server profile page: its explanation and your name there now.
#[derive(uniffi::Record)]
pub struct ServerProfile {
    pub blurb: String,
    pub name: String,
}

/// One pinned message of a room, as the pinned list shows it (#413).
#[derive(uniffi::Record)]
pub struct PinnedItem {
    pub event_id: String,
    /// Who sent it, as a short name; `None` when the message could not be fetched.
    pub sender: Option<String>,
    /// The text, or a short description of what is not text.
    pub body: String,
}

#[uniffi::export]
impl Morpheus {
    /// The room's pinned messages, the newest pin first (the desktop's pinned list).
    pub async fn pinned_messages(&self, room_id_of: String) -> Result<Vec<PinnedItem>, FfiError> {
        let pins = self.session()?.pinned_messages(room_id(&room_id_of)?).await?;
        Ok(pins
            .into_iter()
            .rev()
            .map(|pin| PinnedItem { event_id: pin.event_id.to_string(), sender: pin.sender.map(|s| s.localpart().to_owned()), body: pin.body })
            .collect())
    }

    /// Take a message off the room's pins.
    pub async fn unpin_message(&self, room_id_of: String, event_id: String) -> Result<(), FfiError> {
        let event = OwnedEventId::try_from(event_id.as_str()).map_err(|_| failed(format!("{event_id} is not an event id")))?;
        Ok(self.session()?.pin(room_id(&room_id_of)?, event, false).await?)
    }
}

#[uniffi::export]
impl Morpheus {
    /// Whether this server has not been welcomed yet (its overview shows the first time it is opened, as on the desktop); asking marks it welcomed.
    pub fn take_welcome(&self, space_id: String) -> bool {
        let user = self.account().map(|a| a.user_id).unwrap_or_default();
        if user.is_empty() || morpheus_view_model::last_place::is_welcomed(&self.dir, &user, &space_id) {
            return false;
        }
        let _ = morpheus_view_model::last_place::mark_welcomed(&self.dir, &user, &space_id);
        true
    }
}
