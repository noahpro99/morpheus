//! Going back to a room you were just in shows it as it was: everything it
//! had loaded, at once, where you left the view, with no skeleton rows or
//! loading indicator and no request for older history; and what came in
//! while you were away is there.

#![allow(clippy::expect_used)]

use std::time::{Duration, Instant};

mod common;

use morpheus_e2e::{App, DEFAULT_WAIT, ScrollTo, api_as, new_account, send_many_as, send_as};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

/// A room of `user`'s named `name`, with `count` messages saying `tag N`.
fn room_with_messages(hs: &str, user: &str, name: &str, tag: &str, count: usize) -> String {
    let made = api_as(
        hs,
        user,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": name })),
    )
    .expect("a room");
    let id = made["room_id"].as_str().expect("room id").to_owned();
    let bodies: Vec<String> = (1..=count)
        .map(|n| format!("{tag} message number {n} with some words so that it is a line of text"))
        .collect();
    send_many_as(hs, user, &id, &bodies).expect("history");
    id
}

/// Two rooms of a new account, each with more than a screen of messages,
/// signed in and sitting in `a`. Returns the app and the names `(a, b)`.
async fn two_busy_rooms(hs: &str) -> (App, String, String, String, String, String) {
    let user = new_account(hs, "cache").expect("account");
    let (a, b) = (format!("cachea{}", &user[6..]), format!("cacheb{}", &user[6..]));
    let a_id = room_with_messages(hs, &user, &a, "AAA", 120);
    let b_id = room_with_messages(hs, &user, &b, "BBB", 120);
    let mut app = App::launch(hs).await.expect("launch");
    common::login(&mut app, &user, &format!("{user}-password")).await;
    app.open_room(&a).await.expect("open a");
    (app, a, b, user, a_id, b_id)
}

/// What the timeline showed at one moment.
#[derive(Debug)]
struct Sample {
    at: Duration,
    shows_a: bool,
    shows_b: bool,
    skeletons: usize,
    older: usize,
    history_requests: Option<String>,
    from_bottom: f64,
    range: f64,
    /// The rows reach the top and bottom of the view (within a row).
    full: bool,
}

async fn sample(app: &mut App, since: Instant) -> Sample {
    let texts = app
        .texts(".timeline .message:not(.skeleton-row)")
        .await
        .expect("texts");
    let skeletons = app.count(".timeline .skeleton-row").await.expect("count");
    let older = app.count(".timeline .older-skeleton").await.expect("count");
    let history_requests = app
        .attr(".timeline", "data-history-requests")
        .await
        .expect("attr");
    let state = app
        .scroll_state(".timeline")
        .await
        .expect("scroll")
        .unwrap_or(morpheus_e2e::ScrollState { from_bottom: -1.0, range: 0.0, viewport: 0.0 });
    let view = app.bounds(".timeline").await.expect("bounds");
    let rows = app
        .all_bounds(".timeline .message:not(.skeleton-row)")
        .await
        .expect("rows");
    let full = view.is_some_and(|view| {
        let top = rows.iter().map(|r| r.y).fold(f64::MAX, f64::min);
        let bottom = rows.iter().map(|r| r.bottom()).fold(f64::MIN, f64::max);
        !rows.is_empty() && top <= view.y + 100.0 && bottom >= view.bottom() - 100.0
    });
    Sample {
        at: since.elapsed(),
        shows_a: texts.iter().any(|t| t.contains("AAA message")),
        shows_b: texts.iter().any(|t| t.contains("BBB message")),
        skeletons,
        older,
        history_requests,
        from_bottom: state.from_bottom,
        range: state.range,
        full,
    }
}

/// Clicks the room's row and samples the timeline for 300 ms from the click.
async fn switch_and_watch(app: &mut App, room: &str) -> Vec<Sample> {
    let started = Instant::now();
    app.click_text(".room .room-name", room).await.expect("click room");
    let clicked = Instant::now();
    let mut samples = Vec::new();
    while clicked.elapsed() < Duration::from_millis(300) {
        samples.push(sample(app, started).await);
    }
    samples
}

/// Waits for the room's own messages to be showing and settled.
async fn settle_in(app: &mut App, room: &str, tag: &str) {
    app.wait_for_text(".chat-header h2", room, DEFAULT_WAIT)
        .await
        .expect("header");
    app.wait_for_text(".timeline .message .body", &format!("{tag} message"), DEFAULT_WAIT)
        .await
        .expect("rows");
    // The first fill and its back-pagination are over.
    tokio::time::sleep(Duration::from_secs(2)).await;
}

/// What returning to A must look like: from the first frame that is not B's
/// on, the view is full of A's rows, nothing is loading and nothing asked for
/// history.
fn assert_back_as_it_was(samples: &[Sample], before: &Sample, requests: &str) {
    let back: Vec<&Sample> = samples.iter().skip_while(|s| !s.shows_a && s.shows_b).collect();
    assert!(
        back.iter().any(|s| s.shows_a),
        "A never showed in 300 ms: {samples:#?}"
    );
    for s in &back {
        assert!(s.shows_a && s.full, "a frame back in A that is not full of its rows: {s:#?}\nall: {samples:#?}");
        assert_eq!(s.skeletons, 0, "skeleton rows: {s:#?}");
        assert_eq!(s.older, 0, "history indicator: {s:#?}");
        assert_eq!(s.history_requests.as_deref(), Some(requests), "asked for history: {s:#?}");
        assert!(
            (s.from_bottom - before.from_bottom).abs() < 3.0 && (s.range - before.range).abs() < 3.0,
            "the view moved (was {before:#?}): {s:#?}"
        );
    }
}

