//! Replies, edits, deletes, typing indicators, and the quick switcher.

#![allow(clippy::expect_used)]

use std::time::Duration;

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login_and_open(app: &mut App, user: &str, password: &str, room: &str) {
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

/// Click a hover action (`.action.<kind>`) on the message containing `text`,
/// retrying until `expect_sel` contains `expect_text` (clicks during a
/// timeline re-render can be dropped, as a real user would simply click again).
async fn click_action_until(
    app: &mut App,
    text: &str,
    kind: &str,
    expect_sel: &str,
    expect_text: &str,
) {
    for _ in 0..5 {
        click_action(app, text, kind).await;
        if app
            .wait_for_text(expect_sel, expect_text, Duration::from_secs(3))
            .await
            .is_ok()
        {
            return;
        }
    }
    panic!("{kind} action on {text:?} never produced {expect_sel} containing {expect_text:?}");
}

/// Click a hover action (`.action.<kind>`) on the message containing `text`.
async fn click_action(app: &mut App, text: &str, kind: &str) {
    let js = format!(
        "const msg = [...document.querySelectorAll('.message')].find(m => m.querySelector('.body')?.textContent.includes({}));
         if (!msg) throw new Error('no message with that text');
         const btn = msg.querySelector('.action.{kind}');
         if (!btn) throw new Error('no {kind} action on that message');
         btn.click();",
        serde_json::to_string(text).expect("json"),
    );
    app.eval(&js)
        .await
        .unwrap_or_else(|e| panic!("{kind} action on {text:?}: {e}"));
}

#[tokio::test]
async fn reply_edit_and_delete_own_messages() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "alice", "alice-password", "dev").await;
    let pid = std::process::id();

    // Reply to a seeded message: banner shows the target, the sent message carries a quote.
    app.wait_for_text(".message .body", "sliding sync is working", DEFAULT_WAIT)
        .await
        .expect("seeded");
    click_action_until(
        &mut app,
        "sliding sync is working",
        "reply",
        ".composer-banner",
        "carol",
    )
    .await;
    let reply = format!("reply from e2e {pid}");
    app.type_into(".composer input", &reply)
        .await
        .expect("type reply");
    app.press(".composer input", "Enter")
        .await
        .expect("send reply");
    app.wait_for_js(
        &format!(
            "const m = [...document.querySelectorAll('.message:not(.pending)')].find(m => m.textContent.includes({}));
             return !!m && !!m.querySelector('.reply-quote') && m.querySelector('.reply-quote').textContent.includes('sliding sync');",
            serde_json::to_string(&reply).expect("json")
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("reply rendered with quote");
    assert_eq!(
        app.count(".composer-banner").await.expect("count"),
        0,
        "banner clears after sending"
    );

    // Edit: the composer pre-fills with the original, Enter saves, the row shows (edited).
    let original = format!("draft from e2e {pid}");
    app.type_into(".composer input", &original)
        .await
        .expect("type");
    app.press(".composer input", "Enter").await.expect("send");
    app.wait_for_text(".message:not(.pending) .body", &original, DEFAULT_WAIT)
        .await
        .expect("sent");
    click_action(&mut app, &original, "edit").await;
    app.wait_for_text(".composer-banner", "Editing", DEFAULT_WAIT)
        .await
        .expect("edit banner");
    assert_eq!(
        app.value(".composer input").await.expect("value"),
        Some(original.clone())
    );
    let edited = format!("edited from e2e {pid}");
    app.type_into(".composer input", &edited)
        .await
        .expect("type edit");
    app.press(".composer input", "Enter")
        .await
        .expect("save edit");
    app.wait_for_js(
        &format!(
            "const m = [...document.querySelectorAll('.message')].find(m => m.textContent.includes({}));
             return !!m && m.textContent.includes('(edited)');",
            serde_json::to_string(&edited).expect("json")
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("edited marker");

    // Delete: the row becomes a tombstone.
    click_action_until(
        &mut app,
        &edited,
        "delete",
        ".message .body",
        "Message deleted",
    )
    .await;
    app.wait_for_js(
        &format!(
            "return ![...document.querySelectorAll('.message .body')].some(el => el.textContent.includes({}));",
            serde_json::to_string(&edited).expect("json")
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("edited text gone");
    app.wait_for_text(".message .body", "Message deleted", DEFAULT_WAIT)
        .await
        .expect("tombstone");

    if let Some(path) = app.screenshot("interactions").await.expect("screenshot") {
        eprintln!("screenshot: {}", path.display());
    }
}

#[tokio::test]
async fn typing_indicator_shows_the_other_user() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "alice-password", "general").await;
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "bob-password", "general").await;

    bob.type_into(".composer input", "typing something")
        .await
        .expect("bob types");
    alice
        .wait_for_text(".typing", "bob is typing", DEFAULT_WAIT)
        .await
        .expect("alice sees bob typing");
    assert_eq!(
        bob.count(".typing:not(.empty)").await.expect("count"),
        0,
        "own typing is not shown"
    );

    // Sending clears the notice.
    bob.press(".composer input", "Enter")
        .await
        .expect("bob sends");
    alice
        .wait_for_js(
            "return document.querySelector('.typing.empty') !== null;",
            Duration::from_secs(15),
        )
        .await
        .expect("typing cleared after send");
}

#[tokio::test]
async fn ctrl_k_switches_rooms() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "alice", "alice-password", "general").await;

    app.shortcut(".composer input", "k", true)
        .await
        .expect("ctrl+k");
    app.wait_for(".switcher input", DEFAULT_WAIT)
        .await
        .expect("switcher opens");
    app.type_into(".switcher input", "dev")
        .await
        .expect("filter");
    app.wait_for_exact_text(".switcher-item.active", "#dev", Duration::from_secs(5))
        .await
        .expect("dev highlighted");
    app.press(".switcher input", "Enter").await.expect("choose");
    app.wait_for_text(".chat-header h2", "dev", DEFAULT_WAIT)
        .await
        .expect("switched to dev");
    assert_eq!(
        app.count(".switcher").await.expect("count"),
        0,
        "switcher closes"
    );

    app.shortcut(".composer input", "k", true)
        .await
        .expect("ctrl+k again");
    app.wait_for(".switcher input", DEFAULT_WAIT)
        .await
        .expect("switcher reopens");
    app.press(".switcher input", "Escape")
        .await
        .expect("escape");
    app.wait_for_js(
        "return document.querySelector('.switcher') === null;",
        Duration::from_secs(5),
    )
    .await
    .expect("escape closes");
}
