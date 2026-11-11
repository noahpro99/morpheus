//! The account chip in the sidebar footer: its menu, editing the display
//! name, setting a presence status, and signing out.

#![allow(clippy::expect_used)]

mod common;

use common::login;
use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn open_chip_menu(app: &mut App) {
    app.click(".profile-chip").await.expect("click chip");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("account menu");
}

/// Runs as bob so alice's display name (which other tests match on) is left alone.
#[tokio::test]
async fn account_menu_edits_name_sets_status_and_signs_out() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login(&mut app, "bob", "bob-password").await;

    open_chip_menu(&mut app).await;
    for label in [
        "View profile",
        "Edit profile",
        "Set status",
        "Switch account",
        "Sign out",
    ] {
        app.wait_for_text(".context-item", label, DEFAULT_WAIT)
            .await
            .unwrap_or_else(|e| panic!("menu item {label}: {e}"));
    }
    assert_eq!(
        app.count(".sidebar-footer button[title=\"Sign out\"]")
            .await
            .expect("count"),
        0,
        "no separate sign-out button"
    );

    // Edit the display name; the footer follows.
    app.click_text(".context-item", "Edit profile")
        .await
        .expect("edit profile");
    app.wait_for(".profile-editor input[name=displayname]", DEFAULT_WAIT)
        .await
        .expect("editor");
    let name = format!("Bob {}", std::process::id());
    app.type_into(".profile-editor input[name=displayname]", &name)
        .await
        .expect("type name");
    app.submit(".profile-editor").await.expect("save");
    app.wait_for_text(".profile-editor .ok", "Saved", DEFAULT_WAIT)
        .await
        .expect("saved");
    app.wait_for_text(".sidebar-footer .user", &name, DEFAULT_WAIT)
        .await
        .expect("footer shows the new name");
    app.press(".profile-editor", "Escape")
        .await
        .expect("escape");
    app.wait_for_js(
        "return document.querySelector('.profile-editor') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("editor closed");

    // Put the seeded name back straight away: later tests (and later steps
    // here, if they fail) look for "bob".
    open_chip_menu(&mut app).await;
    app.click_text(".context-item", "Edit profile")
        .await
        .expect("edit again");
    app.wait_for(".profile-editor input[name=displayname]", DEFAULT_WAIT)
        .await
        .expect("editor");
    app.type_into(".profile-editor input[name=displayname]", "bob")
        .await
        .expect("type");
    app.submit(".profile-editor").await.expect("save");
    app.wait_for_text(".sidebar-footer .user", "bob", DEFAULT_WAIT)
        .await
        .expect("restored");
    app.press(".profile-editor", "Escape")
        .await
        .expect("escape");

    // Set a status; it shows on the profile.
    open_chip_menu(&mut app).await;
    app.click_text(".context-item", "Set status")
        .await
        .expect("set status");
    app.wait_for(".status-editor input[name=status]", DEFAULT_WAIT)
        .await
        .expect("status editor");
    app.type_into(".status-editor input[name=status]", "In a meeting")
        .await
        .expect("type status");
    app.submit(".status-editor").await.expect("save status");
    app.wait_for_js(
        "return document.querySelector('.status-editor') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("status editor closes on save");
    open_chip_menu(&mut app).await;
    app.click_text(".context-item", "View profile")
        .await
        .expect("view profile");
    app.wait_for_text(
        ".profile-card .profile-status",
        "In a meeting",
        DEFAULT_WAIT,
    )
    .await
    .expect("status on the profile");
    app.press("body", "Escape").await.expect("close profile");

    // Clear the status so reruns start clean.
    open_chip_menu(&mut app).await;
    app.click_text(".context-item", "Set status")
        .await
        .expect("set status");
    app.wait_for(".status-editor", DEFAULT_WAIT)
        .await
        .expect("status editor");
    app.click_text(".status-editor button", "Clear status")
        .await
        .expect("clear");
    app.wait_for_js(
        "return document.querySelector('.status-editor') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("closed after clearing");

    // Sign out asks first: Cancel keeps you signed in.
    open_chip_menu(&mut app).await;
    app.click_text(".context-item", "Sign out")
        .await
        .expect("sign out");
    app.wait_for_text(".confirm-dialog", "Sign out", DEFAULT_WAIT)
        .await
        .expect("asked first");
    app.click_text(".confirm-dialog button", "Cancel")
        .await
        .expect("cancel");
    app.wait_for_js(
        "return !document.querySelector('.confirm-dialog');",
        DEFAULT_WAIT,
    )
    .await
    .expect("dialog gone");
    assert_eq!(
        app.count("form.login").await.expect("count"),
        0,
        "still signed in"
    );
    // Confirmed, it returns to the login form.
    open_chip_menu(&mut app).await;
    app.click_text(".context-item", "Sign out")
        .await
        .expect("sign out");
    app.click(".confirm-dialog button.danger")
        .await
        .expect("confirm");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("back at the login form");
}
