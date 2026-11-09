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
    let session_for_directory = session.clone();
    let mut friends_open = use_context::<super::friends::FriendsOpen>();

    let scope = selected_space();
    let hide_muted = prefs().hide_muted;
    let hidden = use_context::<super::toast::Hidden>();
    let open_room = selected();
    let (dms, channels): (Vec<RoomSummary>, Vec<RoomSummary>) =
        rooms_in_scope(&rooms.read(), &scope)
            .into_iter()
            // "Hide muted channels" keeps the open room visible.
            .filter(|room| !hide_muted || !room.muted || Some(&room.id) == open_room.as_ref())
            // Rooms just left, still undoable, are out of the list.
            .filter(|room| !hidden.read().0.contains(room.id.as_str()))
            .partition(|room| room.is_dm);
    // DMs: closed ones stay out of the list until a new message comes
    // (or they are opened); pinned ones come first, in the order pinned.
    let marks = use_context::<super::marks::Marks>();
    let dms = {
        let closed = marks.closed.read();
        let pinned = marks.pinned.read();
        let mut dms: Vec<RoomSummary> = dms
            .into_iter()
            .filter(|room| {
                // Back once a message comes after it was closed.
                closed
                    .get(room.id.as_str())
                    .is_none_or(|at| i64::try_from(room.latest_ms).unwrap_or(i64::MAX) > *at)
                    || (Some(&room.id) == open_room.as_ref() && !friends_open().0)
            })
            .collect();
        // Stable: the rest keep their order (by recency).
        dms.sort_by_key(|room| pinned.get(room.id.as_str()).copied().unwrap_or(i64::MAX));
        dms
    };
    // A closed DM back in the list is open again.
    {
        let reopened: Vec<String> = dms
            .iter()
            .map(|room| room.id.to_string())
            .filter(|id| marks.closed.peek().contains_key(id))
            .collect();
        if !reopened.is_empty() {
            let session = session.clone();
            spawn(async move {
                for id in reopened {
                    super::marks::set_mark(&session, morpheus_core::marks::CLOSED, id, None);
                }
            });
        }
    }
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
    let (mut uncategorised, mut categorised): (Vec<RoomSummary>, Vec<RoomSummary>) = channels
        .into_iter()
        .partition(|room| category_of(room).is_none());
    // A server's channels in the order it keeps (as other clients show
    // them), each category's in its own; any it doesn't list come after.
    let children_of = move |space_id: &OwnedRoomId| -> Vec<OwnedRoomId> {
        rooms
            .read()
            .iter()
            .find(|room| &room.id == space_id)
            .map(|room| room.space_children.clone())
            .unwrap_or_default()
    };
    let place = |order: &[OwnedRoomId], id: &OwnedRoomId| {
        order
            .iter()
            .position(|child| child == id)
            .unwrap_or(usize::MAX)
    };
    let mut categories = categories;
    let (server, can_reorder) = match &scope {
        Scope::Space(space_id) => {
            let order = children_of(space_id);
            uncategorised.sort_by_key(|room| place(&order, &room.id));
            categories.sort_by_key(|(id, _)| place(&order, id));
            let can = rooms
                .read()
                .iter()
                .any(|room| &room.id == space_id && room.can_edit_children);
            (Some(space_id.clone()), can)
        }
        _ => (None, false),
    };
    let category_orders: Vec<(OwnedRoomId, Vec<OwnedRoomId>)> = categories
        .iter()
        .map(|(id, _)| (id.clone(), children_of(id)))
        .collect();
    categorised.sort_by_key(|room| {
        category_orders
            .iter()
            .find(|(id, _)| room.parent_spaces.contains(id))
            .map_or(usize::MAX, |(_, order)| place(order, &room.id))
    });
    // The server's channels you aren't in, from its directory: listed last,
    // opening a card to join.
    let directory = use_resource(move || {
        let session = session_for_directory.clone();
        let scope = selected_space();
        async move {
            match scope {
                Scope::Space(space_id) => session.space_rooms(space_id).await.unwrap_or_default(),
                _ => Vec::new(),
            }
        }
    });
    let unjoined: Vec<morpheus_core::SpaceRoom> = match directory.read().as_ref() {
        Some(list) if server.is_some() => list
            .iter()
            .filter(|room| !room.is_space)
            .filter(|room| {
                !rooms
                    .read()
                    .iter()
                    .any(|known| known.id == room.id && !known.invited)
            })
            .cloned()
            .collect(),
        _ => Vec::new(),
    };
    // Dragging a channel onto another puts it there, for everyone.
    let mut dragging = use_signal(|| None::<OwnedRoomId>);
    let reorder = {
        let session = session.clone();
        let server = server.clone();
        let listed: Vec<OwnedRoomId> = uncategorised.iter().map(|room| room.id.clone()).collect();
        move |target: OwnedRoomId| {
            let (Some(from), Some(space_id)) = (dragging.take(), server.clone()) else {
                return;
            };
            if from == target {
                return;
            }
            // The server's whole list (channels you aren't in too), moved.
            let mut order = children_of(&space_id);
            for id in &listed {
                if !order.contains(id) {
                    order.push(id.clone());
                }
            }
            order.retain(|id| *id != from);
            let at = order
                .iter()
                .position(|id| *id == target)
                .unwrap_or(order.len());
            order.insert(at, from);
            let session = session.clone();
            spawn(async move {
                if let Err(err) = session.set_space_order(space_id, order).await {
                    report("reorder channels", &err);
                }
            });
        }
    };

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
        Scope::Favorites => "Favorites".to_string(),
        Scope::Space(space_id) => rooms
            .read()
            .iter()
            .find(|room| &room.id == space_id)
            .map(|room| room.name.clone())
            .unwrap_or_else(|| "Space".to_string()),
    };
    // Inside a space both kinds can appear, so label the groups there.
    let grouped = matches!(scope, Scope::Space(_) | Scope::Favorites);

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
                        crate::icons::Icon { glyph: crate::icons::USERS, size: 22, class: "friends-icon" }
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
                        RoomRow { key: "{room.id}", room, reorder: can_reorder.then(|| (dragging, Callback::new(reorder.clone()))) }
                    }
                }
                for (category_id, category_name) in categories.iter() {
                    {
                        let members: Vec<RoomSummary> = categorised.iter().filter(|r| r.parent_spaces.contains(category_id)).cloned().collect();
                        rsx! {
                            if !members.is_empty() {
                                h4 { key: "{category_id}", class: "section category", "{category_name}" }
                                for room in members {
                                    RoomRow { key: "{room.id}", room, reorder: None }
                                }
                            }
                        }
                    }
                }
                if !unjoined.is_empty() {
                    h4 { class: "section more-channels", "More channels" }
                    for room in unjoined {
                        UnjoinedRow { key: "{room.id}", room }
                    }
                }
            }
            // The call controls live in the profile card (`UserBar`).
        }
    }
}

