//! Dialogs reached from a room's right-click menu (and the sidebar "+"):
//! invite someone, edit name and topic, create a room in the current scope.

use dioxus::prelude::*;
use morpheus_core::{History, OwnedRoomId, OwnedUserId, Privacy, Session};

use super::shell::{MembersVersion, RailEntry, Rooms, Scope, Selected, SelectedSpace, UiPrefs, rooms_in_scope};

/// Which room dialog is open. Provided by the shell.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RoomDialog {
    #[default]
    None,
    Invite(OwnedRoomId),
    Edit(OwnedRoomId),
    Create,
    /// Join rule and history visibility of a room or space.
    Privacy(OwnedRoomId),
    /// Your name and avatar inside a space and its rooms.
    RoomProfile(OwnedRoomId),
    /// A new space: a category inside `Some(parent)`, or a new top-level server.
    CreateSpace(Option<OwnedRoomId>),
    /// Confirm leaving a space and all its rooms.
    LeaveSpace(OwnedRoomId),
    /// Rename a rail folder (by its layout id).
    RenameFolder(String),
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
    let avatar_session = session.clone();
    let avatar_room = room_id.clone();
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
                div { class: "settings-actions",
                    button {
                        r#type: "button",
                        class: "secondary change-icon",
                        disabled: busy(),
                        onclick: move |_| {
                            let session = avatar_session.clone();
                            let room_id = avatar_room.clone();
                            spawn(async move {
                                let Some(path) = crate::attach::pick_attachment().await else { return };
                                busy.set(true);
                                match session.set_room_avatar(room_id, path).await {
                                    Ok(()) => message.set(Some(Ok("Icon updated."))),
                                    Err(err) => message.set(Some(Err(err.to_string()))),
                                }
                                busy.set(false);
                            });
                        },
                        "Change icon…"
                    }
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
    // Categories: spaces nested directly under this one.
    let categories: Vec<(OwnedRoomId, String)> = space
        .as_ref()
        .map(|space_id| {
            rooms
                .read()
                .iter()
                .filter(|r| r.is_space && r.parent_spaces.contains(space_id))
                .map(|r| (r.id.clone(), r.name.clone()))
                .collect()
        })
        .unwrap_or_default();
    let mut category = use_signal(|| None::<OwnedRoomId>);

    let submit = {
        let space = space.clone();
        move |evt: FormEvent| {
            evt.prevent_default();
            if busy() || name().trim().is_empty() {
                return;
            }
            busy.set(true);
            let parent = category().or_else(|| space.clone());
            let future = session.create_room(name(), topic(), parent);
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
                if !categories.is_empty() {
                    label { "Category"
                        select {
                            name: "category",
                            onchange: move |e| category.set(e.value().parse().ok()),
                            option { value: "", "None" }
                            for (id, label) in categories.iter() {
                                option { value: "{id}", "{label}" }
                            }
                        }
                    }
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

#[component]
pub fn PrivacyModal(room_id: OwnedRoomId) -> Element {
    let session = use_context::<Session>();
    let mut dialog = use_context::<OpenRoomDialog>();
    let mut public = use_signal(|| true);
    let mut history = use_signal(|| History::Shared);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(|| None::<Result<&'static str, String>>);
    let mut close = move || dialog.set(RoomDialog::None);

    let current = use_resource({
        let session = session.clone();
        let room_id = room_id.clone();
        move || {
            let session = session.clone();
            let room_id = room_id.clone();
            async move { session.privacy(room_id).await.ok() }
        }
    });
    let mut seeded = use_signal(|| false);
    use_effect(move || {
        if let Some(Some(privacy)) = current()
            && !*seeded.peek()
        {
            seeded.set(true);
            public.set(privacy.public);
            history.set(privacy.history);
        }
    });

    let save = move |evt: FormEvent| {
        evt.prevent_default();
        if busy() {
            return;
        }
        busy.set(true);
        let future = session.set_privacy(room_id.clone(), Privacy { public: public(), history: history() });
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
            form { class: "message-dialog room-dialog privacy-dialog", tabindex: "-1", autofocus: true, onclick: move |e| e.stop_propagation(), onsubmit: save,
                h2 { "Privacy" }
                fieldset { class: "choice-group",
                    legend { "Who can join" }
                    label { class: "choice",
                        input { r#type: "radio", name: "join", checked: public(), onchange: move |_| public.set(true) }
                        span { "Anyone with the link" }
                    }
                    label { class: "choice",
                        input { r#type: "radio", name: "join", checked: !public(), onchange: move |_| public.set(false) }
                        span { "Invite only" }
                    }
                }
                fieldset { class: "choice-group",
                    legend { "History new members can read" }
                    for (choice, label) in [(History::Shared, "Everything"), (History::Invited, "From their invite"), (History::Joined, "From when they joined")] {
                        label { class: "choice",
                            input { r#type: "radio", name: "history", checked: history() == choice, onchange: move |_| history.set(choice) }
                            span { "{label}" }
                        }
                    }
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

/// Your display name and avatar inside a space and every room under it.
#[component]
pub fn RoomProfileModal(room_id: OwnedRoomId) -> Element {
    let session = use_context::<Session>();
    let rooms = use_context::<Rooms>();
    let mut dialog = use_context::<OpenRoomDialog>();
    let mut name = use_signal(String::new);
    let mut avatar_path = use_signal(|| None::<std::path::PathBuf>);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(|| None::<Result<&'static str, String>>);
    let mut close = move || dialog.set(RoomDialog::None);

    let current = use_resource({
        let session = session.clone();
        let room_id = room_id.clone();
        move || {
            let session = session.clone();
            let room_id = room_id.clone();
            async move { session.room_profile(room_id).await.ok() }
        }
    });
    let mut seeded = use_signal(|| false);
    use_effect(move || {
        if let Some(Some(profile)) = current()
            && !*seeded.peek()
        {
            seeded.set(true);
            name.set(profile.name);
        }
    });

    // The space plus everything scoped under it.
    let targets: Vec<OwnedRoomId> = {
        let list = rooms.read();
        let mut ids = vec![room_id.clone()];
        ids.extend(rooms_in_scope(&list, &Scope::Space(room_id.clone())).into_iter().map(|r| r.id));
        ids
    };
    let save = {
        let targets = targets.clone();
        move |evt: FormEvent| {
            evt.prevent_default();
            if busy() {
                return;
            }
            busy.set(true);
            let future = session.set_room_profile(targets.clone(), name(), avatar_path());
            let mut version = consume_context::<MembersVersion>();
            spawn(async move {
                match future.await {
                    Ok(()) => {
                        version.with_mut(|v| v.0 += 1);
                        message.set(Some(Ok("Saved for this server.")));
                    }
                    Err(err) => message.set(Some(Err(err.to_string()))),
                }
                busy.set(false);
            });
        }
    };
    let pick = move |_| {
        spawn(async move {
            if let Some(path) = crate::attach::pick_attachment().await {
                avatar_path.set(Some(path));
            }
        });
    };
    let picked = avatar_path().map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());

    rsx! {
        div { class: "overlay", onclick: move |_| close(),
            form { class: "message-dialog room-dialog room-profile-dialog", tabindex: "-1", autofocus: true, onclick: move |e| e.stop_propagation(), onsubmit: save,
                h2 { "Server profile" }
                p { class: "muted", "How you appear in this server and its {targets.len() - 1} rooms. Your global profile stays as it is." }
                label { "Nickname"
                    input { name: "nickname", value: "{name}", oninput: move |e| name.set(e.value()) }
                }
                div { class: "settings-actions",
                    button { r#type: "button", class: "secondary", onclick: pick, "Choose avatar…" }
                    if let Some(file) = picked {
                        span { class: "muted", "{file}" }
                    }
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

/// A new space: a category under `parent`, or a top-level server.
#[component]
pub fn CreateSpaceModal(parent: Option<OwnedRoomId>) -> Element {
    let session = use_context::<Session>();
    let mut dialog = use_context::<OpenRoomDialog>();
    let mut scope = use_context::<SelectedSpace>();
    let mut name = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut close = move || dialog.set(RoomDialog::None);
    let is_category = parent.is_some();

    let submit = {
        let parent = parent.clone();
        move |evt: FormEvent| {
            evt.prevent_default();
            if busy() || name().trim().is_empty() {
                return;
            }
            busy.set(true);
            let future = session.create_space(name(), parent.clone());
            let parent = parent.clone();
            spawn(async move {
                match future.await {
                    Ok(space_id) => {
                        // A new server opens itself; a category stays inside its parent.
                        scope.set(Scope::Space(parent.unwrap_or(space_id)));
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
            form { class: "message-dialog room-dialog create-space-dialog", tabindex: "-1", autofocus: true, onclick: move |e| e.stop_propagation(), onsubmit: submit,
                h2 { if is_category { "Create category" } else { "Create server" } }
                p { class: "muted",
                    if is_category { "A category groups channels inside this server (a nested space)." } else { "A server is a Matrix space: a home for your channels." }
                }
                label { "Name"
                    input { name: "name", value: "{name}", oninput: move |e| name.set(e.value()) }
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

/// Leaving a server leaves every room under it too, so ask first.
#[component]
pub fn LeaveSpaceModal(space_id: OwnedRoomId) -> Element {
    let session = use_context::<Session>();
    let rooms = use_context::<Rooms>();
    let mut dialog = use_context::<OpenRoomDialog>();
    let mut scope = use_context::<SelectedSpace>();
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut close = move || dialog.set(RoomDialog::None);

    let (name, children) = {
        let list = rooms.read();
        let name = list.iter().find(|r| r.id == space_id).map(|r| r.name.clone()).unwrap_or_else(|| "this server".into());
        let children: Vec<OwnedRoomId> = rooms_in_scope(&list, &Scope::Space(space_id.clone())).into_iter().map(|r| r.id).collect();
        (name, children)
    };
    let count = children.len();
    let leave = move |_| {
        if busy() {
            return;
        }
        busy.set(true);
        let future = session.leave_space(space_id.clone(), children.clone());
        spawn(async move {
            match future.await {
                Ok(()) => {
                    scope.set(Scope::Rooms);
                    close();
                }
                Err(err) => error.set(Some(err.to_string())),
            }
            busy.set(false);
        });
    };

    rsx! {
        div { class: "overlay", onclick: move |_| close(),
            div { class: "message-dialog room-dialog leave-space-dialog", tabindex: "-1", autofocus: true, onclick: move |e| e.stop_propagation(),
                h2 { "Leave {name}?" }
                p { class: "muted", "You will leave the server and its {count} rooms. You can be invited back later." }
                if let Some(text) = error() {
                    p { class: "error", "{text}" }
                }
                div { class: "settings-actions",
                    button { class: "danger", disabled: busy(), onclick: leave, "Leave server" }
                    button { class: "secondary", onclick: move |_| close(), "Cancel" }
                }
            }
        }
    }
}

#[component]
pub fn RenameFolderModal(folder_id: String) -> Element {
    let session = use_context::<Session>();
    let mut dialog = use_context::<OpenRoomDialog>();
    let mut prefs = use_context::<UiPrefs>();
    let current = prefs
        .read()
        .rail
        .iter()
        .find_map(|e| match e {
            RailEntry::Folder { id, name, .. } if *id == folder_id => Some(name.clone()),
            _ => None,
        })
        .unwrap_or_default();
    let mut name = use_signal(|| current);
    let mut close = move || dialog.set(RoomDialog::None);
    let submit = move |evt: FormEvent| {
        evt.prevent_default();
        let new_name = name().trim().to_owned();
        if new_name.is_empty() {
            return;
        }
        prefs.with_mut(|p| {
            for entry in &mut p.rail {
                if let RailEntry::Folder { id, name, .. } = entry
                    && *id == folder_id
                {
                    *name = new_name.clone();
                }
            }
        });
        super::rail::save_prefs(&session, &prefs.peek());
        close();
    };
    rsx! {
        div { class: "overlay", onclick: move |_| close(),
            form { class: "message-dialog room-dialog rename-folder-dialog", tabindex: "-1", autofocus: true, onclick: move |e| e.stop_propagation(), onsubmit: submit,
                h2 { "Rename folder" }
                label { "Name"
                    input { name: "name", value: "{name}", oninput: move |e| name.set(e.value()) }
                }
                div { class: "settings-actions",
                    button { r#type: "submit", class: "primary", "Save" }
                    button { r#type: "button", class: "secondary", onclick: move |_| close(), "Cancel" }
                }
            }
        }
    }
}
