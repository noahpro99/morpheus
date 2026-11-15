//! A changed display name or picture shows everywhere at once, with nothing
//! of the old one left: the account, member lists, timeline senders and the
//! direct message list; for your own change and for someone else's.

#![allow(clippy::expect_used)]

mod common;

use std::time::{Duration, Instant};

use common::login;
use morpheus_e2e::{App, DEFAULT_WAIT, ProfileGuard};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

const PICTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/dusk.png");
const SECONDS: Duration = Duration::from_secs(12);

async fn open_general(app: &mut App) {
    app.open_room("general").await.expect("open general");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer");
}

/// The pictures of `selector`, as their `src`s (a letter has none).
async fn srcs(app: &mut App, selector: &str) -> Vec<String> {
    app.attrs(selector, "src")
        .await
        .expect("attrs")
        .into_iter()
        .flatten()
        .collect()
}

/// Waits until `check` holds, or fails with `what`.
async fn within(app: &mut App, what: &str, mut check: impl AsyncFnMut(&mut App) -> bool) {
    let started = Instant::now();
    while !check(app).await {
        assert!(started.elapsed() < SECONDS, "after {SECONDS:?} still: {what}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

const MEMBER: &str = ".members .member[title=\"@alice:localhost\"]";

async fn text_of(app: &mut App, selector: &str) -> String {
    app.text(selector).await.expect("text").unwrap_or_default()
}

#[tokio::test]
async fn someone_elses_new_name_and_picture_show_everywhere_without_a_restart() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let _restore = ProfileGuard::new(&hs, "alice").expect("alice's profile");
    // Alice starts with the picture the seed gave her (or none): give her a known one.
    let mut app = App::launch(&hs).await.expect("launch app");
    login(&mut app, "bob", "bob-password").await;
    open_general(&mut app).await;
    app.wait_for(MEMBER, DEFAULT_WAIT).await.expect("alice is a member");
    let member_before = srcs(&mut app, &format!("{MEMBER} img.avatar")).await;
    let sender_before = srcs(&mut app, ".message .avatar-button img.avatar").await;
    app.click(".rail-dms").await.expect("DMs");
    app.wait_for_text(".room.dm .room-name", "alice", DEFAULT_WAIT)
        .await
        .expect("the direct message with alice");
    let dm_before = srcs(&mut app, ".room.dm img.avatar").await;

    let name = format!("Alice {}", std::process::id());
    morpheus_e2e::set_display_name_as(&hs, "alice", &name).expect("rename");
    morpheus_e2e::set_avatar_as(&hs, "alice", &std::fs::read(PICTURE).expect("fixture")).expect("picture");

    within(&mut app, "the member list shows the new name", async |app| {
        text_of(app, MEMBER).await.contains(&name)
    })
    .await;
    within(&mut app, "the member list shows the new picture", async |app| {
        let now = srcs(app, &format!("{MEMBER} img.avatar")).await;
        !now.is_empty() && now != member_before
    })
    .await;
    within(&mut app, "a message shows the new sender name", async |app| {
        app.wait_for_text(".message .sender", &name, Duration::from_millis(50)).await.is_ok()
    })
    .await;
    within(&mut app, "a message shows the new sender picture", async |app| {
        let now = srcs(app, ".message .avatar-button img.avatar").await;
        !now.is_empty() && now != sender_before
    })
    .await;
    within(&mut app, "the direct message list shows the new name", async |app| {
        app.wait_for_text(".room.dm .room-name", &name, Duration::from_millis(50)).await.is_ok()
    })
    .await;
    within(&mut app, "the direct message list shows the new picture", async |app| {
        let now = srcs(app, ".room.dm img.avatar").await;
        !now.is_empty() && now != dm_before
    })
    .await;
}

#[tokio::test]
async fn your_own_new_name_and_picture_show_everywhere_at_once() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let _restore = ProfileGuard::new(&hs, "alice").expect("alice's profile");
    let mut app = App::launch_with_env(&hs, &[("MORPHEUS_E2E_PICK", PICTURE)])
        .await
        .expect("launch app");
    login(&mut app, "alice", "alice-password").await;
    open_general(&mut app).await;
    app.wait_for(MEMBER, DEFAULT_WAIT).await.expect("alice is a member");
    let member_before = srcs(&mut app, &format!("{MEMBER} img.avatar")).await;

    app.click(".settings-button").await.expect("gear");
    app.click_text(".settings-nav button", "Edit Profile").await.expect("page");
    let name = format!("Alice {}", std::process::id());
    app.type_into(".settings-page input[name=displayname]", &name).await.expect("name");
    app.click_text(".settings-page button", "Save").await.expect("save");
    app.wait_for_text(".settings-page .ok", "Saved", DEFAULT_WAIT).await.expect("saved");
    app.click_text(".settings-page button", "Choose picture").await.expect("choose");
    app.click_text(".settings-page button", "Set as avatar").await.expect("set");
    app.wait_for_text(".settings-page .ok", "Picture updated", DEFAULT_WAIT)
        .await
        .expect("set");
    app.press(".settings", "Escape").await.expect("close settings");

    within(&mut app, "the account shows the new name", async |app| {
        text_of(app, ".sidebar-footer .user").await.contains(&name)
    })
    .await;
    within(&mut app, "the member list shows the new name", async |app| {
        text_of(app, MEMBER).await.contains(&name)
    })
    .await;
    within(&mut app, "the member list shows the new picture", async |app| {
        let now = srcs(app, &format!("{MEMBER} img.avatar")).await;
        !now.is_empty() && now != member_before
    })
    .await;
    within(&mut app, "a message of yours shows the new name", async |app| {
        app.wait_for_text(".message .sender", &name, Duration::from_millis(50)).await.is_ok()
    })
    .await;
}
