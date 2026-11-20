//! Who is in a room and who is typing, a person's card, and a room's own
//! settings (how it notifies, leaving it). Lines and grouping are
//! `morpheus-view-model` (`typing`, `members`, `profile`).

use std::sync::Arc;

use morpheus_core::{MemberRole, NotifyMode, OwnedUserId};
use morpheus_view_model::{
    members::{self, Row},
    profile::role_tag,
    typing,
};

use crate::{FfiError, MediaHandle, Morpheus, RoomTimeline, TaskHandle, runtime};

fn failed(reason: impl Into<String>) -> FfiError {
    FfiError::Failed { reason: reason.into() }
}

/// Told who is typing (never me): the line to show, or `None` when nobody is.
#[uniffi::export(with_foreign)]
pub trait TypingListener: Send + Sync {
    fn on_typing(&self, line: Option<String>);
}

/// A row of the member list.
#[derive(uniffi::Enum)]
pub enum MemberRow {
    /// "Admins — 2".
    Heading { title: String },
    Member { member: MemberItem },
}

#[derive(uniffi::Record)]
pub struct MemberItem {
    pub user_id: String,
    pub name: String,
    pub avatar: Option<Arc<MediaHandle>>,
    /// "Admin", "Moderator" or "Member".
    pub role: String,
}

/// A person's card.
#[derive(uniffi::Record)]
pub struct ProfileCard {
    pub user_id: String,
    pub name: String,
    pub avatar: Option<Arc<MediaHandle>>,
    pub status: Option<String>,
    /// "Admin" or "Moderator" in the room asked from; nothing for a member.
    pub role: Option<String>,
    /// "3 mutual rooms".
    pub mutual_heading: String,
    /// The names of those rooms.
    pub mutual_rooms: Vec<String>,
    pub is_friend: bool,
    pub is_me: bool,
}

fn user(id: &str) -> Result<OwnedUserId, FfiError> {
    OwnedUserId::try_from(id).map_err(|_| failed(format!("{id} is not a Matrix ID")))
}

fn mode_key(mode: NotifyMode) -> &'static str {
    match mode {
        NotifyMode::AllMessages => "all",
        NotifyMode::MentionsOnly => "mentions",
        NotifyMode::Mute => "mute",
    }
}

#[uniffi::export]
impl RoomTimeline {
    /// Hear who is typing here.
    pub async fn watch_typing(&self, listener: Arc<dyn TypingListener>) -> Result<Arc<TaskHandle>, FfiError> {
        let mut updates = self.session.typing(self.room_id.clone()).await?;
        let task = runtime().spawn(async move {
            while let Some(names) = updates.next().await {
                listener.on_typing(typing::line(&names));
            }
        });
        Ok(TaskHandle::new(task))
    }

    /// The person is typing: tell the room (call on every keystroke; it expires by itself).
    pub async fn renew_typing(&self) -> Result<(), FfiError> {
        Ok(self.handle.renew_typing().await?)
    }

    /// The person stopped (sent, or cleared the box).
    pub async fn stop_typing(&self) -> Result<(), FfiError> {
        Ok(self.handle.stop_typing().await?)
    }

    /// Everyone in the room under their role's heading: admins, moderators, members.
    pub async fn members(&self) -> Result<Vec<MemberRow>, FfiError> {
        let list = self.session.members(self.room_id.clone()).await?;
        Ok(members::rows(&list)
            .into_iter()
            .map(|row| match row {
                Row::Heading { role, total } => MemberRow::Heading { title: members::Row::heading(role, total) },
                Row::Member(i) => {
                    let m = &list[i];
                    MemberRow::Member {
                        member: MemberItem {
                            user_id: m.user_id.to_string(),
                            name: m.name.clone(),
                            avatar: m.avatar.clone().map(|a| MediaHandle::new(a, &self.session)),
                            role: members::role_name(m.role).to_owned(),
                        },
                    }
                }
            })
            .collect())
    }

    /// How this room notifies: `all`, `mentions` or `mute`.
    pub async fn notify_mode(&self) -> Result<String, FfiError> {
        Ok(mode_key(self.session.notify_mode(self.room_id.clone()).await?).to_owned())
    }

    /// Choose how this room notifies: `all`, `mentions` or `mute`.
    pub async fn set_notify_mode(&self, mode: String) -> Result<(), FfiError> {
        let mode = match mode.as_str() {
            "all" => NotifyMode::AllMessages,
            "mentions" => NotifyMode::MentionsOnly,
            "mute" => NotifyMode::Mute,
            other => return Err(failed(format!("{other} is not a notification choice"))),
        };
        Ok(self.session.set_notify_mode(self.room_id.clone(), mode).await?)
    }

    /// The card of someone in this room.
    pub async fn profile(&self, user_id: String) -> Result<ProfileCard, FfiError> {
        let who = user(&user_id)?;
        let session = &self.session;
        let profile = session.profile(who.clone()).await?;
        let mine = session.user_id();
        let role = session
            .members_known(self.room_id.clone())
            .await?
            .members
            .iter()
            .find(|m| m.user_id == who)
            .map(|m| m.role)
            .filter(|role| *role != MemberRole::Member)
            .map(|role| role_tag(role).to_owned());
        let mutual: Vec<String> = session.mutual_rooms(who.clone()).await?.into_iter().map(|(_, name)| name).collect();
        let is_friend = session.friends().await?.contains(&who);
        Ok(ProfileCard {
            user_id: who.to_string(),
            name: profile.name,
            avatar: profile.avatar.map(|a| MediaHandle::new(a, session)),
            status: profile.status,
            role,
            mutual_heading: format!("Mutual rooms — {}", mutual.len()),
            mutual_rooms: mutual,
            is_friend,
            is_me: mine.as_ref() == Some(&who),
        })
    }
}

#[uniffi::export]
impl Morpheus {
    /// Leave a room (the list drops it).
    pub async fn leave_room(&self, room_id: String) -> Result<(), FfiError> {
        let id = morpheus_core::OwnedRoomId::try_from(room_id.as_str()).map_err(|_| failed(format!("{room_id} is not a room id")))?;
        Ok(self.session()?.leave_room(id).await?)
    }
}
