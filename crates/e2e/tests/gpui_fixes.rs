//! Visual rules found by eye in the GPUI app (task 19): the side panel runs
//! to the bottom of the chat, the command palette is laid out evenly, and
//! pictures keep their shape. They hold on Dioxus too where the selectors exist.

#![allow(clippy::expect_used)]

mod common;

use morpheus_e2e::{App, Bounds, DEFAULT_WAIT, api_as, new_account};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

/// The side pane ends where the chat body does (the composer's bottom edge,
/// give or take its own padding), and never past the window.
async fn assert_pane_reaches_bottom(app: &mut App, what: &str) {
    let pane: Bounds = app
        .bounds(".side-pane")
        .await
        .expect("bounds")
        .unwrap_or_else(|| panic!("{what}: no .side-pane"));
    let body = app
        .bounds(".shell-body")
        .await
        .expect("bounds")
        .expect("shell body");
    let window = app.bounds(".shell").await.expect("bounds").expect("shell");
    eprintln!("{what}: pane {pane:?} body {body:?} window {window:?}");
    let pane_bottom = pane.y + pane.h;
    let body_bottom = body.y + body.h;
    // The wrapper pads the pane by 8 px at the bottom.
    assert!(
        (body_bottom - 8.0 - pane_bottom).abs() <= 3.0,
        "{what}: the pane ends at {pane_bottom}, the chat body at {body_bottom}"
    );
    assert!(
        pane_bottom <= window.y + window.h + 1.0,
        "{what}: the pane runs past the window"
    );
}

#[tokio::test]
async fn the_side_panel_runs_to_the_bottom_of_the_chat() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let user = new_account(&hs, "sidepane").expect("account");
    let room = api_as(
        &hs,
        &user,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": "tiny" })),
    )
    .expect("a room");
    let room = room["room_id"]
        .as_str()
        .expect("room id")
        .replace('!', "%21")
        .replace(':', "%3A");
    let root = api_as(
        &hs,
        &user,
        "PUT",
        &format!("rooms/{room}/send/m.room.message/root"),
        Some(serde_json::json!({ "msgtype": "m.text", "body": "findable pangolin" })),
    )
    .expect("root");
    let root = root["event_id"].as_str().expect("root id").to_owned();
    api_as(
        &hs,
        &user,
        "PUT",
        &format!("rooms/{room}/send/m.room.message/reply"),
        Some(serde_json::json!({
            "msgtype": "m.text", "body": "a reply",
            "m.relates_to": { "rel_type": "m.thread", "event_id": root,
                "is_falling_back": true, "m.in_reply_to": { "event_id": root } }
        })),
    )
    .expect("reply");

    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    common::login(&mut app, &user, &format!("{user}-password")).await;
    app.open_room("tiny").await.expect("open room");

    // Members of a room with one member.
    app.wait_for(".members .member", DEFAULT_WAIT)
        .await
        .expect("members");
    assert_pane_reaches_bottom(&mut app, "a room with one member").await;

    // Search.
    app.click(".chat-header .search-toggle").await.expect("search");
    app.wait_for(".search-panel input[name=search]", DEFAULT_WAIT)
        .await
        .expect("search panel");
    assert_pane_reaches_bottom(&mut app, "search, nothing typed").await;
    app.type_into(".search-panel input[name=search]", "pangolin")
        .await
        .expect("type");
    app.wait_for(".search-panel .search-hit", DEFAULT_WAIT)
        .await
        .expect("a hit");
    assert_pane_reaches_bottom(&mut app, "search with a hit").await;

    // A thread.
    app.click(".chat-header .search-toggle").await.ok();
    app.wait_for(".thread-chip", DEFAULT_WAIT).await.expect("chip");
    app.click_text(".thread-chip", "repl").await.expect("thread");
    app.wait_for(".thread-panel .message", DEFAULT_WAIT)
        .await
        .expect("thread panel");
    assert_pane_reaches_bottom(&mut app, "a thread").await;
}

#[tokio::test]
async fn the_member_list_of_a_dm_and_a_big_room_run_to_the_bottom() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    common::login(&mut app, "alice", "alice-password").await;
    app.open_room("crowd").await.expect("open room");
    app.wait_for(".members .member", DEFAULT_WAIT)
        .await
        .expect("members");
    assert_pane_reaches_bottom(&mut app, "a big room").await;

    app.open_room("bob").await.expect("a DM");
    app.wait_for(".composer", DEFAULT_WAIT).await.expect("composer");
    app.wait_for(".side-pane", DEFAULT_WAIT).await.expect("pane");
    assert_pane_reaches_bottom(&mut app, "a DM").await;
}

#[tokio::test]
async fn the_side_panel_runs_to_the_bottom_at_other_window_sizes() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    for (w, h) in [(1000, 520), (1700, 1000)] {
        let mut app = App::launch_sized(&hs, w, h).await.expect("launch app");
        common::login(&mut app, "alice", "alice-password").await;
        app.open_room("crowd").await.expect("open room");
        app.wait_for(".members .member", DEFAULT_WAIT)
            .await
            .expect("members");
        assert_pane_reaches_bottom(&mut app, &format!("crowd at {w}x{h}")).await;
        app.open_room("general").await.expect("open room");
        app.wait_for(".members .member", DEFAULT_WAIT)
            .await
            .expect("members");
        assert_pane_reaches_bottom(&mut app, &format!("general at {w}x{h}")).await;
    }
}

#[tokio::test]
async fn a_long_thread_in_a_short_window_stays_inside_the_pane() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let user = new_account(&hs, "longthread").expect("account");
    let room = api_as(
        &hs,
        &user,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": "long" })),
    )
    .expect("a room");
    let room = room["room_id"]
        .as_str()
        .expect("room id")
        .replace('!', "%21")
        .replace(':', "%3A");
    let long = "a long root that goes on and on ".repeat(12);
    let root = api_as(
        &hs,
        &user,
        "PUT",
        &format!("rooms/{room}/send/m.room.message/root"),
        Some(serde_json::json!({ "msgtype": "m.text", "body": long })),
    )
    .expect("root");
    let root = root["event_id"].as_str().expect("root id").to_owned();
    for n in 0..6 {
        api_as(
            &hs,
            &user,
            "PUT",
            &format!("rooms/{room}/send/m.room.message/r{n}"),
            Some(serde_json::json!({
                "msgtype": "m.text", "body": format!("reply number {n}, with some words in it"),
                "m.relates_to": { "rel_type": "m.thread", "event_id": root,
                    "is_falling_back": true, "m.in_reply_to": { "event_id": root } }
            })),
        )
        .expect("reply");
    }
    let mut app = App::launch_sized(&hs, 1000, 520).await.expect("launch app");
    common::login(&mut app, &user, &format!("{user}-password")).await;
    app.open_room("long").await.expect("open room");
    app.wait_for(".thread-chip", DEFAULT_WAIT).await.expect("chip");
    app.click_text(".thread-chip", "repl").await.expect("thread");
    app.wait_for(".thread-panel .message", DEFAULT_WAIT)
        .await
        .expect("thread panel");
    assert_pane_reaches_bottom(&mut app, "a long thread").await;
    let panel = app.bounds(".thread-panel").await.expect("b").expect("panel");
    let pane = app.bounds(".side-pane").await.expect("b").expect("pane");
    assert!(panel.y + panel.h <= pane.y + pane.h + 1.0, "the panel spills out of the pane");
}
