//! The pill along the bottom of the rail and sidebar: your avatar and name
//! (click for the account menu), mute, deafen and the settings gear, and in a
//! call its controls above them. Out of a call it shares its height and
//! colour with the message box next to it.

use super::toast::report;
use dioxus::core::spawn_forever;
use dioxus::prelude::*;
use morpheus_core::{Core, Session};

use super::context_menu::{MenuItem, open_menu};
use super::profile::{Editor, ProfileEditor, ProfileRefresh, open_profile};
use super::settings::SettingsOpen;
use crate::icons::{CHEVRON_DOWN, HEADPHONE_OFF, HEADPHONES, Icon, MIC, MIC_OFF, SETTINGS};
use crate::media::Avatar;

#[component]
pub fn UserBar() -> Element {
    let session = use_context::<Session>();
    let session_slot = use_context::<Signal<Option<Session>>>();
    let mut settings_open = use_context::<SettingsOpen>();
    let refresh = use_context::<ProfileRefresh>();
    let voice = use_context::<super::call::Voice>();

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
            async move {
                match session.own_profile().await {
                    Ok(profile) => Some(profile),
                    Err(err) => {
                        tracing::warn!(%err, "own profile not loaded");
                        None
                    }
                }
            }
        }
    });
    let (name, avatar) = match profile().flatten() {
        Some(profile) => (profile.name, profile.avatar),
        None => (user_id.trim_start_matches('@').to_owned(), None),
    };

    let core = use_context::<Core>();
    // Accounts stored on this device, for "Switch account".
    let accounts = use_resource({
        let core = core.clone();
        move || {
            let core = core.clone();
            async move { core.accounts().await.unwrap_or_default() }
        }
    });

    // Signing out moves to the next stored account, if there is one.
    let logout = {
        let core = core.clone();
        move || {
            let mut session_slot = session_slot;
            // Stays on screen until signed out, so whatever comes next (the
            // next account, or the login screen) sees the account gone.
            let Some(active) = session_slot.peek().clone() else {
                return;
            };
            let core = core.clone();
            // Outlives the menu that started it (closing the menu drops its tasks).
            spawn_forever(async move {
                if let Err(err) = active.logout().await {
                    report("logout failed; session was dropped locally anyway", &err);
                }
                match core.restore_session().await {
                    Ok(next) => session_slot.set(next),
                    Err(err) => report("opening the next account", &err),
                }
            });
        }
    };

    // Click (or right-click) on your own chip: account actions, Discord-style.
    let account_menu = {
        let own = session.user_id();
        move |e: Event<MouseData>| {
            let mut editor = consume_context::<ProfileEditor>();
            let mut items = Vec::new();
            if let Some(id) = own.clone() {
                items.push(MenuItem::new("View profile", move || {
                    open_profile(id.clone())
                }));
            }
            items.push(MenuItem::new("Edit profile", move || {
                editor.set(Editor::Profile)
            }));
            items.push(MenuItem::new("Set status", move || {
                editor.set(Editor::Status)
            }));
            // Every stored account (this one ticked), then adding another.
            let mut switch: Vec<MenuItem> = accounts()
                .unwrap_or_default()
                .into_iter()
                .map(|account| {
                    let label = account.user_id.to_string();
                    let current = own.as_ref() == Some(&account.user_id);
                    let key = account.key.clone();
                    let core = core.clone();
                    let session = session.clone();
                    MenuItem::new(label.clone(), move || {
                        if current {
                            return;
                        }
                        let (core, session, key) = (core.clone(), session.clone(), key.clone());
                        let mut session_slot = session_slot;
                        // Outlives the menu that started it (closing the menu drops its tasks).
                        spawn_forever(async move {
                            if let Err(err) = session.suspend().await {
                                report("pausing this account", &err);
                            }
                            match core.switch_account(key).await {
                                Ok(Some(next)) => session_slot.set(Some(next)),
                                Ok(None) => session_slot.set(None),
                                Err(err) => report("switching account", &err),
                            }
                        });
                    })
                    .avatar(None, label.trim_start_matches('@').to_owned())
                    .checked(current)
                })
                .collect();
            switch.push(
                MenuItem::new("Add an account", {
                    let session = session.clone();
                    move || {
                        let session = session.clone();
                        let mut session_slot = session_slot;
                        // Outlives the menu that started it (closing the menu drops its tasks).
                        spawn_forever(async move {
                            // Stays signed in; the login screen offers a way back.
                            if let Err(err) = session.suspend().await {
                                report("pausing this account", &err);
                            }
                            session_slot.set(None);
                        });
                    }
                })
                .icon(crate::icons::USER_PLUS),
            );
            items.push(MenuItem::submenu("Switch account", switch));
            items.push(
                MenuItem::new("Sign out", {
                    let logout = logout.clone();
                    move || {
                        let logout = logout.clone();
                        super::confirm::ask(
                            "Sign out",
                            "Are you sure you want to sign out? You will need your password (or single sign-on) to sign back in here.",
                            "Sign out",
                            logout,
                        );
                    }
                })
                .danger(),
            );
            open_menu(&e, items, None);
        }
    };

    rsx! {
        footer { class: "sidebar-footer user-bar",
            div { class: "user-pill",
            // In a call, its controls sit at the top of this card.
            super::call::VoicePanel {}
            div { class: "user-row",
            button {
                class: "profile-chip",
                title: "{user_id}",
                onclick: account_menu.clone(),
                oncontextmenu: account_menu,
                Avatar { media: avatar, fallback: name.clone(), size: 32 }
                span { class: "user", "{name}" }
            }
            // The mic and its arrow are one thing: red together when muted.
            div { class: "device-pair", class: if voice().muted || voice().deafened { "active" },
            button {
                class: "ghost mute-button",
                class: if voice().muted || voice().deafened { "active" },
                title: if voice().muted { "Unmute (Ctrl+Shift+M)" } else { "Mute (Ctrl+Shift+M)" },
                onclick: move |_| super::call::toggle_mute(),
                oncontextmenu: move |e: Event<MouseData>| device_menu(e, false),
                Icon { glyph: if voice().muted || voice().deafened { MIC_OFF } else { MIC }, size: 18 }
            }
            button {
                class: "ghost device-menu-button",
                title: "Microphone",
                onclick: move |e: Event<MouseData>| device_menu(e, false),
                Icon { glyph: CHEVRON_DOWN, size: 12 }
            }
            }
            div { class: "device-pair", class: if voice().deafened { "active" },
            button {
                class: "ghost deafen-button",
                class: if voice().deafened { "active" },
                title: if voice().deafened { "Undeafen (Ctrl+Shift+D)" } else { "Deafen (Ctrl+Shift+D)" },
                onclick: move |_| super::call::toggle_deafen(),
                oncontextmenu: move |e: Event<MouseData>| device_menu(e, true),
                Icon { glyph: if voice().deafened { HEADPHONE_OFF } else { HEADPHONES }, size: 18 }
            }
            button {
                class: "ghost device-menu-button",
                title: "Speaker",
                onclick: move |e: Event<MouseData>| device_menu(e, true),
                Icon { glyph: CHEVRON_DOWN, size: 12 }
            }
            }
            button { class: "ghost settings-button", title: "Settings (Ctrl+,)", onclick: move |_| settings_open.set(true), Icon { glyph: SETTINGS, size: 18 } }
            }
            }
        }
    }
}

/// The quick list of microphones or speakers, under the pointer.
fn device_menu(e: Event<MouseData>, speakers: bool) {
    e.prevent_default();
    e.stop_propagation();
    let at = e.client_coordinates();
    super::call::open_audio_menu(speakers, at.x, at.y);
}
