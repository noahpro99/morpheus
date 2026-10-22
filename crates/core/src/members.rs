use matrix_sdk::{
    RoomMemberships,
    room::RoomMemberRole,
    ruma::{
        Int,
        events::{
            StateEventType,
            ignored_user_list::IgnoredUserListEventContent,
            room::{member::MembershipState, power_levels::UserPowerLevel},
        },
    },
};

use crate::{CoreFuture, Error, MediaRef, OwnedRoomId, OwnedUserId, Session};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub user_id: OwnedUserId,
    pub name: String,
    pub avatar: Option<MediaRef>,
    pub role: MemberRole,
    /// Raw power level, for "can I act on this person" checks.
    pub power_level: i64,
    /// Whether the signed-in user has blocked (ignored) this member.
    pub is_ignored: bool,
}

/// Coarse role buckets, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MemberRole {
    Admin,
    Moderator,
    Member,
}

impl MemberRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Admin => "Admins",
            Self::Moderator => "Moderators",
            Self::Member => "Members",
        }
    }

    /// The power level that grants this role in a default room.
    pub fn power_level(self) -> i64 {
        match self {
            Self::Admin => 100,
            Self::Moderator => 50,
            Self::Member => 0,
        }
    }

    pub(crate) fn from_sdk(role: RoomMemberRole) -> Self {
        match role {
            RoomMemberRole::Creator | RoomMemberRole::Administrator => Self::Admin,
            RoomMemberRole::Moderator => Self::Moderator,
            _ => Self::Member,
        }
    }
}

/// What the signed-in user may do to other members of a room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MemberPermissions {
    pub power_level: i64,
    pub can_kick: bool,
    pub can_ban: bool,
    pub can_invite: bool,
    /// May change other members' power levels.
    pub can_change_roles: bool,
}

impl MemberPermissions {
    /// Moderation only works downwards: you cannot act on someone at or
    /// above your own level, and the server rejects it anyway.
    pub fn outranks(&self, member: &Member) -> bool {
        self.power_level > member.power_level
    }
}

impl Session {
    /// Joined members of a room, admins first, then alphabetical.
    pub fn members(&self, room_id: OwnedRoomId) -> CoreFuture<Vec<Member>> {
        let client = self.client.clone();
        self.core.run(async move {
            let room = client
                .get_room(&room_id)
                .ok_or_else(|| Error::UnknownRoom(room_id.clone()))?;
            let mut members: Vec<Member> = room
                .members(RoomMemberships::JOIN)
                .await?
                .into_iter()
                .map(|member| Member {
                    user_id: member.user_id().to_owned(),
                    name: member.name().to_owned(),
                    avatar: member
                        .avatar_url()
                        .map(|uri| MediaRef::from_mxc(uri.to_owned())),
                    role: MemberRole::from_sdk(member.suggested_role_for_power_level()),
                    power_level: power_level_of(&member),
                    is_ignored: member.is_ignored(),
                })
                .collect();
            members.sort_by(|a, b| {
                a.role
                    .cmp(&b.role)
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
            Ok(members)
        })
    }

    /// The signed-in user's moderation powers in a room.
    pub fn my_permissions(&self, room_id: OwnedRoomId) -> CoreFuture<MemberPermissions> {
        let client = self.client.clone();
        self.core.run(async move {
            let room = client
                .get_room(&room_id)
                .ok_or_else(|| Error::UnknownRoom(room_id.clone()))?;
            let me = client.user_id().ok_or(Error::NoSession)?;
            let Some(member) = room.get_member(me).await? else {
                return Ok(MemberPermissions::default());
            };
            Ok(MemberPermissions {
                power_level: power_level_of(&member),
                can_kick: member.can_kick(),
                can_ban: member.can_ban(),
                can_invite: member.can_invite(),
                can_change_roles: member.can_send_state(StateEventType::RoomPowerLevels),
            })
        })
    }

    pub fn kick(
        &self,
        room_id: OwnedRoomId,
        user_id: OwnedUserId,
        reason: Option<String>,
    ) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let room = client
                .get_room(&room_id)
                .ok_or_else(|| Error::UnknownRoom(room_id.clone()))?;
            room.kick_user(&user_id, reason.as_deref()).await?;
            settle_membership(&room, &user_id, MembershipState::Leave).await;
            Ok(())
        })
    }

    pub fn ban(
        &self,
        room_id: OwnedRoomId,
        user_id: OwnedUserId,
        reason: Option<String>,
    ) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let room = client
                .get_room(&room_id)
                .ok_or_else(|| Error::UnknownRoom(room_id.clone()))?;
            room.ban_user(&user_id, reason.as_deref()).await?;
            settle_membership(&room, &user_id, MembershipState::Ban).await;
            Ok(())
        })
    }

    pub fn unban(&self, room_id: OwnedRoomId, user_id: OwnedUserId) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let room = client
                .get_room(&room_id)
                .ok_or_else(|| Error::UnknownRoom(room_id.clone()))?;
            room.unban_user(&user_id, None).await?;
            settle_membership(&room, &user_id, MembershipState::Leave).await;
            Ok(())
        })
    }

    /// Invite a user to a room.
    pub fn invite(&self, room_id: OwnedRoomId, user_id: OwnedUserId) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let room = client
                .get_room(&room_id)
                .ok_or_else(|| Error::UnknownRoom(room_id.clone()))?;
            room.invite_user_by_id(&user_id).await?;
            Ok(())
        })
    }

    /// Give a member a role by setting the matching power level.
    pub fn set_role(
        &self,
        room_id: OwnedRoomId,
        user_id: OwnedUserId,
        role: MemberRole,
    ) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let room = client
                .get_room(&room_id)
                .ok_or_else(|| Error::UnknownRoom(room_id.clone()))?;
            let level = Int::new_saturating(role.power_level());
            room.update_power_levels(vec![(&user_id, level)]).await?;
            settle_power_level(&room, &user_id, role.power_level()).await;
            Ok(())
        })
    }

    /// Block (ignore) a user account-wide: their messages disappear everywhere.
    pub fn block(&self, user_id: OwnedUserId, blocked: bool) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            if blocked {
                client.account().ignore_user(&user_id).await?;
            } else {
                client.account().unignore_user(&user_id).await?;
            }
            settle_ignored(&client, &user_id, blocked).await;
            Ok(())
        })
    }

    /// The DM room with a user, created if there is none yet.
    pub fn dm_with(&self, user_id: OwnedUserId) -> CoreFuture<OwnedRoomId> {
        let client = self.client.clone();
        self.core.run(async move {
            if let Some(room) = client.get_dm_room(&user_id) {
                // From the server if need be: a device just signed in has not
                // loaded the room's members yet.
                let theirs = room
                    .get_member(&user_id)
                    .await
                    .ok()
                    .flatten()
                    .map(|member| member.membership().clone());
                match theirs {
                    // They left: ask them back, as messaging them means to.
                    Some(MembershipState::Leave) => {
                        room.invite_user_by_id(&user_id).await?;
                        return Ok(room.room_id().to_owned());
                    }
                    // Banned: a fresh conversation.
                    Some(MembershipState::Ban) => {}
                    // In it, invited, or not known yet (a DM just made).
                    _ => return Ok(room.room_id().to_owned()),
                }
            }
            let room = client.create_dm(&user_id).await?;
            Ok(room.room_id().to_owned())
        })
    }
}

