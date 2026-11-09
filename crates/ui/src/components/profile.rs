//! The profile modal: opened by clicking a sender's name or avatar, a member
//! in the right column, or the signed-in user's chip in the sidebar footer.

use dioxus::prelude::*;

use morpheus_core::{MemberRole, OwnedUserId, Session};

use super::context_menu::{MenuItem, copy_to_clipboard, open_menu_at};
use super::shell::{Scope, Selected, SelectedSpace, go_to};
use crate::icons::Icon;

use crate::media::Avatar;

/// The profile that is open, if any. Provided by the shell.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProfileTarget(pub Option<ProfileOpen>);
pub type OpenProfile = Signal<ProfileTarget>;

#[derive(Debug, Clone, PartialEq)]
pub struct ProfileOpen {
    pub user_id: OwnedUserId,
    /// Where the card pops up (viewport px); `None` centres it.
    pub at: Option<(f64, f64)>,
    /// The full-page profile instead of the card.
    pub full: bool,
}

/// Open the profile card for `user_id` from a menu item: it pops up where
/// the menu was.
pub fn open_profile(user_id: OwnedUserId) {
    // Not a hook: this runs from event handlers, so read the context directly.
    let at = consume_context::<super::context_menu::ContextMenu>()
        .peek()
        .position();
    consume_context::<OpenProfile>().set(ProfileTarget(Some(ProfileOpen {
        user_id,
        at,
        full: false,
    })));
}

/// Open the profile card next to a click.
pub fn open_profile_at(user_id: OwnedUserId, e: &Event<MouseData>) {
    let point = e.client_coordinates();
    consume_context::<OpenProfile>().set(ProfileTarget(Some(ProfileOpen {
        user_id,
        at: Some((point.x, point.y)),
        full: false,
    })));
}

#[component]
pub fn ProfileModal(user_id: OwnedUserId) -> Element {
    let session = use_context::<Session>();
    let mut target = use_context::<OpenProfile>();
    let own = session.user_id().as_ref() == Some(&user_id);

    let refresh = use_context::<ProfileRefresh>();
    let profile = {
        let session = session.clone();
        use_resource(use_reactive!(|user_id| {
            let session = session.clone();
            let _ = refresh();
            async move { session.profile(user_id).await.map_err(|e| e.to_string()) }
        }))
    };
    let mut close = move || target.set(ProfileTarget::default());

    // Show what we know right away; the fetch fills in the name and avatar.
    let (name, avatar, status, error) = match &*profile.read() {
        Some(Ok(profile)) => (
            profile.name.clone(),
            profile.avatar.clone(),
            profile.status.clone(),
            None,
        ),
        Some(Err(err)) => (
            user_id.localpart().to_owned(),
            None,
            None,
            Some(err.clone()),
        ),
        None => (user_id.localpart().to_owned(), None, None, None),
    };

    rsx! {
        div { class: "overlay", onclick: move |_| close(),
            div {
                class: "profile-modal",
                tabindex: "-1",
                autofocus: true,
                onclick: move |e| e.stop_propagation(),
                div { class: "profile-banner" }
                div { class: "profile-body",
                    Avatar { media: avatar, fallback: name.clone(), size: 80 }
                    h2 { class: "profile-name", "{name}" }
                    p { class: "profile-id muted", "{user_id}" }
                    if let Some(status) = status {
                        p { class: "profile-status", "{status}" }
                    }
                    if own {
                        span { class: "tag", "This is you" }
                    }
                    if let Some(message) = error {
                        p { class: "error", "{message}" }
                    }
                }
                button { class: "ghost profile-close", title: "Close", onclick: move |_| close(), crate::icons::Icon { glyph: crate::icons::X, size: 18 } }
            }
        }
    }
}

/// Which of the account editors is open. Provided by the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Editor {
    #[default]
    None,
    Profile,
    Status,
}
pub type ProfileEditor = Signal<Editor>;

/// Bumped after a profile change so the footer and open modal refetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProfileVersion(pub u32);
pub type ProfileRefresh = Signal<ProfileVersion>;

