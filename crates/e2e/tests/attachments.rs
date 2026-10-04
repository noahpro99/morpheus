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
    app.type_into("form.login input[type=text]", "alice")
        .await
        .expect("username");
    app.type_into("form.login input[type=password]", "alice-password")
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for_text(".room", room, DEFAULT_WAIT)
        .await
        .expect("room listed");
    app.click_text(".room", room).await.expect("open room");
    app.wait_for_text(".chat-header h2", room, DEFAULT_WAIT)
        .await
        .expect("room header");
    app.wait_for(".composer input:not([disabled])", DEFAULT_WAIT)
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
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "dev").await;

    // File card: icon, name and a working download.
    app.wait_for_text(
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
    app.eval(
        "const card = [...document.querySelectorAll('.attachment-card')]
            .find(el => el.textContent.includes('release-notes.txt'));
         card.querySelector('.attachment-download').click();",
    )
    .await
    .expect("click download");
    app.wait_for_js(
        "const card = [...document.querySelectorAll('.attachment-card')]
            .find(el => el.textContent.includes('release-notes.txt'));
         return card.querySelector('.attachment-status') &&
            card.querySelector('.attachment-status').textContent.includes('Saved to');",
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
