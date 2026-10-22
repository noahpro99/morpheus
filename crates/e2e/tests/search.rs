//! Searching a room's messages from its header: results list the sender and
//! text, and picking one jumps to it in the chat.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

#[tokio::test]
async fn search_finds_a_message_and_jumps_to_it() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
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
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    app.open_room("random").await.expect("open random");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer");

    let word = format!("okapi{}", std::process::id());
    let text = format!("an {word} in the forest");
    app.type_into(".composer .composer-input", &text)
        .await
        .expect("type");
    app.press(".composer .composer-input", "Enter")
        .await
        .expect("send");
    app.wait_for_exact_text(".message.own:not(.pending) .body", &text, DEFAULT_WAIT)
        .await
        .expect("sent");
    // Scroll far away from it.
    app.reveal("older history", "const t = document.querySelector('.timeline'); return t.scrollHeight > t.clientHeight + 1500;", DEFAULT_WAIT)
        .await
        .expect("history to scroll through");
    app.eval("const t = document.querySelector('.timeline'); t.scrollTop = -t.scrollHeight; return true;")
        .await
        .expect("scroll to the top");

    app.click(".chat-header .search-toggle")
        .await
        .expect("search button");
    app.wait_for(".search-panel input[name=search]", DEFAULT_WAIT)
        .await
        .expect("search panel");
    app.type_into(".search-panel input[name=search]", &word)
        .await
        .expect("query");
    app.wait_for_text(".search-panel .search-hit", &word, DEFAULT_WAIT)
        .await
        .expect("a result");
    app.wait_for_text(
        ".search-panel .search-hit .search-sender",
        "alice",
        DEFAULT_WAIT,
    )
    .await
    .expect("the sender shown");

    app.click(".search-panel .search-hit")
        .await
        .expect("pick it");
    app.wait_for_js(
        &format!(
            "const m = [...document.querySelectorAll('.message')].find(m => m.textContent.includes({w})); \
             if (!m || !m.classList.contains('jump-target')) return false; \
             const r = m.getBoundingClientRect(), t = document.querySelector('.timeline').getBoundingClientRect(); \
             return r.top >= t.top - 1 && r.bottom <= t.bottom + 1;",
            w = serde_json::to_string(&word).expect("json")
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("jumped to the message and highlighted it");

    // Escape closes the panel again.
    app.press("body", "Escape").await.expect("escape");
    app.wait_for_js(
        "return !document.querySelector('.search-panel');",
        DEFAULT_WAIT,
    )
    .await
    .expect("panel closed");
}
