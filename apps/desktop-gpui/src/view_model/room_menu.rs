//! The menus of rooms, direct messages and servers
//! (`crates/ui/src/components/{room_list,space_menu}.rs`): which items a
//! room offers (an unread one offers "Mark as read"), what a DM offers for
//! the person in it (friend or not, ignored, blocked, muted), and a
//! server's.

use morpheus_core::{NotifyMode, OwnedRoomId, OwnedUserId, RoomSummary};

use super::{
    marks::MUTE_CHOICES,
    menu::{MenuAction, MenuItem},
    room_dialogs::RoomDialogKind,
};

/// The three notification modes, as a submenu with the current one checked.
fn modes(rooms: &[OwnedRoomId], current: Option<NotifyMode>) -> Vec<MenuItem> {
    [
        (NotifyMode::AllMessages, "All messages"),
        (NotifyMode::MentionsOnly, "Only mentions"),
        (NotifyMode::Mute, "Mute"),
    ]
    .into_iter()
    .map(|(mode, label)| {
        MenuItem::new(label, MenuAction::Notify(rooms.to_vec(), mode)).checked(current == Some(mode))
    })
    .collect()
}

/// Mark as unread, or (when it is already) as read.
fn read_item(room: &RoomSummary) -> MenuItem {
    if room.unread_messages > 0 || room.marked_unread {
        MenuItem::new("Mark as read", MenuAction::MarkRead(vec![room.id.clone()]))
    } else {
        MenuItem::new("Mark as unread", MenuAction::MarkRoomUnread(room.id.clone()))
    }
}

fn copy_room_id(room: &RoomSummary) -> MenuItem {
    MenuItem::new("Copy room ID", MenuAction::CopyId(room.id.to_string()))
}

fn privacy_item(room: &RoomSummary) -> MenuItem {
    MenuItem::new(
        "Privacy & encryption",
        MenuAction::RoomDialog(RoomDialogKind::Privacy(room.id.clone())),
    )
}

/// The right-click menu for a room row. `mode`: its notification mode, if known.
pub fn room_menu(room: &RoomSummary, mode: Option<NotifyMode>) -> Vec<MenuItem> {
    let mut items = vec![read_item(room)];
    if !room.is_dm {
        items.push(MenuItem::new(
            "Invite people",
            MenuAction::RoomDialog(RoomDialogKind::Invite(room.id.clone())),
        ));
    }
    items.push(MenuItem::submenu(
        "Notifications",
        modes(std::slice::from_ref(&room.id), mode),
    ));
    if !room.is_dm {
        items.push(MenuItem::new(
            "Edit room",
            MenuAction::RoomDialog(RoomDialogKind::Edit(room.id.clone())),
        ));
        items.push(MenuItem::new(
            "Create room",
            MenuAction::RoomDialog(RoomDialogKind::Create),
        ));
    }
    items.push(privacy_item(room));
    items.push(MenuItem::new(
        "Copy link",
        MenuAction::CopyLink(format!("https://matrix.to/#/{}", room.id)),
    ));
    items.push(copy_room_id(room));
    items.push(MenuItem::new("Leave room", MenuAction::Leave(room.id.clone(), room.name.clone())).danger());
    items
}

/// What a DM's menu depends on besides the room.
pub struct DmState {
    pub user: OwnedUserId,
    /// What the person is called in the menu (a nickname if there is one).
    pub name: String,
    pub pinned: bool,
    pub ignored: bool,
    pub blocked: bool,
    pub muted: bool,
    pub friend: bool,
    /// The servers (and loose rooms) the person can be invited to: id, name.
    pub invite_to: Vec<(OwnedRoomId, String)>,
}

/// Where a person can be invited from a DM's menu: every server (space)
/// you are in, then the rooms that belong to no server (id, name).
pub fn invite_targets(rooms: &[RoomSummary]) -> Vec<(OwnedRoomId, String)> {
    let servers = rooms.iter().filter(|room| room.is_space && !room.invited);
    let loose = rooms.iter().filter(|room| {
        !room.is_space && !room.is_dm && !room.invited && room.parent_spaces.is_empty()
    });
    servers
        .chain(loose)
        .map(|room| (room.id.clone(), room.name.clone()))
        .collect()
}

