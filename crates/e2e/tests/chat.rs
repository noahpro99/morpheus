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
    app.type_into(".composer .composer-input", &body)
        .await
        .expect("type");
    app.press(".composer .composer-input", "Enter")
        .await
        .expect("enter");
    app.wait_for_text(".message:not(.pending) .body", &body, DEFAULT_WAIT)
        .await
        .expect("message echoed");
    assert_eq!(
        app.text(".composer .composer-input")
            .await
            .expect("composer"),
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
    // While the session restores, the splash shows the logo, not the name.
    let mut saw_splash = false;
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let state = app
            .eval(
                "const s = document.querySelector('.splash'); \
                 return [!!s, !!(s && s.querySelector('svg.splash-logo')), s ? s.textContent.trim() : '', !!document.querySelector('.shell')];",
            )
            .await
            .expect("splash state");
        let [splash, logo, text, shell] = [&state[0], &state[1], &state[2], &state[3]];
        if splash == true {
            saw_splash = true;
            assert_eq!(logo, true, "the splash shows the logo");
            assert_eq!(text, "", "the splash shows no name");
        }
        if shell == true {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "never restored into the shell"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    eprintln!("splash seen while restoring: {saw_splash}");
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

/// Send `text` from the open room's message box.
async fn send(app: &mut App, text: &str) {
    app.type_into(".composer .composer-input", text)
        .await
        .expect("type");
    app.press(".composer .composer-input", "Enter")
        .await
        .expect("send");
}

/// What someone looking at a conversation sees arrive, without touching
/// anything: in a room, and in a direct message started with New Message.
#[tokio::test]
async fn messages_show_up_live_for_whoever_is_looking() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let run = std::process::id();
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login(&mut bob, "bob", "bob-password").await;
    let mut carol = App::launch(&hs).await.expect("launch carol");
    login(&mut carol, "carol", "carol-password").await;

    // In a room both have open.
    bob.open_room("general").await.expect("bob opens general");
    carol
        .open_room("general")
        .await
        .expect("carol opens general");
    carol
        .wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer");
    let text = format!("live in general {run}");
    send(&mut carol, &text).await;
    bob.wait_for_text(".message .body", &text, DEFAULT_WAIT)
        .await
        .expect("bob sees it arrive in #general");

    // In a DM carol starts with New Message.
    carol.click(".rail-dms").await.expect("DMs");
    carol
        .wait_for(".sidebar .friends-row", DEFAULT_WAIT)
        .await
        .expect("Friends row");
    carol.click(".sidebar .friends-row").await.expect("Friends");
    carol
        .wait_for(".friends-header .new-message-tab", DEFAULT_WAIT)
        .await
        .expect("Friends page");
    carol
        .click(".friends-header .new-message-tab")
        .await
        .expect("New Message");
    carol
        .type_into("form.new-message input", "bob")
        .await
        .expect("type");
    carol.submit("form.new-message").await.expect("open");
    carol
        .wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("carol in the DM");
    let first = format!("hello bob {run}");
    send(&mut carol, &first).await;

    // Bob opens it (accepting, if it is new to him) and keeps looking.
    bob.click(".rail-dms").await.expect("DMs");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let invited = bob
        .eval("return [...document.querySelectorAll('.invite-row')].some(r => r.textContent.includes('carol'));")
        .await
        .expect("invites")
        .as_bool()
        .unwrap_or(false);
    if invited {
        bob.click_text(".invite-row", "carol")
            .await
            .expect("open invite");
        bob.click(".invite-card .invite-accept")
            .await
            .expect("accept");
    } else {
        // Joined before (it may sit with the rooms, if his side never
        // marked it as a DM).
        bob.open_room("carol")
            .await
            .expect("open the DM with carol");
    }
    bob.wait_for_text(".message .body", &first, DEFAULT_WAIT)
        .await
        .expect("bob sees carol's first message");
    for n in 0..3 {
        let next = format!("and again {n} {run}");
        send(&mut carol, &next).await;
        bob.wait_for_text(".message .body", &next, DEFAULT_WAIT)
            .await
            .unwrap_or_else(|e| panic!("bob sees message {n} arrive while looking: {e}"));
    }
    // And the other way.
    bob.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("bob's composer");
    let reply = format!("hi carol {run}");
    send(&mut bob, &reply).await;
    carol
        .wait_for_text(".message .body", &reply, DEFAULT_WAIT)
        .await
        .expect("carol sees bob's reply arrive while looking");
}
