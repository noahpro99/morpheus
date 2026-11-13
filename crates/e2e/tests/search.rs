//! Searching a room's messages from its header: results list the sender and
//! text, and picking one jumps to it in the chat.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT, ScrollTo};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

#[tokio::test]
async fn search_finds_a_message_and_jumps_to_it() {
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
    app.type_into("form.login input[name=password]", "alice-password")
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    app.open_room("random").await.expect("open random");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer");

    let word = format!("okapi{}", std::process::id());
    let text = format!("an {word} in the forest");
    app.type_into(".composer .composer-input", &text)
        .await
        .expect("type");
    app.press(".composer .composer-input", "Enter")
        .await
        .expect("send");
    app.wait_for_exact_text(".message.own:not(.pending) .body", &text, DEFAULT_WAIT)
        .await
        .expect("sent");
    // Scroll far away from it.
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let range = app
            .scroll_state(".timeline")
            .await
            .expect("scroll state")
            .map_or(0.0, |s| s.range);
        if range > 1500.0 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for history to scroll through"
        );
        app.scroll_to(".timeline", ScrollTo::Top)
            .await
            .expect("scroll up");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    app.scroll_to(".timeline", ScrollTo::Top)
        .await
        .expect("scroll to the top");

    app.click(".chat-header .search-bar input[name=search]")
        .await
        .expect("search box");
    // The index takes a moment to learn of a message just sent: ask again
    // until it has.
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        app.type_into(".chat-header input[name=search]", &word)
            .await
            .expect("query");
        app.press(".chat-header input[name=search]", "Enter")
            .await
            .expect("search");
        if app
            .wait_for_text(".search-panel .search-hit", &word, std::time::Duration::from_secs(2))
            .await
            .is_ok()
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the message never showed up in the results"
        );
    }
    app.wait_for_text(
        ".search-panel .search-hit .search-sender",
        "alice",
        DEFAULT_WAIT,
    )
    .await
    .expect("the sender shown");

    app.click(".search-panel .search-hit")
        .await
        .expect("pick it");
    // It is scrolled into view and highlighted.
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let target = app
            .bounds_containing(".message.jump-target", &word)
            .await
            .expect("target bounds");
        let timeline = app.bounds(".timeline").await.expect("timeline bounds");
        if let (Some(t), Some(v)) = (target, timeline)
            && t.y >= v.y - 1.0
            && t.y + t.h <= v.y + v.h + 1.0
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "never jumped to the message and highlighted it"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    // Escape in the box closes the panel again.
    app.press(".chat-header input[name=search]", "Escape")
        .await
        .expect("escape");
    app.wait_for_count(".search-panel", 0, DEFAULT_WAIT)
        .await
        .expect("panel closed");
}

/// Discord's search bar: focusing it only widens it in place (no panel);
/// Enter shows the results in a panel at the right, wider than the member
/// list it replaces; the panel stays while the query does, even with the
/// box unfocused; the box's ✕ or Escape clears it and the member list is
/// back; an empty, unfocused box shrinks again.
#[tokio::test]
async fn the_search_box_widens_and_its_results_replace_the_member_list() {
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
    app.type_into("form.login input[name=password]", "alice-password")
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    app.open_room("random").await.expect("open random");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer");
    let word = format!("narwhal{}", std::process::id());
    let text = format!("a {word} in the sea");
    app.type_into(".composer .composer-input", &text)
        .await
        .expect("type");
    app.press(".composer .composer-input", "Enter")
        .await
        .expect("send");
    app.wait_for_exact_text(".message.own:not(.pending) .body", &text, DEFAULT_WAIT)
        .await
        .expect("sent");

    let bar = ".chat-header .search-bar";
    let input = ".chat-header input[name=search]";
    async fn width(app: &mut App, sel: &str) -> f64 {
        app.bounds(sel).await.expect("bounds").map_or(0.0, |b| b.w)
    }
    async fn settle(app: &mut App, sel: &str, wide: bool) -> f64 {
        let deadline = std::time::Instant::now() + DEFAULT_WAIT;
        loop {
            let a = width(app, sel).await;
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            let b = width(app, sel).await;
            if (a - b).abs() < 0.5 && (b > 200.0) == wide {
                return b;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the box never settled {} (at {b})",
                if wide { "wide" } else { "narrow" }
            );
        }
    }

    app.wait_for(".side-pane .members", DEFAULT_WAIT)
        .await
        .expect("the member list");
    let members_w = width(&mut app, ".side-pane").await;
    let rest = settle(&mut app, bar, false).await;
    assert!(rest < 160.0, "the box rests narrow: {rest}");

    // Focusing it widens it in place and opens nothing.
    app.click(input).await.expect("focus the box");
    let wide = settle(&mut app, bar, true).await;
    assert!(wide >= rest + 60.0, "it widens: {rest} -> {wide}");
    assert_eq!(app.count(".search-panel").await.expect("count"), 0, "no panel yet");
    assert_eq!(app.count(".side-pane .members").await.expect("count"), 1, "members stay");

    // Enter searches: a panel wider than the member list, at the right.
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        app.type_into(input, &word).await.expect("query");
        app.press(input, "Enter").await.expect("search");
        if app
            .wait_for_text(".search-panel .search-hit", &word, std::time::Duration::from_secs(2))
            .await
            .is_ok()
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the message never showed up in the results"
        );
    }
    let panel = app.bounds(".side-pane").await.expect("b").expect("side pane");
    let window = app.bounds(".chat-header").await.expect("b").expect("header");
    assert!(panel.w >= members_w + 100.0, "wider than the member list: {panel:?} vs {members_w}");
    assert!(panel.right() >= window.right() - 12.0, "at the right: {panel:?} in {window:?}");
    assert_eq!(app.count(".side-pane .members").await.expect("count"), 0, "members replaced");

    // Clicking elsewhere keeps it, and the box stays wide while it has text.
    app.click(".composer .composer-input").await.expect("click away");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(app.count(".search-panel .search-hit").await.expect("count"), 1, "results stay");
    settle(&mut app, bar, true).await;

    // The ✕ clears it: the member list is back, as it was.
    app.click(".chat-header .search-clear").await.expect("clear");
    app.wait_for_count(".search-panel", 0, DEFAULT_WAIT)
        .await
        .expect("panel closed");
    app.wait_for(".side-pane .members", DEFAULT_WAIT)
        .await
        .expect("members back");
    assert!((width(&mut app, ".side-pane").await - members_w).abs() < 1.0, "same width");
    assert_eq!(app.value(input).await.expect("value").unwrap_or_default(), "", "box emptied");
    // An empty box that lost the caret shrinks back.
    app.click(".composer .composer-input").await.expect("click away");
    settle(&mut app, bar, false).await;

    // Escape in the box does the same.
    app.click(input).await.expect("focus");
    app.type_into(input, &word).await.expect("query");
    app.press(input, "Enter").await.expect("search");
    app.wait_for(".search-panel", DEFAULT_WAIT).await.expect("panel");
    app.press(input, "Escape").await.expect("escape");
    app.wait_for_count(".search-panel", 0, DEFAULT_WAIT)
        .await
        .expect("panel closed");
    app.wait_for(".side-pane .members", DEFAULT_WAIT)
        .await
        .expect("members back");
}
