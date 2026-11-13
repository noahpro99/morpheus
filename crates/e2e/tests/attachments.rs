//! File, audio and video attachments: rendering and downloading, through the
//! real desktop window.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login_and_open(app: &mut App, room: &str) {
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
    app.open_room(room).await.expect("open room");
    app.wait_for_text(".chat-header h2", room, DEFAULT_WAIT)
        .await
        .expect("room header");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
}

/// Seeded by `dev/homeserver/seed.sh`'s `send_file`/`send_video`/`send_audio`
/// into #dev: a text file, a tiny mp4 clip (with thumbnail), and a tiny m4a
/// voice note.
#[tokio::test]
async fn renders_and_downloads_attachments() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login_and_open(&mut app, "dev").await;

    // File card: icon, name and a working download.
    // Seeded long ago: scroll up until the card is revealed.
    app.reveal_text(
        ".attachment-card .attachment-name",
        "release-notes.txt",
        DEFAULT_WAIT,
    )
    .await
    .expect("file card");
    app.wait_for_text(".attachment-card .attachment-size", "B", DEFAULT_WAIT)
        .await
        .expect("human-readable file size");

    // Video card: thumbnail (or placeholder) plus a play button.
    app.wait_for(".video-card .play-button", DEFAULT_WAIT)
        .await
        .expect("video play button");
    app.wait_for_text(".video-card .video-duration", "0:01", DEFAULT_WAIT)
        .await
        .expect("video duration");

    // Audio row: name, duration and a download button.
    app.wait_for_text(
        ".attachment-card.audio .attachment-name",
        "voice-note.m4a",
        DEFAULT_WAIT,
    )
    .await
    .expect("audio row");
    app.wait_for_text(
        ".attachment-card.audio .attachment-size",
        "0:01",
        DEFAULT_WAIT,
    )
    .await
    .expect("audio duration");

    // Download the file attachment and verify it actually landed on disk,
    // in the test's disposable Downloads stand-in.
    app.click_within(
        ".attachment-card",
        "release-notes.txt",
        ".attachment-download",
    )
    .await
    .expect("click download");
    app.wait_for_within(
        ".attachment-card",
        "release-notes.txt",
        ".attachment-status",
        Some("Saved to"),
        DEFAULT_WAIT,
    )
    .await
    .expect("download finished");

    let saved = app.download_dir().join("release-notes.txt");
    assert!(
        saved.exists(),
        "expected {} to exist after downloading",
        saved.display()
    );
    let contents = std::fs::read_to_string(&saved).expect("read downloaded file");
    assert!(contents.contains("Morpheus release notes"));

    if let Some(path) = app.screenshot("attachments").await.expect("screenshot") {
        eprintln!("screenshot: {}", path.display());
    }
}

/// A 1x1 PNG.
const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
    0x44, 0xAE, 0x42, 0x60, 0x82,
];

/// Whether the timeline shows a picture or a file card for `name`.
async fn in_timeline(app: &mut App, name: &str) -> bool {
    let picture = format!(".timeline .image[alt={}]", serde_json::to_string(name).expect("json"));
    app.count(&picture).await.expect("count") > 0
        || app
            .texts(".timeline .attachment-card")
            .await
            .expect("texts")
            .iter()
            .any(|card| card.contains(name))
}

/// Wait until [`in_timeline`] holds.
async fn wait_in_timeline(app: &mut App, name: &str) -> Result<(), String> {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while !in_timeline(app, name).await {
        if std::time::Instant::now() >= deadline {
            return Err(format!("{name} never showed in the timeline"));
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Ok(())
}

/// Pasting or dropping a file attaches it to the message being written, as
/// in Discord: a tile above the box (the picture itself for an image) with a
/// ✕ top right that takes it off; nothing goes out until the message is sent,
/// and then the files go with it.
#[tokio::test]
async fn pasted_and_dropped_files_attach_to_the_message() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "random").await;
    let pid = std::process::id();

    // A pasted picture waits in the composer, shown as itself.
    let removed = format!("removed-{pid}.png");
    app.attach_file(false, &removed, "image/png", TINY_PNG)
        .await
        .expect("paste");
    app.wait_for(
        ".composer .composer-attachment img[src^='data:image/png']",
        DEFAULT_WAIT,
    )
    .await
    .expect("the picture attached to the message");
    // Its ✕ takes it off again; it is never sent.
    app.click(".composer .composer-attachment .remove-attachment")
        .await
        .expect("remove");
    app.wait_for_count(".composer .composer-attachment", 0, DEFAULT_WAIT)
        .await
        .expect("taken off");

    // A pasted picture and a dropped file wait together, until Enter.
    let image = format!("paste-{pid}.png");
    let text = format!("drop-{pid}.txt");
    app.attach_file(false, &image, "image/png", TINY_PNG)
        .await
        .expect("paste");
    app.attach_file(true, &text, "text/plain", b"dropped contents")
        .await
        .expect("drop");
    app.wait_for_count(".composer .composer-attachment", 2, DEFAULT_WAIT)
        .await
        .expect("both attached");
    if std::env::var_os("MORPHEUS_E2E_SHOTS").is_some() {
        let _ = app.screenshot("composer-attachments").await;
    }
    assert_eq!(
        app.count("body.dropping").await.expect("count"),
        0,
        "hint hidden after the drop"
    );
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    assert!(
        !in_timeline(&mut app, &image).await && !in_timeline(&mut app, &text).await,
        "nothing sent before the message"
    );

    let message = format!("with files {pid}");
    app.type_into(".composer .composer-input", &message)
        .await
        .expect("type");
    app.press(".composer .composer-input", "Enter")
        .await
        .expect("send");
    app.wait_for_text(".timeline", &message, DEFAULT_WAIT)
        .await
        .expect("the message");
    wait_in_timeline(&mut app, &image)
        .await
        .expect("the picture sent with it");
    wait_in_timeline(&mut app, &text)
        .await
        .expect("the file sent with it");
    assert_eq!(
        app.count(".composer .composer-attachment")
            .await
            .expect("count"),
        0,
        "the composer is empty again"
    );
    assert!(
        !in_timeline(&mut app, &removed).await,
        "the removed picture was never sent"
    );
}

/// A video plays in the message, as in Discord: the play button swaps the
/// thumbnail for a player (play/pause, seeking, volume and fullscreen are
/// its controls), playing at once; nothing is downloaded.
#[tokio::test]
async fn videos_play_inline() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    if morpheus_e2e::gpui() {
        // Playing an mp4 in a message needs a video decoder in the GPUI app
        // (the Dioxus page has the webview's `<video>`): not built.
        eprintln!("skipped on GPUI: no inline video playback");
        return;
    }
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "dev").await;
    app.reveal_element(".video-card .play-button", DEFAULT_WAIT)
        .await
        .expect("the video");
    app.click(".video-card .play-button").await.expect("play");
    app.wait_for(".video-card video.video-player[controls]", DEFAULT_WAIT)
        .await
        .expect("a player in the message");
    app.wait_for_js(
        "const v = document.querySelector('.video-card video.video-player'); \
         return !!v && v.src.startsWith('blob:') && (v.currentTime > 0 || v.ended);",
        DEFAULT_WAIT,
    )
    .await
    .expect("it plays");
    assert_eq!(
        app.count(".video-card .play-button, .video-card .video-duration")
            .await
            .expect("count"),
        0,
        "the overlay gives way to the player"
    );
    let downloads = std::fs::read_dir(app.download_dir())
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(downloads, 0, "nothing downloaded");
}
