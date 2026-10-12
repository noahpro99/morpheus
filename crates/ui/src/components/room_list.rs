use dioxus::prelude::*;
use morpheus_core::{RoomSummary, Session};

use super::settings::SettingsOpen;
use super::shell::{Rooms, Scope, Selected, SelectedSpace, rooms_in_scope};
use crate::media::Avatar;

#[component]
pub fn RoomList(error: Option<String>) -> Element {
    let session = use_context::<Session>();
    let rooms = use_context::<Rooms>();
    let selected_space = use_context::<SelectedSpace>();
    let mut session_slot = use_context::<Signal<Option<Session>>>();
    let mut settings_open = use_context::<SettingsOpen>();

    let user_id = session
        .user_id()
        .map(|id| id.to_string())
        .unwrap_or_default();
    // The footer shows the display name and avatar, never the raw Matrix ID.
    let profile = use_resource({
        let session = session.clone();
        move || {
            let session = session.clone();
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
            header { class: "sidebar-header", h2 { "{header_title}" } }

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
                div { class: "profile-chip", title: "{user_id}",
                    Avatar { media: avatar, fallback: name.clone(), size: 32 }
                    span { class: "user", "{name}" }
                }
                button { class: "ghost settings-button", title: "Settings", onclick: move |_| settings_open.set(true), "⚙" }
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
            class: if room.is_dm { "dm" },
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
