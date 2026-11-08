//! Threads: a chip on the root opens a side panel with the replies and its
//! own composer; sending from it posts into the thread.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT, api_as, new_account};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

/// A reply in the thread of `root` (replying to `to`), sent as `user`.
fn thread_reply(hs: &str, user: &str, room: &str, root: &str, to: &str, body: &str) -> String {
    let sent = api_as(
        hs,
        user,
        "PUT",
        &format!("rooms/{room}/send/m.room.message/{}", body.len()),
        Some(serde_json::json!({
            "msgtype": "m.text",
            "body": body,
            "m.relates_to": {
                "rel_type": "m.thread",
                "event_id": root,
                "is_falling_back": true,
                "m.in_reply_to": { "event_id": to }
            }
        })),
    )
    .expect("a thread reply");
    sent["event_id"].as_str().expect("reply id").to_owned()
}

#[tokio::test]
async fn thread_panel_shows_replies_and_posts_a_new_one() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    const ROOT: &str = "Let's plan Q4 in this thread";
    // A room of its own: a root with two replies in its thread.
    let user = new_account(&hs, "threads").expect("account");
    let room = api_as(
        &hs,
        &user,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": "plans" })),
    )
    .expect("a room");
    let room = room["room_id"]
        .as_str()
        .expect("room id")
        .replace('!', "%21")
        .replace(':', "%3A");
    let root = api_as(
        &hs,
        &user,
        "PUT",
        &format!("rooms/{room}/send/m.room.message/root"),
        Some(serde_json::json!({ "msgtype": "m.text", "body": ROOT })),
    )
    .expect("root");
    let root = root["event_id"].as_str().expect("root id").to_owned();
    let first = thread_reply(
        &hs,
        &user,
        &room,
        &root,
        &root,
        "I think threads should ship first",
    );
    thread_reply(
        &hs,
        &user,
        &room,
        &root,
        &first,
        "Agreed, then the emoji picker",
    );

    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", &user)
        .await
        .expect("username");
    app.type_into(
        "form.login input[name=password]",
        &format!("{user}-password"),
    )
    .await
    .expect("password");
    app.submit("form.login").await.expect("submit");
    app.open_room("plans").await.expect("open room");

    // The root carries a chip summarising its replies.
    app.wait_for_text(".thread-chip", "2 replies", DEFAULT_WAIT)
        .await
        .expect("thread chip rendered");
    let count_before = 2;

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
        ".thread-panel .composer .composer-input:not([disabled])",
        DEFAULT_WAIT,
    )
    .await
    .expect("thread composer ready");
    let reply = format!("thread reply from e2e {}", std::process::id());
    app.type_into(".thread-panel .composer .composer-input", &reply)
        .await
        .expect("type reply");
    app.press(".thread-panel .composer .composer-input", "Enter")
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
