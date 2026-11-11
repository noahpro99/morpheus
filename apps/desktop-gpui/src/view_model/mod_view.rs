//! The mod view (`crates/ui/src/components/mod_view.rs`), after Discord's:
//! one sheet for a person, from their profile card. Every room you share is
//! listed with where they stand there (role, invited, banned, gone) and the
//! moderation you may do in each: a role, kick, ban, unban. Moderation only
//! works downwards, so rooms where they outrank you list them without
//! controls. What each row shows is decided here; no toolkit in here.

use morpheus_core::{MemberRole, ModRoom, Standing};

/// The roles offered, highest first, with the name each goes by.
pub const ROLES: [(MemberRole, &str); 3] = [
    (MemberRole::Admin, "Admin"),
    (MemberRole::Moderator, "Moderator"),
    (MemberRole::Member, "Member"),
];

pub fn role_name(role: MemberRole) -> &'static str {
    ROLES
        .iter()
        .find(|(candidate, _)| *candidate == role)
        .map_or("Member", |(_, name)| name)
}

/// What a room's row shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowView {
    /// Where they stand, in words (also the row's `data-standing`).
    pub state: &'static str,
    /// A choice of roles (those below yours, and their own), when you may
    /// change roles here: the roles listed and which is theirs.
    pub roles: Option<(Vec<(MemberRole, &'static str)>, MemberRole)>,
    pub kick: bool,
    pub ban: bool,
    pub unban: bool,
}

pub fn row_view(room: &ModRoom) -> RowView {
    let mine = room.mine;
    let below = room.outranked();
    let state = match room.standing {
        Standing::Member(role) => role_name(role),
        Standing::Invited => "Invited",
        Standing::Banned => "Banned",
        Standing::Gone => "Not in the room",
    };
    let present = matches!(room.standing, Standing::Member(_) | Standing::Invited);
    let roles = match room.standing {
        Standing::Member(role) if below && mine.can_change_roles => Some((
            ROLES
                .iter()
                .filter(|(option, _)| *option == role || option.power_level() < mine.power_level)
                .copied()
                .collect(),
            role,
        )),
        _ => None,
    };
    let banned = room.standing == Standing::Banned;
    RowView {
        state,
        roles,
        kick: below && present && mine.can_kick,
        ban: below && !banned && mine.can_ban,
        unban: below && banned && mine.can_ban,
    }
}

/// The reason for a kick or ban: none when only spaces.
pub fn given_reason(reason: &str) -> Option<String> {
    Some(reason.trim().to_owned()).filter(|reason| !reason.is_empty())
}

pub const NO_ROOMS: &str = "You share no rooms with them.";
pub const LOOKING: &str = "Looking through your rooms…";

#[cfg(test)]
mod tests {
    use super::*;
    use morpheus_core::MemberPermissions;

    fn room(standing: Standing, theirs: i64, mine_level: i64, mod_rights: bool) -> ModRoom {
        ModRoom {
            room_id: "!r:x".try_into().unwrap(),
            name: "general".into(),
            standing,
            power_level: theirs,
            mine: MemberPermissions {
                power_level: mine_level,
                can_kick: mod_rights,
                can_ban: mod_rights,
                can_invite: true,
                can_change_roles: mod_rights,
            },
        }
    }

    #[test]
    fn an_admin_sees_the_roles_below_hers_and_kick_and_ban() {
        let view = row_view(&room(Standing::Member(MemberRole::Member), 0, 100, true));
        assert_eq!(view.state, "Member");
        let (roles, current) = view.roles.unwrap();
        assert_eq!(current, MemberRole::Member);
        let names: Vec<_> = roles.iter().map(|(_, name)| *name).collect();
        assert_eq!(names, ["Moderator", "Member"]);
        assert!(view.kick && view.ban && !view.unban);
    }

    #[test]
    fn someone_who_outranks_you_has_no_controls() {
        let view = row_view(&room(Standing::Member(MemberRole::Admin), 100, 50, true));
        assert_eq!(view.roles, None);
        assert!(!view.kick && !view.ban && !view.unban);
        assert_eq!(view.state, "Admin");
    }

    #[test]
    fn a_banned_person_can_be_unbanned_not_kicked_or_banned() {
        let view = row_view(&room(Standing::Banned, 0, 100, true));
        assert_eq!(view.state, "Banned");
        assert!(view.unban && !view.ban && !view.kick);
    }

    #[test]
    fn someone_who_left_can_only_be_banned_and_without_rights_nothing_shows() {
        let gone = row_view(&room(Standing::Gone, 0, 100, true));
        assert_eq!(gone.state, "Not in the room");
        assert!(gone.ban && !gone.kick);
        let invited = row_view(&room(Standing::Invited, 0, 100, true));
        assert!(invited.kick);
        let powerless = row_view(&room(Standing::Member(MemberRole::Member), 0, 100, false));
        assert!(powerless.roles.is_none() && !powerless.kick && !powerless.ban);
    }

    #[test]
    fn reasons_are_trimmed_and_may_be_empty() {
        assert_eq!(given_reason("  spam "), Some("spam".into()));
        assert_eq!(given_reason("   "), None);
    }
}
