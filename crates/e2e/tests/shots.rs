//! Screenshots for comparing the two apps by eye, not a test: run with
//! `MORPHEUS_SHOTS=<dir>` set (each shot is named after the app and the
//! scene) against either app; skipped otherwise.

#![allow(clippy::expect_used)]

use std::time::Duration;

use morpheus_e2e::{App, DEFAULT_WAIT};

async fn shot(app: &mut App, name: &str) {
    let Some(dir) = std::env::var_os("MORPHEUS_SHOTS") else {
        return;
    };
    tokio::time::sleep(Duration::from_millis(900)).await;
    let who = if app.is_gpui() { "gpui" } else { "dioxus" };
    let script = std::path::Path::new(&dir).join("shot.sh");
    let pid = app.pid().expect("pid").to_string();
    let status = std::process::Command::new(script)
        .arg(format!("{who}-{name}"))
        .arg(pid)
        .env("SHOT_DIR", &dir)
        .status()
        .expect("shot.sh");
    assert!(status.success());
}

async fn login(app: &mut App, user: &str) {
    app.wait_for("form.login", DEFAULT_WAIT).await.expect("login form");
    app.type_into("form.login input[name=username]", user).await.expect("username");
    app.type_into("form.login input[name=password]", &format!("{user}-password"))
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("room listed");
}

async fn close(app: &mut App) {
    app.press("body", "Escape").await.expect("escape");
    app.wait_for_count(".overlay", 0, DEFAULT_WAIT).await.expect("closed");
}

#[tokio::test]
async fn room_dialogs() {
    let (Some(hs), Some(_)) = (
        std::env::var("MORPHEUS_TEST_HOMESERVER").ok(),
        std::env::var_os("MORPHEUS_SHOTS"),
    ) else {
        return;
    };
    let mut app = App::launch_sized(&hs, 1200, 760).await.expect("launch");
    login(&mut app, "alice").await;
    app.open_room("general").await.expect("open general");
    shot(&mut app, "00-chat").await;

    app.click(".create-room-button").await.expect("plus");
    app.wait_for(".create-room-dialog", DEFAULT_WAIT).await.expect("dialog");
    shot(&mut app, "01-create-room").await;
    close(&mut app).await;

    app.right_click_text(".room", "general").await.expect("menu");
    app.wait_for(".context-menu", DEFAULT_WAIT).await.expect("menu");
    shot(&mut app, "02-room-menu").await;
    app.click_text(".context-item", "Edit room").await.expect("edit");
    app.wait_for(".edit-room-dialog", DEFAULT_WAIT).await.expect("dialog");
    shot(&mut app, "03-edit-room").await;
    close(&mut app).await;

    app.right_click_text(".room", "general").await.expect("menu");
    app.click_text(".context-item", "Invite people").await.expect("invite");
    app.wait_for(".invite-dialog", DEFAULT_WAIT).await.expect("dialog");
    app.type_into(".invite-dialog input[name=user]", "bob").await.expect("type");
    app.submit(".invite-dialog").await.expect("submit");
    app.wait_for(".invite-dialog .error", DEFAULT_WAIT).await.expect("error");
    shot(&mut app, "04-invite-error").await;
    close(&mut app).await;

    app.right_click_text(".room", "general").await.expect("menu");
    app.click_text(".context-item", "Privacy").await.expect("privacy");
    app.wait_for(".privacy-dialog", DEFAULT_WAIT).await.expect("dialog");
    shot(&mut app, "05-privacy").await;
    close(&mut app).await;

    app.click(".rail-add").await.expect("rail plus");
    app.wait_for(".create-space-dialog", DEFAULT_WAIT).await.expect("dialog");
    shot(&mut app, "06-create-server").await;
    close(&mut app).await;

    app.click(".rail-item[title=\"Engineering\"]").await.expect("space");
    app.click(".sidebar-header h2.server-name").await.expect("name");
    app.wait_for(".context-menu", DEFAULT_WAIT).await.expect("menu");
    shot(&mut app, "07-server-menu").await;
    app.click_text(".context-item", "Edit per-server profile").await.expect("profile");
    app.wait_for(".room-profile-dialog", DEFAULT_WAIT).await.expect("dialog");
    shot(&mut app, "08-server-profile").await;
    close(&mut app).await;
    app.click(".sidebar-header h2.server-name").await.expect("name");
    app.click_text(".context-item.danger", "Leave server").await.expect("leave");
    app.wait_for(".leave-space-dialog", DEFAULT_WAIT).await.expect("dialog");
    shot(&mut app, "09-leave-server").await;
}

#[tokio::test]
async fn settings_pages() {
    let (Some(hs), Some(_)) = (
        std::env::var("MORPHEUS_TEST_HOMESERVER").ok(),
        std::env::var_os("MORPHEUS_SHOTS"),
    ) else {
        return;
    };
    let mut app = App::launch_sized(&hs, 1200, 760).await.expect("launch");
    login(&mut app, "alice").await;
    app.click(".settings-button").await.expect("gear");
    app.wait_for(".settings", DEFAULT_WAIT).await.expect("settings");
    shot(&mut app, "s0-account").await;
    app.type_into(".settings .settings-search", "notif").await.expect("search");
    app.wait_for(".settings-result", DEFAULT_WAIT).await.expect("result");
    shot(&mut app, "s1-search").await;
    app.press(".settings .settings-search", "Enter").await.expect("pick");
    shot(&mut app, "s2-notifications-highlight").await;
    for (nav, name) in [
        ("Notifications", "s3-notifications"),
        ("Appearance", "s4-appearance"),
        ("Voice & Video", "s5-voice"),
        ("Startup", "s6-startup"),
        ("Help & Feedback", "s7-help"),
    ] {
        app.click_text(".settings-nav button", nav).await.expect("page");
        shot(&mut app, name).await;
    }
}

