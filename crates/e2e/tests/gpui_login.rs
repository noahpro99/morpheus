//! Sign-in in the GPUI app: the shared `login` helper (common/mod.rs) reaches the room list. Runs only with `MORPHEUS_E2E_UI=gpui` and
//! `MORPHEUS_E2E_BIN` set to a `morpheus-gpui` built with `--features e2e`.

#![allow(clippy::expect_used, dead_code)]

mod common;

use morpheus_e2e::{App, DEFAULT_WAIT};

fn gpui_homeserver() -> Option<String> {
    if !morpheus_e2e::gpui() {
        eprintln!("run against the web or a phone; skipping");
        return None;
    }
    let hs = std::env::var("MORPHEUS_TEST_HOMESERVER").ok();
    if hs.is_none() {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
    }
    hs
}

#[tokio::test]
async fn gpui_login_reaches_the_room_list() {
    let Some(hs) = gpui_homeserver() else { return };
    let mut app = App::launch(&hs).await.expect("launch app");
    common::login(&mut app, "bob", "bob-password").await;
    app.wait_for_text(".room", "random", DEFAULT_WAIT)
        .await
        .expect("rooms synced");
    assert_eq!(app.count("form.login").await.expect("count"), 0);
}

#[tokio::test]
async fn gpui_session_survives_relaunch() {
    let Some(hs) = gpui_homeserver() else { return };
    let mut app = App::launch(&hs).await.expect("launch app");
    common::login(&mut app, "bob", "bob-password").await;
    let mut app = app.relaunch().await.expect("relaunch");
    app.wait_for_text(".room", "random", DEFAULT_WAIT)
        .await
        .expect("rooms after restore");
    assert_eq!(app.count("form.login").await.expect("count"), 0);
}

#[tokio::test]
async fn gpui_wrong_password_shows_error() {
    let Some(hs) = gpui_homeserver() else { return };
    let mut app = App::launch(&hs).await.expect("launch app");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", "alice")
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", "nope")
        .await
        .expect("password");
    assert_eq!(
        app.value("form.login input[name=password]")
            .await
            .expect("value")
            .as_deref(),
        Some("nope")
    );
    app.submit("form.login").await.expect("submit");
    app.wait_for("form.login .error", DEFAULT_WAIT)
        .await
        .expect("error shown");
    assert_eq!(app.count(".room").await.expect("count"), 0);
}