/// A server's channel you aren't in: opens a card to join it.
#[component]
fn UnjoinedRow(room: morpheus_core::SpaceRoom) -> Element {
    let selected = use_context::<Selected>();
    let mut preview = use_context::<super::shell::Preview>();
    let is_selected = selected().as_ref() == Some(&room.id);
    rsx! {
        button {
            class: "room unjoined",
            class: if is_selected { "selected" },
            "data-room": "{room.id}",
            title: "Not joined",
            onclick: {
                let room = room.clone();
                move |_| {
                    preview.set(Some(room.clone()));
                    go_to(room.id.clone());
                }
            },
            if room.is_voice {
                Icon { glyph: crate::icons::VOLUME_2, size: 18, class: "voice-icon" }
            } else {
                span { class: "hash", "#" }
            }
            span { class: "room-name", "{room.name}" }
        }
    }
}

/// `reorder`: the channel being dragged and where a drop puts it, when you
/// may reorder the server's channels.
#[component]
fn RoomRow(
    room: RoomSummary,
    #[props(default)] reorder: Option<(Signal<Option<OwnedRoomId>>, Callback<OwnedRoomId>)>,
) -> Element {
    let session = use_context::<Session>();
    let selected = use_context::<Selected>();
    let is_selected = selected().as_ref() == Some(&room.id);
    let title = try_use_context::<super::nicknames::Nicknames>()
        .map(|map| super::nicknames::room_title(&map(), &room))
        .unwrap_or_else(|| room.name.clone());
    let marks = use_context::<super::marks::Marks>();
    let pinned = room.is_dm && marks.pinned.read().contains_key(room.id.as_str());
    let muted = marks.is_muted(&room);
    let id = room.id.clone();
    let (is_voice, name, join_session) = (room.is_voice, room.name.clone(), session.clone());
    let active = use_context::<super::call::ActiveCall>();
    let in_this_call = {
        let id = room.id.clone();
        move || {
            active
                .peek()
                .0
                .as_ref()
                .is_some_and(|call| call.room_id == id)
        }
    };

    // The menu needs the room's notification mode, so it is built after a
    // quick (local) lookup and opened at the pointer's position.
    let on_context = {
        let room = room.clone();
        move |e: Event<MouseData>| {
            e.prevent_default();
            e.stop_propagation();
            let point = e.client_coordinates();
            open_room_menu_at(&session, room.clone(), point.x, point.y);
        }
    };

    let drag_id = room.id.clone();
    let drop_id = room.id.clone();
    let dragged = reorder.is_some_and(|(dragging, _)| dragging().as_ref() == Some(&room.id));

    rsx! {
        button {
            class: "room",
            "data-room": "{room.id}",
            oncontextmenu: on_context,
            draggable: if reorder.is_some() { "true" },
            class: if dragged { "dragging" },
            ondragstart: move |_| {
                if let Some((mut dragging, _)) = reorder {
                    dragging.set(Some(drag_id.clone()));
                }
            },
            ondragover: move |e| {
                if reorder.is_some() {
                    e.prevent_default();
                }
            },
            ondrop: move |e| {
                if let Some((_, on_drop)) = reorder {
                    e.prevent_default();
                    on_drop(drop_id.clone());
                }
            },
            ondragend: move |_| {
                if let Some((mut dragging, _)) = reorder {
                    dragging.set(None);
                }
            },
            class: if room.is_dm { "dm" },
            class: if room.is_voice { "voice" },
            class: if pinned { "pinned" },
            class: if muted { "muted" },
            class: if is_selected { "selected" },
            class: if room.unread_messages > 0 || room.marked_unread { "unread" },
            onclick: move |_| {
                go_to(id.clone());
                // A voice channel, as in Discord: clicking it joins its call
                // (already in it, it only opens).
                if is_voice && !in_this_call() {
                    super::call::join_call(join_session.clone(), id.clone(), name.clone());
                }
            },
            if room.is_dm {
                Avatar { media: room.avatar.clone(), fallback: room.name.clone(), size: 32 }
            } else if room.is_voice {
                Icon { glyph: crate::icons::VOLUME_2, size: 18, class: "voice-icon" }
            } else {
                span { class: "hash", "#" }
            }
            span { class: "room-name", "{title}" }
            if pinned {
                Icon { glyph: crate::icons::PIN, size: 12, class: "room-pin" }
            }
            if room.highlight_count > 0 {
                span { class: "badge", "{room.highlight_count}" }
            }
        }
        if !room.call_participants.is_empty() {
            super::call::RoomCall {
                room_id: room.id.clone(),
                room_name: room.name.clone(),
                participants: room.call_participants.clone(),
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
/// behind the chat header's ellipsis. Built after quick lookups (the room's
/// notification mode; for a DM, whether you blocked them).
pub fn open_room_menu_at(session: &Session, room: RoomSummary, x: f64, y: f64) {
    let session = session.clone();
    spawn(async move {
        let items = match room.dm_user.clone().filter(|_| room.is_dm) {
            Some(user_id) => {
                let blocked = session.is_blocked(user_id.clone()).await.unwrap_or(false);
                dm_menu(&session, &room, user_id, blocked)
            }
            None => {
                let mode = session.notify_mode(room.id.clone()).await.ok();
                room_menu(&session, &room, mode)
            }
        };
        open_menu_at(x, y, items, None);
    });
}

/// Mark as unread, or (when it is already) as read.
fn read_item(session: &Session, room: &RoomSummary) -> MenuItem {
    let (session, room_id) = (session.clone(), room.id.clone());
    if room.unread_messages > 0 || room.marked_unread {
        MenuItem::new("Mark as read", move || {
            run("mark read", session.mark_room_read(room_id.clone()))
        })
    } else {
        MenuItem::new("Mark as unread", move || {
            run("mark unread", session.mark_room_unread(room_id.clone()))
        })
    }
}

fn copy_room_id(room: &RoomSummary) -> MenuItem {
    let id = room.id.to_string();
    MenuItem::new("Copy room ID", move || copy_to_clipboard(&id))
}

/// The menu of a DM, as Discord's: what you do with the conversation and
/// with the person in it.
fn dm_menu(
    session: &Session,
    room: &RoomSummary,
    user_id: morpheus_core::OwnedUserId,
    blocked: bool,
) -> Vec<MenuItem> {
    use morpheus_core::marks;
    let marks_now = consume_context::<super::marks::Marks>();
    let key = room.id.to_string();
    let name = try_consume_context::<super::nicknames::Nicknames>()
        .map(|map| super::nicknames::room_title(&map.peek(), room))
        .unwrap_or_else(|| room.name.clone());
    let pinned = marks_now.pinned.peek().contains_key(&key);
    let ignored = marks_now.ignored.peek().contains_key(user_id.as_str());
    let muted = marks_now.is_muted(room);
    let friend = try_consume_context::<super::friends::Friends>()
        .is_some_and(|friends| friends.peek().0.contains(&user_id));

    let mut items = vec![read_item(session, room)];
    items.push(MenuItem::new(if pinned { "Unpin" } else { "Pin" }, {
        let (session, key) = (session.clone(), key.clone());
        move || {
            let at = (!pinned).then(super::marks::now_ms);
            super::marks::set_mark(&session, marks::PINNED, key.clone(), at);
        }
    }));
    items.push(MenuItem::new(
        if room.favorite {
            "Remove from favorites"
        } else {
            "Add to favorites"
        },
        {
            let (session, room_id, favorite) = (session.clone(), room.id.clone(), room.favorite);
            move || {
                run(
                    "favorites",
                    session.set_favorite(room_id.clone(), !favorite),
                )
            }
        },
    ));
    items.push(MenuItem::new("Profile", {
        let user_id = user_id.clone();
        move || super::profile::open_profile(user_id.clone())
    }));
    if friend {
        items.push(MenuItem::new("Change friend nickname", {
            let (user_id, name) = (user_id.clone(), name.clone());
            move || {
                consume_context::<super::nicknames::EditNickname>().set(
                    super::nicknames::NicknameTarget(Some((user_id.clone(), name.clone()))),
                )
            }
        }));
    }
    items.push(MenuItem::new("Close DM", {
        let (session, key, room_id) = (session.clone(), key.clone(), room.id.clone());
        move || {
            // Read, or its unread messages would bring it straight back.
            run("mark read", session.mark_room_read(room_id.clone()));
            super::marks::set_mark(
                &session,
                marks::CLOSED,
                key.clone(),
                Some(super::marks::now_ms()),
            );
            // Closing the open DM goes to the Friends page, as Discord does.
            let selected = consume_context::<Selected>();
            if selected.peek().as_ref().map(ToString::to_string).as_deref() == Some(key.as_str()) {
                consume_context::<super::friends::FriendsOpen>()
                    .set(super::friends::FriendsShown(true));
            }
        }
    }));
    if let Some(invite) = super::invites::invite_menu(session, &user_id, None) {
        items.push(invite);
    }
    items.push(MenuItem::new(
        if friend {
            "Remove friend"
        } else {
            "Add friend"
        },
        {
            let (session, user_id) = (session.clone(), user_id.clone());
            move || super::friends::set_friend(&session, user_id.clone(), !friend)
        },
    ));
    items.push(MenuItem::new(
        if ignored { "Unignore" } else { "Ignore" },
        {
            let (session, user_id) = (session.clone(), user_id.clone());
            move || {
                let at = (!ignored).then(super::marks::now_ms);
                super::marks::set_mark(&session, marks::IGNORED, user_id.to_string(), at);
            }
        },
    ));
    items.push(
        MenuItem::new(if blocked { "Unblock" } else { "Block" }, {
            let (session, user_id) = (session.clone(), user_id.clone());
            move || run("block", session.block(user_id.clone(), !blocked))
        })
        .danger(),
    );
    if muted {
        items.push(MenuItem::new(format!("Unmute @{name}"), {
            let (session, room_id) = (session.clone(), room.id.clone());
            move || super::marks::unmute(&session, room_id.clone())
        }));
    } else {
        const MINUTE: i64 = 60_000;
        let choices = [
            ("For 15 minutes", 15 * MINUTE),
            ("For 1 hour", 60 * MINUTE),
            ("For 3 hours", 3 * 60 * MINUTE),
            ("For 8 hours", 8 * 60 * MINUTE),
            ("For 24 hours", 24 * 60 * MINUTE),
            ("Until I turn it back on", 0),
        ]
        .into_iter()
        .map(|(label, length)| {
            MenuItem::new(label, {
                let (session, room_id) = (session.clone(), room.id.clone());
                move || {
                    let until = if length == 0 {
                        0
                    } else {
                        super::marks::now_ms() + length
                    };
                    super::marks::mute_until(&session, room_id.clone(), until);
                }
            })
        })
        .collect();
        items.push(MenuItem::submenu(format!("Mute @{name}"), choices));
    }
    items.push(MenuItem::new("Copy user ID", {
        let id = user_id.to_string();
        move || copy_to_clipboard(&id)
    }));
    items.push(copy_room_id(room));
    items
}

/// The right-click menu for a room row.
fn room_menu(session: &Session, room: &RoomSummary, mode: Option<NotifyMode>) -> Vec<MenuItem> {
    let room_id = &room.id;
    let is_dm = room.is_dm;
    let mut items = vec![read_item(session, room)];
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
    items.push(copy_room_id(room));
    items.push(
        MenuItem::new("Leave room", {
            let (session, room_id, name) = (session.clone(), room_id.clone(), room.name.clone());
            // No "are you sure": it goes from the list now, undoable a moment.
            move || {
                let (session, room_id) = (session.clone(), room_id.clone());
                super::toast::hide_with_undo(
                    room_id.to_string(),
                    format!("Left #{name}"),
                    "leave",
                    move || session.leave_room(room_id),
                );
            }
        })
        .danger(),
    );
    items
}
