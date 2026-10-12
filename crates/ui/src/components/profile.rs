//! The profile modal: opened by clicking a sender's name or avatar, a member
//! in the right column, or the signed-in user's chip in the sidebar footer.

use dioxus::prelude::*;
use morpheus_core::{OwnedUserId, Session};

use crate::media::Avatar;

/// The user whose profile is open, if any. Provided by the shell.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProfileTarget(pub Option<OwnedUserId>);
pub type OpenProfile = Signal<ProfileTarget>;

/// Open the profile modal for `user_id` from any component under the shell.
pub fn open_profile(user_id: OwnedUserId) {
    // Not a hook: this runs from event handlers, so read the context directly.
    let mut target = consume_context::<OpenProfile>();
    target.set(ProfileTarget(Some(user_id)));
}

#[component]
pub fn ProfileModal(user_id: OwnedUserId) -> Element {
    let session = use_context::<Session>();
    let mut target = use_context::<OpenProfile>();
    let own = session.user_id().as_ref() == Some(&user_id);

    let refresh = use_context::<ProfileRefresh>();
    let profile = use_resource(use_reactive!(|user_id| {
        let session = session.clone();
        let _ = refresh();
        async move { session.profile(user_id).await.map_err(|e| e.to_string()) }
    }));
    let mut close = move || target.set(ProfileTarget::default());

    // Show what we know right away; the fetch fills in the name and avatar.
    let (name, avatar, status, error) = match &*profile.read() {
        Some(Ok(profile)) => (profile.name.clone(), profile.avatar.clone(), profile.status.clone(), None),
        Some(Err(err)) => (user_id.localpart().to_owned(), None, None, Some(err.clone())),
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
                button { class: "ghost profile-close", title: "Close", onclick: move |_| close(), "✕" }
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
    // Seed the field once; a late fetch must not clobber what was typed.
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
                    button { r#type: "button", class: "secondary", disabled: busy(), onclick: pick_avatar, "Change avatar…" }
                }
                label { "Display name"
                    input { name: "displayname", value: "{name}", oninput: move |e| name.set(e.value()) }
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
                    input { name: "status", placeholder: "In a meeting", value: "{status}", oninput: move |e| status.set(e.value()) }
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
