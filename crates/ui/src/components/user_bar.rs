//! The pill along the bottom of the rail and sidebar: your avatar and name
//! (click for the account menu) and the settings gear. Shares its height and
//! colour with the message box next to it.

use dioxus::prelude::*;
use super::toast::report;
use morpheus_core::Session;

use super::context_menu::{MenuItem, open_menu};
use super::profile::{Editor, ProfileEditor, ProfileRefresh, open_profile};
use super::settings::SettingsOpen;
use crate::icons::{Icon, SETTINGS};
use crate::media::Avatar;

#[component]
pub fn UserBar() -> Element {
    let session = use_context::<Session>();
    let session_slot = use_context::<Signal<Option<Session>>>();
    let mut settings_open = use_context::<SettingsOpen>();
    let refresh = use_context::<ProfileRefresh>();

    let user_id = session
        .user_id()
        .map(|id| id.to_string())
        .unwrap_or_default();
    // Display name and avatar, never the raw Matrix ID.
    let profile = use_resource({
        let session = session.clone();
        move || {
            let session = session.clone();
            let _ = refresh();
            async move { session.own_profile().await.ok() }
        }
    });
    let (name, avatar) = match profile().flatten() {
        Some(profile) => (profile.name, profile.avatar),
        None => (user_id.trim_start_matches('@').to_owned(), None),
    };

    let logout = move || {
        let mut session_slot = session_slot;
        let Some(active) = session_slot.take() else {
            return;
        };
        spawn(async move {
            if let Err(err) = active.logout().await {
                report("logout failed; session was dropped locally anyway", &err);
            }
        });
    };

    // Click (or right-click) on your own chip: account actions, Discord-style.
    let account_menu = {
        let own = session.user_id();
        move |e: Event<MouseData>| {
            let mut editor = consume_context::<ProfileEditor>();
            let mut items = Vec::new();
            if let Some(id) = own.clone() {
                items.push(MenuItem::new("View profile", move || open_profile(id.clone())));
            }
            items.push(MenuItem::new("Edit profile", move || editor.set(Editor::Profile)));
            items.push(MenuItem::new("Set status", move || editor.set(Editor::Status)));
            // Switching signs out and returns to the login screen; sessions for
            // several accounts are not stored side by side yet.
            items.push(MenuItem::new("Switch account", logout));
            items.push(MenuItem::new("Sign out", logout).danger());
            open_menu(&e, items, None);
        }
    };

    rsx! {
        footer { class: "sidebar-footer user-bar",
            div { class: "user-pill",
            button {
                class: "profile-chip",
                title: "{user_id}",
                onclick: account_menu.clone(),
                oncontextmenu: account_menu,
                Avatar { media: avatar, fallback: name.clone(), size: 32 }
                span { class: "user", "{name}" }
            }
            button { class: "ghost settings-button", title: "Settings", onclick: move |_| settings_open.set(true), Icon { glyph: SETTINGS, size: 18 } }
            }
        }
    }
}