/// The menu of a DM, as Discord's: what you do with the conversation and
/// with the person in it.
pub fn dm_menu(room: &RoomSummary, dm: &DmState) -> Vec<MenuItem> {
    let mut items = vec![read_item(room)];
    items.push(MenuItem::new(
        if dm.pinned { "Unpin" } else { "Pin" },
        MenuAction::PinDm(room.id.clone(), !dm.pinned),
    ));
    items.push(MenuItem::new(
        if room.favorite {
            "Remove from favorites"
        } else {
            "Add to favorites"
        },
        MenuAction::Favorite(room.id.clone(), !room.favorite),
    ));
    items.push(MenuItem::new("Profile", MenuAction::Profile(dm.user.clone())));
    if dm.friend {
        items.push(MenuItem::new(
            "Change friend nickname",
            MenuAction::RoomDialog(RoomDialogKind::Nickname(dm.user.clone(), dm.name.clone())),
        ));
    }
    items.push(MenuItem::new("Close DM", MenuAction::CloseDm(room.id.clone())));
    if !dm.invite_to.is_empty() {
        let targets = dm
            .invite_to
            .iter()
            .map(|(id, name)| {
                MenuItem::new(name.clone(), MenuAction::Invite(id.clone(), dm.user.clone()))
                    .avatar(name.clone())
            })
            .collect();
        items.push(MenuItem::submenu("Invite to server", targets).icon("user-plus"));
    }
    items.push(MenuItem::new(
        if dm.friend {
            "Remove friend"
        } else {
            "Add friend"
        },
        MenuAction::Friend(dm.user.clone(), !dm.friend),
    ));
    items.push(MenuItem::new(
        if dm.ignored { "Unignore" } else { "Ignore" },
        MenuAction::Ignore(dm.user.clone(), !dm.ignored),
    ));
    items.push(
        MenuItem::new(
            if dm.blocked { "Unblock" } else { "Block" },
            MenuAction::Block(dm.user.clone(), !dm.blocked),
        )
        .danger(),
    );
    if dm.muted {
        items.push(MenuItem::new(
            format!("Unmute @{}", dm.name),
            MenuAction::Unmute(room.id.clone()),
        ));
    } else {
        let choices = MUTE_CHOICES
            .iter()
            .map(|(label, length)| MenuItem::new(*label, MenuAction::Mute(room.id.clone(), *length)))
            .collect();
        items.push(MenuItem::submenu(format!("Mute @{}", dm.name), choices));
    }
    items.push(privacy_item(room));
    items.push(MenuItem::new(
        "Copy user ID",
        MenuAction::CopyId(dm.user.to_string()),
    ));
    items.push(copy_room_id(room));
    items
}

