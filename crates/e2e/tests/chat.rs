//! End-to-end tests through the real desktop window.
//!
//! Needs `just hs-up hs-seed` and `just e2e-build`; self-skips unless
//! `MORPHEUS_TEST_HOMESERVER` is set. `just e2e` does all of it.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login(app: &mut App, user: &str, password: &str) {
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", user)
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", password)
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".shell", DEFAULT_WAIT)
        .await
        .expect("shell after login");
}

#[tokio::test]
async fn login_browse_rooms_and_send() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login(&mut app, "alice", "alice-password").await;

    // Room list synced; open #general (the most recent room auto-opens, which varies).
    app.wait_for_text(".room", "general", DEFAULT_WAIT)
        .await
        .expect("general in room list");
    app.open_room("general").await.expect("click general");
    app.wait_for_text(".chat-header h2", "general", DEFAULT_WAIT)
        .await
        .expect("general header");
    app.wait_for(".message .body", DEFAULT_WAIT)
        .await
        .expect("timeline loaded");
    // The first seeded message is far up by now (every run adds messages).
    app.reveal_message("Welcome to Morpheus", DEFAULT_WAIT)
        .await
        .expect("seeded message");

    // Switch rooms.
    app.open_room("dev").await.expect("click dev");
    app.wait_for_text(".chat-header h2", "dev", DEFAULT_WAIT)
        .await
        .expect("dev header");
    app.reveal_message("sliding sync is working", DEFAULT_WAIT)
        .await
        .expect("dev history");

    // Send a message and wait for the remote echo (no longer pending).
    let body = format!("hello from e2e {}", std::process::id());
    app.type_into(".composer input", &body).await.expect("type");
    app.press(".composer input", "Enter").await.expect("enter");
    app.wait_for_text(".message:not(.pending) .body", &body, DEFAULT_WAIT)
        .await
        .expect("message echoed");
    assert_eq!(
        app.text(".composer input").await.expect("composer"),
        Some(String::new()),
        "composer clears after send"
    );

    if let Some(path) = app.screenshot("chat").await.expect("screenshot") {
        eprintln!("screenshot: {}", path.display());
    }
}

#[tokio::test]
async fn session_survives_relaunch() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login(&mut app, "bob", "bob-password").await;
    app.wait_for_text(".room", "random", DEFAULT_WAIT)
        .await
        .expect("rooms synced");

    let mut app = app.relaunch().await.expect("relaunch");
    app.wait_for(".shell", DEFAULT_WAIT)
        .await
        .expect("restored straight into the shell");
    assert_eq!(
        app.count("form.login").await.expect("count"),
        0,
        "no login form after restore"
    );
    app.wait_for_text(
        ".sidebar-footer .profile-chip[title=\"@bob:localhost\"] .user",
        "bob",
        DEFAULT_WAIT,
    )
    .await
    .expect("bob is signed in");
}

#[tokio::test]
async fn wrong_password_shows_error() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
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
    app.submit("form.login").await.expect("submit");
    app.wait_for("form.login .error", DEFAULT_WAIT)
        .await
        .expect("error shown");
    assert_eq!(app.count(".shell").await.expect("count"), 0);
}
