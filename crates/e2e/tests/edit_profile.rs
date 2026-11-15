//! Settings' Edit Profile page: change the display name, pick a picture,
//! preview it and set it, remove it; errors show on the page.

#![allow(clippy::expect_used)]

mod common;

use common::login;
use morpheus_e2e::{App, DEFAULT_WAIT, ProfileGuard};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

const PICTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/dusk.png");

async fn open_edit_profile(app: &mut App) {
    app.click(".settings-button").await.expect("gear button");
    app.wait_for(".settings", DEFAULT_WAIT).await.expect("settings");
    app.click_text(".settings-nav button", "Edit Profile")
        .await
        .expect("Edit Profile in the page list");
    app.wait_for(".settings-page input[name=displayname]", DEFAULT_WAIT)
        .await
        .expect("the page's name box");
}

#[tokio::test]
async fn the_edit_profile_page_changes_the_name_and_the_picture() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let _restore = ProfileGuard::new(&hs, "carol").expect("carol's profile");
    let mut app = App::launch_with_env(&hs, &[("MORPHEUS_E2E_PICK", PICTURE)])
        .await
        .expect("launch app");
    login(&mut app, "carol", "carol-password").await;
    let before = app
        .attr(".sidebar-footer img.avatar", "src")
        .await
        .expect("attr")
        .expect("the account shows a picture");

    open_edit_profile(&mut app).await;

    // The name.
    let name = format!("Carol {}", std::process::id());
    app.type_into(".settings-page input[name=displayname]", &name)
        .await
        .expect("type name");
    app.click_text(".settings-page button", "Save").await.expect("save");
    app.wait_for_text(".settings-page .ok", "Saved", DEFAULT_WAIT)
        .await
        .expect("saved");
    app.wait_for_text(".sidebar-footer .user", &name, DEFAULT_WAIT)
        .await
        .expect("the account shows the new name");

    // The picture: pick, preview, then set.
    app.click_text(".settings-page button", "Choose picture")
        .await
        .expect("choose");
    app.wait_for(".settings-page .profile-preview img.avatar", DEFAULT_WAIT)
        .await
        .expect("a preview of the picked picture");
    assert_eq!(
        app.attr(".sidebar-footer img.avatar", "src").await.expect("attr").as_deref(),
        Some(before.as_str()),
        "the preview alone does not change the account"
    );
    app.click_text(".settings-page button", "Set as avatar")
        .await
        .expect("set");
    wait_src_not(&mut app, &before).await;

    // Remove it: back to the letter.
    app.click_text(".settings-page button", "Remove picture")
        .await
        .expect("remove");
    app.wait_for_count(".sidebar-footer img.avatar", 0, DEFAULT_WAIT)
        .await
        .expect("the picture is gone from the account");
}

#[tokio::test]
async fn a_name_the_server_refuses_shows_an_error_on_the_page() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let _restore = ProfileGuard::new(&hs, "carol").expect("carol's profile");
    let mut app = App::launch(&hs).await.expect("launch app");
    login(&mut app, "carol", "carol-password").await;
    open_edit_profile(&mut app).await;
    // Matrix limits a display name to 256 characters.
    app.type_into(".settings-page input[name=displayname]", &"x".repeat(300))
        .await
        .expect("type");
    app.click_text(".settings-page button", "Save").await.expect("save");
    app.wait_for(".settings-page .error", DEFAULT_WAIT)
        .await
        .expect("an error shows on the page");
}

/// Waits until the account's picture is not the one with `src`.
async fn wait_src_not(app: &mut App, src: &str) {
    let started = std::time::Instant::now();
    loop {
        let now = app.attr(".sidebar-footer img.avatar", "src").await.expect("attr");
        if now.as_deref().is_some_and(|now| now != src) {
            return;
        }
        assert!(started.elapsed() < DEFAULT_WAIT, "the account still shows the old picture");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