/// The menu of a server (space): what clicking its name or right-clicking
/// its badge offers. `children`: its rooms. `hide_muted`: the current
/// "Hide muted channels" setting.
/// `folders`: the rail's folders, as (id, name, whether the server is in it).
pub fn space_menu(
    space: &RoomSummary,
    children: &[RoomSummary],
    hide_muted: bool,
    folders: &[(String, String, bool)],
) -> Vec<MenuItem> {
    let ids: Vec<OwnedRoomId> = children.iter().map(|r| r.id.clone()).collect();
    let all_muted = !children.is_empty() && children.iter().all(|r| r.muted);
    let mut items = vec![
        MenuItem::new("Mark as read", MenuAction::MarkRead(ids.clone())),
        MenuItem::new("Invite to server", MenuAction::RoomDialog(RoomDialogKind::Invite(space.id.clone())))
            .icon("user-plus"),
        MenuItem::new(
            if all_muted { "Unmute server" } else { "Mute server" },
            MenuAction::Notify(
                ids.clone(),
                if all_muted {
                    NotifyMode::AllMessages
                } else {
                    NotifyMode::Mute
                },
            ),
        )
        .icon(if all_muted { "bell" } else { "bell-off" }),
    ];
    let notifications = modes(&ids, None)
        .into_iter()
        .map(|item| {
            let muted = matches!(item.action, Some(MenuAction::Notify(_, NotifyMode::Mute)));
            item.checked(muted && all_muted)
        })
        .collect();
    items.push(MenuItem::submenu("Notifications", notifications));
    items.push(
        MenuItem::new("Hide muted channels", MenuAction::ToggleHideMuted)
            .checked(hide_muted)
            .icon("eye-off"),
    );
    items.push(
        MenuItem::new("Server settings", MenuAction::RoomDialog(RoomDialogKind::Edit(space.id.clone()))).icon("settings"),
    );
    items.push(
        MenuItem::new("Privacy settings", MenuAction::RoomDialog(RoomDialogKind::Privacy(space.id.clone())))
            .icon("lock"),
    );
    items.push(
        MenuItem::new(
            "Edit per-server profile",
            MenuAction::RoomDialog(RoomDialogKind::RoomProfile(space.id.clone())),
        )
        .icon("user-cog"),
    );
    items.push(MenuItem::new("Create channel", MenuAction::CreateChannel(space.id.clone())).icon("plus"));
    items.push(
        MenuItem::new("Create category", MenuAction::RoomDialog(RoomDialogKind::CreateSpace(Some(space.id.clone()))))
            .icon("folder-plus"),
    );
    // Move to a folder (or out of one).
    let mut targets: Vec<MenuItem> = folders
        .iter()
        .map(|(id, name, inside)| {
            MenuItem::new(name.clone(), MenuAction::MoveToFolder(space.id.clone(), id.clone()))
                .checked(*inside)
                .icon("folder")
        })
        .collect();
    targets.push(
        MenuItem::new("New folder", MenuAction::MoveToNewFolder(space.id.clone())).icon("folder-plus"),
    );
    targets.push(
        MenuItem::new("No folder", MenuAction::MoveOutOfFolders(space.id.clone())).icon("move"),
    );
    items.push(MenuItem::submenu("Move to", targets).icon("move"));
    items.push(
        MenuItem::new("Leave server", MenuAction::RoomDialog(RoomDialogKind::LeaveSpace(space.id.clone()))).danger(),
    );
    items.push(
        MenuItem::submenu(
            "Copy server info",
            vec![
                MenuItem::new("Copy server ID", MenuAction::CopyId(space.id.to_string())).icon("hash"),
                MenuItem::new(
                    "Copy invite link",
                    MenuAction::CopyLink(format!("https://matrix.to/#/{}", space.id)),
                )
                .icon("link"),
            ],
        )
        .icon("info"),
    );
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::room;

    fn labels(items: &[MenuItem]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_str()).collect()
    }

    fn user() -> OwnedUserId {
        OwnedUserId::try_from("@bob:x").expect("user id")
    }

    fn dm_state() -> DmState {
        DmState {
            user: user(),
            name: "bob".into(),
            pinned: false,
            ignored: false,
            blocked: false,
            muted: false,
            friend: false,
            invite_to: Vec::new(),
        }
    }

    #[test]
    fn a_room_menu_offers_read_or_unread_by_its_state() {
        let mut r = room("!r:x", "random");
        assert_eq!(room_menu(&r, None)[0].label, "Mark as unread");
        r.unread_messages = 2;
        assert_eq!(room_menu(&r, None)[0].label, "Mark as read");
        r.unread_messages = 0;
        r.marked_unread = true;
        assert_eq!(room_menu(&r, None)[0].label, "Mark as read");
    }

    #[test]
    fn a_room_menu_lists_what_discord_has_and_leave_is_dangerous() {
        let r = room("!r:x", "random");
        let items = room_menu(&r, None);
        assert_eq!(
            labels(&items),
            [
                "Mark as unread",
                "Invite people",
                "Notifications",
                "Edit room",
                "Create room",
                "Privacy & encryption",
                "Copy link",
                "Copy room ID",
                "Leave room",
            ]
        );
        assert!(items.last().is_some_and(|i| i.danger));
    }

    #[test]
    fn the_current_notification_mode_is_checked() {
        let r = room("!r:x", "random");
        let items = room_menu(&r, Some(NotifyMode::Mute));
        let notifications = items.iter().find(|i| i.label == "Notifications").expect("row");
        let checked: Vec<_> = notifications
            .children
            .iter()
            .filter(|c| c.checked)
            .map(|c| c.label.as_str())
            .collect();
        assert_eq!(checked, ["Mute"]);
        assert_eq!(labels(&notifications.children), ["All messages", "Only mentions", "Mute"]);
    }

    #[test]
    fn a_dm_that_is_not_a_friend_offers_add_friend() {
        let mut r = room("!d:x", "bob");
        r.is_dm = true;
        let items = dm_menu(&r, &dm_state());
        assert_eq!(
            labels(&items),
            [
                "Mark as unread",
                "Pin",
                "Add to favorites",
                "Profile",
                "Close DM",
                "Add friend",
                "Ignore",
                "Block",
                "Mute @bob",
                "Privacy & encryption",
                "Copy user ID",
                "Copy room ID",
            ]
        );
    }

    #[test]
    fn a_friend_dm_has_a_nickname_and_unfriending_and_states_flip_the_labels() {
        let mut r = room("!d:x", "bob");
        r.is_dm = true;
        r.favorite = true;
        let state = DmState {
            pinned: true,
            ignored: true,
            blocked: true,
            muted: true,
            friend: true,
            invite_to: vec![(OwnedRoomId::try_from("!s:x").expect("id"), "Engineering".into())],
            ..dm_state()
        };
        let items = dm_menu(&r, &state);
        assert_eq!(
            labels(&items),
            [
                "Mark as unread",
                "Unpin",
                "Remove from favorites",
                "Profile",
                "Change friend nickname",
                "Close DM",
                "Invite to server",
                "Remove friend",
                "Unignore",
                "Unblock",
                "Unmute @bob",
                "Privacy & encryption",
                "Copy user ID",
                "Copy room ID",
            ]
        );
        let block = items.iter().find(|i| i.label == "Unblock").expect("row");
        assert!(block.danger);
        assert_eq!(block.action, Some(MenuAction::Block(user(), false)));
    }

    #[test]
    fn muting_a_dm_offers_the_discord_lengths() {
        let mut r = room("!d:x", "bob");
        r.is_dm = true;
        let items = dm_menu(&r, &dm_state());
        let mute = items.iter().find(|i| i.label == "Mute @bob").expect("row");
        assert_eq!(
            labels(&mute.children),
            [
                "For 15 minutes",
                "For 1 hour",
                "For 3 hours",
                "For 8 hours",
                "For 24 hours",
                "Until I turn it back on",
            ]
        );
        assert_eq!(mute.children[0].action, Some(MenuAction::Mute(r.id.clone(), 15 * 60_000)));
        assert_eq!(mute.children[5].action, Some(MenuAction::Mute(r.id.clone(), 0)));
    }

    #[test]
    fn every_item_of_these_menus_has_an_icon_or_picture() {
        let mut r = room("!r:x", "random");
        r.unread_messages = 1;
        let mut dm = room("!d:x", "bob");
        dm.is_dm = true;
        let mut space = room("!s:x", "Server");
        space.is_space = true;
        let state = DmState {
            friend: true,
            invite_to: vec![(space.id.clone(), "Server".into())],
            ..dm_state()
        };
        for items in [
            room_menu(&r, None),
            dm_menu(&dm, &state),
            space_menu(&space, &[r.clone()], false, &[]),
        ] {
            for item in &items {
                assert!(
                    item.glyph().is_some() || item.avatar.is_some(),
                    "{} has no icon",
                    item.label
                );
            }
        }
    }

    #[test]
    fn move_to_lists_the_folders_with_the_current_one_checked() {
        let mut space = room("!s:x", "Server");
        space.is_space = true;
        let folders = [
            ("f1".to_owned(), "Work".to_owned(), true),
            ("f2".to_owned(), "Play".to_owned(), false),
        ];
        let items = space_menu(&space, &[], false, &folders);
        let moving = items.iter().find(|i| i.label == "Move to").expect("row");
        assert_eq!(labels(&moving.children), ["Work", "Play", "New folder", "No folder"]);
        assert!(moving.children[0].checked && !moving.children[1].checked);
        assert_eq!(
            moving.children[1].action,
            Some(MenuAction::MoveToFolder(space.id.clone(), "f2".into()))
        );
        let names = labels(&items);
        let at = names.iter().position(|l| *l == "Move to").expect("row");
        assert_eq!(names[at + 1], "Leave server", "just before leaving");
    }

    #[test]
    fn a_server_menu_mutes_all_its_rooms_or_unmutes_when_all_are_muted() {
        let mut space = room("!s:x", "Server");
        space.is_space = true;
        let (mut a, b) = (room("!a:x", "a"), room("!b:x", "b"));
        let items = space_menu(&space, &[a.clone(), b.clone()], false, &[]);
        let mute = items.iter().find(|i| i.label == "Mute server").expect("row");
        assert_eq!(
            mute.action,
            Some(MenuAction::Notify(vec![a.id.clone(), b.id.clone()], NotifyMode::Mute))
        );
        a.muted = true;
        let mut b = b;
        b.muted = true;
        let items = space_menu(&space, &[a.clone(), b.clone()], true, &[]);
        let unmute = items.iter().find(|i| i.label == "Unmute server").expect("row");
        assert_eq!(
            unmute.action,
            Some(MenuAction::Notify(vec![a.id.clone(), b.id.clone()], NotifyMode::AllMessages))
        );
        let hide = items.iter().find(|i| i.label == "Hide muted channels").expect("row");
        assert!(hide.checked);
        let notifications = items.iter().find(|i| i.label == "Notifications").expect("row");
        assert!(notifications.children.iter().any(|c| c.label == "Mute" && c.checked));
        // No rooms: nothing is "all muted".
        let empty = space_menu(&space, &[], false, &[]);
        assert!(empty.iter().any(|i| i.label == "Mute server"));
    }
}
