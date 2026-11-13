//! Timing checks on a room with hundreds of messages. Prints each wait's
//! duration (`[timing] ...`) so regressions are visible, and fails only on
//! grossly slow operations. They read the timeline through the bridge
//! (`count`, `texts`, `scroll_to`), so they measure the GPUI app: its rows
//! are painted only when near the view, which is what the row counts check.

#![allow(clippy::expect_used)]

use std::time::Duration;

use morpheus_e2e::{App, DEFAULT_WAIT, ScrollTo};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

/// Generous upper bound; the point is the printed numbers, not the limit.
const SLOW: Duration = Duration::from_secs(8);

async fn login_as(app: &mut App, user: &str, password: &str, room: &str) {
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", user)
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", password)
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for_text(".room", room, DEFAULT_WAIT)
        .await
        .unwrap_or_else(|e| panic!("{room} listed: {e}"));
}

/// The number in the first message body showing that starts with `prefix`
/// and is a number after it, at the top of the painted rows.
async fn lowest_number(app: &mut App, prefix: &str) -> Option<i64> {
    app.texts(".message .body")
        .await
        .expect("bodies")
        .iter()
        .filter_map(|b| b.trim().strip_prefix(prefix)?.parse::<i64>().ok())
        .min()
}

#[tokio::test]
async fn big_room_opens_and_switches_quickly() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_as(&mut app, "alice", "alice-password", "firehose").await;

    // Open the big room: time until its newest message is on screen.
    app.open_room("firehose").await.expect("open firehose");
    // Test runs rename bob in every room he is in and the memory test's accounts
    // join this one, so the newest rows can be those, not message 600: scroll up.
    app.reveal_message("firehose message 600", DEFAULT_WAIT)
        .await
        .expect("newest message visible");
    let rendered = app.count(".message").await.expect("count");
    eprintln!("[timing] rows painted after open: {rendered}");

    // Scroll to the top repeatedly: pages come from the server.
    for _ in 0..12 {
        app.scroll_to(".timeline", ScrollTo::Top)
            .await
            .expect("scroll");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let rendered = app.count(".message").await.expect("count");
    eprintln!("[timing] rows painted after scrolling: {rendered}");

    // Switching away and back must stay snappy even with the big room loaded.
    app.open_room("general").await.expect("switch to general");
    app.wait_for_text(".chat-header h2", "general", DEFAULT_WAIT)
        .await
        .expect("general header");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("general ready");
    app.open_room("firehose").await.expect("back to firehose");
    app.wait_for(".timeline .message", DEFAULT_WAIT)
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
/// pools swelled memory with them). The app's own process and any helpers
/// it started count together. Desktop only (the test reads /proc).
#[tokio::test]
async fn threads_and_memory_stay_modest() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    // A fresh account in #firehose alone: the shared test accounts gather
    // rooms, devices and history run after run, which the numbers followed
    // (0.1.0 measured 420 MB when new, 640 MB on the same accounts later).
    let user = morpheus_e2e::new_account(&hs, "memory").expect("an account");
    morpheus_e2e::admin_join(&hs, &user, "#firehose:localhost").expect("in #firehose");
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login_as(&mut app, &user, &format!("{user}-password"), "firehose").await;
    app.open_room("firehose").await.expect("open firehose");
    app.reveal_message("firehose message 600", DEFAULT_WAIT)
        .await
        .expect("newest message visible");
    for _ in 0..12 {
        app.scroll_to(".timeline", ScrollTo::Top)
            .await
            .expect("scroll");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    tokio::time::sleep(Duration::from_secs(2)).await;

    let pid = app.pid().expect("the app's process");
    let threads = std::fs::read_dir(format!("/proc/{pid}/task"))
        .map(|tasks| tasks.count())
        .unwrap_or_default();
    let mut names = std::collections::BTreeMap::<String, usize>::new();
    for task in std::fs::read_dir(format!("/proc/{pid}/task")).into_iter().flatten().flatten() {
        let name = std::fs::read_to_string(task.path().join("comm")).unwrap_or_default();
        *names.entry(name.trim().to_owned()).or_default() += 1;
    }
    eprintln!("[memory] threads by name: {names:?}");
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
    // Each process's share, to tell the app from WebKit's helpers.
    for process in &processes {
        let name = std::fs::read_to_string(format!("/proc/{process}/comm")).unwrap_or_default();
        let rollup =
            std::fs::read_to_string(format!("/proc/{process}/smaps_rollup")).unwrap_or_default();
        let field = |key: &str| -> u64 {
            rollup
                .lines()
                .find_map(|l| l.strip_prefix(key))
                .and_then(|v| v.split_whitespace().next()?.parse().ok())
                .unwrap_or(0)
                / 1024
        };
        eprintln!(
            "[memory]   {:>5} MB  {} (heap and other anonymous {} MB, files {} MB)",
            pss_kb(*process) / 1024,
            name.trim(),
            field("Pss_Anon:"),
            field("Pss_File:"),
        );
    }
    let total_mb = processes.iter().map(|p| pss_kb(*p)).sum::<u64>() / 1024;
    eprintln!(
        "[memory] {threads} threads, {total_mb} MB over {} processes",
        processes.len()
    );
    // GPUI starts one "Worker-N" thread per core for its own executor (a library
    // setting we cannot change): what the app starts itself is what is held down.
    let gpui_workers: usize = names
        .iter()
        .filter(|(name, _)| name.starts_with("Worker-"))
        .map(|(_, count)| count)
        .sum();
    let own = threads - gpui_workers;
    eprintln!("[memory] {own} threads besides GPUI's {gpui_workers} workers");
    assert!(own < 60, "{own} threads in the app besides GPUI's workers");
    assert!(total_mb < 650, "{total_mb} MB for the app (4K window)");
}


/// A long scroll back keeps only the rows near the screen painted:
/// 2000 messages back, the tree holds a few hundred rows, not thousands
/// (every row is layout and memory). Coming back down brings the newer
/// ones back.
#[tokio::test]
async fn a_long_scroll_back_keeps_the_page_small() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_as(&mut app, "alice", "alice-password", "deep").await;
    app.open_room("deep").await.expect("open deep");
    app.reveal_message("deep message 3000", DEFAULT_WAIT)
        .await
        .expect("newest message");

    // Back and back, until message 1000 is painted.
    let mut most = 0usize;
    for _ in 0..400 {
        if lowest_number(&mut app, "deep message ").await.is_some_and(|n| n <= 1000) {
            break;
        }
        app.scroll_to(".timeline", ScrollTo::Top)
            .await
            .expect("scroll");
        tokio::time::sleep(Duration::from_millis(150)).await;
        most = most.max(app.count(".message").await.expect("count"));
    }
    assert!(
        lowest_number(&mut app, "deep message ").await.is_some_and(|n| n <= 1000),
        "never got back to message 1000"
    );
    eprintln!("[timing] most rows painted while scrolling back: {most}");
    assert!(most <= 400, "{most} rows painted");

    // Down again, a screen at a time: the view never jumps back (the message
    // at its top only moves on) until the newest is there.
    let mut last = 0i64;
    let mut seen = Vec::new();
    for _ in 0..400 {
        let state = app
            .scroll_state(".timeline")
            .await
            .expect("scroll state")
            .expect("timeline");
        if state.from_bottom < 50.0 {
            break;
        }
        app.scroll_to(
            ".timeline",
            ScrollTo::FromBottom((state.from_bottom - state.viewport).max(0.0)),
        )
        .await
        .expect("scroll down");
        tokio::time::sleep(Duration::from_millis(120)).await;
        if let Some(number) = lowest_number(&mut app, "deep message ").await {
            seen.push(number);
            assert!(
                number + 5 >= last,
                "the view jumped back from message {last} to {number} on the way down: {seen:?}"
            );
            last = last.max(number);
        }
    }
    app.wait_for_exact_text(".message .body", "deep message 3000", DEFAULT_WAIT)
        .await
        .unwrap_or_else(|e| panic!("never got back down to the newest: {e}; {seen:?}"));

    // And back to now from far up, in one go.
    app.scroll_to(".timeline", ScrollTo::Top)
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
