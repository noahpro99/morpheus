use super::toast::report;
use dioxus::prelude::*;
use morpheus_core::{NotifyMode, OwnedRoomId, RoomSummary, Session};

use super::context_menu::{MenuItem, copy_to_clipboard, open_menu, open_menu_at};
use super::invites::{InviteRow, pending_invites};
use super::room_dialogs::{OpenRoomDialog, RoomDialog};
use super::shell::{Rooms, Scope, Selected, SelectedSpace, UiPrefs, go_to, rooms_in_scope};
use super::space_menu::space_menu;
use crate::icons::Icon;
use crate::media::Avatar;

#[component]
pub fn RoomList(error: Option<String>) -> Element {
    let rooms = use_context::<Rooms>();
    let selected_space = use_context::<SelectedSpace>();
    let selected = use_context::<Selected>();
    let prefs = use_context::<UiPrefs>();
    let session = use_context::<Session>();
    let mut friends_open = use_context::<super::friends::FriendsOpen>();

    let scope = selected_space();
    let hide_muted = prefs().hide_muted;
    let open_room = selected();
    let (dms, channels): (Vec<RoomSummary>, Vec<RoomSummary>) =
        rooms_in_scope(&rooms.read(), &scope)
            .into_iter()
            // "Hide muted channels" keeps the open room visible.
            .filter(|room| !hide_muted || !room.muted || Some(&room.id) == open_room.as_ref())
            .partition(|room| room.is_dm);
    // Inside a space, rooms under a nested space ("category") group under its name.
    let categories: Vec<(OwnedRoomId, String)> = match &scope {
        Scope::Space(space_id) => rooms
            .read()
            .iter()
            .filter(|r| r.is_space && r.parent_spaces.contains(space_id))
            .map(|r| (r.id.clone(), r.name.clone()))
            .collect(),
        _ => Vec::new(),
    };
    let category_of = |room: &RoomSummary| {
        categories
            .iter()
            .find(|(id, _)| room.parent_spaces.contains(id))
            .cloned()
    };
    // Invites show in every scope, like Discord's pending list.
    let invites = pending_invites(&rooms.read());
    let no_rooms = channels.is_empty();
    let (uncategorised, categorised): (Vec<RoomSummary>, Vec<RoomSummary>) = channels
        .into_iter()
        .partition(|room| category_of(room).is_none());

    // Clicking the space name opens the server menu.
    let header_menu = {
        let scope = scope.clone();
        move |e: Event<MouseData>| {
            let Scope::Space(space_id) = &scope else {
                return;
            };
            let Some(space) = rooms.read().iter().find(|r| &r.id == space_id).cloned() else {
                return;
            };
            open_menu(&e, space_menu(&session, rooms, &space), None);
        }
    };

    // Sidebar header: what the rail selected.
    let header_title = match &scope {
        Scope::Dms => "Direct messages".to_string(),
        Scope::Rooms => "Rooms".to_string(),
        Scope::Space(space_id) => rooms
            .read()
            .iter()
            .find(|room| &room.id == space_id)
            .map(|room| room.name.clone())
            .unwrap_or_else(|| "Space".to_string()),
    };
    // Inside a space both kinds can appear, so label the groups there.
    let grouped = matches!(scope, Scope::Space(_));

    rsx! {
        aside { class: "sidebar",
            header { class: "sidebar-header",
                h2 {
                    class: if matches!(scope, Scope::Space(_)) { "server-name" },
                    onclick: header_menu.clone(),
                    oncontextmenu: header_menu,
                    "{header_title}"
                    if matches!(scope, Scope::Space(_)) {
                        Icon { glyph: crate::icons::CHEVRON_RIGHT, size: 14, class: "server-caret" }
                    }
                }
                if scope != Scope::Dms {
                    button {
                        class: "ghost create-room-button",
                        title: "Create room",
                        onclick: move |_| consume_context::<OpenRoomDialog>().set(RoomDialog::Create),
                        Icon { glyph: crate::icons::PLUS, size: 18 }
                    }
                }
            }

            div { class: "room-list",
                if let Some(message) = error {
                    p { class: "error", "{message}" }
                }
                if !invites.is_empty() {
                    h4 { class: "section invites", "Invites" }
                    for room in invites {
                        InviteRow { key: "{room.id}", room }
                    }
                }
                if dms.is_empty() && no_rooms {
                    p { class: "muted pad",
                        match scope {
                            Scope::Dms => "No direct messages yet.",
                            _ => "No rooms here.",
                        }
                    }
                }
                // The Friends page, at the top of the DMs (as in Discord).
                if scope == Scope::Dms {
                    button {
                        class: "room friends-row",
                        class: if friends_open().0 { "selected" },
                        onclick: move |_| {
                            friends_open.set(super::friends::FriendsShown(true));
                            // On a phone, the page comes forward like a chat.
                            consume_context::<super::shell::ChatOpen>()
                                .set(super::shell::ChatInFront(true));
                        },
                        crate::icons::Icon { glyph: crate::icons::USERS, size: 18 }
                        span { class: "room-name", "Friends" }
                    }
                }
                if !dms.is_empty() {
                    if grouped {
                        h4 { class: "section", "Direct messages" }
                    }
                    for room in dms {
                        RoomRow { key: "{room.id}", room }
                    }
                }
                if !uncategorised.is_empty() {
                    if grouped {
                        h4 { class: "section", "Rooms" }
                    }
                    for room in uncategorised {
                        RoomRow { key: "{room.id}", room }
                    }
                }
                for (category_id, category_name) in categories.iter() {
                    {
                        let members: Vec<RoomSummary> = categorised.iter().filter(|r| r.parent_spaces.contains(category_id)).cloned().collect();
                        rsx! {
                            if !members.is_empty() {
                                h4 { key: "{category_id}", class: "section category", "{category_name}" }
                                for room in members {
                                    RoomRow { key: "{room.id}", room }
                                }
                            }
                        }
                    }
                }
            }

        }
    }
}