/// Edit the signed-in user's display name and avatar.
#[component]
pub fn EditProfileModal() -> Element {
    let session = use_context::<Session>();
    let mut editor = use_context::<ProfileEditor>();
    let mut refresh = use_context::<ProfileRefresh>();
    let mut name = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(|| None::<Result<String, String>>);
    let mut close = move || editor.set(Editor::None);

    // Start from the current name.
    let current = use_resource({
        let session = session.clone();
        move || {
            let session = session.clone();
            async move { session.own_profile().await.ok() }
        }
    });
    // Seed the field once, unless the user has already typed (see `oninput`):
    // a late fetch must not put the old name back for Save to store.
    let mut seeded = use_signal(|| false);
    use_effect(move || {
        if let Some(Some(profile)) = current()
            && !*seeded.peek()
        {
            seeded.set(true);
            name.set(profile.name);
        }
    });

    let save = {
        let session = session.clone();
        move |evt: FormEvent| {
            evt.prevent_default();
            if busy() {
                return;
            }
            busy.set(true);
            let session = session.clone();
            let new_name = name();
            spawn(async move {
                match session.set_display_name(new_name).await {
                    Ok(()) => {
                        refresh.with_mut(|v| v.0 += 1);
                        message.set(Some(Ok("Saved.".into())));
                    }
                    Err(err) => message.set(Some(Err(err.to_string()))),
                }
                busy.set(false);
            });
        }
    };

    let pick_avatar = {
        let session = session.clone();
        move |_| {
            if busy() {
                return;
            }
            let session = session.clone();
            spawn(async move {
                let Some(path) = crate::attach::pick_attachment().await else {
                    return;
                };
                busy.set(true);
                match session.set_avatar(path).await {
                    Ok(()) => {
                        refresh.with_mut(|v| v.0 += 1);
                        message.set(Some(Ok("Avatar updated.".into())));
                    }
                    Err(err) => message.set(Some(Err(err.to_string()))),
                }
                busy.set(false);
            });
        }
    };

    let avatar = current().flatten().and_then(|p| p.avatar);
    rsx! {
        div { class: "overlay", onclick: move |_| close(),
            form {
                class: "profile-editor",
                tabindex: "-1",
                autofocus: true,
                onclick: move |e| e.stop_propagation(),
                onsubmit: save,
                h2 { "Edit profile" }
                div { class: "profile-editor-avatar",
                    Avatar { media: avatar, fallback: name(), size: 64 }
                    button { r#type: "button", class: "secondary", disabled: busy(), onclick: pick_avatar, crate::icons::Icon { glyph: crate::icons::IMAGE } "Change avatar…" }
                }
                label { "Display name"
                    input {
                        name: "displayname",
                        value: "{name}",
                        // Typing wins over a fetch that lands late (slow phone links).
                        oninput: move |e| {
                            seeded.set(true);
                            name.set(e.value());
                        },
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

/// Set the presence status message shown on your profile.
#[component]
pub fn StatusModal() -> Element {
    let session = use_context::<Session>();
    let mut editor = use_context::<ProfileEditor>();
    let refresh = use_context::<ProfileRefresh>();
    let mut status = use_signal(String::new);
    let busy = use_signal(|| false);
    let error = use_signal(|| None::<String>);
    let mut close = move || editor.set(Editor::None);

    let current = use_resource({
        let session = session.clone();
        move || {
            let session = session.clone();
            async move { session.own_profile().await.ok() }
        }
    });
    let mut seeded = use_signal(|| false);
    use_effect(move || {
        if let Some(Some(profile)) = current()
            && !*seeded.peek()
        {
            seeded.set(true);
            status.set(profile.status.unwrap_or_default());
        }
    });

    let apply = {
        let session = session.clone();
        move |value: Option<String>| {
            let (mut busy, mut error, mut refresh, mut close) = (busy, error, refresh, close);
            if busy() {
                return;
            }
            busy.set(true);
            let session = session.clone();
            spawn(async move {
                match session.set_status(value).await {
                    Ok(()) => {
                        refresh.with_mut(|v| v.0 += 1);
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
            form {
                class: "profile-editor status-editor",
                tabindex: "-1",
                autofocus: true,
                onclick: move |e| e.stop_propagation(),
                onsubmit: {
                    let apply = apply.clone();
                    move |evt: FormEvent| {
                        evt.prevent_default();
                        apply(Some(status()));
                    }
                },
                h2 { "Set status" }
                label { "What are you up to?"
                    input {
                        name: "status",
                        placeholder: "In a meeting",
                        value: "{status}",
                        // Typing wins over a fetch that lands late.
                        oninput: move |e| {
                            seeded.set(true);
                            status.set(e.value());
                        },
                    }
                }
                if let Some(text) = error() {
                    p { class: "error", "{text}" }
                }
                div { class: "settings-actions",
                    button { r#type: "submit", class: "primary", disabled: busy(), "Save" }
                    button { r#type: "button", class: "secondary", disabled: busy(), onclick: move |_| apply(None), "Clear status" }
                    button { r#type: "button", class: "secondary", onclick: move |_| close(), "Close" }
                }
            }
        }
    }
}

/// The quick profile card: pops up by the click like a menu. Photo, names,
/// status, mutual rooms, your note, their role here, a message box, and the
/// moderation and more menus up top. "View full profile" opens the modal.
#[component]
pub fn ProfileCard(open: ProfileOpen) -> Element {
    let session = use_context::<Session>();
    let mut target = use_context::<OpenProfile>();
    let refresh = use_context::<ProfileRefresh>();
    let selected = use_context::<Selected>();
    let user_id = open.user_id.clone();
    let own = session.user_id().as_ref() == Some(&user_id);
    let mut close = move || target.set(ProfileTarget::default());
    let mut note = use_signal(String::new);
    let mut note_saved = use_signal(|| false);
    let mut draft = use_signal(String::new);
    let mut report = use_signal(|| false);
    // A phone shows it as a bottom sheet rising from the bottom edge.
    let sheet = try_use_context::<super::shell::NarrowWindow>().is_some_and(|narrow| narrow().0);
    let mut blocked_now = use_signal(|| None::<bool>);

    let profile = {
        let session = session.clone();
        use_resource(use_reactive!(|user_id| {
            let session = session.clone();
            let _ = refresh();
            async move { session.profile(user_id).await.ok() }
        }))
    };
    let mutual = {
        let session = session.clone();
        use_resource(use_reactive!(|user_id| {
            let session = session.clone();
            async move { session.mutual_rooms(user_id).await.unwrap_or_default() }
        }))
    };
    let role = {
        let session = session.clone();
        use_resource(use_reactive!(|user_id| {
            let session = session.clone();
            let room_id = selected.peek().clone();
            async move {
                match room_id {
                    Some(room_id) => session.member_role(room_id, user_id).await.ok().flatten(),
                    None => None,
                }
            }
        }))
    };
    let stored_note = {
        let session = session.clone();
        use_resource(use_reactive!(|user_id| {
            let session = session.clone();
            async move { session.note(user_id).await.unwrap_or_default() }
        }))
    };
    let mut seeded = use_signal(|| false);
    use_effect(move || {
        if let Some(text) = stored_note()
            && !*seeded.peek()
        {
            seeded.set(true);
            note.set(text);
        }
    });
    let blocked = {
        let session = session.clone();
        use_resource(use_reactive!(|user_id| {
            let session = session.clone();
            async move { session.is_blocked(user_id).await.unwrap_or(false) }
        }))
    };
    let is_blocked = blocked_now().unwrap_or_else(|| blocked().unwrap_or(false));

    let seen = try_use_context::<SeenPeople>().and_then(|seen| seen.peek().get(&user_id).cloned());
    let (name, avatar, status) = match (profile().flatten(), seen) {
        (Some(p), _) => (p.name, p.avatar, p.status),
        // As seen where it was opened from, until the profile comes.
        (None, Some((name, avatar))) => (name, avatar, None),
        (None, None) => (user_id.localpart().to_owned(), None, None),
    };
    // Your nickname for them leads; their own name goes beside it.
    let nickname = try_use_context::<super::nicknames::Nicknames>()
        .and_then(|map| map().0.get(user_id.as_str()).cloned());

    let save_note = {
        let session = session.clone();
        let user_id = user_id.clone();
        move || {
            let future = session.set_note(user_id.clone(), note());
            spawn(async move {
                if let Err(err) = future.await {
                    super::toast::report("saving the note", &err);
                } else {
                    note_saved.set(true);
                }
            });
        }
    };
    let send_message = {
        let session = session.clone();
        let user_id = user_id.clone();
        move || {
            let body = draft().trim().to_owned();
            if body.is_empty() {
                return;
            }
            let session = session.clone();
            let user_id = user_id.clone();
            let mut scope = consume_context::<SelectedSpace>();
            spawn(async move {
                match session.dm_with(user_id).await {
                    Ok(dm) => {
                        match session.timeline(dm.clone()).await {
                            Ok(opened) => {
                                if let Err(err) = opened.handle.send_text(body).await {
                                    super::toast::report("sending from the card", &err);
                                }
                            }
                            Err(err) => super::toast::report("opening the DM", &err),
                        }
                        scope.set(Scope::Dms);
                        go_to(dm);
                        close();
                    }
                    Err(err) => super::toast::report("no DM for the message", &err),
                }
            });
        }
    };

    // "Mod view": this person's moderation sheet, across your rooms.
    let mod_view = {
        let user_id = user_id.clone();
        move |e: Event<MouseData>| {
            e.stop_propagation();
            close();
            consume_context::<super::room_dialogs::OpenRoomDialog>()
                .set(super::room_dialogs::RoomDialog::ModView(user_id.clone()));
        }
    };

    // The "…" menu.
    let more = {
        let session = session.clone();
        let user_id = user_id.clone();
        let open = open.clone();
        move |e: Event<MouseData>| {
            e.stop_propagation();
            let point = e.client_coordinates();
            let mut items = vec![
                MenuItem::new("View full profile", {
                    let open = open.clone();
                    move || {
                        consume_context::<OpenProfile>().set(ProfileTarget(Some(ProfileOpen {
                            full: true,
                            ..open.clone()
                        })));
                    }
                })
                .icon(crate::icons::USER),
                MenuItem::new("Share profile", {
                    let link = format!("https://matrix.to/#/{user_id}");
                    move || copy_to_clipboard(&link)
                })
                .icon(crate::icons::SHARE_2),
            ];
            if !own {
                if let Some(invite) = super::invites::invite_menu(&session, &user_id, None) {
                    items.push(invite);
                }
                items.push(
                    MenuItem::new(if is_blocked { "Unblock" } else { "Block" }, {
                        let session = session.clone();
                        let user_id = user_id.clone();
                        move || {
                            let future = session.block(user_id.clone(), !is_blocked);
                            spawn(async move {
                                if let Err(err) = future.await {
                                    super::toast::report("block", &err);
                                } else {
                                    blocked_now.set(Some(!is_blocked));
                                }
                            });
                        }
                    })
                    .icon(crate::icons::BAN),
                );
                items.push(
                    MenuItem::new("Report user profile", move || report.set(true))
                        .icon(crate::icons::FLAG),
                );
            }
            items.push(
                MenuItem::submenu(
                    "Copy user info",
                    vec![
                        MenuItem::new("Copy user ID", {
                            let id = user_id.to_string();
                            move || copy_to_clipboard(&id)
                        })
                        .icon(crate::icons::HASH),
                        MenuItem::new("Copy profile link", {
                            let link = format!("https://matrix.to/#/{user_id}");
                            move || copy_to_clipboard(&link)
                        })
                        .icon(crate::icons::LINK),
                    ],
                )
                .icon(crate::icons::COPY),
            );
            open_menu_at(point.x, point.y, items, None);
        }
    };

    // Placed by `CARD_CLAMP_JS` once mounted: beside the element that was
    // clicked (never over it) and always inside the window.
    let (ax, ay) = open.at.unwrap_or((-1.0, -1.0));
    let style = "left: -9999px; top: 0;".to_string();
    // The room you are both in right now comes first.
    let mut mutual_list = mutual().unwrap_or_default();
    if let Some(here) = selected.peek().clone()
        && let Some(pos) = mutual_list.iter().position(|(id, _)| *id == here)
    {
        let current = mutual_list.remove(pos);
        mutual_list.insert(0, current);
    }
    let role_label = role().flatten().map(|r| match r {
        MemberRole::Admin => "Admin",
        MemberRole::Moderator => "Moderator",
        MemberRole::Member => "Member",
    });

    rsx! {
        div {
            class: "overlay panel-overlay card-overlay",
            class: if sheet { "sheet-dim" },
            onclick: move |_| close(),
            div {
                class: "profile-card",
                class: if sheet { "sheet" },
                style: if !sheet { "{style}" },
                "data-ax": "{ax}",
                "data-ay": "{ay}",
                tabindex: "-1",
                autofocus: true,
                onclick: move |e| e.stop_propagation(),
                onmounted: move |_| {
                    if !sheet {
                        document::eval(CARD_CLAMP_JS);
                    }
                },
                div { class: "profile-banner",
                    if sheet {
                        div { class: "sheet-handle" }
                    }
                    div { class: "card-actions",
                        if !own && selected().is_some() {
                            button { class: "ghost card-button card-mod-view", title: "Mod view", onclick: mod_view, Icon { glyph: crate::icons::SHIELD_CHECK, size: 16 } }
                        }
                        button { class: "ghost card-button more", title: "More", onclick: more, Icon { glyph: crate::icons::ELLIPSIS, size: 16 } }
                    }
                }
                div { class: "profile-body",
                    Avatar { media: avatar, fallback: name.clone(), size: 72 }
                    if let Some(nick) = nickname.as_ref() {
                        h2 { class: "profile-name", "{nick}" }
                        p { class: "profile-realname muted", "{name}" }
                    } else {
                        h2 { class: "profile-name", "{name}" }
                    }
                    p { class: "profile-id muted", "{user_id}" }
                    if let Some(status) = status {
                        p { class: "profile-status", "{status}" }
                    }
                    div { class: "card-tags",
                        if own { span { class: "tag", "This is you" } }
                        if is_blocked { span { class: "tag warn", "Blocked" } }
                        if let Some(role) = role_label { span { class: "tag", "{role}" } }
                    }
                    if !own {
                        section { class: "card-section mutual",
                            h4 { "Mutual rooms — {mutual_list.len()}" }
                            if mutual_list.is_empty() {
                                p { class: "muted", "None yet." }
                            }
                            for (id, room_name) in mutual_list.iter().take(8) {
                                button {
                                    key: "{id}",
                                    class: "mutual-room",
                                    title: "Open #{room_name}",
                                    onclick: {
                                        let id = id.clone();
                                        move |_| {
                                            open_room_from_card(id.clone());
                                            close();
                                        }
                                    },
                                    "#{room_name}"
                                }
                            }
                        }
                    }
                    section { class: "card-section",
                        h4 { "Note" }
                        textarea {
                            class: "note",
                            name: "note",
                            placeholder: "Only you can see this",
                            value: "{note}",
                            rows: "2",
                            oninput: move |e| {
                                note.set(e.value());
                                note_saved.set(false);
                            },
                            onblur: {
                                let save_note = save_note.clone();
                                move |_| save_note()
                            },
                        }
                        if note_saved() { span { class: "muted note-saved", "Saved" } }
                    }
                    if !own {
                        input {
                            class: "card-message",
                            name: "message",
                            placeholder: "Message @{name}",
                            value: "{draft}",
                            oninput: move |e| draft.set(e.value()),
                            onkeydown: {
                                let send_message = send_message.clone();
                                move |e| {
                                    if e.key() == Key::Enter {
                                        e.prevent_default();
                                        send_message();
                                    }
                                }
                            },
                        }
                    }
                }
                if report() {
                    ReportUserForm { user_id: user_id.clone(), on_done: move |()| report.set(false) }
                }
            }
        }
    }
}

const CARD_CLAMP_JS: &str = "const c = document.querySelector('.profile-card'); if (!c) return; \
    const ax = parseFloat(c.dataset.ax), ay = parseFloat(c.dataset.ay); \
    const w = c.offsetWidth, h = c.offsetHeight, pad = 8; \
    let x, y; \
    /* The card's own backdrop covers the point; look beneath it. */ \
    const anchor = ax >= 0 ? (document.elementsFromPoint(ax, ay).find(e => !e.closest('.card-overlay, .profile-card')) || null) : null; \
    const el = anchor ? (anchor.closest('.avatar-button, button.sender, .member, .profile-chip, .context-item') || anchor) : null; \
    if (el) { const r = el.getBoundingClientRect(); \
      /* Beside the avatar, on the side nearer the middle of the window, never over it. */ \
      const mid = (r.left + r.right) / 2; \
      x = mid < innerWidth / 2 ? r.right + pad : r.left - w - pad; \
      if (x + w > innerWidth - pad) x = r.left - w - pad; \
      if (x < pad) x = r.right + pad; \
      y = r.top; } \
    else if (ax >= 0) { x = ax < innerWidth / 2 ? ax + 16 : ax - w - 16; y = ay; } \
    else { x = (innerWidth - w) / 2; y = innerHeight * 0.2; } \
    y = Math.max(pad, Math.min(y, innerHeight - h - pad)); \
    x = Math.max(pad, Math.min(x, innerWidth - w - pad)); \
    c.style.left = x + 'px'; c.style.top = y + 'px';";

#[component]
fn ReportUserForm(user_id: OwnedUserId, on_done: Callback<()>) -> Element {
    let session = use_context::<Session>();
    let mut reason = use_signal(String::new);
    let mut message = use_signal(|| None::<Result<&'static str, String>>);
    let submit = move |evt: FormEvent| {
        evt.prevent_default();
        let future = session.report_user(user_id.clone(), reason());
        spawn(async move {
            match future.await {
                Ok(()) => message.set(Some(Ok("Reported to the server admins."))),
                Err(err) => message.set(Some(Err(err.to_string()))),
            }
        });
    };
    rsx! {
        form { class: "report-user", onsubmit: submit,
            label { "Report this user"
                input { name: "reason", placeholder: "What is wrong?", value: "{reason}", oninput: move |e| reason.set(e.value()) }
            }
            match message() {
                Some(Ok(text)) => rsx! { p { class: "ok", "{text}" } },
                Some(Err(text)) => rsx! { p { class: "error", "{text}" } },
                None => rsx! {},
            }
            div { class: "settings-actions",
                button { r#type: "submit", class: "danger", crate::icons::Icon { glyph: crate::icons::FLAG } "Report" }
                button { r#type: "button", class: "secondary", onclick: move |_| on_done(()), "Cancel" }
            }
        }
    }
}

/// People as last seen on screen (name and picture), so a card opened from a
/// member row or a message shows them at once while it fetches the rest.
pub type SeenPeople =
    Signal<std::collections::HashMap<OwnedUserId, (String, Option<morpheus_core::MediaRef>)>>;

/// Note someone as just seen, before opening their card.
pub fn remember_person(user_id: &OwnedUserId, name: &str, avatar: Option<morpheus_core::MediaRef>) {
    if let Some(mut seen) = try_consume_context::<SeenPeople>() {
        seen.write()
            .insert(user_id.clone(), (name.to_owned(), avatar));
    }
}

/// Open a room picked on a card (a mutual room), in its server or the DMs.
fn open_room_from_card(room_id: morpheus_core::OwnedRoomId) {
    let rooms = consume_context::<super::shell::Rooms>();
    let mut scope = consume_context::<SelectedSpace>();
    let found = rooms.peek().iter().find(|r| r.id == room_id).cloned();
    if let Some(room) = found {
        let wanted = super::shell::scope_for(&rooms.peek(), &room);
        if wanted != *scope.peek() {
            scope.set(wanted);
        }
    }
    go_to(room_id);
}
