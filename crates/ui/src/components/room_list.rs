use dioxus::prelude::*;
use morpheus_core::{RoomSummary, Session};

use super::shell::{Rooms, Selected};
use crate::media::Avatar;

#[component]
pub fn RoomList(error: Option<String>) -> Element {
    let session = use_context::<Session>();
    let rooms = use_context::<Rooms>();
    let mut session_slot = use_context::<Signal<Option<Session>>>();

    let user = session
        .user_id()
        .map(|id| id.to_string())
        .unwrap_or_default();
    let (dms, channels): (Vec<RoomSummary>, Vec<RoomSummary>) = rooms
        .read()
        .iter()
        .filter(|room| !room.is_space)
        .cloned()
        .partition(|room| room.is_dm);

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
            header { class: "sidebar-header", h2 { "Morpheus" } }

            div { class: "room-list",
                if let Some(message) = error {
                    p { class: "error", "{message}" }
                }
                if dms.is_empty() && channels.is_empty() {
                    p { class: "muted pad", "No rooms yet." }
                }
                if !dms.is_empty() {
                    h4 { class: "section", "Direct messages" }
                    for room in dms {
                        RoomRow { key: "{room.id}", room }
                    }
                }
                if !channels.is_empty() {
                    h4 { class: "section", "Rooms" }
                    for room in channels {
                        RoomRow { key: "{room.id}", room }
                    }
                }
            }

            footer { class: "sidebar-footer",
                Avatar { media: None, fallback: user.trim_start_matches('@').to_owned(), size: 32 }
                span { class: "user", "{user}" }
                button { class: "ghost", title: "Sign out", onclick: logout, "⏻" }
            }
        }
    }
}

#[component]
fn RoomRow(room: RoomSummary) -> Element {
    let mut selected = use_context::<Selected>();
    let is_selected = selected().as_ref() == Some(&room.id);
    let id = room.id.clone();

    rsx! {
        button {
            class: "room",
            class: if is_selected { "selected" },
            class: if room.unread_messages > 0 { "unread" },
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
