use dioxus::prelude::*;
use morpheus_core::RoomSummary;

use super::about::{AboutOpen, AboutShown};
use super::settings::SettingsOpen;
use super::shell::{Rooms, SelectedSpace, go_to, scope_for};
use super::titlebar::{OpenTopPanel, TopPanel};
use crate::icons::{CIRCLE_HELP, HEART, INBOX, Icon, REFRESH_CW, SETTINGS};
use crate::media::Avatar;

/// What a row in the palette does: open a room, or run an app action.
#[derive(Clone, PartialEq)]
enum Entry {
    Room(Box<RoomSummary>),
    Action(Action),
}

impl Entry {
    fn key(&self) -> String {
        match self {
            Entry::Room(room) => room.id.to_string(),
            Entry::Action(action) => action.label().to_owned(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Settings,
    Shortcuts,
    Inbox,
    CheckForUpdates,
    Acknowledgements,
}

impl Action {
    const ALL: [Action; 5] = [
        Action::Settings,
        Action::Shortcuts,
        Action::Inbox,
        Action::CheckForUpdates,
        Action::Acknowledgements,
    ];

    fn label(self) -> &'static str {
        match self {
            Action::Settings => "Settings",
            Action::Shortcuts => "Keyboard shortcuts",
            Action::Inbox => "Inbox",
            Action::CheckForUpdates => "Check for updates",
            Action::Acknowledgements => "Acknowledgements",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Action::Settings => SETTINGS,
            Action::Shortcuts => CIRCLE_HELP,
            Action::Inbox => INBOX,
            Action::CheckForUpdates => REFRESH_CW,
            Action::Acknowledgements => HEART,
        }
    }

    fn run(self) {
        match self {
            Action::Settings => consume_context::<SettingsOpen>().set(true),
            Action::Shortcuts => consume_context::<OpenTopPanel>().set(TopPanel::Help),
            Action::Inbox => consume_context::<OpenTopPanel>().set(TopPanel::Inbox),
            // Through the shell: this palette closes (and takes its tasks along) at once.
            Action::CheckForUpdates => consume_context::<crate::AppConfig>()
                .commands
                .send(crate::AppCommand::CheckForUpdates),
            Action::Acknowledgements => consume_context::<AboutOpen>().set(AboutShown(true)),
        }
    }
}

/// Ctrl+K command palette: type to filter rooms and actions, arrows to
/// move, Enter to pick. Rooms come first; actions match on their label.
#[component]
pub fn QuickSwitcher(open: Signal<bool>) -> Element {
    let rooms = use_context::<Rooms>();
    let mut selected_space = use_context::<SelectedSpace>();
    let mut query = use_signal(String::new);
    let mut cursor = use_signal(|| 0usize);

    let matches: Vec<Entry> = {
        let needle = query().to_lowercase();
        let mut list: Vec<Entry> = rooms
            .read()
            .iter()
            .filter(|room| {
                !room.is_space
                    && !room.invited
                    && (needle.is_empty() || room.name.to_lowercase().contains(&needle))
            })
            .take(8)
            .cloned()
            .map(|room| Entry::Room(Box::new(room)))
            .collect();
        list.extend(
            Action::ALL
                .into_iter()
                .filter(|action| {
                    needle.is_empty() || action.label().to_lowercase().contains(&needle)
                })
                .map(Entry::Action),
        );
        list
    };
    let cursor_now = cursor().min(matches.len().saturating_sub(1));

    let mut close = move || {
        open.set(false);
        query.set(String::new());
        cursor.set(0);
    };

    let choose = {
        let matches = matches.clone();
        move |index: usize| {
            match matches.get(index) {
                Some(Entry::Room(room)) => {
                    // Scope first, so the shell does not bounce the selection back.
                    let scope = scope_for(&rooms.read(), room);
                    if scope != *selected_space.peek() {
                        selected_space.set(scope);
                    }
                    go_to(room.id.clone());
                }
                Some(Entry::Action(action)) => action.run(),
                None => {}
            }
            close();
        }
    };

    rsx! {
        div { class: "overlay", onclick: move |_| close(),
            div { class: "switcher", onclick: move |e| e.stop_propagation(),
                input {
                    r#type: "text",
                    autofocus: true,
                    // `autofocus` is unreliable in the webview; focus explicitly.
                    onmounted: move |e| {
                        spawn(async move {
                            let _ = e.data().set_focus(true).await;
                        });
                    },
                    placeholder: "Jump to a room or an action…",
                    value: "{query}",
                    oninput: move |e| {
                        query.set(e.value());
                        cursor.set(0);
                    },
                    onkeydown: {
                        let mut choose = choose.clone();
                        move |e| match e.key() {
                        Key::ArrowDown => cursor.set(cursor_now + 1),
                        Key::ArrowUp => cursor.set(cursor_now.saturating_sub(1)),
                        Key::Enter => {
                            e.prevent_default();
                            choose(cursor_now);
                        }
                        _ => {}
                        }
                    },
                }
                ul { class: "switcher-results",
                    for (index, entry) in matches.iter().enumerate() {
                        li {
                            // By what it is: a row keeps its picture, so it must stay with its room.
                            key: "{entry.key()}",
                            "data-key": "{entry.key()}",
                            class: "switcher-item",
                            class: if matches!(entry, Entry::Action(_)) { "action" },
                            class: if index == cursor_now { "active" },
                            onclick: {
                                let mut choose = choose.clone();
                                move |_| choose(index)
                            },
                            match entry {
                                Entry::Room(room) => rsx! {
                                    Avatar { media: room.avatar.clone(), fallback: room.name.clone(), size: 24 }
                                    span { class: "name", "{room.name}" }
                                    span { class: "kind", if room.is_dm { "Direct message" } else { "Room" } }
                                },
                                Entry::Action(action) => rsx! {
                                    Icon { glyph: action.icon(), size: 16, class: "hash" }
                                    span { "{action.label()}" }
                                },
                            }
                        }
                    }
                    if matches.is_empty() {
                        li { class: "switcher-item muted", "Nothing matches" }
                    }
                }
            }
        }
    }
}
