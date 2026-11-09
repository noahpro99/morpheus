//! Timeline scrolling: rooms open at the newest message (also after switching
//! rooms), a jump button appears once you scroll up, and messages that cannot
//! be decrypted yet render as placeholders rather than an error.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

/// Holds once the timeline has more history than fits on screen.
const SCROLLABLE: &str = "const t = document.querySelector('.timeline'); return !!t && t.scrollHeight > t.clientHeight + 600;";

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
    // A tall window can show the whole first page; load enough history to scroll.
    app.reveal("a scrollable timeline", SCROLLABLE, DEFAULT_WAIT)
        .await
        .expect("history");
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

/// A device without the keys (a fresh sign-in, nothing verified) must say
/// so: locked rows instead of an endless loading shimmer, and a notice with
/// the two ways in, each opening the place to do it.
#[tokio::test]
async fn locked_messages_say_how_to_unlock_them() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login(&mut app).await;

    app.open_room("encrypted")
        .await
        .expect("open encrypted room");
    app.wait_for(".message .undecrypted.locked", DEFAULT_WAIT)
        .await
        .expect("a locked row for the message without keys");
    app.wait_for_text(".message .undecrypted.locked", "Encrypted", DEFAULT_WAIT)
        .await
        .expect("the row says it is encrypted");
    let shimmer = app
        .eval("return getComputedStyle(document.querySelector('.undecrypted')).animationName;")
        .await
        .expect("animation");
    assert_ne!(
        shimmer,
        serde_json::json!("shimmer"),
        "locked rows must not look like loading"
    );

    app.wait_for_text(".key-banner", "this device", DEFAULT_WAIT)
        .await
        .expect("a notice explains the locked messages");
    if let Some(path) = app.screenshot("locked-messages").await.expect("screenshot") {
        eprintln!("screenshot: {}", path.display());
    }
    app.wait_for_text(
        ".key-banner button",
        "Verify with another device",
        DEFAULT_WAIT,
    )
    .await
    .expect("verify offered");
    app.click_text(".key-banner button", "Enter recovery key")
        .await
        .expect("recovery offered");
    app.wait_for(".settings .recovery-form input", DEFAULT_WAIT)
        .await
        .expect("settings open at the recovery key");
    app.wait_for_js(
        "return document.activeElement === document.querySelector('.settings .recovery-form input');",
        DEFAULT_WAIT,
    )
    .await
    .expect("the recovery key field is focused");
}

/// Scrolling up through a long room keeps loading older messages until its
/// first one, as the wheel does (no "Load older messages" click): far back,
/// past the most rows the page keeps, history still comes in.
#[tokio::test]
async fn scrolling_up_reaches_the_start_of_a_long_room() {
    use morpheus_e2e::{api_as, new_account, send_many_as};
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let user = new_account(&hs, "longroom").expect("account");
    let room = api_as(
        &hs,
        &user,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": "archive" })),
    )
    .expect("a room");
    let room = room["room_id"].as_str().expect("room id").to_owned();
    let mut bodies = vec!["the very first message".to_owned()];
    bodies.extend((1..=500).map(|n| format!("history message {n}")));
    send_many_as(&hs, &user, &room, &bodies).expect("history");

    let mut app = App::launch(&hs).await.expect("launch");
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
    app.open_room("archive").await.expect("open the room");
    app.wait_for_text(".timeline", "history message 500", DEFAULT_WAIT)
        .await
        .expect("the newest message");
    // What shows above the oldest loaded message on the way up: placeholder
    // rows to scroll onto, never a "loading" line.
    app.eval(
        "window.__loadingText = false; window.__olderSkeletons = false; \
         const look = () => { const t = document.querySelector('.timeline'); if (!t) return; \
           if (/Loading older/.test(t.textContent)) window.__loadingText = true; \
           if (t.querySelector('.older-skeleton')) window.__olderSkeletons = true; }; \
         new MutationObserver(look).observe(document.body, { childList: true, subtree: true, characterData: true }); look(); return true;",
    )
    .await
    .expect("watch");

    // The wheel: the view moves up a little at a time; at the top it stays.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(45);
    loop {
        let found = app
            .eval(
                "const t = document.querySelector('.timeline');
                 if ([...t.querySelectorAll('.message .body')].some(b => b.textContent.includes('the very first message'))) return true;
                 t.scrollTop -= 600; return false;",
            )
            .await
            .expect("scroll");
        if found.as_bool() == Some(true) {
            break;
        }
        let at = app
            .eval(
                "const t = document.querySelector('.timeline'); return [t.dataset.skip, t.dataset.window, [...t.querySelectorAll('.message .body')].map(b => b.textContent).filter(s => s.startsWith('history')).pop()];",
            )
            .await
            .expect("where");
        assert!(
            std::time::Instant::now() < deadline,
            "scrolling up stopped short of the room's first message (skip, window, oldest shown: {at})"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    // Past the first message are the room's creation events: once those are
    // in, the placeholders give way to the room's beginning.
    app.wait_for_js(
        "const t = document.querySelector('.timeline'); t.scrollTop = -t.scrollHeight; \
         return !t.querySelector('.older-skeleton') && !!t.querySelector('.timeline-start');",
        DEFAULT_WAIT,
    )
    .await
    .expect("the beginning of the room, no placeholders");
    let seen = app
        .eval("return [window.__olderSkeletons, window.__loadingText, document.querySelectorAll('.timeline .older-skeleton').length];")
        .await
        .expect("what showed");
    assert_eq!(
        seen,
        serde_json::json!([true, false, 0]),
        "placeholder rows on the way up (and none at the start), never a loading line"
    );
}
