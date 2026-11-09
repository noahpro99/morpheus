use super::toast::report;
use dioxus::prelude::*;
use morpheus_core::{OwnedRoomId, RoomSummary, Session};

use super::context_menu::{MenuItem, open_menu};
use super::room_dialogs::{OpenRoomDialog, RoomDialog};
use super::shell::{Prefs, RailEntry, Rooms, Scope, SelectedSpace, UiPrefs};
use super::space_menu::space_menu;
use crate::icons::{FOLDER, Icon, Logo, PLUS};
use crate::media::Avatar;

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

/// The rail's scopes top to bottom, as drawn: DMs, Rooms, Favorites (when
/// any), then each server in the rail's order (a folder's in its order).
pub fn rail_scopes(prefs: &Prefs, rooms: &morpheus_core::Vector<RoomSummary>) -> Vec<Scope> {
    let spaces: Vec<RoomSummary> = rooms
        .iter()
        .filter(|room| room.is_space && room.parent_spaces.is_empty())
        .cloned()
        .collect();
    let mut all = vec![Scope::Dms, Scope::Rooms];
    if rooms.iter().any(|room| room.favorite && !room.invited) {
        all.push(Scope::Favorites);
    }
    for entry in layout(prefs, &spaces) {
        match entry {
            RailEntry::Space(id) => all.push(Scope::Space(id)),
            RailEntry::Folder { spaces, .. } => all.extend(spaces.into_iter().map(Scope::Space)),
        }
    }
    all
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
    // Favorites get a place of their own once there are any.
    let has_favorites = rooms
        .read()
        .iter()
        .any(|room| room.favorite && !room.invited);
    // The last one taken out: back to the DMs.
    use_effect(move || {
        let rooms = rooms.read();
        let none = !rooms.iter().any(|room| room.favorite && !room.invited);
        if none && !rooms.is_empty() && selected_space() == Scope::Favorites {
            selected_space.set(Scope::Dms);
        }
    });
    let unread_dms = rooms
        .read()
        .iter()
        // Pending invites count here too, like Discord's DM pill.
        .filter(|room| room.invited || (room.is_dm && room.highlight_count > 0))
        .count();
    // Scopes with something unread get a dot beside them on the rail.
    let unread_in = move |scope: &Scope| scope_has_unread(&rooms.read(), scope);
    let entries = layout(&prefs.read(), &spaces);
    let space_by_id = |id: &OwnedRoomId| spaces.iter().find(|s| &s.id == id).cloned();

    // Dragging a badge (RAIL_DRAG_JS): where it was let go, applied to the
    // layout as it is now drawn.
    use_future({
        let session = session.clone();
        move || {
            let session = session.clone();
            async move {
                let mut script = document::eval(RAIL_DRAG_JS);
                while let Ok(dropped) = script.recv::<serde_json::Value>().await {
                    let Some(moved) = dropped["from"]
                        .as_str()
                        .and_then(|id| OwnedRoomId::try_from(id).ok())
                    else {
                        continue;
                    };
                    let index = dropped["index"].as_u64().unwrap_or(0) as usize;
                    let onto = dropped["onto"].as_bool().unwrap_or(false);
                    let spaces: Vec<RoomSummary> = rooms
                        .peek()
                        .iter()
                        .filter(|room| room.is_space && room.parent_spaces.is_empty())
                        .cloned()
                        .collect();
                    let entries = layout(&prefs.peek(), &spaces);
                    let target = if onto {
                        entries.get(index).cloned()
                    } else {
                        None
                    };
                    prefs.with_mut(|p| move_badge(p, entries, moved, index, target));
                    save_prefs(&session, &prefs.peek());
                }
            }
        }
    });

    rsx! {
        nav { class: "rail",
            button {
                class: "rail-item rail-dms",
                class: if current == Scope::Dms { "selected" },
                class: if current != Scope::Dms && (unread_dms > 0 || unread_in(&Scope::Dms)) { "unread" },
                title: "Direct messages",
                onclick: move |_| selected_space.set(Scope::Dms),
                RailPill {}
                // The mark is a wide, flat shape: draw it large so it fills the circle.
                Logo { size: 40 }
                if unread_dms > 0 {
                    span { class: "rail-badge", "{unread_dms}" }
                }
            }
            button {
                class: "rail-item rail-rooms",
                class: if current == Scope::Rooms { "selected" },
                class: if current != Scope::Rooms && unread_in(&Scope::Rooms) { "unread" },
                title: "Rooms",
                onclick: move |_| selected_space.set(Scope::Rooms),
                RailPill {}
                "#"
            }
            if has_favorites {
                button {
                    class: "rail-item rail-favorites",
                    class: if current == Scope::Favorites { "selected" },
                    class: if current != Scope::Favorites && unread_in(&Scope::Favorites) { "unread" },
                    title: "Favorites",
                    onclick: move |_| selected_space.set(Scope::Favorites),
                    RailPill {}
                    Icon { glyph: crate::icons::STAR, size: 20 }
                }
            }
            div { class: "rail-divider" }
            for (index, entry) in entries.iter().cloned().enumerate() {
                match entry.clone() {
                    RailEntry::Space(id) => {
                        if let Some(space) = space_by_id(&id) {
                            rsx! { SpaceBadge { key: "{space.id}", unread: unread_in(&Scope::Space(space.id.clone())), space, folder: None, index: Some(index), selected: current.space_id() == Some(&id) } }
                        } else {
                            rsx! {}
                        }
                    }
                    RailEntry::Folder { id, name, spaces: members } => {
                        let open = open_folders().contains(&id);
                        let members: Vec<RoomSummary> = members.iter().filter_map(space_by_id).collect();
                        let folder_unread = members.iter().any(|s| unread_in(&Scope::Space(s.id.clone())));
                        let contains_current = members.iter().any(|s| current.space_id() == Some(&s.id));
                        rsx! {
                            div { key: "{id}", class: "rail-folder", "data-index": "{index}", "data-folder": "{id}", class: if open { "open" }, class: if contains_current { "has-current" },
                                button {
                                    class: "rail-item folder-badge",
                                    class: if contains_current && !open { "selected" },
                                    class: if !contains_current && folder_unread { "unread" },
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
                                    RailPill {}
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
                                            SpaceBadge { key: "{space.id}", unread: unread_in(&Scope::Space(space.id.clone())), space: space.clone(), folder: Some(id.clone()), index: None, selected: current.space_id() == Some(&space.id) }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
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

/// One space badge: click to scope, right-click for the server menu. Pressed
/// and moved, it is dragged (see [`RAIL_DRAG_JS`]): a line shows where it will
/// land, and dropping it on another server starts a folder.
#[component]
fn SpaceBadge(
    space: RoomSummary,
    folder: Option<String>,
    selected: bool,
    unread: bool,
    /// Its place in the rail, for a top-level badge (one in a folder has its
    /// folder's).
    index: Option<usize>,
) -> Element {
    let mut selected_space = use_context::<SelectedSpace>();
    let session = use_context::<Session>();
    let rooms = use_context::<Rooms>();
    let id = space.id.clone();
    let _ = &folder;

    rsx! {
        button {
            class: "rail-item space-badge",
            class: if selected { "selected" },
            class: if unread && !selected { "unread" },
            title: "{space.name}",
            "data-space": "{space.id}",
            "data-index": index.map(|i| i.to_string()),
            onclick: {
                let id = id.clone();
                move |_| selected_space.set(Scope::Space(id.clone()))
            },
            oncontextmenu: {
                let space = space.clone();
                let session = session.clone();
                move |e| open_menu(&e, space_menu(&session, rooms, &space), None)
            },
            RailPill {}
            Avatar { media: space.avatar.clone(), fallback: space.name.clone(), size: 48 }
        }
    }
}

/// Whether a scope holds an unread room (muted ones aside), for the dot
/// beside its badge.
fn scope_has_unread(rooms: &morpheus_core::Vector<RoomSummary>, scope: &Scope) -> bool {
    super::shell::rooms_in_scope(rooms, scope)
        .iter()
        .any(|room| {
            room.invited
                || (!room.muted
                    && (room.unread_messages > 0 || room.marked_unread || room.highlight_count > 0))
        })
}

/// The mark left of a rail badge: a tall bar for the scope you are in, a
/// dot for one with unread messages (drawn by CSS from the badge's classes).
#[component]
fn RailPill() -> Element {
    rsx! { span { class: "rail-pill" } }
}

/// Put `moved` where it was dropped in the drawn layout `entries`: onto
/// another server (a new folder of the two, as in Discord), onto a folder
/// (into it), or before the entry at `before`.
fn move_badge(
    p: &mut Prefs,
    entries: Vec<RailEntry>,
    moved: OwnedRoomId,
    before: usize,
    onto: Option<RailEntry>,
) {
    // Taken out first: a server moved down lands one place higher.
    let was = entries
        .iter()
        .position(|e| matches!(e, RailEntry::Space(s) if *s == moved));
    let before = match was {
        Some(was) if was < before => before - 1,
        _ => before,
    };
    // Work on the drawn layout so unknown spaces have a position.
    p.rail = entries;
    p.detach(&moved);
    match onto {
        Some(RailEntry::Folder { id, .. }) => {
            if let Some(members) = p.folder_mut(&id) {
                members.push(moved);
            }
        }
        Some(RailEntry::Space(target)) if target != moved => {
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
        Some(_) => {}
        None => {
            let index = before.min(p.rail.len());
            p.rail.insert(index, RailEntry::Space(moved));
        }
    }
}

/// Dragging a server on the rail, by hand rather than the browser's drag
/// (which drags a picture of the badge): pressed and moved, the pointer
/// grabs, a white line shows where it would land (or the server under it
/// is marked, to make a folder of the two), nothing else moves, and on
/// letting go the rail is told `{from, index, onto}`. Esc cancels.
const RAIL_DRAG_JS: &str = r#"
window.__railDragSend = m => dioxus.send(m);
if (!window.__railDrag) {
  window.__railDrag = true;
  let press = null, dragging = null, line = null, target = null, swallow = false;
  const entries = () => [...document.querySelectorAll('.rail > [data-index]')];
  const end = () => {
    document.body.classList.remove('rail-dragging');
    if (dragging) dragging.classList.remove('dragging');
    document.querySelectorAll('.rail .drop-into').forEach(e => e.classList.remove('drop-into'));
    if (line) line.remove();
    press = dragging = line = target = null;
  };
  // Where a drop at height y goes: onto an entry's middle, else the gap.
  const where = y => {
    const list = entries();
    if (!list.length) return null;
    for (let i = 0; i < list.length; i++) {
      const el = list[i], r = el.getBoundingClientRect();
      const prev = i > 0 ? list[i - 1].getBoundingClientRect().bottom : r.top - 8;
      const next = i + 1 < list.length ? list[i + 1].getBoundingClientRect().top : r.bottom + 8;
      if (y < r.top + r.height * 0.25) return { index: +el.dataset.index, onto: false, edge: (prev + r.top) / 2 };
      if (y <= r.bottom - r.height * 0.25) return el.contains(dragging) ? null : { index: +el.dataset.index, onto: true, el };
      if (y < next) return { index: +el.dataset.index + 1, onto: false, edge: (r.bottom + next) / 2 };
    }
    const last = list[list.length - 1];
    return { index: +last.dataset.index + 1, onto: false, edge: last.getBoundingClientRect().bottom + 4 };
  };
  document.addEventListener('mousedown', e => {
    if (e.button !== 0) return;
    const badge = e.target.closest && e.target.closest('.rail .space-badge');
    if (badge) press = { badge, x: e.clientX, y: e.clientY };
  }, true);
  document.addEventListener('mousemove', e => {
    if (!press) return;
    if (!dragging) {
      if (Math.hypot(e.clientX - press.x, e.clientY - press.y) < 5) return;
      dragging = press.badge;
      dragging.classList.add('dragging');
      document.body.classList.add('rail-dragging');
      line = document.createElement('div');
      line.className = 'rail-drop-line';
      document.body.append(line);
    }
    e.preventDefault();
    document.querySelectorAll('.rail .drop-into').forEach(el => el.classList.remove('drop-into'));
    target = where(e.clientY);
    const rail = document.querySelector('.rail').getBoundingClientRect();
    if (target && !target.onto) {
      line.style.display = 'block';
      line.style.left = (rail.left + 12) + 'px';
      line.style.width = (rail.width - 24) + 'px';
      line.style.top = (target.edge - 1) + 'px';
    } else {
      line.style.display = 'none';
      if (target) (target.el.querySelector('.rail-item') || target.el).classList.add('drop-into');
    }
  }, true);
  document.addEventListener('mouseup', e => {
    if (!press) return;
    const was = dragging, at = target;
    if (was) {
      // Only a click that comes with this release (one the browser sends
      // when the pointer is let go over the badge it pressed).
      swallow = true;
      setTimeout(() => { swallow = false; }, 0);
      if (at) window.__railDragSend({ from: was.dataset.space, index: at.index, onto: at.onto });
    }
    end();
  }, true);
  // The click that ends a drag is not a click on the badge.
  document.addEventListener('click', e => {
    if (swallow) { swallow = false; e.preventDefault(); e.stopPropagation(); }
  }, true);
  document.addEventListener('keydown', e => { if (e.key === 'Escape' && dragging) { swallow = false; end(); } }, true);
}
"#;
