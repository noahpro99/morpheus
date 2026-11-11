//! The message box: lines, replies, edits, Up to edit the last message. Runs
//! on the Dioxus app and on the GPUI app (`MORPHEUS_E2E_UI=gpui`).

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

mod common;

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

const INPUT: &str = ".composer .composer-input";

async fn open(app: &mut App, room: &str) {
    common::login(app, "alice", "alice-password").await;
    app.open_room(room).await.expect("open room");
    app.wait_for(
        ".composer .composer-input:not([disabled])",
        DEFAULT_WAIT,
    )
    .await
    .expect("composer ready");
}

async fn send(app: &mut App, text: &str) {
    app.type_into(INPUT, text).await.expect("type");
    app.press(INPUT, "Enter").await.expect("send");
}

/// Whether some message (not still sending) has all of `needles` in its text.
async fn message_with(app: &mut App, needles: &[&str]) -> bool {
    app.texts(".message:not(.pending)")
        .await
        .expect("texts")
        .iter()
        .any(|text| needles.iter().all(|n| text.contains(n)))
}

async fn wait_for_message_with(app: &mut App, needles: &[&str]) {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while !message_with(app, needles).await {
        assert!(
            std::time::Instant::now() < deadline,
            "no message with {needles:?}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// Click a message's hover action until `then` shows (the bar appears on hover).
async fn hover_action(app: &mut App, message: &str, action: &str, then: &str, then_text: &str) {
    let selector = format!(".action.{action}");
    for _ in 0..5 {
        app.click_within(".message", message, &selector)
            .await
            .unwrap_or_else(|e| panic!("{action} on {message:?}: {e}"));
        if app
            .wait_for_text(then, then_text, std::time::Duration::from_secs(3))
            .await
            .is_ok()
        {
            return;
        }
    }
    panic!("{action} on {message:?} never produced {then} with {then_text:?}");
}

#[tokio::test]
async fn the_box_names_the_room_and_grows_with_its_lines() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    open(&mut app, "general").await;
    assert_eq!(
        app.count(".composer .composer-input[placeholder=\"Message #general\"]")
            .await
            .expect("count"),
        1,
        "the placeholder names the room"
    );
    let one = app.bounds(INPUT).await.expect("bounds").expect("box");

    // A line break is Shift+Enter, not a send; the box grows with its lines.
    let pid = std::process::id();
    let text = format!("first {pid}\nsecond {pid}");
    app.type_into(INPUT, &text).await.expect("type lines");
    app.wait_for_value(INPUT, &text, DEFAULT_WAIT)
        .await
        .expect("both lines in the box");
    let two = app.bounds(INPUT).await.expect("bounds").expect("box");
    assert!(
        two.h > one.h + 5.0,
        "two lines are taller than one: {one:?} then {two:?}"
    );
    assert_eq!(
        app.count(".composer .composer-input[rows=\"2\"]")
            .await
            .expect("count"),
        1
    );

    // Enter sends both lines as one message, and the box is empty again.
    app.press(INPUT, "Enter").await.expect("send");
    wait_for_message_with(&mut app, &[&format!("first {pid}"), &format!("second {pid}")]).await;
    app.wait_for_value(INPUT, "", DEFAULT_WAIT)
        .await
        .expect("box emptied");
    let after = app.bounds(INPUT).await.expect("bounds").expect("box");
    assert!(after.h < two.h, "one line again: {after:?}");
}

#[tokio::test]
async fn replies_and_edits_start_from_a_messages_hover_bar() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    open(&mut app, "dev").await;
    let pid = std::process::id();

    // Reply to a message: the banner names its sender, the sent message
    // carries a quote of it.
    let target = format!("reply target {pid}");
    send(&mut app, &target).await;
    wait_for_message_with(&mut app, &[&target]).await;
    hover_action(&mut app, &target, "reply", ".composer-banner", "alice").await;
    app.wait_for_text(".composer-banner", "Replying to", DEFAULT_WAIT)
        .await
        .expect("reply banner");
    let reply = format!("reply from composer {pid}");
    send(&mut app, &reply).await;
    wait_for_message_with(&mut app, &[&reply, &target]).await;
    app.wait_for_count(".composer-banner", 0, DEFAULT_WAIT)
        .await
        .expect("the banner clears after sending");

    // A cancelled reply leaves the box as it was.
    hover_action(
        &mut app,
        &reply,
        "reply",
        ".composer-banner",
        "Replying to",
    )
    .await;
    app.click(".composer-banner .ghost").await.expect("cancel");
    app.wait_for_count(".composer-banner", 0, DEFAULT_WAIT)
        .await
        .expect("cancelled");

    // Edit: the box is filled with the original, Enter saves it.
    let original = format!("draft from composer {pid}");
    send(&mut app, &original).await;
    wait_for_message_with(&mut app, &[&original]).await;
    hover_action(&mut app, &original, "edit", ".composer-banner", "Editing").await;
    app.wait_for_value(INPUT, &original, DEFAULT_WAIT)
        .await
        .expect("the box holds the message");
    let edited = format!("edited from composer {pid}");
    send(&mut app, &edited).await;
    wait_for_message_with(&mut app, &[&edited, "(edited)"]).await;
    app.wait_for_count(".composer-banner", 0, DEFAULT_WAIT)
        .await
        .expect("the banner clears after saving");
    app.wait_for_value(INPUT, "", DEFAULT_WAIT)
        .await
        .expect("box emptied");
}

#[tokio::test]
async fn up_in_an_empty_box_edits_the_last_message_and_escape_cancels() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    open(&mut app, "general").await;

    let text = format!("edit me {}", std::process::id());
    send(&mut app, &text).await;
    wait_for_message_with(&mut app, &[&text]).await;

    // Up in the empty box edits it: the banner shows and the text is back in the box.
    app.key_combo(INPUT, "ArrowUp", false, false, false)
        .await
        .expect("up");
    app.wait_for_text(".composer-banner", "Editing message", DEFAULT_WAIT)
        .await
        .expect("edit mode");
    app.wait_for_value(INPUT, &text, DEFAULT_WAIT)
        .await
        .expect("draft holds the message");

    // Escape cancels the edit and empties the box.
    app.press(INPUT, "Escape").await.expect("cancel edit");
    app.wait_for_count(".composer-banner", 0, DEFAULT_WAIT)
        .await
        .expect("edit cancelled");
    app.wait_for_value(INPUT, "", DEFAULT_WAIT)
        .await
        .expect("box emptied");
}
