//! Timeline scrolling: rooms open at the newest message (also after switching
//! rooms), a jump button appears once you scroll up, and messages that cannot
//! be decrypted yet render as placeholders rather than an error.
//!
//! The scroll checks read the timeline through `App::scroll_state`,
//! `scroll_to` and `bounds`, so they run on the Dioxus and GPUI apps alike.

#![allow(clippy::expect_used)]

use std::time::{Duration, Instant};

mod common;

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
    // The room ends with the joins of the memory test's accounts (every run adds
    // one), so its newest message is not necessarily the last row: what opens
    // is the newest end with rows showing, which `at_bottom` checks.
    app.wait_for(".timeline .message", DEFAULT_WAIT)
        .await
        .expect("newest rows loaded");
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
    // The seeded event is the oldest: every calls run since piles permission
    // changes (rows of their own) on top of it, so scroll up to it.
    app.reveal_text(".message .undecrypted.locked", "", DEFAULT_WAIT)
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
    let mut previous = f64::MAX;
    let mut range = 0.0;
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let state = app
            .scroll_state(".timeline")
            .await
            .expect("scroll state")
            .expect("timeline");
        // A frame between changing the rows and measuring them reads as no range.
        if state.range == 0.0 {
            continue;
        }
        // Two readings in a row: one frame of rows still being measured can read
        // anywhere.
        lowest = lowest.min(state.from_bottom.max(previous));
        previous = state.from_bottom;
        range = state.range;
    }
    assert!(
        lowest > range / 2.0,
        "thrown back down while older messages loaded: down to {lowest} from the bottom of {range}"
    );
}

// ---- Smooth wheel scrolling (GPUI, task 23) --------------------------------

/// Positions (from the bottom) of `selector` every few ms for `for_ms`, with
/// the time each was read, relative to `t0`.
async fn watch(app: &mut App, selector: &str, t0: Instant, for_ms: u64) -> Vec<(u128, f64)> {
    let mut seen = Vec::new();
    while t0.elapsed() < Duration::from_millis(for_ms) {
        let s = app
            .scroll_state(selector)
            .await
            .expect("scroll state")
            .expect("scrolls");
        seen.push((t0.elapsed().as_millis(), s.from_bottom));
        tokio::time::sleep(Duration::from_millis(4)).await;
    }
    seen
}

/// What a watch shows about one eased scroll that started at `start`:
/// the distinct positions strictly between start and end, the end position
/// and when it was reached.
fn eased(seen: &[(u128, f64)], start: f64) -> (usize, f64, u128) {
    let end = seen.last().expect("samples").1;
    let mut between: Vec<i64> = seen
        .iter()
        .map(|&(_, p)| p)
        .filter(|p| (p - start).abs() > 0.5 && (p - end).abs() > 0.5)
        .map(|p| (p * 4.0) as i64)
        .collect();
    between.dedup();
    let reached = seen
        .iter()
        .find(|&&(_, p)| (p - end).abs() < 0.5)
        .map_or(0, |&(t, _)| t);
    (between.len(), end, reached)
}

async fn long_room_open(hs: &str, name: &str, messages: usize) -> App {
    let user = morpheus_e2e::new_account(hs, "smooth").expect("account");
    let room = morpheus_e2e::api_as(
        hs,
        &user,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": name })),
    )
    .expect("a room");
    let room = room["room_id"].as_str().expect("room id").to_owned();
    let bodies: Vec<String> = (1..=messages).map(|n| format!("line {n}")).collect();
    morpheus_e2e::send_many_as(hs, &user, &room, &bodies).expect("history");
    let mut app = App::launch(hs).await.expect("launch app");
    app.wait_for("form.login", DEFAULT_WAIT).await.expect("login form");
    app.type_into("form.login input[name=username]", &user).await.expect("username");
    app.type_into("form.login input[name=password]", &format!("{user}-password"))
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.open_room(name).await.expect("open the room");
    app.wait_for_exact_text(".message .body", &format!("line {messages}"), DEFAULT_WAIT)
        .await
        .expect("the newest");
    tokio::time::sleep(Duration::from_secs(1)).await;
    app
}

