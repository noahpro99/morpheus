//! The bar across the very top, replacing the native title bar: back and
//! forward through visited rooms, the current scope's name, help, the inbox
//! (rooms with unread or mentions), and window controls when the platform
//! provides them.

use dioxus::prelude::*;
use morpheus_core::{OwnedRoomId, RoomSummary, Session};
use tracing::warn;

use super::invites::InboxInvites;
use super::shell::{NavHistory, NoticeLog, Rooms, Scope, Selected, SelectedSpace, go_to, scope_for};
use crate::WindowControls;
use crate::icons::{CHEVRON_LEFT, CHEVRON_RIGHT, CIRCLE_HELP, INBOX, Icon, Logo, MINUS, SQUARE, X};

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
}

/// Keep at most this many visited rooms.
const HISTORY_CAP: usize = 50;

impl Nav {
    pub fn record(&mut self, room: OwnedRoomId) {
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
        .filter(|r| !r.is_space && !r.invited && (r.unread_messages > 0 || r.highlight_count > 0 || r.marked_unread))
        .count();
    // For tests and debugging: the visited rooms by name.
    let history_names = nav()
        .entries
        .iter()
        .map(|id| rooms.read().iter().find(|r| &r.id == id).map(|r| r.name.clone()).unwrap_or_else(|| "?".into()))
        .collect::<Vec<_>>()
        .join(",");

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
            div { class: "titlebar-nav", "data-nav": "{nav().index}/{nav().entries.len()}", "data-nav-entries": "{history_names}",
                button {
                    class: "ghost nav-button",
                    title: "Back",
                    disabled: !nav().can_go_back(),
                    onmousedown: move |e| e.stop_propagation(),
                    onclick: move |_| jump(-1),
                    Icon { glyph: CHEVRON_LEFT, size: 18 }
                }
                button {
                    class: "ghost nav-button",
                    title: "Forward",
                    disabled: !nav().can_go_forward(),
                    onmousedown: move |e| e.stop_propagation(),
                    onclick: move |_| jump(1),
                    Icon { glyph: CHEVRON_RIGHT, size: 18 }
                }
            }
            div { class: "titlebar-title", Logo { size: 16, class: "titlebar-logo" } "{title}" }
            div { class: "titlebar-actions",
                button {
                    class: "ghost titlebar-button help-button",
                    title: "Help",
                    onmousedown: move |e| e.stop_propagation(),
                    onclick: move |_| panel.set(if panel() == TopPanel::Help { TopPanel::None } else { TopPanel::Help }),
                    Icon { glyph: CIRCLE_HELP, size: 16 }
                }
                button {
                    class: "ghost titlebar-button inbox-button",
                    title: "Inbox",
                    onmousedown: move |e| e.stop_propagation(),
                    onclick: move |_| panel.set(if panel() == TopPanel::Inbox { TopPanel::None } else { TopPanel::Inbox }),
                    Icon { glyph: INBOX, size: 16 }
                    if unread > 0 {
                        span { class: "rail-badge inbox-badge", "{unread}" }
                    }
                }
                if let Some(controls) = controls.clone() {
                    div { class: "window-controls", onmousedown: move |e| e.stop_propagation(),
                        button { class: "ghost", title: "Minimize", onclick: { let c = controls.clone(); move |_| (c.minimize)() }, Icon { glyph: MINUS, size: 14 } }
                        button { class: "ghost", title: "Maximize", onclick: { let c = controls.clone(); move |_| (c.toggle_maximize)() }, Icon { glyph: SQUARE, size: 13 } }
                        button { class: "ghost close", title: "Close", onclick: move |_| (controls.close)(), Icon { glyph: X, size: 14 } }
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
    let mut scope = use_context::<SelectedSpace>();
    let mut panel = use_context::<OpenTopPanel>();
    let mut close = move || panel.set(TopPanel::None);

    let mut items: Vec<_> = rooms
        .read()
        .iter()
        .filter(|r| !r.is_space && !r.invited && (r.unread_messages > 0 || r.highlight_count > 0 || r.marked_unread))
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
                InboxInvites {
                    on_open: move |room: RoomSummary| {
                        go_to(room.id.clone());
                        close();
                    },
                }
                InboxMentions {
                    on_open: move |room_id: OwnedRoomId| {
                        if let Some(room) = rooms.read().iter().find(|r| r.id == room_id).cloned() {
                            let room_scope = scope_for(&rooms.read(), &room);
                            if room_scope != *scope.peek() {
                                scope.set(room_scope);
                            }
                        }
                        go_to(room_id);
                        close();
                    },
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
                                go_to(room.id.clone());
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
                h2 { "Keyboard shortcuts" }
                for (group, rows) in super::hotkeys::GROUPS {
                    h3 { class: "help-group", "{group}" }
                    dl { class: "help-list",
                        for (keys, what) in rows.iter() {
                            dt { "{keys}" }
                            dd { "{what}" }
                        }
                    }
                }
                p { class: "muted", "Morpheus is a Matrix client: your account lives on your homeserver and works in any Matrix app." }
                div { class: "settings-actions",
                    button { class: "secondary", onclick: move |_| close(), "Close" }
                }
            }
        }
    }
}

/// Move the history cursor by `step` and open that room, switching scope
/// first so the shell does not bounce the selection back out of it.
pub fn nav_jump(
    step: isize,
    mut nav: NavHistory,
    rooms: Rooms,
    mut scope: SelectedSpace,
    mut selected: Selected,
) {
    let target = {
        let current = nav.peek();
        match current.index.checked_add_signed(step) {
            Some(i) if i < current.entries.len() => Some((i, current.entries[i].clone())),
            _ => None,
        }
    };
    let Some((index, room_id)) = target else { return };
    nav.with_mut(|n| n.index = index);
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
}

/// Recent mentions, newest first, from what was announced since launch.
#[component]
fn InboxMentions(on_open: Callback<OwnedRoomId>) -> Element {
    let notices = use_context::<NoticeLog>();
    let mentions: Vec<_> = notices().0.iter().rev().filter(|n| n.is_mention).take(10).cloned().collect();
    if mentions.is_empty() {
        return rsx! {};
    }
    rsx! {
        h4 { class: "section", "Mentions" }
        for (index, notice) in mentions.into_iter().enumerate() {
            button {
                key: "{index}",
                class: "inbox-item inbox-mention",
                onclick: {
                    let room_id = notice.room_id.clone();
                    move |_| on_open.call(room_id.clone())
                },
                span { class: "inbox-name", "{notice.sender_name} in {notice.room_name}" }
                span { class: "inbox-body", "{notice.body_summary}" }
            }
        }
    }
}
