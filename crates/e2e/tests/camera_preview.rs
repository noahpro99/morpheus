//! Settings → Voice & Video has a camera preview, as Discord's "Video
//! Settings": the camera picker, and a Test Video button that opens the
//! chosen camera and shows it live (mirrored, in its own shape, rounded)
//! until Stop. The camera is let go when stopped, when the page is left and
//! when Settings closes; in a call with the camera on, the preview shows
//! that same feed instead of opening the camera twice.
//!
//! Test runs use the test camera (never the real one): its opens are
//! counted (`.camera-preview[data-open]` now, `[data-opened]` ever) and
//! `MORPHEUS_CALL_CAMERA_FAIL` (`none`, `denied`, `busy`) makes it fail.

#![allow(clippy::expect_used)]

mod common;

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn open_video_settings(app: &mut App) {
    app.click(".settings-button").await.expect("gear");
    app.wait_for_text(".settings-nav button", "Voice & Video", DEFAULT_WAIT)
        .await
        .expect("settings");
    app.click_text(".settings-nav button", "Voice & Video")
        .await
        .expect("voice page");
    app.wait_for(".voice-video .camera-preview", DEFAULT_WAIT)
        .await
        .expect("the camera preview on the voice page");
}

async fn close_settings(app: &mut App) {
    app.click(".settings-header button[title=Close]")
        .await
        .expect("close settings");
    app.wait_for_count(".settings", 0, DEFAULT_WAIT)
        .await
        .expect("settings closed");
}

async fn number(app: &mut App, selector: &str, attr: &str) -> i64 {
    app.attr(selector, attr)
        .await
        .expect("read")
        .and_then(|v| v.parse().ok())
        .unwrap_or(-1)
}