/// A thread: the chip under its root, then the panel.
#[tokio::test]
async fn thread_panel() {
    let (Some(hs), Some(_)) = (
        std::env::var("MORPHEUS_TEST_HOMESERVER").ok(),
        std::env::var_os("MORPHEUS_SHOTS"),
    ) else {
        return;
    };
    let user = morpheus_e2e::new_account(&hs, "shotthreads").expect("account");
    let room = morpheus_e2e::api_as(
        &hs,
        &user,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": "plans" })),
    )
    .expect("a room");
    let room = room["room_id"]
        .as_str()
        .expect("room id")
        .replace('!', "%21")
        .replace(':', "%3A");
    let send = |txn: &str, body: serde_json::Value| {
        let sent = morpheus_e2e::api_as(
            &hs,
            &user,
            "PUT",
            &format!("rooms/{room}/send/m.room.message/{txn}"),
            Some(body),
        )
        .expect("sent");
        sent["event_id"].as_str().expect("event id").to_owned()
    };
    let root = send(
        "root",
        serde_json::json!({ "msgtype": "m.text", "body": "Let's plan Q4 in this thread" }),
    );
    for (txn, text) in [("a", "I think threads should ship first"), ("b", "Agreed, then the emoji picker")] {
        send(
            txn,
            serde_json::json!({
                "msgtype": "m.text", "body": text,
                "m.relates_to": { "rel_type": "m.thread", "event_id": root,
                    "is_falling_back": true, "m.in_reply_to": { "event_id": root } }
            }),
        );
    }
    let mut app = App::launch_sized(&hs, 1200, 760).await.expect("launch");
    login(&mut app, &user).await;
    app.open_room("plans").await.expect("open plans");
    app.wait_for(".thread-chip", DEFAULT_WAIT).await.expect("chip");
    shot(&mut app, "t0-thread-chip").await;
    app.click(".thread-chip").await.expect("open thread");
    app.wait_for(".thread-panel .message .body", DEFAULT_WAIT).await.expect("panel");
    shot(&mut app, "t1-thread-panel").await;
}

/// The room search panel with a result.
#[tokio::test]
async fn search_panel() {
    let (Some(hs), Some(_)) = (
        std::env::var("MORPHEUS_TEST_HOMESERVER").ok(),
        std::env::var_os("MORPHEUS_SHOTS"),
    ) else {
        return;
    };
    let mut app = App::launch_sized(&hs, 1200, 760).await.expect("launch");
    login(&mut app, "alice").await;
    app.open_room("general").await.expect("open general");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT).await.expect("composer");
    let word = format!("okapi{}", std::process::id());
    app.type_into(".composer .composer-input", &format!("an {word} in the forest")).await.expect("type");
    app.press(".composer .composer-input", "Enter").await.expect("send");
    app.wait_for_text(".message.own:not(.pending) .body", &word, DEFAULT_WAIT).await.expect("sent");
    app.click(".chat-header .search-toggle").await.expect("search");
    app.wait_for(".search-panel input[name=search]", DEFAULT_WAIT).await.expect("panel");
    shot(&mut app, "s0-search-empty").await;
    for _ in 0..10 {
        app.type_into(".search-panel input[name=search]", &word).await.expect("query");
        if app.wait_for(".search-hit", Duration::from_secs(2)).await.is_ok() {
            break;
        }
    }
    shot(&mut app, "s1-search-hit").await;
}

/// The Friends page: the list, and New Message.
#[tokio::test]
async fn friends_page() {
    let (Some(hs), Some(_)) = (
        std::env::var("MORPHEUS_TEST_HOMESERVER").ok(),
        std::env::var_os("MORPHEUS_SHOTS"),
    ) else {
        return;
    };
    let mut app = App::launch_sized(&hs, 1200, 760).await.expect("launch");
    login(&mut app, "carol").await;
    app.open_room("general").await.expect("general");
    app.wait_for_text(".members .member", "bob", DEFAULT_WAIT).await.expect("bob");
    app.right_click_text(".members .member", "bob").await.expect("menu");
    app.click_text(".context-menu .context-item", "Add friend").await.expect("add");
    app.click(".rail-dms").await.expect("dms");
    app.wait_for(".sidebar .friends-row", DEFAULT_WAIT).await.expect("row");
    app.click(".sidebar .friends-row").await.expect("friends");
    app.wait_for(".friends-page .friend", DEFAULT_WAIT).await.expect("bob listed");
    shot(&mut app, "f0-friends").await;
    app.click(".new-message-tab").await.expect("tab");
    shot(&mut app, "f1-new-message").await;
    app.click(".all-friends-tab").await.expect("tab");
    app.click_text(".friends-page .friend button", "Remove").await.expect("remove");
}
