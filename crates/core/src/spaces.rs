//! Space ("server") level actions and the account-wide UI preferences blob.

use matrix_sdk::ruma::{
    OwnedRoomId, OwnedUserId,
    api::client::room::create_room,
    events::{
        AnyGlobalAccountDataEventContent, GlobalAccountDataEventType, InitialStateEvent,
        room::{
            history_visibility::{HistoryVisibility, RoomHistoryVisibilityEventContent},
            join_rules::{JoinRule, RoomJoinRulesEventContent},
            member::RoomMemberEventContent,
        },
        space::{child::SpaceChildEventContent, parent::SpaceParentEventContent},
    },
    room::RoomType,
    serde::Raw,
};
use serde_json::Value;

use crate::{CoreFuture, Error, MediaRef, Session};

/// Who can join and how much history newcomers see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Privacy {
    pub public: bool,
    pub history: History,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum History {
    /// Anyone who ever joins sees everything (Matrix "shared").
    Shared,
    /// From the moment they were invited.
    Invited,
    /// From the moment they joined.
    Joined,
}

/// Your name and avatar as members of one room see them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomProfile {
    pub name: String,
    pub avatar: Option<MediaRef>,
}

const UI_PREFS_TYPE: &str = "dev.morpheus.ui";

impl Session {
    pub fn privacy(&self, room_id: OwnedRoomId) -> CoreFuture<Privacy> {
        let room = self.room(&room_id);
        self.core.run(async move {
            let room = room?;
            let public = matches!(room.join_rule(), Some(JoinRule::Public));
            let history = match room.history_visibility_or_default() {
                HistoryVisibility::Joined => History::Joined,
                HistoryVisibility::Invited => History::Invited,
                _ => History::Shared,
            };
            Ok(Privacy { public, history })
        })
    }