#[tokio::test]
async fn returning_to_a_room_shows_it_whole_at_once() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (mut app, a, b, ..) = two_busy_rooms(&hs).await;
    settle_in(&mut app, &a, "AAA").await;
    app.open_room(&b).await.expect("open b");
    settle_in(&mut app, &b, "BBB").await;
    let before_a = {
        app.open_room(&a).await.expect("open a again");
        settle_in(&mut app, &a, "AAA").await;
        sample(&mut app, Instant::now()).await
    };
    assert!(before_a.range > before_a.from_bottom, "A has history: {before_a:#?}");
    app.open_room(&b).await.expect("open b");
    settle_in(&mut app, &b, "BBB").await;
    let requests = sample(&mut app, Instant::now())
        .await
        .history_requests
        .expect("the request counter");
    let samples = switch_and_watch(&mut app, &a).await;
    assert_back_as_it_was(&samples, &before_a, &requests);
}

#[tokio::test]
async fn returning_to_a_room_scrolled_up_keeps_the_place() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (mut app, a, b, ..) = two_busy_rooms(&hs).await;
    settle_in(&mut app, &a, "AAA").await;
    app.open_room(&b).await.expect("open b");
    settle_in(&mut app, &b, "BBB").await;
    app.open_room(&a).await.expect("open a");
    settle_in(&mut app, &a, "AAA").await;
    // Page all of the history in (up to the room's start), then back down a way.
    let deadline = Instant::now() + DEFAULT_WAIT;
    while app.count(".timeline .timeline-start").await.expect("count") == 0 {
        assert!(Instant::now() < deadline, "the room's start never loaded");
        app.scroll_to(".timeline", ScrollTo::Top).await.expect("to the top");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    app.scroll_to(".timeline", ScrollTo::FromBottom(300.0))
        .await
        .expect("scroll up");
    tokio::time::sleep(Duration::from_secs(1)).await;
    let before = sample(&mut app, Instant::now()).await;
    assert!(
        before.from_bottom > 100.0,
        "scrolled up: {before:#?}"
    );
    app.open_room(&b).await.expect("open b");
    settle_in(&mut app, &b, "BBB").await;
    let requests = sample(&mut app, Instant::now())
        .await
        .history_requests
        .expect("the request counter");
    let samples = switch_and_watch(&mut app, &a).await;
    assert_back_as_it_was(&samples, &before, &requests);
}

#[tokio::test]
async fn what_came_in_while_away_is_there_on_return() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (mut app, a, b, user, a_id, _) = two_busy_rooms(&hs).await;
    settle_in(&mut app, &a, "AAA").await;
    app.open_room(&b).await.expect("open b");
    settle_in(&mut app, &b, "BBB").await;
    app.open_room(&a).await.expect("open a");
    settle_in(&mut app, &a, "AAA").await;
    app.open_room(&b).await.expect("open b");
    settle_in(&mut app, &b, "BBB").await;
    send_as(&hs, &user, &a_id, "AAA message sent while away").expect("send");
    // Let it reach the app, then go back.
    tokio::time::sleep(Duration::from_secs(3)).await;
    app.open_room(&a).await.expect("open a");
    app.wait_for_text(".timeline .message .body", "AAA message sent while away", Duration::from_secs(2))
        .await
        .expect("the message sent while away");
    let last = app
        .texts(".timeline .message:not(.skeleton-row) .body")
        .await
        .expect("texts");
    assert!(
        last.last().is_some_and(|t| t.contains("sent while away")),
        "newest at the bottom: {last:?}"
    );
}

#[tokio::test]
async fn a_room_opened_for_the_first_time_shows_a_full_screen_at_once() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let user = new_account(&hs, "cache").expect("account");
    let (a, b) = (format!("cachea{}", &user[6..]), format!("cacheb{}", &user[6..]));
    room_with_messages(&hs, &user, &a, "AAA", 120);
    room_with_messages(&hs, &user, &b, "BBB", 120);
    let mut app = App::launch(&hs).await.expect("launch");
    common::login(&mut app, &user, &format!("{user}-password")).await;
    // A's history comes to this device, then the app starts again on B: A is
    // a first visit of this run, with what it needs already on disk.
    app.open_room(&a).await.expect("open a");
    settle_in(&mut app, &a, "AAA").await;
    app.open_room(&b).await.expect("open b");
    settle_in(&mut app, &b, "BBB").await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let mut app = app.relaunch().await.expect("relaunch");
    app.wait_for_text(".chat-header h2", &b, DEFAULT_WAIT).await.expect("back in b");
    settle_in(&mut app, &b, "BBB").await;
    let started = Instant::now();
    app.click_text(".room .room-name", &a).await.expect("click room");
    let clicked = Instant::now();
    let mut shown = Vec::new();
    while clicked.elapsed() < Duration::from_millis(1500) {
        let s = sample(&mut app, started).await;
        if s.shows_a {
            shown.push(s);
        }
    }
    assert!(!shown.is_empty(), "A never showed");
    for s in &shown {
        assert!(s.full, "rows of A that do not fill the view, shown before the rest came: {s:#?}");
        assert_eq!(s.skeletons, 0, "skeleton rows with messages: {s:#?}");
    }
}