/// A wheel notch moves the timeline over about 150 ms in several steps, to
/// the same place the jump used to go to (a notch is 20 px there); a notch
/// that comes while one is going extends it rather than starting over.
#[tokio::test]
async fn a_wheel_notch_scrolls_the_timeline_smoothly() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = long_room_open(&hs, "smooth room", 120).await;
    if !app.is_gpui() {
        eprintln!("skipped: GPUI's wheel");
        return;
    }
    app.scroll_to(".timeline", ScrollTo::FromBottom(400.0)).await.expect("scroll");
    tokio::time::sleep(Duration::from_millis(300)).await;
    let start = app.scroll_state(".timeline").await.expect("s").expect("t").from_bottom;

    // Three notches in one event: 60 px.
    let t0 = Instant::now();
    app.wheel(".timeline", 3.0).await.expect("wheel");
    let seen = watch(&mut app, ".timeline", t0, 500).await;
    let (steps, end, reached) = eased(&seen, start);
    eprintln!("3 notches: from {start} to {end}, {steps} steps, there after {reached} ms");
    assert!((start - end - 60.0).abs() < 1.5, "moved {} px, not 60", start - end);
    assert!(steps >= 3, "{steps} intermediate positions: it jumped");
    assert!((60..=400).contains(&reached), "arrived after {reached} ms");

    // A notch, then another while it goes: one move of 40 px, never backwards.
    let start = end;
    let t0 = Instant::now();
    app.wheel(".timeline", 1.0).await.expect("wheel");
    tokio::time::sleep(Duration::from_millis(40)).await;
    app.wheel(".timeline", 1.0).await.expect("wheel");
    let seen = watch(&mut app, ".timeline", t0, 500).await;
    let (steps, end, _) = eased(&seen, start);
    eprintln!("2 notches: from {start} to {end}, {steps} steps");
    assert!((start - end - 40.0).abs() < 1.5, "moved {} px, not 40", start - end);
    assert!(steps >= 3, "{steps} intermediate positions");
    assert!(
        seen.windows(2).all(|w| w[1].1 <= w[0].1 + 0.01),
        "went back on the way"
    );

    // And up again, the other way round.
    let start = end;
    app.wheel(".timeline", -2.0).await.expect("wheel");
    tokio::time::sleep(Duration::from_millis(400)).await;
    let back = app.scroll_state(".timeline").await.expect("s").expect("t").from_bottom;
    assert!((back - start - 40.0).abs() < 1.5, "moved {} px up, not 40", back - start);
}

/// A touchpad's pixel deltas are applied exactly and at once, with no easing.
#[tokio::test]
async fn touchpad_pixels_scroll_exactly_and_at_once() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = long_room_open(&hs, "pixel room", 120).await;
    if !app.is_gpui() {
        eprintln!("skipped: GPUI's wheel");
        return;
    }
    app.scroll_to(".timeline", ScrollTo::FromBottom(400.0)).await.expect("scroll");
    tokio::time::sleep(Duration::from_millis(300)).await;
    let start = app.scroll_state(".timeline").await.expect("s").expect("t").from_bottom;
    app.wheel_pixels(".timeline", 37.0).await.expect("wheel");
    let at = app.scroll_state(".timeline").await.expect("s").expect("t").from_bottom;
    assert!((start - at - 37.0).abs() < 0.5, "moved {} px, not 37", start - at);
    app.wheel_pixels(".timeline", -11.0).await.expect("wheel");
    let at = app.scroll_state(".timeline").await.expect("s").expect("t").from_bottom;
    assert!((start - at - 26.0).abs() < 0.5, "then {} px, not 26", start - at);
}

