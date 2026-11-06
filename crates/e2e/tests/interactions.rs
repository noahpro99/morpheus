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
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
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
    app.reveal_message("sliding sync is working", DEFAULT_WAIT)
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
    app.type_into(".composer .composer-input", &reply)
        .await
        .expect("type reply");
    app.press(".composer .composer-input", "Enter")
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
    app.type_into(".composer .composer-input", &original)
        .await
        .expect("type");
    app.press(".composer .composer-input", "Enter")
        .await
        .expect("send");
    app.wait_for_text(".message:not(.pending) .body", &original, DEFAULT_WAIT)
        .await
        .expect("sent");
    click_action(&mut app, &original, "edit").await;
    app.wait_for_text(".composer-banner", "Editing", DEFAULT_WAIT)
        .await
        .expect("edit banner");
    assert_eq!(
        app.value(".composer .composer-input").await.expect("value"),
        Some(original.clone())
    );
    let edited = format!("edited from e2e {pid}");
    app.type_into(".composer .composer-input", &edited)
        .await
        .expect("type edit");
    app.press(".composer .composer-input", "Enter")
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

    // Delete: the row goes at once (undoable from a toast), then becomes a
    // tombstone.
    click_action(&mut app, &edited, "delete").await;
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

    bob.type_into(".composer .composer-input", "typing something")
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
    bob.press(".composer .composer-input", "Enter")
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

    app.shortcut(".composer .composer-input", "k", true)
        .await
        .expect("ctrl+k");
    app.wait_for(".switcher input", DEFAULT_WAIT)
        .await
        .expect("switcher opens");
    app.type_into(".switcher input", "dev")
        .await
        .expect("filter");
    app.wait_for_exact_text(".switcher-item.active .name", "dev", Duration::from_secs(5))
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
    // #dev lives in the Engineering space: the rail follows, and the room
    // stays open rather than bouncing back to the Rooms scope.
    app.wait_for(".rail-item.selected[title=\"Engineering\"]", DEFAULT_WAIT)
        .await
        .expect("rail follows the switcher");
    tokio::time::sleep(Duration::from_millis(500)).await;
    app.wait_for_text(".chat-header h2", "dev", Duration::from_secs(2))
        .await
        .expect("dev still open");

    app.shortcut(".composer .composer-input", "k", true)
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

/// Each Ctrl+K row shows its own room's picture after typing: filtering
/// moves rooms up the list, and the picture must move with the name (it
/// stayed with the row's place, so a name showed another room's picture).
#[tokio::test]
async fn ctrl_k_pictures_follow_their_rooms() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "alice", "alice-password", "general").await;

    app.shortcut(".composer .composer-input", "k", true)
        .await
        .expect("ctrl+k");
    app.wait_for(".switcher input", DEFAULT_WAIT)
        .await
        .expect("switcher opens");
    // Each row (its room and name) and picture (an image's source, or its letter).
    let pictures = "return [...document.querySelectorAll('.switcher-item')].filter(r => r.querySelector('.name')).map(r => { \
         const a = r.querySelector('.avatar'); \
         return [r.dataset.key + ' ' + r.querySelector('.name').textContent.trim(), a.tagName === 'IMG' ? a.getAttribute('src') : 'letter ' + a.textContent.trim()]; });";
    // Wait for the pictures to load, then note whose is whose.
    app.wait_for_js(
        "return document.querySelectorAll('.switcher-item img.avatar').length > 0;",
        DEFAULT_WAIT,
    )
    .await
    .expect("pictures load");
    tokio::time::sleep(Duration::from_secs(2)).await;
    let before = app.eval(pictures).await.expect("pictures");
    let before: std::collections::HashMap<String, String> =
        serde_json::from_value::<Vec<(String, String)>>(before)
            .expect("rows")
            .into_iter()
            .collect();
    eprintln!("[switcher] before: {before:?}");

    // Filter so rooms further down move up to the top rows.
    for query in ["b", "c", "d"] {
        app.eval("const i = document.querySelector('.switcher input'); i.value = ''; i.dispatchEvent(new Event('input', { bubbles: true }));")
            .await
            .expect("clear");
        app.type_into(".switcher input", query).await.expect("filter");
        tokio::time::sleep(Duration::from_secs(1)).await;
        let after: Vec<(String, String)> =
            serde_json::from_value(app.eval(pictures).await.expect("pictures")).expect("rows");
        eprintln!("[switcher] '{query}': {after:?}");
        for (name, picture) in &after {
            if let Some(own) = before.get(name) {
                assert_eq!(picture, own, "'{name}' shows another room's picture after typing '{query}'");
            }
        }
    }
}
