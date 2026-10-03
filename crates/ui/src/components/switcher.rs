use dioxus::prelude::*;

use super::shell::{Rooms, Selected};

/// Ctrl+K quick switcher: type to filter rooms, arrows to move, Enter to open.
#[component]
pub fn QuickSwitcher(open: Signal<bool>) -> Element {
    let rooms = use_context::<Rooms>();
    let mut selected = use_context::<Selected>();
    let mut query = use_signal(String::new);
    let mut cursor = use_signal(|| 0usize);

    let matches: Vec<_> = {
        let needle = query().to_lowercase();
        rooms
            .read()
            .iter()
            .filter(|room| {
                !room.is_space && (needle.is_empty() || room.name.to_lowercase().contains(&needle))
            })
            .take(8)
            .cloned()
            .collect()
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
            if let Some(room) = matches.get(index) {
                selected.set(Some(room.id.clone()));
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
                    placeholder: "Jump to a room…",
                    value: "{query}",
                    oninput: move |e| {
                        query.set(e.value());
                        cursor.set(0);
                    },
                    onkeydown: {
                        let mut choose = choose.clone();
                        move |e| match e.key() {
                        Key::Escape => close(),
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
                    for (index, room) in matches.iter().enumerate() {
                        li {
                            key: "{room.id}",
                            class: "switcher-item",
                            class: if index == cursor_now { "active" },
                            onclick: {
                                let mut choose = choose.clone();
                                move |_| choose(index)
                            },
                            if room.is_dm {
                                span { class: "hash", "@" }
                            } else {
                                span { class: "hash", "#" }
                            }
                            span { "{room.name}" }
                        }
                    }
                    if matches.is_empty() {
                        li { class: "switcher-item muted", "No rooms match" }
                    }
                }
            }
        }
    }
}
