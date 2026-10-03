use dioxus::prelude::*;
use morpheus_core::Session;

use super::shell::{Rooms, Selected};

#[component]
pub fn RoomList(error: Option<String>) -> Element {
    let session = use_context::<Session>();
    let rooms = use_context::<Rooms>();
    let mut selected = use_context::<Selected>();
    let mut session_slot = use_context::<Signal<Option<Session>>>();

    let user = session
        .user_id()
        .map(|id| id.to_string())
        .unwrap_or_default();
    let visible: Vec<_> = rooms
        .read()
        .iter()
        .filter(|room| !room.is_space)
        .cloned()
        .collect();

    let logout = move |_| {
        let Some(active) = session_slot.take() else {
            return;
        };
        spawn(async move {
            if let Err(err) = active.logout().await {
                tracing::warn!(%err, "logout failed; session was dropped locally anyway");
            }
        });
    };

    rsx! {
        aside { class: "sidebar",
            header { class: "sidebar-header", h2 { "Rooms" } }

            div { class: "room-list",
                if let Some(message) = error {
                    p { class: "error", "{message}" }
                }
                if visible.is_empty() {
                    p { class: "muted pad", "No rooms yet." }
                }
                for room in visible {
                    button {
                        key: "{room.id}",
                        class: "room",
                        class: if selected().as_ref() == Some(&room.id) { "selected" },
                        class: if room.unread_messages > 0 { "unread" },
                        onclick: {
                            let id = room.id.clone();
                            move |_| selected.set(Some(id.clone()))
                        },
                        span { class: "hash", "#" }
                        span { class: "room-name", "{room.name}" }
                        if room.highlight_count > 0 {
                            span { class: "badge", "{room.highlight_count}" }
                        }
                    }
                }
            }

            footer { class: "sidebar-footer",
                div { class: "avatar", "{user.chars().nth(1).unwrap_or('?')}" }
                span { class: "user", "{user}" }
                button { class: "ghost", title: "Sign out", onclick: logout, "⏻" }
            }
        }
    }
}