/// The room list (a plain scroll view) eases its notches too, to the same
/// distance as ever: the view's own line height.
#[tokio::test]
async fn a_wheel_notch_scrolls_the_room_list_smoothly() {
    use morpheus_e2e::{api_as, new_account};
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let user = new_account(&hs, "smoothlist").expect("account");
    for n in 1..=30 {
        api_as(&hs, &user, "POST", "createRoom", Some(serde_json::json!({ "name": format!("room {n:02}") })))
            .expect("a room");
    }
    let mut app = App::launch_sized(&hs, 1000, 600).await.expect("launch");
    if !app.is_gpui() {
        eprintln!("skipped: GPUI's wheel");
        return;
    }
    app.wait_for("form.login", DEFAULT_WAIT).await.expect("login form");
    app.type_into("form.login input[name=username]", &user).await.expect("username");
    app.type_into("form.login input[name=password]", &format!("{user}-password"))
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for_text(".room-list", "room 30", DEFAULT_WAIT).await.expect("rooms");
    app.scroll_to(".room-list", ScrollTo::FromBottom(300.0)).await.expect("scroll");
    tokio::time::sleep(Duration::from_millis(300)).await;
    let start = app.scroll_state(".room-list").await.expect("s").expect("t").from_bottom;
    let t0 = Instant::now();
    app.wheel(".room-list", 3.0).await.expect("wheel");
    let seen = watch(&mut app, ".room-list", t0, 500).await;
    let (steps, end, reached) = eased(&seen, start);
    eprintln!("room list, 3 notches: from {start} to {end}, {steps} steps, there after {reached} ms");
    assert!((start - end - 72.0).abs() < 1.5, "moved {} px, not 72", start - end);
    assert!(steps >= 3, "{steps} intermediate positions: it jumped");
    assert!((60..=400).contains(&reached), "arrived after {reached} ms");
}

/// `n`, mean, p50, p95, p99 and max of a list of frame times.
fn stats(label: &str, mut ms: Vec<f32>) -> String {
    ms.sort_by(|a, b| a.total_cmp(b));
    let at = |q: f32| ms.get(((ms.len() as f32 * q) as usize).min(ms.len().saturating_sub(1))).copied().unwrap_or(0.0);
    let mean = ms.iter().sum::<f32>() / ms.len().max(1) as f32;
    format!(
        "{label}: {} frames, mean {mean:.2} ms, p50 {:.2}, p95 {:.2}, p99 {:.2}, max {:.2}",
        ms.len(),
        at(0.5),
        at(0.95),
        at(0.99),
        at(1.0)
    )
}

/// Turns a view one `step` px at a time for `frames` events, up and down (as a
/// touchpad does), and gives back the [draw, present, prepaint, paint] times
/// in ms of the frames it took.
async fn scroll_about(app: &mut App, selector: &str, step: f32, frames: u32) -> Vec<[f32; 4]> {
    app.frame_times().await.expect("start the record");
    for i in 0..frames {
        // Down for a while, then up again.
        let dir = if (i / 40) % 2 == 0 { 1.0 } else { -1.0 };
        app.wheel_pixels(selector, dir * step).await.expect("wheel");
    }
    let frames = app.frame_times().await.expect("frame times");
    frames
}

fn column(frames: &[[f32; 4]], i: usize) -> Vec<f32> {
    frames.iter().map(|f| f[i]).collect()
}

fn p95(mut v: Vec<f32>) -> f32 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[(v.len() as f32 * 0.95) as usize]
}

