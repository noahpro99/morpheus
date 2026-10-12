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

/// JS that builds a `File` from bytes and dispatches `event` ("drop" or
/// "paste") carrying it on the window.
fn file_event_js(event: &str, name: &str, mime: &str, bytes: &[u8]) -> String {
    let bytes: Vec<String> = bytes.iter().map(|b| b.to_string()).collect();
    let name = serde_json::to_string(name).expect("json");
    let mime = serde_json::to_string(mime).expect("json");
    let transfer = format!(
        "const dt = new DataTransfer(); dt.items.add(new File([new Uint8Array([{}])], {name}, {{ type: {mime} }}));",
        bytes.join(",")
    );
    match event {
        "drop" => format!(
            "{transfer} const target = document.querySelector('.chat'); \
             target.dispatchEvent(new DragEvent('dragover', {{ bubbles: true, cancelable: true, dataTransfer: dt }})); \
             target.dispatchEvent(new DragEvent('drop', {{ bubbles: true, cancelable: true, dataTransfer: dt }}));"
        ),
        _ => format!(
            "{transfer} document.querySelector('.composer input').dispatchEvent(new ClipboardEvent('paste', {{ bubbles: true, cancelable: true, clipboardData: dt }}));"
        ),
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

#[tokio::test]
async fn dropped_and_pasted_files_upload() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "random").await;
    let pid = std::process::id();

    // Dragging a file over the window shows the hint; dropping it sends an image.
    let image = format!("drop-{pid}.png");
    app.eval(&file_event_js("drop", &image, "image/png", TINY_PNG))
        .await
        .expect("drop");
    app.wait_for_js(
        &format!(
            "return [...document.querySelectorAll('.image')].some(el => el.alt === {0} || el.textContent.includes({0}))",
            serde_json::to_string(&image).expect("json")
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("dropped image in the timeline");
    assert!(
        !app.eval("return document.body.classList.contains('dropping')")
            .await
            .expect("eval")
            .as_bool()
            .unwrap_or(true),
        "hint hidden after the drop"
    );

    // Pasting a file sends it as a file attachment.
    let text = format!("paste-{pid}.txt");
    app.eval(&file_event_js(
        "paste",
        &text,
        "text/plain",
        b"pasted contents",
    ))
    .await
    .expect("paste");
    app.wait_for_text(".attachment-card .attachment-name", &text, DEFAULT_WAIT)
        .await
        .expect("pasted file in the timeline");
}
