//! Threads: a chip on the root opens a side panel with the replies and its
//! own composer; sending from it posts into the thread.

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

/// The reply count shown on the thread chip under the message containing
/// `root_text`, or `-1` if that message has no chip (or doesn't exist).
async fn thread_reply_count(app: &mut App, root_text: &str) -> i64 {
    let js = format!(
        "const msg = [...document.querySelectorAll('.message')].find(m => m.querySelector('.body')?.textContent.includes({}));
         if (!msg) return -1;
         const chip = msg.querySelector('.thread-chip');
         if (!chip) return -1;
         const match = chip.textContent.match(/\\d+/);
         return match ? parseInt(match[0], 10) : 0;",
        serde_json::to_string(root_text).expect("json")
    );
    app.eval(&js)
        .await
        .expect("thread count")
        .as_i64()
        .expect("number")
}

#[tokio::test]
async fn thread_panel_shows_replies_and_posts_a_new_one() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;

    const ROOT: &str = "Let's plan Q4 in this thread";

    // The seeded thread root carries a chip summarising its replies.
    app.wait_for_text(".message .body", ROOT, DEFAULT_WAIT)
        .await
        .expect("thread root seeded");
    app.wait_for_text(".thread-chip", "repl", DEFAULT_WAIT)
        .await
        .expect("thread chip rendered");
    let count_before = thread_reply_count(&mut app, ROOT).await;
    assert!(
        count_before >= 2,
        "seeded thread should have at least 2 replies, got {count_before}"
    );

    // Clicking the chip opens the thread panel with the root and its replies.
    app.click_text(".thread-chip", "repl")
        .await
        .expect("open thread panel");
    app.wait_for(".thread-panel", DEFAULT_WAIT)
        .await
        .expect("thread panel opens");
    app.wait_for_text(".thread-panel-root .body", ROOT, DEFAULT_WAIT)
        .await
        .expect("root message shown in panel");
    app.wait_for_text(
        ".thread-panel .message .body",
        "I think threads should ship first",
        DEFAULT_WAIT,
    )
    .await
    .expect("first seeded reply visible");
    app.wait_for_text(
        ".thread-panel .message .body",
        "Agreed, then the emoji picker",
        DEFAULT_WAIT,
    )
    .await
    .expect("second seeded reply visible");

    // The members panel is replaced, not merely covered.
    assert_eq!(
        app.count(".members").await.expect("count"),
        0,
        "members panel is swapped out while a thread is open"
    );

    // Post a reply from the thread's own composer.
    app.wait_for(
        ".thread-panel .composer input:not([disabled])",
        DEFAULT_WAIT,
    )
    .await
    .expect("thread composer ready");
    let reply = format!("thread reply from e2e {}", std::process::id());
    app.type_into(".thread-panel .composer input", &reply)
        .await
        .expect("type reply");
    app.press(".thread-panel .composer input", "Enter")
        .await
        .expect("send reply");
    app.wait_for_text(
        ".thread-panel .message:not(.pending) .body",
        &reply,
        DEFAULT_WAIT,
    )
    .await
    .expect("reply appears in the thread panel");

    if let Some(path) = app.screenshot("thread-panel").await.expect("screenshot") {
        eprintln!("screenshot: {}", path.display());
    }

    // Close the panel; the members column comes back and the chip's count grew by one.
    app.click(".thread-panel-header .ghost")
        .await
        .expect("close thread");
    app.wait_for_js(
        "return document.querySelector('.thread-panel') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("panel closes");
    app.wait_for(".members", DEFAULT_WAIT)
        .await
        .expect("members panel restored");

    app.wait_for_js(
        &format!(
            "const msg = [...document.querySelectorAll('.message')].find(m => m.querySelector('.body')?.textContent.includes({}));
             const chip = msg && msg.querySelector('.thread-chip');
             const match = chip && chip.textContent.match(/\\d+/);
             return !!match && parseInt(match[0], 10) === {};",
            serde_json::to_string(ROOT).expect("json"),
            count_before + 1
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("chip's reply count increased by one");
}
