//! The server menu: what clicking a space's name or right-clicking its rail
//! badge offers. Discord's list, mapped onto Matrix spaces.

use dioxus::prelude::*;
use morpheus_core::{NotifyMode, RoomSummary, Session};
use tracing::warn;

use super::context_menu::{MenuItem, copy_to_clipboard};
use super::room_dialogs::{OpenRoomDialog, RoomDialog};
use super::shell::{RailEntry, Rooms, Scope, SelectedSpace, UiPrefs, rooms_in_scope};

fn run(what: &'static str, future: morpheus_core::CoreFuture<()>) {
    spawn(async move {
        if let Err(err) = future.await {
            warn!(%err, what, "space action failed");
        }
    });
}

/// Items for `space`. `rooms` is the whole list; the space's own rooms are
/// derived from it.
pub fn space_menu(session: &Session, rooms: Rooms, space: &RoomSummary) -> Vec<MenuItem> {
    let space_id = space.id.clone();
    let children: Vec<RoomSummary> = rooms_in_scope(&rooms.read(), &Scope::Space(space_id.clone()));
    let child_ids: Vec<_> = children.iter().map(|r| r.id.clone()).collect();
    let all_muted = !children.is_empty() && children.iter().all(|r| r.muted);
    let mut prefs = consume_context::<UiPrefs>();
    let hide_muted = prefs.peek().hide_muted;

    let mut items = Vec::new();
    items.push(MenuItem::new("Mark as read", {
        let (session, ids) = (session.clone(), child_ids.clone());
        move || {
            for id in ids.clone() {
                run("mark read", session.mark_room_read(id));
            }
        }
    }));
    items.push(MenuItem::new("Invite to server", {
        let space_id = space_id.clone();
        move || consume_context::<OpenRoomDialog>().set(RoomDialog::Invite(space_id.clone()))
    }).icon(crate::icons::USER_PLUS));
    items.push(
        MenuItem::new(if all_muted { "Unmute server" } else { "Mute server" }, {
            let (session, ids) = (session.clone(), child_ids.clone());
            let mode = if all_muted { NotifyMode::AllMessages } else { NotifyMode::Mute };
            move || {
                for id in ids.clone() {
                    run("notification mode", session.set_notify_mode(id, mode));
                }
            }
        })
        .icon(if all_muted { crate::icons::BELL } else { crate::icons::BELL_OFF }),
    );
    let modes = [
        (NotifyMode::AllMessages, "All messages"),
        (NotifyMode::MentionsOnly, "Only mentions"),
        (NotifyMode::Mute, "Mute"),
    ]
    .into_iter()
    .map(|(mode, label)| {
        MenuItem::new(label, {
            let (session, ids) = (session.clone(), child_ids.clone());
            move || {
                for id in ids.clone() {
                    run("notification mode", session.set_notify_mode(id, mode));
                }
            }
        })
        .checked(mode == NotifyMode::Mute && all_muted)
    })
    .collect();
    items.push(MenuItem::submenu("Notifications", modes));
    items.push(
        MenuItem::new("Hide muted channels", {
            let session = session.clone();
            move || {
                prefs.with_mut(|p| p.hide_muted = !p.hide_muted);
                run("save prefs", session.set_ui_prefs(prefs.peek().to_json()));
            }
        })
        .checked(hide_muted)
        .icon(crate::icons::EYE_OFF),
    );
    items.push(MenuItem::new("Server settings", {
        let space_id = space_id.clone();
        move || consume_context::<OpenRoomDialog>().set(RoomDialog::Edit(space_id.clone()))
    }).icon(crate::icons::SETTINGS));
    items.push(MenuItem::new("Privacy settings", {
        let space_id = space_id.clone();
        move || consume_context::<OpenRoomDialog>().set(RoomDialog::Privacy(space_id.clone()))
    }).icon(crate::icons::LOCK));
    items.push(MenuItem::new("Edit per-server profile", {
        let space_id = space_id.clone();
        move || consume_context::<OpenRoomDialog>().set(RoomDialog::RoomProfile(space_id.clone()))
    }).icon(crate::icons::USER_COG));
    items.push(MenuItem::new("Create channel", {
        let space_id = space_id.clone();
        move || {
            consume_context::<SelectedSpace>().set(Scope::Space(space_id.clone()));
            consume_context::<OpenRoomDialog>().set(RoomDialog::Create);
        }
    }).icon(crate::icons::PLUS));
    items.push(MenuItem::new("Create category", {
        let space_id = space_id.clone();
        move || consume_context::<OpenRoomDialog>().set(RoomDialog::CreateSpace(Some(space_id.clone())))
    }).icon(crate::icons::FOLDER_PLUS));
    // Move to a folder (or out of one).
    {
        let layout = prefs.peek().clone();
        let mut targets: Vec<MenuItem> = layout
            .rail
            .iter()
            .filter_map(|e| match e {
                RailEntry::Folder { id, name, spaces } => Some((id.clone(), name.clone(), spaces.contains(&space_id))),
                _ => None,
            })
            .map(|(folder_id, name, inside)| {
                MenuItem::new(name, {
                    let (session, space_id, folder_id) = (session.clone(), space_id.clone(), folder_id.clone());
                    move || {
                        prefs.with_mut(|p| {
                            p.detach(&space_id);
                            if let Some(members) = p.folder_mut(&folder_id) {
                                members.push(space_id.clone());
                            }
                        });
                        super::rail::save_prefs(&session, &prefs.peek());
                    }
                })
                .checked(inside)
                .icon(crate::icons::FOLDER)
            })
            .collect();
        targets.push(
            MenuItem::new("New folder", {
                let (session, space_id) = (session.clone(), space_id.clone());
                move || {
                    prefs.with_mut(|p| {
                        p.detach(&space_id);
                        let folder = RailEntry::Folder { id: p.new_folder_id(), name: "New folder".into(), spaces: vec![space_id.clone()] };
                        p.rail.push(folder);
                    });
                    super::rail::save_prefs(&session, &prefs.peek());
                }
            })
            .icon(crate::icons::FOLDER_PLUS),
        );
        targets.push(
            MenuItem::new("No folder", {
                let (session, space_id) = (session.clone(), space_id.clone());
                move || {
                    prefs.with_mut(|p| {
                        p.detach(&space_id);
                        p.rail.push(RailEntry::Space(space_id.clone()));
                    });
                    super::rail::save_prefs(&session, &prefs.peek());
                }
            })
            .icon(crate::icons::MOVE),
        );
        items.push(MenuItem::submenu("Move to", targets).icon(crate::icons::MOVE));
    }
    items.push(
        MenuItem::new("Leave server", {
            let space_id = space_id.clone();
            move || consume_context::<OpenRoomDialog>().set(RoomDialog::LeaveSpace(space_id.clone()))
        })
        .danger(),
    );
    items.push(MenuItem::submenu(
        "Copy server info",
        vec![
            MenuItem::new("Copy server ID", {
                let id = space_id.to_string();
                move || copy_to_clipboard(&id)
            })
            .icon(crate::icons::HASH),
            MenuItem::new("Copy invite link", {
                let link = format!("https://matrix.to/#/{space_id}");
                move || copy_to_clipboard(&link)
            })
            .icon(crate::icons::LINK),
        ],
    ).icon(crate::icons::INFO));
    items
}
