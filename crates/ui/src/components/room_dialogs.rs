//! Dialogs reached from a room's right-click menu (and the sidebar "+"):
//! invite someone, edit name and topic, create a room in the current scope.

use dioxus::prelude::*;
use morpheus_core::{OwnedRoomId, OwnedUserId, Session};

use super::shell::{Rooms, Scope, Selected, SelectedSpace};

/// Which room dialog is open. Provided by the shell.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RoomDialog {
    #[default]
    None,
    Invite(OwnedRoomId),
    Edit(OwnedRoomId),
    Create,
}
pub type OpenRoomDialog = Signal<RoomDialog>;

#[component]
pub fn InviteModal(room_id: OwnedRoomId) -> Element {
    let session = use_context::<Session>();
    let mut dialog = use_context::<OpenRoomDialog>();
    let mut user = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(|| None::<Result<String, String>>);
    let mut close = move || dialog.set(RoomDialog::None);

    let submit = move |evt: FormEvent| {
        evt.prevent_default();
        if busy() {
            return;
        }
        let Ok(user_id) = user().trim().parse::<OwnedUserId>() else {
            message.set(Some(Err("Enter a full Matrix ID like @name:server".into())));
            return;
        };
        busy.set(true);
        let future = session.invite(room_id.clone(), user_id.clone());
        spawn(async move {
            match future.await {
                Ok(()) => {
                    message.set(Some(Ok(format!("Invited {user_id}."))));
                    user.set(String::new());
                }
                Err(err) => message.set(Some(Err(err.to_string()))),
            }
            busy.set(false);
        });
    };

    rsx! {
        div { class: "overlay", onclick: move |_| close(),
            form { class: "message-dialog room-dialog invite-dialog", tabindex: "-1", autofocus: true, onclick: move |e| e.stop_propagation(), onsubmit: submit,
                h2 { "Invite people" }
                label { "Matrix ID"
                    input { name: "user", placeholder: "@friend:matrix.org", value: "{user}", oninput: move |e| user.set(e.value()) }
                }
                match message() {
                    Some(Ok(text)) => rsx! { p { class: "ok", "{text}" } },
                    Some(Err(text)) => rsx! { p { class: "error", "{text}" } },
                    None => rsx! {},
                }
                div { class: "settings-actions",
                    button { r#type: "submit", class: "primary", disabled: busy(), "Invite" }
                    button { r#type: "button", class: "secondary", onclick: move |_| close(), "Close" }
                }
            }
        }
    }
}

#[component]
pub fn EditRoomModal(room_id: OwnedRoomId) -> Element {
    let session = use_context::<Session>();
    let rooms = use_context::<Rooms>();
    let mut dialog = use_context::<OpenRoomDialog>();
    let current = rooms.read().iter().find(|r| r.id == room_id).cloned();
    let mut name = use_signal(|| current.as_ref().map(|r| r.name.clone()).unwrap_or_default());
    let mut topic = use_signal(|| current.as_ref().and_then(|r| r.topic.clone()).unwrap_or_default());
    let mut busy = use_signal(|| false);
    let mut message = use_signal(|| None::<Result<&'static str, String>>);
    let mut close = move || dialog.set(RoomDialog::None);

    let submit = move |evt: FormEvent| {
        evt.prevent_default();
        if busy() {
            return;
        }
        busy.set(true);
        let future = session.edit_room(room_id.clone(), Some(name()), Some(topic()));
        spawn(async move {
            match future.await {
                Ok(()) => message.set(Some(Ok("Saved."))),
                Err(err) => message.set(Some(Err(err.to_string()))),
            }
            busy.set(false);
        });
    };

    rsx! {
        div { class: "overlay", onclick: move |_| close(),
            form { class: "message-dialog room-dialog edit-room-dialog", tabindex: "-1", autofocus: true, onclick: move |e| e.stop_propagation(), onsubmit: submit,
                h2 { "Edit room" }
                label { "Name"
                    input { name: "name", value: "{name}", oninput: move |e| name.set(e.value()) }
                }
                label { "Topic"
                    input { name: "topic", placeholder: "What is this room about?", value: "{topic}", oninput: move |e| topic.set(e.value()) }
                }
                match message() {
                    Some(Ok(text)) => rsx! { p { class: "ok", "{text}" } },
                    Some(Err(text)) => rsx! { p { class: "error", "{text}" } },
                    None => rsx! {},
                }
                div { class: "settings-actions",
                    button { r#type: "submit", class: "primary", disabled: busy(), "Save" }
                    button { r#type: "button", class: "secondary", onclick: move |_| close(), "Close" }
                }
            }
        }
    }
}

/// Create a room in the current scope: inside the selected space, or
/// ungrouped under Rooms.
#[component]
pub fn CreateRoomModal() -> Element {
    let session = use_context::<Session>();
    let mut dialog = use_context::<OpenRoomDialog>();
    let mut selected = use_context::<Selected>();
    let mut scope_signal = use_context::<SelectedSpace>();
    let rooms = use_context::<Rooms>();
    let mut name = use_signal(String::new);
    let mut topic = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut close = move || dialog.set(RoomDialog::None);

    let space = scope_signal().space_id().cloned();
    let space_name = space.as_ref().and_then(|id| {
        rooms.read().iter().find(|r| &r.id == id).map(|r| r.name.clone())
    });

    let submit = {
        let space = space.clone();
        move |evt: FormEvent| {
            evt.prevent_default();
            if busy() || name().trim().is_empty() {
                return;
            }
            busy.set(true);
            let future = session.create_room(name(), topic(), space.clone());
            let space = space.clone();
            spawn(async move {
                match future.await {
                    Ok(room_id) => {
                        // Land in the new room, in the scope it belongs to.
                        scope_signal.set(space.map_or(Scope::Rooms, Scope::Space));
                        selected.set(Some(room_id));
                        close();
                    }
                    Err(err) => error.set(Some(err.to_string())),
                }
                busy.set(false);
            });
        }
    };

    rsx! {
        div { class: "overlay", onclick: move |_| close(),
            form { class: "message-dialog room-dialog create-room-dialog", tabindex: "-1", autofocus: true, onclick: move |e| e.stop_propagation(), onsubmit: submit,
                h2 { "Create room" }
                p { class: "muted",
                    match space_name {
                        Some(space) => format!("In the {space} space."),
                        None => "Not in any space.".to_string(),
                    }
                }
                label { "Name"
                    input { name: "name", placeholder: "new-room", value: "{name}", oninput: move |e| name.set(e.value()) }
                }
                label { "Topic (optional)"
                    input { name: "topic", value: "{topic}", oninput: move |e| topic.set(e.value()) }
                }
                if let Some(text) = error() {
                    p { class: "error", "{text}" }
                }
                div { class: "settings-actions",
                    button { r#type: "submit", class: "primary", disabled: busy(), "Create" }
                    button { r#type: "button", class: "secondary", onclick: move |_| close(), "Cancel" }
                }
            }
        }
    }
}
