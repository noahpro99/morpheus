//! "Share to": what another app shared, a message (its text) and the rooms
//! to send it to, recent ones first, with a search. Picking one and Send
//! opens that room, whose message box sends the files and the message.

use dioxus::prelude::*;
use morpheus_core::OwnedRoomId;

use super::shell::{Rooms, open_room_in_scope};
use crate::{media::Avatar, shared::Shared};

/// The share waiting for the dialog, if any. Provided by the shell.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShareShown(pub Option<Shared>);
pub type ShareOpen = Signal<ShareShown>;

/// A share on its way to a room: the message box of `room` sends it once
/// that room's timeline is open (not the room left behind).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareFor {
    pub room: OwnedRoomId,
    pub text: Option<String>,
    pub files: Vec<std::path::PathBuf>,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SharePending(pub Option<ShareFor>);
pub type ShareToSend = Signal<SharePending>;

#[component]
pub fn ShareDialog(shared: Shared) -> Element {
    let rooms = use_context::<Rooms>();
    let mut open = use_context::<ShareOpen>();
    let mut to_send = use_context::<ShareToSend>();
    let mut message = use_signal(|| shared.text.clone().unwrap_or_default());
    let mut search = use_signal(String::new);
    let mut picked = use_signal(|| None::<OwnedRoomId>);
    let mut close = move || open.set(ShareShown(None));

    let query = search().trim().to_lowercase();
    let list: Vec<_> = rooms
        .read()
        .iter()
        .filter(|r| !r.is_space && !r.invited)
        .filter(|r| query.is_empty() || r.name.to_lowercase().contains(&query))
        .take(50)
        .cloned()
        .collect();
    let names: Vec<String> = shared
        .files
        .iter()
        .map(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
        .collect();
    let files = shared.files.clone();
    let send = move |_| {
        let Some(room) = picked() else { return };
        let text = Some(message().trim().to_owned()).filter(|t| !t.is_empty());
        to_send.set(SharePending(Some(ShareFor {
            room: room.clone(),
            text,
            files: files.clone(),
        })));
        close();
        open_room_in_scope(room);
    };

    rsx! {
        div { class: "overlay", onclick: move |_| close(),
            div { class: "message-dialog share-dialog", onclick: move |e| e.stop_propagation(),
                h2 { "Share to…" }
                if !names.is_empty() {
                    ul { class: "share-files",
                        for name in names {
                            li { class: "share-file", crate::icons::Icon { glyph: crate::icons::PAPERCLIP, size: 14 } "{name}" }
                        }
                    }
                }
                textarea {
                    class: "share-message",
                    rows: "2",
                    placeholder: "Add a message",
                    value: "{message}",
                    oninput: move |e| message.set(e.value()),
                }
                input {
                    class: "share-search",
                    r#type: "search",
                    placeholder: "Search rooms and people",
                    value: "{search}",
                    oninput: move |e| search.set(e.value()),
                }
                div { class: "share-rooms",
                    for room in list {
                        button {
                            key: "{room.id}",
                            class: "share-room",
                            class: if picked().as_ref() == Some(&room.id) { "picked" },
                            r#type: "button",
                            onclick: {
                                let id = room.id.clone();
                                move |_| picked.set(Some(id.clone()))
                            },
                            Avatar { media: room.avatar.clone(), fallback: room.name.clone(), size: 28 }
                            span { class: "share-room-name", "{room.name}" }
                        }
                    }
                }
                div { class: "settings-actions",
                    button { class: "primary share-send", disabled: picked().is_none(), onclick: send, "Send" }
                    button { class: "secondary", onclick: move |_| close(), "Cancel" }
                }
            }
        }
    }
}