#[component]
fn RoomRow(room: RoomSummary) -> Element {
    let session = use_context::<Session>();
    let selected = use_context::<Selected>();
    let is_selected = selected().as_ref() == Some(&room.id);
    let title = try_use_context::<super::nicknames::Nicknames>()
        .map(|map| super::nicknames::room_title(&map(), &room))
        .unwrap_or_else(|| room.name.clone());
    let id = room.id.clone();

    // The menu needs the room's notification mode, so it is built after a
    // quick (local) lookup and opened at the pointer's position.
    let on_context = {
        let room_id = room.id.clone();
        let is_dm = room.is_dm;
        move |e: Event<MouseData>| {
            e.prevent_default();
            e.stop_propagation();
            let point = e.client_coordinates();
            open_room_menu_at(&session, room_id.clone(), is_dm, point.x, point.y);
        }
    };

    rsx! {
        button {
            class: "room",
            oncontextmenu: on_context,
            class: if room.is_dm { "dm" },
            class: if is_selected { "selected" },
            class: if room.unread_messages > 0 || room.marked_unread { "unread" },
            onclick: move |_| go_to(id.clone()),
            if room.is_dm {
                Avatar { media: room.avatar.clone(), fallback: room.name.clone(), size: 24 }
            } else {
                span { class: "hash", "#" }
            }
            span { class: "room-name", "{title}" }
            if room.highlight_count > 0 {
                span { class: "badge", "{room.highlight_count}" }
            }
        }
    }
}

/// Runs a room action in the background and logs failures.
fn run(what: &'static str, future: morpheus_core::CoreFuture<()>) {
    spawn(async move {
        if let Err(err) = future.await {
            report(what, &err);
        }
    });
}

/// Open a room's menu at a point: what right-clicking its row shows, also
/// behind the chat header's ellipsis. Built after a quick (local) lookup of
/// the room's notification mode.
pub fn open_room_menu_at(
    session: &Session,
    room_id: morpheus_core::OwnedRoomId,
    is_dm: bool,
    x: f64,
    y: f64,
) {
    let session = session.clone();
    spawn(async move {
        let mode = session.notify_mode(room_id.clone()).await.ok();
        let items = room_menu(&session, &room_id, is_dm, mode);
        open_menu_at(x, y, items, None);
    });
}

/// The right-click menu for a room row.
fn room_menu(
    session: &Session,
    room_id: &morpheus_core::OwnedRoomId,
    is_dm: bool,
    mode: Option<NotifyMode>,
) -> Vec<MenuItem> {
    let mut items = vec![MenuItem::new("Mark as read", {
        let (session, room_id) = (session.clone(), room_id.clone());
        move || run("mark read", session.mark_room_read(room_id.clone()))
    })];
    if !is_dm {
        items.push(MenuItem::new("Invite people", {
            let room_id = room_id.clone();
            move || consume_context::<OpenRoomDialog>().set(RoomDialog::Invite(room_id.clone()))
        }));
    }
    let modes = [
        (NotifyMode::AllMessages, "All messages"),
        (NotifyMode::MentionsOnly, "Only mentions"),
        (NotifyMode::Mute, "Mute"),
    ]
    .into_iter()
    .map(|(choice, label)| {
        MenuItem::new(label, {
            let (session, room_id) = (session.clone(), room_id.clone());
            move || {
                run(
                    "notification mode",
                    session.set_notify_mode(room_id.clone(), choice),
                )
            }
        })
        .checked(mode == Some(choice))
    })
    .collect();
    items.push(MenuItem::submenu("Notifications", modes));
    if !is_dm {
        items.push(MenuItem::new("Edit room", {
            let room_id = room_id.clone();
            move || consume_context::<OpenRoomDialog>().set(RoomDialog::Edit(room_id.clone()))
        }));
        items.push(MenuItem::new("Create room", || {
            consume_context::<OpenRoomDialog>().set(RoomDialog::Create);
        }));
    }
    items.push(MenuItem::new("Copy link", {
        let link = format!("https://matrix.to/#/{room_id}");
        move || copy_to_clipboard(&link)
    }));
    items.push(MenuItem::new("Copy room ID", {
        let id = room_id.to_string();
        move || copy_to_clipboard(&id)
    }));
    items.push(
        MenuItem::new("Leave room", {
            let (session, room_id) = (session.clone(), room_id.clone());
            move || run("leave", session.leave_room(room_id.clone()))
        })
        .danger(),
    );
    items
}