/// Wait until the camera is open `n` times now.
async fn wait_open(app: &mut App, n: i64) {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let now = number(app, ".camera-preview", "data-open").await;
        if now == n {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the camera is open {now} times, wanted {n}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// The preview's picture gets newer frames.
async fn wait_frames_advance(app: &mut App) {
    app.wait_for(
        ".camera-preview .preview-video[data-shown=true]",
        DEFAULT_WAIT,
    )
    .await
    .expect("a picture in the preview");
    let first = number(app, ".preview-video", "data-seq").await;
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while number(app, ".preview-video", "data-seq").await < first + 5 {
        assert!(
            std::time::Instant::now() < deadline,
            "the preview's frames do not advance (stuck at {first})"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn test_video_shows_the_camera_live_and_stop_lets_it_go() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch");
    common::login(&mut app, "alice", "alice-password").await;
    open_video_settings(&mut app).await;
    assert_eq!(
        number(&mut app, ".camera-preview", "data-open").await,
        0,
        "closed at first"
    );
    assert_eq!(
        app.count(".preview-video").await.expect("count"),
        0,
        "nothing shown yet"
    );
    app.click(".camera-test").await.expect("Test Video");
    wait_frames_advance(&mut app).await;
    wait_open(&mut app, 1).await;
    assert_eq!(
        app.attr(".preview-video", "data-mirrored")
            .await
            .expect("attr")
            .as_deref(),
        Some("true"),
        "the preview is a mirror"
    );
    // Its own shape (the test camera is 16:9), not a fixed box.
    assert_eq!(
        app.attr(".preview-video", "data-size")
            .await
            .expect("attr")
            .as_deref(),
        Some("1280x720")
    );
    let bounds = app
        .bounds(".camera-preview .preview-video")
        .await
        .expect("bounds")
        .expect("shown");
    let ratio = bounds.w / bounds.h;
    assert!(
        (ratio - 16.0 / 9.0).abs() < 0.03,
        "the preview is {}x{}",
        bounds.w,
        bounds.h
    );
    app.click(".camera-stop").await.expect("Stop");
    app.wait_for(".camera-test", DEFAULT_WAIT)
        .await
        .expect("Test Video is back");
    assert_eq!(
        app.count(".preview-video").await.expect("count"),
        0,
        "the picture goes"
    );
    wait_open(&mut app, 0).await;
    assert_eq!(
        number(&mut app, ".camera-preview", "data-opened").await,
        1,
        "opened once"
    );
}

#[tokio::test]
async fn leaving_the_page_or_closing_settings_lets_the_camera_go() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch");
    common::login(&mut app, "alice", "alice-password").await;
    open_video_settings(&mut app).await;
    app.click(".camera-test").await.expect("Test Video");
    wait_open(&mut app, 1).await;
    // Another page: the camera goes; back on this one it waits.
    app.click_text(".settings-nav button", "Appearance")
        .await
        .expect("another page");
    app.click_text(".settings-nav button", "Voice & Video")
        .await
        .expect("back");
    app.wait_for(".camera-test", DEFAULT_WAIT)
        .await
        .expect("not running");
    wait_open(&mut app, 0).await;
    // Closing Settings with it running.
    app.click(".camera-test").await.expect("Test Video");
    wait_open(&mut app, 1).await;
    close_settings(&mut app).await;
    open_video_settings(&mut app).await;
    wait_open(&mut app, 0).await;
    assert_eq!(
        app.count(".preview-video").await.expect("count"),
        0,
        "not shown again"
    );
    assert_eq!(number(&mut app, ".camera-preview", "data-opened").await, 2);
}

/// The camera chosen is the one a call sends (and the preview opens).
#[tokio::test]
async fn the_camera_chosen_is_the_one_the_preview_and_calls_use() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch");
    common::login(&mut app, "alice", "alice-password").await;
    open_video_settings(&mut app).await;
    app.click(".voice-devices .device-choice[data-kind=camera] .device-current")
        .await
        .expect("open the picker");
    app.click(".device-choice[data-kind=camera] .device-option[data-id=\"test-camera-2\"]")
        .await
        .expect("pick camera 2");
    app.click(".camera-test").await.expect("Test Video");
    app.wait_for(
        ".camera-preview[data-camera=\"test-camera-2\"][data-open=\"1\"]",
        DEFAULT_WAIT,
    )
    .await
    .expect("the preview opens the chosen camera");
    app.click(".camera-stop").await.expect("Stop");
    close_settings(&mut app).await;
    join_hangout(&mut app).await;
    app.click(".voice-panel .camera-button")
        .await
        .expect("camera on");
    app.wait_for(
        ".call-stage .video-tile.self[data-camera=\"test-camera-2\"]",
        DEFAULT_WAIT,
    )
    .await
    .expect("the call sends the camera chosen");
}

async fn join_hangout(app: &mut App) {
    app.key_combo("body", "k", true, false, false)
        .await
        .expect("ctrl+k");
    app.wait_for(".switcher input", DEFAULT_WAIT)
        .await
        .expect("switcher");
    app.type_into(".switcher input", "hangout")
        .await
        .expect("filter");
    app.wait_for_text(".switcher-item.active", "hangout", DEFAULT_WAIT)
        .await
        .expect("room offered");
    app.press(".switcher input", "Enter").await.expect("pick");
    app.click(".voice-page-join .join-voice")
        .await
        .expect("join");
    app.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("in the call");
}

/// In a call with the camera on, the preview is that feed: no second open.
#[tokio::test]
async fn in_a_call_with_the_camera_on_the_preview_shows_the_calls_feed() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch");
    common::login(&mut app, "alice", "alice-password").await;
    join_hangout(&mut app).await;
    app.click(".voice-panel .camera-button")
        .await
        .expect("camera on");
    app.wait_for(
        ".call-stage .video-tile.self .video[data-shown=true]",
        DEFAULT_WAIT,
    )
    .await
    .expect("the camera is on");
    open_video_settings(&mut app).await;
    assert_eq!(
        number(&mut app, ".camera-preview", "data-open").await,
        1,
        "the call has it"
    );
    let opened = number(&mut app, ".camera-preview", "data-opened").await;
    app.click(".camera-test").await.expect("Test Video");
    wait_frames_advance(&mut app).await;
    assert_eq!(
        app.attr(".preview-video", "data-mirrored")
            .await
            .expect("attr")
            .as_deref(),
        Some("true")
    );
    assert_eq!(
        number(&mut app, ".camera-preview", "data-open").await,
        1,
        "still one open"
    );
    assert_eq!(
        number(&mut app, ".camera-preview", "data-opened").await,
        opened,
        "the camera was not opened again"
    );
    // Stopping the preview leaves the call's camera on.
    app.click(".camera-stop").await.expect("Stop");
    app.wait_for(".camera-test", DEFAULT_WAIT)
        .await
        .expect("stopped");
    assert_eq!(
        number(&mut app, ".camera-preview", "data-open").await,
        1,
        "the call keeps it"
    );
    // Camera off in the call, then the preview opens its own.
    close_settings(&mut app).await;
    app.click(".voice-panel .camera-button")
        .await
        .expect("camera off");
    open_video_settings(&mut app).await;
    wait_open(&mut app, 0).await;
    app.click(".camera-test").await.expect("Test Video");
    wait_frames_advance(&mut app).await;
    wait_open(&mut app, 1).await;
    // The call's camera on while the preview runs: the preview follows it.
    close_settings(&mut app).await;
    app.click(".voice-panel .camera-button")
        .await
        .expect("camera on");
    app.wait_for(
        ".call-stage .video-tile.self .video[data-shown=true]",
        DEFAULT_WAIT,
    )
    .await
    .expect("the camera is on");
    open_video_settings(&mut app).await;
    assert_eq!(
        number(&mut app, ".camera-preview", "data-open").await,
        1,
        "one open, the call's"
    );
}

#[tokio::test]
async fn with_no_camera_the_preview_says_so() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    for (fail, words) in [
        ("none", "No camera found"),
        ("denied", "not allowed"),
        ("busy", "in use"),
    ] {
        let mut app = App::launch_with_env(&hs, &[("MORPHEUS_CALL_CAMERA_FAIL", fail)])
            .await
            .expect("launch");
        common::login(&mut app, "alice", "alice-password").await;
        open_video_settings(&mut app).await;
        app.click(".camera-test").await.expect("Test Video");
        app.wait_for_text(".camera-preview .camera-message", words, DEFAULT_WAIT)
            .await
            .unwrap_or_else(|e| panic!("{fail}: no message with {words:?}: {e}"));
        assert_eq!(
            app.count(".preview-video").await.expect("count"),
            0,
            "{fail}: no picture"
        );
        assert_eq!(
            number(&mut app, ".camera-preview", "data-open").await,
            0,
            "{fail}: nothing open"
        );
        // Not stuck: Test Video is offered again.
        app.wait_for(".camera-test", DEFAULT_WAIT)
            .await
            .expect("offered again");
    }
}
