//! The member list (`crates/ui/src/components/members.rs`): joined members
//! grouped by power level (admins, moderators, members) with a count per
//! group as the rows the list draws, and the menu a member's row offers
//! (gated by what the signed-in user may do in the room). The front end
//! draws the groups and fetches the list.

use morpheus_core::{Member, MemberPermissions, MemberRole, OwnedRoomId, OwnedUserId};

use super::{
    menu::{MenuAction, MenuItem},
    room_dialogs::RoomDialogKind,
};

/// A role's name in a menu ("Admin").
pub fn role_name(role: MemberRole) -> &'static str {
    match role {
        MemberRole::Admin => "Admin",
        MemberRole::Moderator => "Moderator",
        MemberRole::Member => "Member",
    }
}

/// One row of the list as drawn: a role's heading, or a member (by their
/// place in the list). The list draws only the rows on screen, so a room of
/// thousands costs a screenful (#115).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// "Admins — 2": the role and how many have it.
    Heading { role: MemberRole, total: usize },
    Member(usize),
}

impl Row {
    /// A heading's text: "Admins — 2".
    pub fn heading(role: MemberRole, total: usize) -> String {
        let name = match role {
            MemberRole::Admin => crate::i18n::t("Admins"),
            MemberRole::Moderator => crate::i18n::t("Moderators"),
            MemberRole::Member => crate::i18n::t("Members"),
        };
        format!("{name} — {}", crate::i18n::number(total as u64))
    }
}

/// The list's rows: each role in order admins, moderators, members (empty
/// ones left out) under its heading, its members in `list`'s order (the
/// core's: by name). Worked out once per list, not per frame; as more of a
/// room's members arrive the list is a new one, and so are its rows.
pub fn rows(list: &[Member]) -> Vec<Row> {
    let mut rows = Vec::with_capacity(list.len() + 3);
    for role in [MemberRole::Admin, MemberRole::Moderator, MemberRole::Member] {
        let at = rows.len();
        rows.extend(
            list.iter()
                .enumerate()
                .filter(|(_, m)| m.role == role)
                .map(|(i, _)| Row::Member(i)),
        );
        let total = rows.len() - at;
        if total > 0 {
            rows.insert(at, Row::Heading { role, total });
        }
    }
    rows
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

/// The chat keeps at least this much beside the member panel (#512).
pub const MIN_CHAT_WITH_PANEL: f32 = super::sidebar::MIN_CHAT;

/// Whether the member panel has room: the window less the rail, the channel list and the
/// panel still leaves the chat its minimum width. When it does not, the panel hides and its
/// button is disabled; the user's own on/off choice is kept and applies again when it fits.
pub fn panel_fits(window: f32, rail: f32, sidebar: f32, panel: f32) -> bool {
    window - rail - sidebar - panel >= MIN_CHAT_WITH_PANEL
}

/// Whether the panel is shown: the user wants it and it fits.
pub fn panel_shown(wanted: bool, fits: bool) -> bool {
    wanted && fits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_needs_the_chat_its_minimum_width() {
        assert!(panel_fits(952.0, 72.0, 248.0, 232.0));
        assert!(!panel_fits(951.0, 72.0, 248.0, 232.0));
        assert!(panel_fits(1400.0, 72.0, 248.0, 232.0));
        assert!(!panel_fits(800.0, 72.0, 248.0, 232.0));
    }

    #[test]
    fn the_choice_is_kept_while_it_does_not_fit() {
        assert!(!panel_shown(true, false));
        assert!(panel_shown(true, true));
        assert!(!panel_shown(false, true));
    }

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
    fn rows_run_admins_moderators_members_under_counted_headings() {
        let rows = rows(&list());
        assert_eq!(
            rows,
            [
                Row::Heading { role: MemberRole::Admin, total: 1 },
                Row::Member(0),
                Row::Heading { role: MemberRole::Moderator, total: 1 },
                Row::Member(1),
                Row::Heading { role: MemberRole::Member, total: 2 },
                Row::Member(2),
                Row::Member(3),
            ]
        );
        assert_eq!(Row::heading(MemberRole::Member, 2), "Members — 2");
        let only_members = [member("bob", MemberRole::Member, 0)];
        assert_eq!(
            super::rows(&only_members),
            [Row::Heading { role: MemberRole::Member, total: 1 }, Row::Member(0)],
            "no heading for a role nobody has"
        );
        assert!(super::rows(&[]).is_empty());
    }

    #[test]
    fn every_member_of_a_big_list_has_a_row_in_its_group() {
        let mut many: Vec<Member> = (0..3000)
            .map(|i| member(&format!("m{i:04}"), MemberRole::Member, 0))
            .collect();
        // Out of role order: a moderator among the members.
        many.insert(1500, member("mod", MemberRole::Moderator, 50));
        let rows = rows(&many);
        assert_eq!(rows.len(), 3001 + 2);
        assert_eq!(rows[0], Row::Heading { role: MemberRole::Moderator, total: 1 });
        assert_eq!(rows[1], Row::Member(1500));
        assert_eq!(rows[2], Row::Heading { role: MemberRole::Member, total: 3000 });
        assert_eq!(rows[3], Row::Member(0));
        assert_eq!(rows[3002], Row::Member(3000), "the last one last");
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
