//! Message times read "just now", "N min ago", "N hr ago", then "Today at
//! 14:05" / "Yesterday at 14:05"; the date divider says Today. The full date
//! is the hover hint, and the relative words keep up as time passes.

#![allow(clippy::expect_used)]

mod common;

use morpheus_e2e::{App, DEFAULT_WAIT, api_as, new_account};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

#[tokio::test]
async fn message_times_are_relative_and_keep_up() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let ann = new_account(&hs, "timesann").expect("account");
    let room = api_as(
        &hs,
        &ann,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": "times room" })),
    )
    .expect("room")["room_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    common::login(&mut app, &ann, &format!("{ann}-password")).await;
    app.open_room("times room").await.expect("the room");
    api_as(
        &hs,
        &ann,
        "PUT",
        &format!(
            "rooms/{}/send/m.room.message/t1",
            room.replace('!', "%21").replace(':', "%3A")
        ),
        Some(serde_json::json!({ "msgtype": "m.text", "body": "a fresh message" })),
    )
    .expect("send");
    app.wait_for_text(".message .body", "a fresh message", DEFAULT_WAIT)
        .await
        .expect("message");
    app.wait_for_text(".message.with-header .time", "just now", DEFAULT_WAIT)
        .await
        .expect("a message from this moment reads just now");
    app.wait_for_text(".date-divider", "Today", DEFAULT_WAIT)
        .await
        .expect("the divider says Today");
    let hint = app
        .attr(".message.with-header .time", "title")
        .await
        .expect("title")
        .unwrap_or_default();
    assert!(
        hint.contains(" at ") && hint.contains(", 20"),
        "the hover hint is the full date: {hint:?}"
    );

    // A minute on, the same message reads "1 min ago" without anything else happening.
    app.wait_for_text(
        ".message.with-header .time",
        "1 min ago",
        std::time::Duration::from_secs(100),
    )
    .await
    .expect("the time keeps up");
}

#[tokio::test]
async fn the_gutter_beside_a_grouped_message_shows_only_the_clock_time() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let ann = new_account(&hs, "gutterann").expect("account");
    let room = api_as(
        &hs,
        &ann,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": "gutter room" })),
    )
    .expect("room")["room_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    common::login(&mut app, &ann, &format!("{ann}-password")).await;
    app.open_room("gutter room").await.expect("the room");
    for (n, body) in ["first of a group", "second of a group"].iter().enumerate() {
        api_as(
            &hs,
            &ann,
            "PUT",
            &format!(
                "rooms/{}/send/m.room.message/g{n}",
                room.replace('!', "%21").replace(':', "%3A")
            ),
            Some(serde_json::json!({ "msgtype": "m.text", "body": body })),
        )
        .expect("send");
    }
    app.wait_for_text(".message .body", "second of a group", DEFAULT_WAIT)
        .await
        .expect("messages");
    app.wait_for(".message .time.gutter", DEFAULT_WAIT)
        .await
        .expect("a gutter time");
    let text = app
        .text(".message .time.gutter")
        .await
        .expect("text")
        .unwrap_or_default();
    let clock = text.len() == 5
        && text.as_bytes()[2] == b':'
        && text
            .chars()
            .enumerate()
            .all(|(i, c)| i == 2 || c.is_ascii_digit());
    assert!(clock, "the gutter shows only HH:MM, got {text:?}");
}
