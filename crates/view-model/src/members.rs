//! The member list (`crates/ui/src/components/members.rs`): joined members
//! grouped by power level (admins, moderators, members) with a count per
//! group, how many show at first, and the menu a member's row offers
//! (gated by what the signed-in user may do in the room). The front end
//! draws the groups and fetches the list.

use morpheus_core::{Member, MemberPermissions, MemberRole, OwnedRoomId, OwnedUserId};

use super::{
    menu::{MenuAction, MenuItem},
    room_dialogs::RoomDialogKind,
};

/// Rows shown at first. Big rooms have thousands of members, and every
/// shown row fetches a picture, so the list grows only on request.
pub const PAGE: usize = 100;

/// A role's name in a menu ("Admin").
pub fn role_name(role: MemberRole) -> &'static str {
    match role {
        MemberRole::Admin => "Admin",
        MemberRole::Moderator => "Moderator",
        MemberRole::Member => "Member",
    }
}

/// One group of the list: a role, how many have it, and those shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub role: MemberRole,
    /// "Admins".
    pub label: &'static str,
    /// Everyone in the room with the role, shown or not.
    pub total: usize,
    pub members: Vec<Member>,
}

impl Group {
    /// The group's heading: "Admins — 2".
    pub fn heading(&self) -> String {
        format!("{} — {}", self.label, self.total)
    }
}

/// The list as drawn: groups in order admins, moderators, members (empty
/// ones left out) and how many members are not shown yet. `list` is in the
/// core's order (by role, then name); the first `shown` rows of it show.
pub fn groups(list: &[Member], shown: usize) -> (Vec<Group>, usize) {
    let visible = &list[..list.len().min(shown)];
    let hidden = list.len() - visible.len();
    let groups = [MemberRole::Admin, MemberRole::Moderator, MemberRole::Member]
        .into_iter()
        .filter_map(|role| {
            let total = list.iter().filter(|m| m.role == role).count();
            (total > 0).then(|| Group {
                role,
                label: role.label(),
                total,
                members: visible.iter().filter(|m| m.role == role).cloned().collect(),
            })
        })
        .collect();
    (groups, hidden)
}

/// "Show 100 more (250 hidden)".
pub fn show_more_label(hidden: usize) -> String {
    format!("Show {} more ({hidden} hidden)", hidden.min(PAGE))
}

/// What a member's menu depends on besides the member.
pub struct MemberMenuInput<'a> {
    pub room: &'a OwnedRoomId,
    pub member: &'a Member,
    pub permissions: MemberPermissions,
    /// The member is the signed-in user.
    pub own: bool,
    pub friend: bool,
    /// Where the person can be invited from here: id, name.
    pub invite_to: Vec<(OwnedRoomId, String)>,
}

