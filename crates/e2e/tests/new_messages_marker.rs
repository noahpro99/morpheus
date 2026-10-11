//! The "new messages" divider: bob posts while alice is looking elsewhere,
//! and switching into the room should show the line above his message.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login_and_open(app: &mut App, user: &str, password: &str, room: &str) {
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[type=text]", user)
        .await
        .expect("username");
    app.type_into("form.login input[type=password]", password)
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

/// JS that returns true once `.new-divider` and a timeline row containing
/// `needle` both exist, with the divider sitting above the message. Rows are
/// rendered newest-first (CSS flips it visually), so "above" means the
/// divider must appear *after* the message in document order.
fn marker_above_message_js(needle: &str) -> String {
    format!(
        "const rows = [...document.querySelectorAll('.timeline > *')];
         const dividerIdx = rows.findIndex(el => el.classList.contains('new-divider'));
         const msgIdx = rows.findIndex(el => el.textContent && el.textContent.includes({needle}));
         return dividerIdx !== -1 && msgIdx !== -1 && msgIdx < dividerIdx;",
        needle = serde_json::to_string(needle).expect("json"),
    )
}

#[tokio::test]
async fn new_message_marker_appears_above_unread_and_clears_on_send() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };

    // Alice looks at #dev while bob posts in #general.
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "alice-password", "dev").await;

    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "bob-password", "general").await;

    let body = format!("marker test {}", std::process::id());
    bob.type_into(".composer input", &body).await.expect("type");
    bob.press(".composer input", "Enter").await.expect("send");
    bob.wait_for_text(".message:not(.pending) .body", &body, DEFAULT_WAIT)
        .await
        .expect("bob's message echoed");

    // Alice switches into #general: the NEW divider should sit above bob's message.
    alice
        .click_text(".room", "general")
        .await
        .expect("open general");
    alice
        .wait_for_text(".chat-header h2", "general", DEFAULT_WAIT)
        .await
        .expect("general header");
    alice
        .wait_for_text(".message .body", &body, DEFAULT_WAIT)
        .await
        .expect("bob's message visible");
    alice
        .wait_for_js(&marker_above_message_js(&body), DEFAULT_WAIT)
        .await
        .expect("NEW divider shown above bob's message");

    // Sending a message means alice is caught up: the divider drops immediately.
    let own_body = format!("caught up {}", std::process::id());
    alice
        .type_into(".composer input", &own_body)
        .await
        .expect("type");
    alice.press(".composer input", "Enter").await.expect("send");
    alice
        .wait_for_text(".message:not(.pending) .body", &own_body, DEFAULT_WAIT)
        .await
        .expect("alice's message echoed");
    alice
        .wait_for_js(
            "return document.querySelector('.new-divider') === null;",
            DEFAULT_WAIT,
        )
        .await
        .expect("divider cleared after sending");
}