fn power_level_of(member: &matrix_sdk::room::RoomMember) -> i64 {
    match member.power_level() {
        // Room creators in v12+ rooms outrank everyone.
        UserPowerLevel::Infinite => i64::MAX,
        UserPowerLevel::Int(level) => i64::from(level),
        _ => 0,
    }
}

/// Requests return once the server accepts them, but the local store only
/// learns the result from the next sync. Callers refetch right after an
/// action, so wait (briefly) until the store agrees before returning.
const SETTLE_DEADLINE: std::time::Duration = std::time::Duration::from_secs(8);
const SETTLE_POLL: std::time::Duration = std::time::Duration::from_millis(100);

/// Wait until the member's local membership state is one of `wanted`.
async fn settle_membership(
    room: &matrix_sdk::Room,
    user_id: &matrix_sdk::ruma::UserId,
    wanted: MembershipState,
) {
    let deadline = tokio::time::Instant::now() + SETTLE_DEADLINE;
    while tokio::time::Instant::now() < deadline {
        let now = match room.get_member_no_sync(user_id).await {
            Ok(Some(member)) => member.membership().clone(),
            Ok(None) => MembershipState::Leave,
            Err(_) => MembershipState::Leave,
        };
        if now == wanted {
            return;
        }
        tokio::time::sleep(SETTLE_POLL).await;
    }
    tracing::debug!(%user_id, "membership change not reflected locally before the deadline");
}

/// Wait until the member's local power level equals `level`.
async fn settle_power_level(
    room: &matrix_sdk::Room,
    user_id: &matrix_sdk::ruma::UserId,
    level: i64,
) {
    let deadline = tokio::time::Instant::now() + SETTLE_DEADLINE;
    while tokio::time::Instant::now() < deadline {
        if matches!(room.get_member_no_sync(user_id).await, Ok(Some(m)) if power_level_of(&m) == level)
        {
            return;
        }
        tokio::time::sleep(SETTLE_POLL).await;
    }
    tracing::debug!(%user_id, "power level change not reflected locally before the deadline");
}

/// Wait until the ignore list locally agrees with `blocked`.
async fn settle_ignored(
    client: &matrix_sdk::Client,
    user_id: &matrix_sdk::ruma::UserId,
    blocked: bool,
) {
    let deadline = tokio::time::Instant::now() + SETTLE_DEADLINE;
    while tokio::time::Instant::now() < deadline {
        if is_ignored(client, user_id).await == blocked {
            return;
        }
        tokio::time::sleep(SETTLE_POLL).await;
    }
    tracing::debug!(%user_id, "ignore list change not reflected locally before the deadline");
}

pub(crate) async fn is_ignored(
    client: &matrix_sdk::Client,
    user_id: &matrix_sdk::ruma::UserId,
) -> bool {
    client
        .account()
        .account_data::<IgnoredUserListEventContent>()
        .await
        .ok()
        .flatten()
        .and_then(|raw| raw.deserialize().ok())
        .is_some_and(|content| content.ignored_users.contains_key(user_id))
}