/// The right-click menu for a member: everything Discord offers that Matrix
/// has an equivalent for, gated by the signed-in user's power level.
pub fn member_menu(input: &MemberMenuInput) -> Vec<MenuItem> {
    let MemberMenuInput {
        room,
        member,
        permissions,
        own,
        friend,
        invite_to,
    } = input;
    let user: OwnedUserId = member.user_id.clone();
    let mut items = vec![MenuItem::new("Profile", MenuAction::Profile(user.clone()))];
    let copy = MenuItem::new("Copy user ID", MenuAction::CopyId(user.to_string()));
    if *own {
        items.push(copy);
        return items;
    }
    items.push(MenuItem::new(
        if *friend { "Remove friend" } else { "Add friend" },
        MenuAction::Friend(user.clone(), !friend),
    ));
    items.push(MenuItem::new(
        "Set nickname…",
        MenuAction::RoomDialog(RoomDialogKind::Nickname(user.clone(), member.name.clone())),
    ));
    items.push(MenuItem::new("Mention", MenuAction::Mention(member.name.clone())));
    items.push(MenuItem::new("Message", MenuAction::Message(user.clone())));
    let targets: Vec<(OwnedRoomId, String)> = invite_to
        .iter()
        .filter(|(id, _)| id != *room)
        .cloned()
        .collect();
    if !targets.is_empty() {
        let children = targets
            .into_iter()
            .map(|(id, name)| {
                MenuItem::new(name.clone(), MenuAction::Invite(id, user.clone())).avatar(name)
            })
            .collect();
        items.push(MenuItem::submenu("Invite to server", children).icon("user-plus"));
    }
    items.push(MenuItem::new(
        if member.is_ignored { "Unblock" } else { "Block" },
        MenuAction::Block(user.clone(), !member.is_ignored),
    ));
    let outranks = permissions.outranks(member);
    if permissions.can_change_roles && outranks {
        let roles = [MemberRole::Member, MemberRole::Moderator, MemberRole::Admin]
            .into_iter()
            .filter(|role| role.power_level() <= permissions.power_level)
            .map(|role| {
                MenuItem::new(
                    role_name(role),
                    MenuAction::SetRole((*room).clone(), user.clone(), role),
                )
                .checked(role == member.role)
            })
            .collect();
        items.push(MenuItem::submenu("Roles", roles));
    }
    if permissions.can_kick && outranks {
        items.push(
            MenuItem::new(
                format!("Kick {}", member.name),
                MenuAction::Kick((*room).clone(), user.clone()),
            )
            .danger(),
        );
    }
    if permissions.can_ban && outranks {
        items.push(
            MenuItem::new(
                format!("Ban {}", member.name),
                MenuAction::Ban((*room).clone(), user.clone()),
            )
            .danger(),
        );
    }
    items.push(copy);
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(name: &str, role: MemberRole, power: i64) -> Member {
        Member {
            user_id: OwnedUserId::try_from(format!("@{name}:x")).expect("user id"),
            name: name.into(),
            avatar: None,
            role,
            power_level: power,
            is_ignored: false,
        }
    }

    fn room() -> OwnedRoomId {
        OwnedRoomId::try_from("!r:x").expect("room id")
    }

    fn list() -> Vec<Member> {
        vec![
            member("alice", MemberRole::Admin, 100),
            member("carol", MemberRole::Moderator, 50),
            member("bob", MemberRole::Member, 0),
            member("dave", MemberRole::Member, 0),
        ]
    }

    fn labels(items: &[MenuItem]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_str()).collect()
    }

    fn admin() -> MemberPermissions {
        MemberPermissions {
            power_level: 100,
            can_kick: true,
            can_ban: true,
            can_invite: true,
            can_change_roles: true,
        }
    }

    fn input<'a>(
        room: &'a OwnedRoomId,
        member: &'a Member,
        permissions: MemberPermissions,
    ) -> MemberMenuInput<'a> {
        MemberMenuInput {
            room,
            member,
            permissions,
            own: false,
            friend: false,
            invite_to: Vec::new(),
        }
    }

    #[test]
    fn groups_run_admins_moderators_members_with_counts_and_no_empty_ones() {
        let (groups, hidden) = groups(&list(), PAGE);
        assert_eq!(hidden, 0);
        let headings: Vec<_> = groups.iter().map(Group::heading).collect();
        assert_eq!(headings, ["Admins — 1", "Moderators — 1", "Members — 2"]);
        let only_members = [member("bob", MemberRole::Member, 0)];
        let (groups, _) = super::groups(&only_members, PAGE);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].role, MemberRole::Member);
    }

    #[test]
    fn a_long_list_shows_a_prefix_and_counts_what_is_hidden() {
        let mut many = vec![member("boss", MemberRole::Admin, 100)];
        many.extend((0..250).map(|i| member(&format!("m{i:03}"), MemberRole::Member, 0)));
        let (shown, hidden) = groups(&many, PAGE);
        assert_eq!(hidden, 151);
        assert_eq!(shown[0].members.len(), 1);
        assert_eq!(shown[1].members.len(), PAGE - 1);
        assert_eq!(shown[1].total, 250, "the count is of everyone");
        assert_eq!(show_more_label(hidden), "Show 100 more (151 hidden)");
        assert_eq!(show_more_label(7), "Show 7 more (7 hidden)");
    }

    #[test]
    fn role_names_are_singular() {
        assert_eq!(role_name(MemberRole::Admin), "Admin");
        assert_eq!(role_name(MemberRole::Moderator), "Moderator");
        assert_eq!(role_name(MemberRole::Member), "Member");
    }

    #[test]
    fn your_own_row_offers_only_profile_and_the_id() {
        let me = member("alice", MemberRole::Admin, 100);
        let room = room();
        let items = member_menu(&MemberMenuInput {
            own: true,
            ..input(&room, &me, admin())
        });
        assert_eq!(labels(&items), ["Profile", "Copy user ID"]);
    }

    #[test]
    fn a_plain_member_cannot_moderate() {
        let bob = member("bob", MemberRole::Member, 0);
        let room = room();
        let items = member_menu(&input(
            &room,
            &member("carol", MemberRole::Moderator, 50),
            MemberPermissions::default(),
        ));
        assert_eq!(
            labels(&items),
            [
                "Profile",
                "Add friend",
                "Set nickname…",
                "Mention",
                "Message",
                "Block",
                "Copy user ID"
            ]
        );
        let _ = bob;
    }

    #[test]
    fn an_admin_can_change_roles_kick_and_ban_those_below() {
        let bob = member("bob", MemberRole::Member, 0);
        let room = room();
        let items = member_menu(&input(&room, &bob, admin()));
        assert_eq!(
            labels(&items),
            [
                "Profile",
                "Add friend",
                "Set nickname…",
                "Mention",
                "Message",
                "Block",
                "Roles",
                "Kick bob",
                "Ban bob",
                "Copy user ID"
            ]
        );
        let roles = items.iter().find(|i| i.label == "Roles").expect("roles");
        assert_eq!(labels(&roles.children), ["Member", "Moderator", "Admin"]);
        assert!(roles.children[0].checked, "bob is a member");
        assert!(items.iter().find(|i| i.label == "Kick bob").is_some_and(|i| i.danger));
    }

    #[test]
    fn moderation_only_works_downwards_and_roles_stop_at_your_level() {
        let room = room();
        // A moderator cannot act on an equal or an admin.
        let moderator = MemberPermissions {
            power_level: 50,
            ..admin()
        };
        let peer = member("carol", MemberRole::Moderator, 50);
        let boss = member("alice", MemberRole::Admin, 100);
        for target in [&peer, &boss] {
            let items = member_menu(&input(&room, target, moderator));
            let names = labels(&items);
            assert!(!names.iter().any(|l| l.starts_with("Kick") || l.starts_with("Ban") || *l == "Roles"));
        }
        // Over a plain member, roles go up to the moderator's own level.
        let bob = member("bob", MemberRole::Member, 0);
        let items = member_menu(&input(&room, &bob, moderator));
        let roles = items.iter().find(|i| i.label == "Roles").expect("roles");
        assert_eq!(labels(&roles.children), ["Member", "Moderator"]);
    }

    #[test]
    fn friends_blocked_people_and_invites_change_the_rows() {
        let mut bob = member("bob", MemberRole::Member, 0);
        bob.is_ignored = true;
        let room = room();
        let other = OwnedRoomId::try_from("!s:x").expect("id");
        let items = member_menu(&MemberMenuInput {
            friend: true,
            invite_to: vec![(room.clone(), "here".into()), (other.clone(), "Engineering".into())],
            ..input(&room, &bob, MemberPermissions::default())
        });
        let names = labels(&items);
        assert!(names.contains(&"Remove friend"));
        assert!(names.contains(&"Unblock"));
        let invite = items.iter().find(|i| i.label == "Invite to server").expect("invite");
        assert_eq!(labels(&invite.children), ["Engineering"], "not into this room itself");
        assert_eq!(
            invite.children[0].action,
            Some(MenuAction::Invite(other, bob.user_id.clone()))
        );
    }
}
