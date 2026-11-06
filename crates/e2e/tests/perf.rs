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
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
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

/// Memory and threads, after the big room is opened and scrolled through:
/// a chat app needs a few threads, not one per CPU core for each of its
/// runtimes (a 32-core machine had 146, and the allocator's per-thread
/// pools swelled memory with them). The app's own process and WebKit's
/// helpers count together. Desktop only (the test reads /proc).
#[tokio::test]
async fn threads_and_memory_stay_modest() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
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
    app.open_room("firehose").await.expect("open firehose");
    app.reveal_message("firehose message 600", DEFAULT_WAIT)
        .await
        .expect("newest message visible");
    for _ in 0..12 {
        app.eval("const t = document.querySelector('.timeline'); t.scrollTop = -t.scrollHeight;")
            .await
            .expect("scroll");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    tokio::time::sleep(Duration::from_secs(2)).await;

    let pid = app.pid().expect("the app's process");
    let threads = std::fs::read_dir(format!("/proc/{pid}/task"))
        .map(|tasks| tasks.count())
        .unwrap_or_default();
    let mut processes = vec![pid];
    let mut at = 0;
    while at < processes.len() {
        let parent = processes[at];
        at += 1;
        if let Ok(children) =
            std::fs::read_to_string(format!("/proc/{parent}/task/{parent}/children"))
        {
            processes.extend(
                children
                    .split_whitespace()
                    .filter_map(|c| c.parse::<u32>().ok()),
            );
        }
    }
    let pss_kb = |pid: u32| -> u64 {
        std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup"))
            .unwrap_or_default()
            .lines()
            .find_map(|l| l.strip_prefix("Pss:"))
            .and_then(|v| v.split_whitespace().next()?.parse().ok())
            .unwrap_or(0)
    };
    let total_mb = processes.iter().map(|p| pss_kb(*p)).sum::<u64>() / 1024;
    eprintln!(
        "[memory] {threads} threads, {total_mb} MB over {} processes",
        processes.len()
    );
    assert!(threads < 60, "{threads} threads in the app");
    assert!(total_mb < 500, "{total_mb} MB for the app");
}

/// A long scroll back keeps only the rows near the screen in the page:
/// 2000 messages back, the page holds a few hundred rows, not thousands
/// (every row is DOM, layout and memory). Coming back down brings the newer
/// ones back.
#[tokio::test]
async fn a_long_scroll_back_keeps_the_page_small() {
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
    app.wait_for_text(".room", "deep", DEFAULT_WAIT)
        .await
        .expect("deep listed");
    app.open_room("deep").await.expect("open deep");
    app.reveal_message("deep message 3000", DEFAULT_WAIT)
        .await
        .expect("newest message");

    // Back and back, until message 1000 is on the page.
    let target = "return [...document.querySelectorAll('.message .body')].some(b => b.textContent.trim() === 'deep message 1000');";
    let mut most = 0u64;
    for _ in 0..400 {
        if app.eval(target).await.expect("look") == serde_json::json!(true) {
            break;
        }
        app.eval("const t = document.querySelector('.timeline'); t.scrollTop = -t.scrollHeight;")
            .await
            .expect("scroll");
        tokio::time::sleep(Duration::from_millis(150)).await;
        let rows = app
            .eval("return document.querySelectorAll('.message').length;")
            .await
            .expect("count")
            .as_u64()
            .unwrap_or_default();
        most = most.max(rows);
    }
    assert_eq!(
        app.eval(target).await.expect("look"),
        serde_json::json!(true),
        "never got back to message 1000"
    );
    eprintln!("[timing] most rows on the page while scrolling back: {most}");
    assert!(most <= 400, "{most} rows in the page");

    // Down again, a screen at a time: rows come back below as the oldest
    // go, and the view never jumps back (the message at its top only moves
    // on), until the newest is there.
    let top_number = "const t = document.querySelector('.timeline'); const top = t.getBoundingClientRect().top; \
         const m = [...t.querySelectorAll('.message')].find(m => m.getBoundingClientRect().bottom > top + 4); \
         const b = m && [...m.querySelectorAll('.body')].map(b => b.textContent.trim()).find(x => x.startsWith('deep message ')); \
         return b ? parseInt(b.slice(13)) : null;";
    let newest = "return [...document.querySelectorAll('.message .body')].some(b => b.textContent.trim() === 'deep message 3000');";
    let mut last = 0i64;
    let mut seen = Vec::new();
    for _ in 0..400 {
        if app.eval(newest).await.expect("look") == serde_json::json!(true)
            && app
                .eval("return document.querySelector('.timeline').scrollTop > -50;")
                .await
                .expect("at bottom")
                == serde_json::json!(true)
        {
            break;
        }
        app.eval("const t = document.querySelector('.timeline'); t.scrollTop = Math.min(0, t.scrollTop + t.clientHeight);")
            .await
            .expect("scroll down");
        tokio::time::sleep(Duration::from_millis(120)).await;
        if let Some(number) = app.eval(top_number).await.expect("top").as_i64() {
            seen.push(number);
            assert!(
                number + 5 >= last,
                "the view jumped back from message {last} to {number} on the way down: {seen:?}"
            );
            last = last.max(number);
        }
    }
    assert_eq!(
        app.eval(newest).await.expect("look"),
        serde_json::json!(true),
        "never got back down to the newest: {seen:?}"
    );

    // And back to now from far up, in one go.
    app.eval("const t = document.querySelector('.timeline'); t.scrollTop = -t.scrollHeight;")
        .await
        .expect("up a bit");
    app.wait_for(".jump-to-bottom", DEFAULT_WAIT)
        .await
        .expect("a way back to the newest");
    app.click(".jump-to-bottom").await.expect("jump");
    app.wait_for_exact_text(".message .body", "deep message 3000", DEFAULT_WAIT)
        .await
        .expect("the newest again");
}
