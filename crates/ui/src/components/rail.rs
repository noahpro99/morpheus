use super::toast::report;
use dioxus::prelude::*;
use morpheus_core::{OwnedRoomId, RoomSummary, Session};

use super::context_menu::{MenuItem, open_menu};
use super::room_dialogs::{OpenRoomDialog, RoomDialog};
use super::shell::{Prefs, RailEntry, Rooms, Scope, SelectedSpace, UiPrefs};
use super::space_menu::space_menu;
use crate::icons::{FOLDER, Icon, Logo, PLUS};
use crate::media::Avatar;

/// What is being dragged in the rail, if anything.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dragging(pub Option<OwnedRoomId>);
pub type RailDrag = Signal<Dragging>;

/// The rail as drawn: the saved layout, then spaces the layout does not know.
fn layout(prefs: &Prefs, spaces: &[RoomSummary]) -> Vec<RailEntry> {
    let known = |id: &OwnedRoomId| {
        prefs.rail.iter().any(|e| match e {
            RailEntry::Space(s) => s == id,
            RailEntry::Folder { spaces, .. } => spaces.contains(id),
        })
    };
    let mut entries: Vec<RailEntry> = prefs
        .rail
        .iter()
        .filter_map(|entry| match entry {
            // Spaces we left or never had drop out of the drawing.
            RailEntry::Space(id) => spaces.iter().any(|s| &s.id == id).then(|| entry.clone()),
            RailEntry::Folder {
                id,
                name,
                spaces: members,
            } => {
                let present: Vec<OwnedRoomId> = members
                    .iter()
                    .filter(|m| spaces.iter().any(|s| &s.id == *m))
                    .cloned()
                    .collect();
                (!present.is_empty()).then(|| RailEntry::Folder {
                    id: id.clone(),
                    name: name.clone(),
                    spaces: present,
                })
            }
        })
        .collect();
    entries.extend(
        spaces
            .iter()
            .filter(|s| !known(&s.id))
            .map(|s| RailEntry::Space(s.id.clone())),
    );
    entries
}

/// Persist the layout after a change.
pub fn save_prefs(session: &Session, prefs: &Prefs) {
    let future = session.set_ui_prefs(prefs.to_json());
    spawn(async move {
        if let Err(err) = future.await {
            report("saving the rail layout", &err);
        }
    });
}

