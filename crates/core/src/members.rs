use matrix_sdk::{RoomMemberships, room::RoomMemberRole};

use crate::{CoreFuture, Error, MediaRef, OwnedRoomId, OwnedUserId, Session};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub user_id: OwnedUserId,
    pub name: String,
    pub avatar: Option<MediaRef>,
    pub role: MemberRole,
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
                    role: match member.suggested_role_for_power_level() {
                        RoomMemberRole::Creator | RoomMemberRole::Administrator => {
                            MemberRole::Admin
                        }
                        RoomMemberRole::Moderator => MemberRole::Moderator,
                        _ => MemberRole::Member,
                    },
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
}
