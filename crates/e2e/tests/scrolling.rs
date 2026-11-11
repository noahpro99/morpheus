//! Timeline scrolling: rooms open at the newest message (also after switching
//! rooms), a jump button appears once you scroll up, and messages that cannot
//! be decrypted yet render as placeholders rather than an error.
//!
//! The scroll checks read the timeline through `App::scroll_state`,
//! `scroll_to` and `bounds`, so they run on the Dioxus and GPUI apps alike.

#![allow(clippy::expect_used)]

use std::time::{Duration, Instant};

use morpheus_e2e::{App, DEFAULT_WAIT, ScrollTo};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login(app: &mut App) {
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
}

/// Poll an async condition until it holds or `timeout` runs out.
macro_rules! until {
    ($what:expr, $timeout:expr, $check:expr) => {{
        let deadline = Instant::now() + $timeout;
        loop {
            if $check {
                break;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {}", $what);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }};
}

/// The view sits at its newest end and a message is showing there.
async fn at_bottom(app: &mut App) -> bool {
    let Some(state) = app.scroll_state(".timeline").await.expect("scroll state") else {
        return false;
    };
    state.from_bottom.abs() < 2.0 && app.count(".timeline .message").await.expect("count") > 0
}

/// Scroll to the top until the timeline can scroll more than 600 px.
async fn reveal_scrollable(app: &mut App) {
    until!("a scrollable timeline", DEFAULT_WAIT, {
        let range = app
            .scroll_state(".timeline")
            .await
            .expect("scroll state")
            .map_or(0.0, |s| s.range);
        if range <= 600.0 {
            app.scroll_to(".timeline", ScrollTo::Top)
                .await
                .expect("scroll up");
        }
        range > 600.0
    });
}

#[tokio::test]
async fn rooms_open_at_the_newest_message_and_jump_back_after_scrolling() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login(&mut app).await;

    app.open_room("firehose").await.expect("open firehose");
    app.wait_for_exact_text(".message .body", "firehose message 600", DEFAULT_WAIT)
        .await
        .expect("newest message loaded");
    until!(
        "opened at the newest message",
        DEFAULT_WAIT,
        at_bottom(&mut app).await
    );
    assert_eq!(
        app.count(".jump-to-bottom").await.expect("count"),
        0,
        "no jump button while at the bottom"
    );

    // Scroll up a few screens: the button appears.
    // A tall window can show the whole first page; load enough history to scroll.
    reveal_scrollable(&mut app).await;
    app.scroll_to(".timeline", ScrollTo::FromBottom(2000.0))
        .await
        .expect("scroll up");
    app.wait_for(".jump-to-bottom", DEFAULT_WAIT)
        .await
        .expect("jump button after scrolling up");
    // Centred over the messages, clear of the message box's buttons.
    let button = app
        .bounds(".jump-to-bottom")
        .await
        .expect("button")
        .expect("button box");
    let timeline = app
        .bounds(".timeline")
        .await
        .expect("timeline")
        .expect("timeline box");
    let off = (button.centre_x() - timeline.centre_x()).abs();
    assert!(off <= 4.0, "jump button {off}px off centre");
    app.click(".jump-to-bottom").await.expect("jump");
    until!(
        "back at the newest message",
        DEFAULT_WAIT,
        at_bottom(&mut app).await
    );
    until!(
        "button gone at the bottom",
        DEFAULT_WAIT,
        app.count(".jump-to-bottom").await.expect("count") == 0
    );

    // Switching rooms (and back) lands at the newest message every time.
    for room in ["random", "firehose", "bob", "random"] {
        app.open_room(room).await.expect("switch room");
        until!(
            format!("{room} opened at the newest message"),
            DEFAULT_WAIT,
            at_bottom(&mut app).await
        );
    }
}

/// A device without the keys (a fresh sign-in, nothing verified) must say
/// so: locked rows instead of an endless loading shimmer, and a notice with
/// the two ways in, each opening the place to do it.
#[tokio::test]
async fn locked_messages_say_how_to_unlock_them() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login(&mut app).await;

    app.open_room("encrypted")
        .await
        .expect("open encrypted room");
    app.wait_for(".message .undecrypted.locked", DEFAULT_WAIT)
        .await
        .expect("a locked row for the message without keys");
    app.wait_for_text(".message .undecrypted.locked", "Encrypted", DEFAULT_WAIT)
        .await
        .expect("the row says it is encrypted");
    assert_eq!(
        app.count(".undecrypted.skeleton").await.expect("skeletons"),
        0,
        "locked rows must not look like loading"
    );

    app.wait_for_text(".key-banner", "this device", DEFAULT_WAIT)
        .await
        .expect("a notice explains the locked messages");
    if let Some(path) = app.screenshot("locked-messages").await.expect("screenshot") {
        eprintln!("screenshot: {}", path.display());
    }
    app.wait_for_text(
        ".key-banner button",
        "Verify with another device",
        DEFAULT_WAIT,
    )
    .await
    .expect("verify offered");
    app.click_text(".key-banner button", "Enter recovery key")
        .await
        .expect("recovery offered");
    app.wait_for(".settings .recovery-form input", DEFAULT_WAIT)
        .await
        .expect("settings open at the recovery key");
    if app.is_gpui() {
        app.wait_for(".settings .recovery-form input[data-focused]", DEFAULT_WAIT)
            .await
            .expect("the recovery key field is focused");
    } else {
        app.wait_for_js(
            "return document.activeElement === document.querySelector('.settings .recovery-form input');",
            DEFAULT_WAIT,
        )
        .await
        .expect("the recovery key field is focused");
    }
}

/// Scrolling up through a long room keeps loading older messages until its
/// first one, as the wheel does (no "Load older messages" click): far back,
/// past the most rows the page keeps, history still comes in.
#[tokio::test]
async fn scrolling_up_reaches_the_start_of_a_long_room() {
    use morpheus_e2e::{api_as, new_account, send_many_as};
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let user = new_account(&hs, "longroom").expect("account");
    let room = api_as(
        &hs,
        &user,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": "archive" })),
    )
    .expect("a room");
    let room = room["room_id"].as_str().expect("room id").to_owned();
    let mut bodies = vec!["the very first message".to_owned()];
    bodies.extend((1..=500).map(|n| format!("history message {n}")));
    send_many_as(&hs, &user, &room, &bodies).expect("history");

    let mut app = App::launch(&hs).await.expect("launch");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", &user)
        .await
        .expect("username");
    app.type_into(
        "form.login input[name=password]",
        &format!("{user}-password"),
    )
    .await
    .expect("password");
    app.submit("form.login").await.expect("submit");
    app.open_room("archive").await.expect("open the room");
    app.wait_for_text(".timeline", "history message 500", DEFAULT_WAIT)
        .await
        .expect("the newest message");

    // The wheel: the view moves up a little at a time; at the top it stays.
    // What shows above the oldest loaded message on the way up: placeholder
    // rows to scroll onto, never a "loading" line.
    let (mut skeletons, mut loading_text) = (false, false);
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        skeletons |= app.count(".timeline .older-skeleton").await.expect("count") > 0;
        loading_text |= app
            .text(".timeline")
            .await
            .expect("text")
            .is_some_and(|t| t.contains("Loading older"));
        if app
            .count(".message .body")
            .await
            .expect("count")
            > 0
            && app
                .texts(".message .body")
                .await
                .expect("texts")
                .iter()
                .any(|b| b.contains("the very first message"))
        {
            break;
        }
        let from_bottom = app
            .scroll_state(".timeline")
            .await
            .expect("scroll state")
            .map_or(0.0, |s| s.from_bottom);
        app.scroll_to(".timeline", ScrollTo::FromBottom(from_bottom + 600.0))
            .await
            .expect("scroll");
        assert!(
            Instant::now() < deadline,
            "scrolling up stopped short of the room's first message (from the bottom: {from_bottom})"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // Past the first message are the room's creation events: once those are
    // in, the placeholders give way to the room's beginning.
    until!("the beginning of the room, no placeholders", DEFAULT_WAIT, {
        app.scroll_to(".timeline", ScrollTo::Top)
            .await
            .expect("scroll");
        app.count(".timeline .older-skeleton").await.expect("count") == 0
            && app.count(".timeline .timeline-start").await.expect("count") > 0
    });
    assert!(
        skeletons,
        "placeholder rows on the way up"
    );
    assert!(!loading_text, "never a loading line");
}

/// Scrolled to the top, the view stays up there while older messages load
/// (it was thrown back to the newest one).
#[tokio::test]
async fn reaching_the_top_stays_there_while_older_messages_load() {
    let Some(hs) = std::env::var("MORPHEUS_TEST_HOMESERVER").ok() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let user = morpheus_e2e::new_account(&hs, "topload").expect("account");
    let room = morpheus_e2e::api_as(
        &hs,
        &user,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": "top load" })),
    )
    .expect("a room");
    let room = room["room_id"].as_str().expect("room id").to_owned();
    let bodies: Vec<String> = (1..=80).map(|n| format!("line {n}")).collect();
    morpheus_e2e::send_many_as(&hs, &user, &room, &bodies).expect("history");
    let mut app = morpheus_e2e::App::launch(&hs).await.expect("launch app");
    app.wait_for("form.login", morpheus_e2e::DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", &user)
        .await
        .expect("username");
    app.type_into(
        "form.login input[name=password]",
        &format!("{user}-password"),
    )
    .await
    .expect("password");
    app.submit("form.login").await.expect("submit");
    app.open_room("top load").await.expect("open the room");
    app.wait_for_exact_text(".message .body", "line 80", morpheus_e2e::DEFAULT_WAIT)
        .await
        .expect("the newest");
    tokio::time::sleep(Duration::from_secs(2)).await;
    // Once to the top, as a scroll wheel would, then watch.
    app.scroll_to(".timeline", ScrollTo::Top)
        .await
        .expect("scroll up");
    let mut lowest = f64::MAX;
    let mut range = 0.0;
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let state = app
            .scroll_state(".timeline")
            .await
            .expect("scroll state")
            .expect("timeline");
        lowest = lowest.min(state.from_bottom);
        range = state.range;
    }
    assert!(
        lowest > range / 2.0,
        "thrown back down while older messages loaded: down to {lowest} from the bottom of {range}"
    );
}
