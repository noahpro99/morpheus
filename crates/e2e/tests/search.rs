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

    app.click(".chat-header .search-toggle")
        .await
        .expect("search button");
    app.wait_for(".search-panel input[name=search]", DEFAULT_WAIT)
        .await
        .expect("search panel");
    // The index takes a moment to learn of a message just sent: ask again
    // until it has.
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        app.type_into(".search-panel input[name=search]", &word)
            .await
            .expect("query");
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

    // Escape closes the panel again.
    app.press("body", "Escape").await.expect("escape");
    app.wait_for_count(".search-panel", 0, DEFAULT_WAIT)
        .await
        .expect("panel closed");
}
