//! Timing checks on a room with hundreds of messages. Prints each wait's
//! duration (`[timing] ...`) so regressions are visible, and fails only on
//! grossly slow operations.

#![allow(clippy::expect_used)]

use std::time::Duration;

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

/// Generous upper bound; the point is the printed numbers, not the limit.
const SLOW: Duration = Duration::from_secs(8);

#[tokio::test]
async fn big_room_opens_and_switches_quickly() {
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
    app.wait_for_text(".room", "firehose", DEFAULT_WAIT)
        .await
        .expect("firehose listed");

    // Open the big room: time until its newest message is on screen.
    app.open_room("firehose").await.expect("open firehose");
    // Test runs rename bob in every room he is in, so the newest page the
    // server hands over can be all (hidden) profile changes; scroll up.
    app.reveal_message("firehose message 600", DEFAULT_WAIT)
        .await
        .expect("newest message visible");
    let rendered = app
        .eval("return document.querySelectorAll('.message').length;")
        .await
        .expect("count");
    eprintln!("[timing] rows rendered after open: {rendered}");

    // Scroll to the top repeatedly: reveals windowed rows first, then pages from the server.
    for _ in 0..12 {
        app.eval("const t = document.querySelector('.timeline'); t.scrollTop = -t.scrollHeight;")
            .await
            .expect("scroll");
        app.wait_for_js("return true;", Duration::from_secs(1))
            .await
            .expect("tick");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let rendered = app
        .eval("return document.querySelectorAll('.message').length;")
        .await
        .expect("count");
    eprintln!("[timing] rows rendered after scrolling: {rendered}");

    // Switching away and back must stay snappy even with the big room loaded.
    app.open_room("general").await.expect("switch to general");
    app.wait_for_text(".chat-header h2", "general", DEFAULT_WAIT)
        .await
        .expect("general header");
    app.wait_for(".composer input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("general ready");
    app.open_room("firehose").await.expect("back to firehose");
    app.wait_for_exact_text(".message .body", "firehose message 600", DEFAULT_WAIT)
        .await
        .expect("firehose again");

    app.report_timings("big room");
    let slowest = app
        .timings()
        .iter()
        .map(|(_, d)| *d)
        .max()
        .unwrap_or_default();
    assert!(slowest < SLOW, "slowest wait took {slowest:?}");
}
