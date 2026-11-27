//! A person's profile on the phone: the note only you see, the card's "..." menu (full profile, share, invite,
//! block, report, copy: view_model::profile::more_menu), the full profile page and reporting someone.

use std::sync::Arc;

use morpheus_core::{OwnedRoomId, OwnedUserId};
use morpheus_view_model::{nicknames, profile, room_menu::available_invite_targets};

use crate::{FfiError, MediaHandle, Morpheus, menus::MenuRow};

fn user(id: &str) -> Result<OwnedUserId, FfiError> {
    OwnedUserId::try_from(id).map_err(|_| FfiError::Failed { reason: format!("{id} is not a Matrix ID") })
}

/// The full profile of someone.
#[derive(uniffi::Record)]
pub struct FullProfile {
    pub user_id: String,
    pub name: String,
    pub avatar: Option<Arc<MediaHandle>>,
    pub status: Option<String>,
    pub is_me: bool,
    /// "Mutual servers — 2".
    pub mutual_heading: String,
    /// The servers you both are in, by name.
    pub servers: Vec<String>,
    /// The link to the profile (what Share copies).
    pub link: String,
}

#[uniffi::export]
impl Morpheus {
    /// The card of anyone, from anywhere (#488): with the room it was opened from, their role there shows too.
    pub async fn profile_card(&self, room_id: Option<String>, user_id: String) -> Result<crate::ProfileCard, FfiError> {
        let session = self.session()?;
        let who = user(&user_id)?;
        let found = session.profile(who.clone()).await?;
        let role = match room_id {
            Some(room) => {
                let id = OwnedRoomId::try_from(room.as_str()).map_err(|_| FfiError::Failed { reason: format!("{room} is not a room id") })?;
                session
                    .members_known(id)
                    .await?
                    .members
                    .iter()
                    .find(|m| m.user_id == who)
                    .map(|m| m.role)
                    .filter(|role| *role != morpheus_core::MemberRole::Member)
                    .map(|role| profile::role_tag(role).to_owned())
            }
            None => None,
        };
        let own = session.user_id().as_ref() == Some(&who);
        let mutual: Vec<String> = session.mutual_rooms(who.clone()).await?.into_iter().map(|(_, name)| name).collect();
        let is_friend = session.friends().await?.contains(&who);
        Ok(crate::ProfileCard {
            user_id: who.to_string(),
            name: nicknames::shown_name(&self.nicks.snapshot(), who.as_str(), &found.name),
            avatar: found.avatar.map(|a| MediaHandle::new(a, &session)),
            status: found.status,
            role,
            mutual_heading: morpheus_view_model::i18n::tf("Mutual rooms — %{count}", &[("count", &mutual.len().to_string())]),
            mutual_rooms: mutual,
            is_friend,
            is_me: own,
        })
    }

    /// The note you keep about someone (empty for none).
    pub async fn note_of(&self, user_id: String) -> Result<String, FfiError> {
        Ok(self.session()?.note(user(&user_id)?).await?)
    }

    /// Keep this note about someone (empty takes it away).
    pub async fn set_note(&self, user_id: String, note: String) -> Result<(), FfiError> {
        Ok(self.session()?.set_note(user(&user_id)?, note).await?)
    }

    /// The card's "..." menu: the full profile, share, invite to a server, block, report, copying.
    pub async fn profile_more_rows(&self, user_id: String) -> Result<Vec<MenuRow>, FfiError> {
        let session = self.session()?;
        let who = user(&user_id)?;
        let own = session.user_id().as_ref() == Some(&who);
        let blocked = session.is_blocked(who.clone()).await.unwrap_or(false);
        let mutual: Vec<OwnedRoomId> = session.mutual_spaces(who.clone()).await.unwrap_or_default();
        let rooms = self.rooms_now.lock().map(|r| r.clone()).unwrap_or_default();
        let invite_to = available_invite_targets(&rooms, None, &mutual);
        Ok(self.store_menu(&profile::more_menu(&who, own, blocked, &invite_to)))
    }

    /// Their profile page: picture, name, ID, status and the servers you share.
    pub async fn full_profile(&self, user_id: String) -> Result<FullProfile, FfiError> {
        let session = self.session()?;
        let who = user(&user_id)?;
        let found = session.profile(who.clone()).await?;
        let own = session.user_id().as_ref() == Some(&who);
        let rooms = self.rooms_now.lock().map(|r| r.clone()).unwrap_or_default();
        let mutual = if own { Vec::new() } else { session.mutual_spaces(who.clone()).await.unwrap_or_default() };
        let servers: Vec<String> = profile::shared_servers(&rooms, &mutual).into_iter().map(|s| s.name).collect();
        Ok(FullProfile {
            name: nicknames::shown_name(&self.nicks.snapshot(), who.as_str(), &found.name),
            avatar: found.avatar.map(|m| MediaHandle::new(m, &session)),
            status: found.status.filter(|s| !s.trim().is_empty()),
            is_me: own,
            mutual_heading: profile::mutual_heading(servers.len()),
            servers,
            link: format!("https://matrix.to/#/{who}"),
            user_id: who.to_string(),
        })
    }

    /// Tell the server's admins about someone's profile; says so in words when it went out.
    pub async fn report_user(&self, user_id: String, reason: String) -> Result<String, FfiError> {
        self.session()?.report_user(user(&user_id)?, reason).await?;
        Ok(morpheus_view_model::i18n::t("Reported to the server admins."))
    }
}