    pub fn set_privacy(&self, room_id: OwnedRoomId, privacy: Privacy) -> CoreFuture<()> {
        let room = self.room(&room_id);
        self.core.run(async move {
            let room = room?;
            let rule = if privacy.public {
                JoinRule::Public
            } else {
                JoinRule::Invite
            };
            room.send_state_event(RoomJoinRulesEventContent::new(rule))
                .await?;
            let visibility = match privacy.history {
                History::Shared => HistoryVisibility::Shared,
                History::Invited => HistoryVisibility::Invited,
                History::Joined => HistoryVisibility::Joined,
            };
            room.send_state_event(RoomHistoryVisibilityEventContent::new(visibility.clone()))
                .await?;
            // Settle: the dialog re-reads right after saving.
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(8);
            while tokio::time::Instant::now() < deadline {
                let rule_ok = matches!(room.join_rule(), Some(JoinRule::Public)) == privacy.public;
                if rule_ok && room.history_visibility_or_default() == visibility {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            Ok(())
        })
    }

    /// Upload an image and make it the room's (or space's) avatar.
    pub fn set_room_avatar(
        &self,
        room_id: OwnedRoomId,
        path: std::path::PathBuf,
    ) -> CoreFuture<()> {
        let room = self.room(&room_id);
        self.core.run(async move {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            let bytes = tokio::fs::read(&path).await?;
            room?.upload_avatar(&mime, bytes, None).await?;
            Ok(())
        })
    }

    /// How you appear in one room (falls back to your global profile).
    pub fn room_profile(&self, room_id: OwnedRoomId) -> CoreFuture<RoomProfile> {
        let room = self.room(&room_id);
        let me = self.user_id();
        self.core.run(async move {
            let me = me.ok_or(Error::NoSession)?;
            let member = room?.get_member(&me).await?;
            Ok(match member {
                Some(member) => RoomProfile {
                    name: member.name().to_owned(),
                    avatar: member
                        .avatar_url()
                        .map(|u| MediaRef::from_mxc(u.to_owned())),
                },
                None => RoomProfile {
                    name: me.localpart().to_owned(),
                    avatar: None,
                },
            })
        })
    }

    /// Set a per-room display name and, optionally, a per-room avatar from a
    /// file. Applied to every room in `rooms` (a space and its children).
    pub fn set_room_profile(
        &self,
        rooms: Vec<OwnedRoomId>,
        name: String,
        avatar: Option<std::path::PathBuf>,
    ) -> CoreFuture<()> {
        let client = self.client.clone();
        let me = self.user_id();
        self.core.run(async move {
            let me: OwnedUserId = me.ok_or(Error::NoSession)?;
            let name = name.trim().to_owned();
            // Upload once, reuse the URI in every room.
            let avatar_uri = match avatar {
                Some(path) => {
                    let mime = mime_guess::from_path(&path).first_or_octet_stream();
                    let bytes = tokio::fs::read(&path).await?;
                    Some(client.media().upload(&mime, bytes, None).await?.content_uri)
                }
                None => None,
            };
            let all = rooms.clone();
            for room_id in rooms {
                let Some(room) = client.get_room(&room_id) else { continue };
                let Some(member) = room.get_member(&me).await? else { continue };
                let mut content = RoomMemberEventContent::new(member.membership().clone());
                content.displayname = (!name.is_empty()).then(|| name.clone());
                content.avatar_url = match &avatar_uri {
                    Some(uri) => Some(uri.clone()),
                    None => member.avatar_url().map(ToOwned::to_owned),
                };
                room.send_state_event_for_key(&me, content).await?;
            }
            // Wait for every room to reflect the new name locally, so a member
            // list refetched right away shows it.
            if !name.is_empty() {
                let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
                for room_id in all {
                    let Some(room) = client.get_room(&room_id) else { continue };
                    while tokio::time::Instant::now() < deadline {
                        if matches!(room.get_member_no_sync(&me).await, Ok(Some(m)) if m.name() == name) {
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                }
            }
            Ok(())
        })
    }

    /// Create a space, optionally nested under `parent` (a Discord-style
    /// "category").
    pub fn create_space(
        &self,
        name: String,
        parent: Option<OwnedRoomId>,
    ) -> CoreFuture<OwnedRoomId> {
        let client = self.client.clone();
        self.core.run(async move {
            let server = client
                .user_id()
                .ok_or(Error::NoSession)?
                .server_name()
                .to_owned();
            let mut request = create_room::v3::Request::new();
            request.name = Some(name.trim().to_owned());
            let mut creation = create_room::v3::CreationContent::new();
            creation.room_type = Some(RoomType::Space);
            request.creation_content = Some(Raw::new(&creation)?);
            request.preset = Some(create_room::v3::RoomPreset::PublicChat);
            if let Some(parent_id) = &parent {
                let link = InitialStateEvent::new(
                    parent_id.clone(),
                    SpaceParentEventContent::new(vec![server.clone()]),
                );
                request.initial_state = vec![link.to_raw_any()];
            }
            let space = client.create_room(request).await?;
            if let Some(parent_id) = parent
                && let Some(parent_room) = client.get_room(&parent_id)
            {
                parent_room
                    .send_state_event_for_key(
                        space.room_id(),
                        SpaceChildEventContent::new(vec![server]),
                    )
                    .await?;
            }
            Ok(space.room_id().to_owned())
        })
    }

    /// Leave a space and every room listed with it.
    pub fn leave_space(&self, space: OwnedRoomId, rooms: Vec<OwnedRoomId>) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            for room_id in rooms.into_iter().chain(std::iter::once(space)) {
                if let Some(room) = client.get_room(&room_id) {
                    room.leave().await?;
                }
            }
            Ok(())
        })
    }

    /// The UI preferences blob (rail order, folders, hidden muted rooms…),
    /// synced through account data so every device sees the same layout.
    pub fn ui_prefs(&self) -> CoreFuture<Value> {
        let client = self.client.clone();
        self.core.run(async move {
            let raw = client
                .account()
                .account_data_raw(GlobalAccountDataEventType::from(UI_PREFS_TYPE))
                .await?;
            Ok(raw
                .and_then(|raw| raw.deserialize_as::<Value>().ok())
                .unwrap_or_else(|| Value::Object(Default::default())))
        })
    }

    pub fn set_ui_prefs(&self, prefs: Value) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let raw: Raw<AnyGlobalAccountDataEventContent> =
                Raw::from_json(serde_json::value::to_raw_value(&prefs)?);
            client
                .account()
                .set_account_data_raw(GlobalAccountDataEventType::from(UI_PREFS_TYPE), raw)
                .await?;
            Ok(())
        })
    }
}
