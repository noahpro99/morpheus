//! The bar across the very top, replacing the native title bar: back and
//! forward through visited rooms, the current scope's name, help, the inbox
//! (rooms with unread or mentions), and window controls when the platform
//! provides them.

use dioxus::prelude::*;
use morpheus_core::{OwnedRoomId, Session};
use tracing::warn;

use super::shell::{NavHistory, Rooms, Scope, Selected, SelectedSpace, scope_for};
use crate::WindowControls;

/// Which top-bar panel is open. Provided by the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TopPanel {
    #[default]
    None,
    Inbox,
    Help,
}
pub type OpenTopPanel = Signal<TopPanel>;

/// Visited rooms in order, with a cursor for back and forward.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Nav {
    pub entries: Vec<OwnedRoomId>,
    pub index: usize,
    /// Set while back/forward changes the selection, so the move is not
    /// recorded as a new visit.
    pub jumping: bool,
}

/// Keep at most this many visited rooms.
const HISTORY_CAP: usize = 50;

impl Nav {
    pub fn record(&mut self, room: OwnedRoomId) {
        if self.jumping {
            self.jumping = false;
            return;
        }
        if self.entries.get(self.index) == Some(&room) {
            return;
        }
        if !self.entries.is_empty() {
            self.entries.truncate(self.index + 1);
        }
        self.entries.push(room);
        if self.entries.len() > HISTORY_CAP {
            self.entries.remove(0);
        }
        self.index = self.entries.len() - 1;
    }

    pub fn can_go_back(&self) -> bool {
        self.index > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.index + 1 < self.entries.len()
    }
}

#[component]
pub fn TitleBar() -> Element {
    let rooms = use_context::<Rooms>();
    let mut selected = use_context::<Selected>();
    let mut scope = use_context::<SelectedSpace>();
    let mut nav = use_context::<NavHistory>();
    let mut panel = use_context::<OpenTopPanel>();
    let controls = use_hook(try_consume_context::<WindowControls>);

    let title = match scope() {
        Scope::Dms => "Direct messages".to_string(),
        Scope::Rooms => "Rooms".to_string(),
        Scope::Space(id) => rooms
            .read()
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.name.clone())
            .unwrap_or_else(|| "Space".into()),
    };
    let unread = rooms
        .read()
        .iter()
        .filter(|r| !r.is_space && (r.unread_messages > 0 || r.highlight_count > 0 || r.marked_unread))
        .count();

    // Move the cursor and open that room (scope first, so the shell does not
    // bounce the selection back out of it).
    let mut jump = move |step: isize| {
        let target = {
            let current = nav.peek();
            let next = current.index.checked_add_signed(step);
            match next {
                Some(i) if i < current.entries.len() => Some((i, current.entries[i].clone())),
                _ => None,
            }
        };
        let Some((index, room_id)) = target else { return };
        nav.with_mut(|n| {
            n.index = index;
            n.jumping = true;
        });
        let room_scope = rooms
            .read()
            .iter()
            .find(|r| r.id == room_id)
            .map(|room| scope_for(&rooms.read(), room));
        if let Some(room_scope) = room_scope
            && room_scope != *scope.peek()
        {
            scope.set(room_scope);
        }
        selected.set(Some(room_id));
    };

    let drag = controls.clone();
    let maximize = controls.clone();
    rsx! {
        header {
            class: "titlebar",
            // Anywhere that is not a button drags the window.
            onmousedown: move |e| {
                if e.trigger_button() == Some(dioxus::html::input_data::MouseButton::Primary)
                    && let Some(controls) = &drag
                {
                    (controls.drag)();
                }
            },
            ondoubleclick: move |_| {
                if let Some(controls) = &maximize {
                    (controls.toggle_maximize)();
                }
            },
            div { class: "titlebar-nav",
                button {
                    class: "ghost nav-button",
                    title: "Back",
                    disabled: !nav().can_go_back(),
                    onmousedown: move |e| e.stop_propagation(),
                    onclick: move |_| jump(-1),
                    "‹"
                }
                button {
                    class: "ghost nav-button",
                    title: "Forward",
                    disabled: !nav().can_go_forward(),
                    onmousedown: move |e| e.stop_propagation(),
                    onclick: move |_| jump(1),
                    "›"
                }
            }
            div { class: "titlebar-title", "{title}" }
            div { class: "titlebar-actions",
                button {
                    class: "ghost titlebar-button help-button",
                    title: "Help",
                    onmousedown: move |e| e.stop_propagation(),
                    onclick: move |_| panel.set(if panel() == TopPanel::Help { TopPanel::None } else { TopPanel::Help }),
                    "?"
                }
                button {
                    class: "ghost titlebar-button inbox-button",
                    title: "Inbox",
                    onmousedown: move |e| e.stop_propagation(),
                    onclick: move |_| panel.set(if panel() == TopPanel::Inbox { TopPanel::None } else { TopPanel::Inbox }),
                    "✉"
                    if unread > 0 {
                        span { class: "rail-badge inbox-badge", "{unread}" }
                    }
                }
                if let Some(controls) = controls.clone() {
                    div { class: "window-controls", onmousedown: move |e| e.stop_propagation(),
                        button { class: "ghost", title: "Minimize", onclick: { let c = controls.clone(); move |_| (c.minimize)() }, "–" }
                        button { class: "ghost", title: "Maximize", onclick: { let c = controls.clone(); move |_| (c.toggle_maximize)() }, "▢" }
                        button { class: "ghost close", title: "Close", onclick: move |_| (controls.close)(), "✕" }
                    }
                }
            }
        }
    }
}

