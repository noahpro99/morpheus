use dioxus::prelude::*;
use morpheus_core::{NotifyMode, RoomSummary, Session};

use super::context_menu::{MenuItem, copy_to_clipboard, open_menu, open_menu_at};
use super::room_dialogs::{OpenRoomDialog, RoomDialog};
use super::profile::{Editor, ProfileEditor, open_profile};
use super::settings::SettingsOpen;
use super::shell::{Rooms, Scope, Selected, SelectedSpace, rooms_in_scope};
use crate::media::Avatar;

#[component]
pub fn RoomList(error: Option<String>) -> Element {
    let session = use_context::<Session>();
    let rooms = use_context::<Rooms>();
    let selected_space = use_context::<SelectedSpace>();
    let session_slot = use_context::<Signal<Option<Session>>>();
    let mut settings_open = use_context::<SettingsOpen>();

    let user_id = session
        .user_id()
        .map(|id| id.to_string())
        .unwrap_or_default();
    // The footer shows the display name and avatar, never the raw Matrix ID.
    let refresh = use_context::<super::profile::ProfileRefresh>();
    let profile = use_resource({
        let session = session.clone();
        move || {
            let session = session.clone();
            let _ = refresh();
            async move { session.own_profile().await.ok() }
        }
    });
    let (name, avatar) = match profile().flatten() {
        Some(profile) => (profile.name, profile.avatar),
        None => (user_id.trim_start_matches('@').to_owned(), None),
    };

    let scope = selected_space();
    let (dms, channels): (Vec<RoomSummary>, Vec<RoomSummary>) =
        rooms_in_scope(&rooms.read(), &scope)
            .into_iter()
            .partition(|room| room.is_dm);

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

    let logout = move || {
        let mut session_slot = session_slot;
        let Some(active) = session_slot.take() else {
            return;
        };
        spawn(async move {
            if let Err(err) = active.logout().await {
                tracing::warn!(%err, "logout failed; session was dropped locally anyway");
            }
        });
    };

    // Click (or right-click) on your own chip: account actions, Discord-style.
    let account_menu = {
        let own = session.user_id();
        move |e: Event<MouseData>| {
            let mut editor = consume_context::<ProfileEditor>();
            let mut items = Vec::new();
            if let Some(id) = own.clone() {
                items.push(MenuItem::new("View profile", move || open_profile(id.clone())));
            }
            items.push(MenuItem::new("Edit profile", move || editor.set(Editor::Profile)));
            items.push(MenuItem::new("Set status", move || editor.set(Editor::Status)));
            // Switching signs out and returns to the login screen; sessions for
            // several accounts are not stored side by side yet.
            items.push(MenuItem::new("Switch account", logout));
            items.push(MenuItem::new("Sign out", logout).danger());
            open_menu(&e, items, None);
        }
    };

    rsx! {
        aside { class: "sidebar",
            header { class: "sidebar-header",
                h2 { "{header_title}" }
                if scope != Scope::Dms {
                    button {
                        class: "ghost create-room-button",
                        title: "Create room",
                        onclick: move |_| consume_context::<OpenRoomDialog>().set(RoomDialog::Create),
                        "+"
                    }
                }
            }

            div { class: "room-list",
                if let Some(message) = error {
                    p { class: "error", "{message}" }
                }
                if dms.is_empty() && channels.is_empty() {
                    p { class: "muted pad",
                        match scope {
                            Scope::Dms => "No direct messages yet.",
                            _ => "No rooms here.",
                        }
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
                if !channels.is_empty() {
                    if grouped {
                        h4 { class: "section", "Rooms" }
                    }
                    for room in channels {
                        RoomRow { key: "{room.id}", room }
                    }
                }
            }

            footer { class: "sidebar-footer",
                button {
                    class: "profile-chip",
                    title: "{user_id}",
                    onclick: account_menu.clone(),
                    oncontextmenu: account_menu,
                    Avatar { media: avatar, fallback: name.clone(), size: 32 }
                    span { class: "user", "{name}" }
                }
                button { class: "ghost settings-button", title: "Settings", onclick: move |_| settings_open.set(true), "⚙" }
            }
        }
    }
}

#[component]
fn RoomRow(room: RoomSummary) -> Element {
    let session = use_context::<Session>();
    let mut selected = use_context::<Selected>();
    let is_selected = selected().as_ref() == Some(&room.id);
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
            let session = session.clone();
            let room_id = room_id.clone();
            spawn(async move {
                let mode = session.notify_mode(room_id.clone()).await.ok();
                let items = room_menu(&session, &room_id, is_dm, mode);
                open_menu_at(point.x, point.y, items, None);
            });
        }
    };

    rsx! {
        button {
            class: "room",
            oncontextmenu: on_context,
            class: if room.is_dm { "dm" },
            class: if is_selected { "selected" },
            class: if room.unread_messages > 0 || room.marked_unread { "unread" },
            onclick: move |_| selected.set(Some(id.clone())),
            if room.is_dm {
                Avatar { media: room.avatar.clone(), fallback: room.name.clone(), size: 24 }
            } else {
                span { class: "hash", "#" }
            }
            span { class: "room-name", "{room.name}" }
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
            tracing::warn!(%err, what, "room action failed");
        }
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
            move || run("notification mode", session.set_notify_mode(room_id.clone(), choice))
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