/// Left-most column: direct messages, ungrouped rooms, then one badge per
/// top-level space (or folder of spaces). Badges drag to reorder; dropping
/// one onto another makes a folder.
#[component]
pub fn Rail() -> Element {
    let rooms = use_context::<Rooms>();
    let mut selected_space = use_context::<SelectedSpace>();
    let session = use_context::<Session>();
    let mut prefs = use_context::<UiPrefs>();
    let mut drag = use_context::<RailDrag>();
    let mut open_folders = use_signal(Vec::<String>::new);

    // Only top-level spaces get their own badge; a nested space's rooms show
    // up when its top-level ancestor is selected instead.
    let spaces: Vec<RoomSummary> = rooms
        .read()
        .iter()
        .filter(|room| room.is_space && room.parent_spaces.is_empty())
        .cloned()
        .collect();
    let current = selected_space();
    let unread_dms = rooms
        .read()
        .iter()
        // Pending invites count here too, like Discord's DM pill.
        .filter(|room| room.invited || (room.is_dm && room.highlight_count > 0))
        .count();
    let entries = layout(&prefs.read(), &spaces);
    let space_by_id = |id: &OwnedRoomId| spaces.iter().find(|s| &s.id == id).cloned();

    // Drop handling: `before` is where in the drawn list the drop landed.
    let drop_at = Callback::new({
        let session = session.clone();
        let entries = entries.clone();
        move |(before, onto): (usize, Option<RailEntry>)| {
            let Some(moved) = drag.peek().0.clone() else {
                return;
            };
            drag.set(Dragging::default());
            prefs.with_mut(|p| {
                // Work on the drawn layout so unknown spaces have a position.
                p.rail = entries.clone();
                p.detach(&moved);
                match onto {
                    Some(RailEntry::Folder { id, .. }) => {
                        if let Some(members) = p.folder_mut(&id) {
                            members.push(moved);
                        }
                    }
                    Some(RailEntry::Space(target)) if target != moved => {
                        // Two badges together make a folder, like Discord.
                        let folder = RailEntry::Folder {
                            id: p.new_folder_id(),
                            name: "New folder".into(),
                            spaces: vec![target.clone(), moved],
                        };
                        let pos = p
                            .rail
                            .iter()
                            .position(|e| matches!(e, RailEntry::Space(s) if *s == target))
                            .unwrap_or(p.rail.len());
                        p.rail.remove(pos.min(p.rail.len().saturating_sub(1)));
                        p.rail.insert(pos.min(p.rail.len()), folder);
                    }
                    _ => {
                        let index = before.min(p.rail.len());
                        p.rail.insert(index, RailEntry::Space(moved));
                    }
                }
            });
            save_prefs(&session, &prefs.peek());
        }
    });

    rsx! {
        nav { class: "rail",
            button {
                class: "rail-item rail-dms",
                class: if current == Scope::Dms { "selected" },
                title: "Direct messages",
                onclick: move |_| selected_space.set(Scope::Dms),
                // The mark is a wide, flat shape: draw it large so it fills the circle.
                Logo { size: 40 }
                if unread_dms > 0 {
                    span { class: "rail-badge", "{unread_dms}" }
                }
            }
            button {
                class: "rail-item rail-rooms",
                class: if current == Scope::Rooms { "selected" },
                title: "Rooms",
                onclick: move |_| selected_space.set(Scope::Rooms),
                "#"
            }
            div { class: "rail-divider" }
            for (index, entry) in entries.iter().cloned().enumerate() {
                // A thin drop zone before each badge reorders.
                div {
                    key: "drop-{index}",
                    class: "rail-drop",
                    class: if drag().0.is_some() { "active" },
                    ondragover: move |e| e.prevent_default(),
                    ondrop: move |e| {
                        e.prevent_default();
                        drop_at.call((index, None));
                    },
                }
                match entry.clone() {
                    RailEntry::Space(id) => {
                        if let Some(space) = space_by_id(&id) {
                            rsx! { SpaceBadge { key: "{space.id}", space, folder: None, selected: current.space_id() == Some(&id), on_drop: move |target: RailEntry| drop_at.call((index, Some(target))) } }
                        } else {
                            rsx! {}
                        }
                    }
                    RailEntry::Folder { id, name, spaces: members } => {
                        let open = open_folders().contains(&id);
                        let members: Vec<RoomSummary> = members.iter().filter_map(space_by_id).collect();
                        let contains_current = members.iter().any(|s| current.space_id() == Some(&s.id));
                        let folder_entry = RailEntry::Folder { id: id.clone(), name: name.clone(), spaces: members.iter().map(|s| s.id.clone()).collect() };
                        rsx! {
                            div { key: "{id}", class: "rail-folder", class: if open { "open" }, class: if contains_current { "has-current" },
                                button {
                                    class: "rail-item folder-badge",
                                    title: "{name}",
                                    onclick: {
                                        let id = id.clone();
                                        move |_| open_folders.with_mut(|list| {
                                            if let Some(pos) = list.iter().position(|f| *f == id) { list.remove(pos); } else { list.push(id.clone()); }
                                        })
                                    },
                                    oncontextmenu: {
                                        let (id, name) = (id.clone(), name.clone());
                                        let session = session.clone();
                                        move |e| {
                                            let items = vec![
                                                MenuItem::new("Rename folder", {
                                                    let id = id.clone();
                                                    move || consume_context::<OpenRoomDialog>().set(RoomDialog::RenameFolder(id.clone()))
                                                }).icon(crate::icons::PENCIL),
                                                MenuItem::new("Ungroup", {
                                                    let (id, session) = (id.clone(), session.clone());
                                                    move || {
                                                        prefs.with_mut(|p| {
                                                            let Some(pos) = p.rail.iter().position(|e| matches!(e, RailEntry::Folder { id: f, .. } if *f == id)) else { return };
                                                            if let RailEntry::Folder { spaces, .. } = p.rail.remove(pos) {
                                                                for (offset, space) in spaces.into_iter().enumerate() {
                                                                    p.rail.insert(pos + offset, RailEntry::Space(space));
                                                                }
                                                            }
                                                        });
                                                        save_prefs(&session, &prefs.peek());
                                                    }
                                                }).icon(crate::icons::FOLDER),
                                            ];
                                            let _ = &name;
                                            open_menu(&e, items, None);
                                        }
                                    },
                                    ondragover: move |e| e.prevent_default(),
                                    ondrop: {
                                        let folder_entry = folder_entry.clone();
                                        move |e| {
                                            e.prevent_default();
                                            e.stop_propagation();
                                            drop_at.call((index, Some(folder_entry.clone())));
                                        }
                                    },
                                    if open {
                                        Icon { glyph: FOLDER, size: 20 }
                                    } else {
                                        div { class: "folder-grid",
                                            for space in members.iter().take(4) {
                                                Avatar { key: "{space.id}", media: space.avatar.clone(), fallback: space.name.clone(), size: 18 }
                                            }
                                        }
                                    }
                                }
                                if open {
                                    div { class: "folder-items",
                                        for space in members {
                                            SpaceBadge { key: "{space.id}", space: space.clone(), folder: Some(id.clone()), selected: current.space_id() == Some(&space.id), on_drop: move |target: RailEntry| drop_at.call((index, Some(target))) }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            div {
                class: "rail-drop",
                class: if drag().0.is_some() { "active" },
                ondragover: move |e| e.prevent_default(),
                ondrop: {
                    let len = entries.len();
                    move |e| {
                        e.prevent_default();
                        drop_at.call((len, None));
                    }
                },
            }
            button {
                class: "rail-item rail-add",
                title: "Create a server",
                onclick: move |_| consume_context::<OpenRoomDialog>().set(RoomDialog::CreateSpace(None)),
                Icon { glyph: PLUS, size: 22 }
            }
        }
    }
}

/// One space badge: click to scope, right-click for the server menu, drag
/// to move; a drop on it starts a folder (or, inside a folder, joins it).
#[component]
fn SpaceBadge(
    space: RoomSummary,
    folder: Option<String>,
    selected: bool,
    on_drop: Callback<RailEntry>,
) -> Element {
    let mut selected_space = use_context::<SelectedSpace>();
    let session = use_context::<Session>();
    let rooms = use_context::<Rooms>();
    let mut drag = use_context::<RailDrag>();
    let id = space.id.clone();
    let target = match &folder {
        Some(folder_id) => RailEntry::Folder {
            id: folder_id.clone(),
            name: String::new(),
            spaces: Vec::new(),
        },
        None => RailEntry::Space(space.id.clone()),
    };

    rsx! {
        button {
            class: "rail-item space-badge",
            class: if selected { "selected" },
            class: if drag().0.as_ref() == Some(&id) { "dragging" },
            title: "{space.name}",
            draggable: "true",
            "data-space": "{space.id}",
            onclick: {
                let id = id.clone();
                move |_| selected_space.set(Scope::Space(id.clone()))
            },
            oncontextmenu: {
                let space = space.clone();
                let session = session.clone();
                move |e| open_menu(&e, space_menu(&session, rooms, &space), None)
            },
            ondragstart: {
                let id = id.clone();
                move |_| drag.set(Dragging(Some(id.clone())))
            },
            ondragend: move |_| drag.set(Dragging::default()),
            ondragover: move |e| e.prevent_default(),
            ondrop: {
                let target = target.clone();
                move |e| {
                    e.prevent_default();
                    e.stop_propagation();
                    on_drop(target.clone());
                }
            },
            Avatar { media: space.avatar.clone(), fallback: space.name.clone(), size: 48 }
        }
    }
}