/// Rooms with something unread, mentions first. Click to open.
#[component]
pub fn InboxPanel() -> Element {
    let rooms = use_context::<Rooms>();
    let session = use_context::<Session>();
    let mut selected = use_context::<Selected>();
    let mut scope = use_context::<SelectedSpace>();
    let mut panel = use_context::<OpenTopPanel>();
    let mut close = move || panel.set(TopPanel::None);

    let mut items: Vec<_> = rooms
        .read()
        .iter()
        .filter(|r| !r.is_space && (r.unread_messages > 0 || r.highlight_count > 0 || r.marked_unread))
        .cloned()
        .collect();
    items.sort_by(|a, b| b.highlight_count.cmp(&a.highlight_count).then_with(|| a.name.cmp(&b.name)));

    let mark_all = {
        let session = session.clone();
        let ids: Vec<OwnedRoomId> = items.iter().map(|r| r.id.clone()).collect();
        move |_| {
            for id in ids.clone() {
                let future = session.mark_room_read(id);
                spawn(async move {
                    if let Err(err) = future.await {
                        warn!(%err, "mark read failed");
                    }
                });
            }
        }
    };

    rsx! {
        div { class: "overlay panel-overlay", onclick: move |_| close(),
            div { class: "inbox-panel", role: "dialog", onclick: move |e| e.stop_propagation(),
                header { class: "inbox-header",
                    h3 { "Inbox" }
                    if !items.is_empty() {
                        button { class: "ghost", onclick: mark_all, "Mark all as read" }
                    }
                }
                if items.is_empty() {
                    p { class: "muted pad", "You're all caught up." }
                }
                for room in items {
                    button {
                        key: "{room.id}",
                        class: "inbox-item",
                        onclick: {
                            let room = room.clone();
                            move |_| {
                                let room_scope = scope_for(&rooms.read(), &room);
                                if room_scope != *scope.peek() {
                                    scope.set(room_scope);
                                }
                                selected.set(Some(room.id.clone()));
                                close();
                            }
                        },
                        span { class: "inbox-name", if room.is_dm { "@" } else { "#" } "{room.name}" }
                        if room.highlight_count > 0 {
                            span { class: "badge", "{room.highlight_count}" }
                        } else if room.unread_messages > 0 {
                            span { class: "muted", "{room.unread_messages} new" }
                        } else {
                            span { class: "muted", "marked unread" }
                        }
                    }
                }
            }
        }
    }
}

/// Keyboard shortcuts and how things work.
#[component]
pub fn HelpModal() -> Element {
    let mut panel = use_context::<OpenTopPanel>();
    let mut close = move || panel.set(TopPanel::None);
    rsx! {
        div { class: "overlay", onclick: move |_| close(),
            div { class: "message-dialog help-dialog", tabindex: "-1", autofocus: true, onclick: move |e| e.stop_propagation(),
                h2 { "Help" }
                dl { class: "help-list",
                    dt { "Ctrl+K" } dd { "Jump to any room." }
                    dt { "Esc" } dd { "Close the menu or dialog on top." }
                    dt { "Right-click" } dd { "Actions for a message, a room, or a member." }
                    dt { "Enter" } dd { "Send; Shift+Enter for a new line." }
                    dt { "Back / Forward" } dd { "Walk through the rooms you visited." }
                }
                p { class: "muted", "Morpheus is a Matrix client: your account lives on your homeserver and works in any Matrix app." }
                div { class: "settings-actions",
                    button { class: "secondary", onclick: move |_| close(), "Close" }
                }
            }
        }
    }
}
