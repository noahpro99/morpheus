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

    let profile = use_resource(use_reactive!(|user_id| {
        let session = session.clone();
        async move { session.profile(user_id).await.map_err(|e| e.to_string()) }
    }));
    let mut close = move || target.set(ProfileTarget::default());

    // Show what we know right away; the fetch fills in the name and avatar.
    let (name, avatar, error) = match &*profile.read() {
        Some(Ok(profile)) => (profile.name.clone(), profile.avatar.clone(), None),
        Some(Err(err)) => (user_id.localpart().to_owned(), None, Some(err.clone())),
        None => (user_id.localpart().to_owned(), None, None),
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
