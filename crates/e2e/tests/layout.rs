//! Shell layout and modal behaviour: the member list toggle in the chat
//! header, and Escape closing whichever modal is open.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login_and_open(app: &mut App, room: &str) {
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", "alice")
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", "alice-password")
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("room listed");
    app.open_room(room).await.expect("open room");
}

#[tokio::test]
async fn member_toggle_hides_and_shows_the_member_list() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;

    app.wait_for_text(".members .member", "alice", DEFAULT_WAIT)
        .await
        .expect("members shown by default");

    app.click(".member-toggle").await.expect("toggle off");
    app.wait_for_js(
        "return document.querySelector('.members') === null && document.querySelector('.shell').classList.contains('no-side');",
        DEFAULT_WAIT,
    )
    .await
    .expect("member list hidden and the column collapsed");

    // The choice sticks across rooms.
    app.open_room("random").await.expect("switch room");
    assert_eq!(app.count(".members").await.expect("count"), 0, "still hidden in the next room");

    app.click(".member-toggle").await.expect("toggle on");
    app.wait_for_text(".members .member", "alice", DEFAULT_WAIT)
        .await
        .expect("member list back");
}

#[tokio::test]
async fn escape_closes_open_modals() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;

    // Settings.
    app.click(".settings-button").await.expect("open settings");
    app.wait_for(".settings", DEFAULT_WAIT)
        .await
        .expect("settings modal");
    app.press(".settings", "Escape").await.expect("escape");
    app.wait_for_js(
        "return document.querySelector('.settings') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("settings closed by Escape");

    // Quick switcher.
    app.shortcut(".shell", "k", true).await.expect("ctrl+k");
    app.wait_for(".switcher input", DEFAULT_WAIT)
        .await
        .expect("switcher");
    app.press(".switcher input", "Escape")
        .await
        .expect("escape");
    app.wait_for_js(
        "return document.querySelector('.switcher') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("switcher closed by Escape");
}
