//! Timeline scrolling: rooms open at the newest message (also after switching
//! rooms), a jump button appears once you scroll up, and messages that cannot
//! be decrypted yet render as placeholders rather than an error.

#![allow(clippy::expect_used)]

use std::time::Duration;

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login(app: &mut App) {
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
}

/// The newest row is inside the viewport and the container sits at its end.
const AT_BOTTOM: &str = "const t = document.querySelector('.timeline'); if (!t) return false; \
    const last = t.querySelector('.message'); if (!last) return false; \
    const box = t.getBoundingClientRect(); const row = last.getBoundingClientRect(); \
    return Math.abs(t.scrollTop) < 2 && row.bottom <= box.bottom + 1 && row.top >= box.top - 1;";

#[tokio::test]
async fn rooms_open_at_the_newest_message_and_jump_back_after_scrolling() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login(&mut app).await;

    app.open_room("firehose").await.expect("open firehose");
    app.wait_for_exact_text(".message .body", "firehose message 600", DEFAULT_WAIT)
        .await
        .expect("newest message loaded");
    app.wait_for_js(AT_BOTTOM, DEFAULT_WAIT)
        .await
        .expect("opened at the newest message");
    assert_eq!(
        app.count(".jump-to-bottom").await.expect("count"),
        0,
        "no jump button while at the bottom"
    );

    // Scroll up a few screens: the button appears.
    app.eval("document.querySelector('.timeline').scrollTop = -2000;")
        .await
        .expect("scroll up");
    app.wait_for(".jump-to-bottom", DEFAULT_WAIT)
        .await
        .expect("jump button after scrolling up");
    app.click(".jump-to-bottom").await.expect("jump");
    app.wait_for_js(AT_BOTTOM, DEFAULT_WAIT)
        .await
        .expect("back at the newest message");
    app.wait_for_js(
        "return document.querySelector('.jump-to-bottom') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("button gone at the bottom");

    // Switching rooms (and back) lands at the newest message every time.
    for room in ["random", "firehose", "bob", "random"] {
        app.open_room(room).await.expect("switch room");
        app.wait_for_js(AT_BOTTOM, DEFAULT_WAIT)
            .await
            .unwrap_or_else(|e| panic!("{room} opened at the newest message: {e}"));
    }
}

#[tokio::test]
async fn undecryptable_messages_render_as_placeholders() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login(&mut app).await;

    app.open_room("encrypted").await.expect("open encrypted room");
    app.wait_for(".message .undecrypted.skeleton", DEFAULT_WAIT)
        .await
        .expect("a placeholder for the message without keys");
    let text = app
        .eval("return document.querySelector('.timeline').textContent;")
        .await
        .expect("timeline text");
    assert!(
        !text.as_str().unwrap_or_default().contains("Unable to decrypt"),
        "no error text for messages still waiting on keys"
    );
    // Placeholders are inert: nothing to click or hover into actions.
    app.wait_for_js(
        "return getComputedStyle(document.querySelector('.undecrypted')).animationName === 'shimmer';",
        Duration::from_secs(5),
    )
    .await
    .expect("placeholder shimmers");
}