/// How long frames take while a long timeline with pictures, and the room
/// list, scroll (printed, and appended to target/frame-times.txt): the work
/// of one frame must not grow with the length of the room.
#[tokio::test]
async fn frames_stay_short_while_scrolling_a_long_timeline_and_the_room_list() {
    use morpheus_e2e::{api_as, new_account, send_image_as, send_many_as};
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let user = new_account(&hs, "frames").expect("account");
    for n in 1..=30 {
        api_as(&hs, &user, "POST", "createRoom", Some(serde_json::json!({ "name": format!("room {n:02}") })))
            .expect("a room");
    }
    let room = api_as(&hs, &user, "POST", "createRoom", Some(serde_json::json!({ "name": "long pictures" })))
        .expect("a room");
    let room = room["room_id"].as_str().expect("room id").to_owned();
    let wide: &[u8] = include_bytes!("../fixtures/wide.png");
    let tall: &[u8] = include_bytes!("../fixtures/tall.png");
    for block in 0..12 {
        let bodies: Vec<String> = (1..=8)
            .map(|n| format!("message {} of the long room: some words to wrap over more than a line, the way a real chat goes on and on about things", block * 8 + n))
            .collect();
        send_many_as(&hs, &user, &room, &bodies).expect("history");
        if block % 2 == 0 {
            send_image_as(&hs, &user, &room, wide, "wide.png", None).expect("a picture");
        } else {
            send_image_as(&hs, &user, &room, tall, "tall.png", None).expect("a picture");
        }
    }
    let mut app = App::launch_sized(&hs, 1200, 760).await.expect("launch");
    if !app.is_gpui() {
        eprintln!("skipped: GPUI's frames");
        return;
    }
    common::login(&mut app, &user, &format!("{user}-password")).await;
    app.open_room("long pictures").await.expect("open the room");
    app.wait_for("img.image", DEFAULT_WAIT).await.expect("a picture");
    tokio::time::sleep(Duration::from_secs(3)).await;

    let timeline = scroll_about(&mut app, ".timeline", 40.0, 160).await;
    let list = scroll_about(&mut app, ".room-list", 20.0, 160).await;
    // The same without the e2e element tags: what a build for people costs.
    app.set_tagging(false).await.expect("tagging off");
    let timeline_plain = scroll_about(&mut app, ".timeline", 40.0, 160).await;
    let list_plain = scroll_about(&mut app, ".room-list", 20.0, 160).await;
    let mut report = Vec::new();
    for (name, frames) in [
        ("timeline", &timeline),
        ("room list", &list),
        ("timeline, no tags", &timeline_plain),
        ("room list, no tags", &list_plain),
    ] {
        for (i, part) in ["draw", "present", "prepaint (render, layout)", "paint"].iter().enumerate() {
            report.push(stats(&format!("{name} {part}"), column(frames, i)));
        }
    }
    for line in &report {
        eprintln!("{line}");
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/frame-times.txt"))
        .expect("log");
    use std::io::Write as _;
    writeln!(file, "--- {}", std::env::var("FRAME_LABEL").unwrap_or_default()).expect("write");
    for line in &report {
        writeln!(file, "{line}").expect("write");
    }
    assert!(timeline.len() > 40, "only {} frames drawn while scrolling", timeline.len());
    assert!(p95(column(&timeline, 0)) < 16.0, "the timeline's frames are too long to scroll at 60 fps");
    assert!(p95(column(&list, 0)) < 16.0, "the room list's frames are too long to scroll at 60 fps");
}

/// Page Up / Page Down ease like the wheel does (task 31): the position
/// passes through several values over 80 ms or more and ends one page
/// (nine tenths of the view) further, where the jump used to land.
#[tokio::test]
async fn page_up_and_down_scroll_smoothly_by_a_page() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = long_room_open(&hs, "paged room", 120).await;
    if !app.is_gpui() {
        eprintln!("skipped: GPUI's own easing");
        return;
    }
    app.scroll_to(".timeline", ScrollTo::Bottom).await.expect("scroll");
    tokio::time::sleep(Duration::from_millis(300)).await;
    let s = app.scroll_state(".timeline").await.expect("s").expect("t");
    assert!(s.range > s.viewport * 1.5, "a long enough chat: {s:?}");
    let (start, page) = (s.from_bottom, s.viewport * 0.9);

    for (key, sign) in [("PageUp", 1.0), ("PageDown", -1.0)] {
        let from = app.scroll_state(".timeline").await.expect("s").expect("t").from_bottom;
        let t0 = Instant::now();
        app.press(".composer .composer-input", key).await.expect("press");
        let seen = watch(&mut app, ".timeline", t0, 600).await;
        let (steps, end, reached) = eased(&seen, from);
        let first = seen.iter().find(|&&(_, p)| (p - from).abs() > 0.5).map_or(0, |&(t, _)| t);
        eprintln!("{key}: from {from} to {end}, {steps} steps, moving {first}..{reached} ms");
        assert!((end - from - sign * page).abs() < 3.0, "{key} moved {} px, not {page}", end - from);
        assert!(steps >= 3, "{key}: {steps} intermediate positions: it jumped");
        assert!(reached.saturating_sub(first) >= 80, "{key}: took {} ms", reached.saturating_sub(first));
    }
    let back = app.scroll_state(".timeline").await.expect("s").expect("t").from_bottom;
    assert!((back - start).abs() < 3.0, "up then down returns to {start}, not {back}");
}
