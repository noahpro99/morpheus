//! Desktop notifications: a message from someone else is announced (the app
//! mirrors what it announced into a hidden log), mentions are flagged, and
//! the inbox lists recent mentions.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login(app: &mut App, user: &str) {
    app.wait_for("form.login", DEFAULT_WAIT).await.expect("login form");
    app.type_into("form.login input[name=username]", user).await.expect("username");
    app.type_into("form.login input[name=password]", &format!("{user}-password")).await.expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("room listed");
}

async fn send(app: &mut App, text: &str) {
    app.type_into(".composer input", text).await.expect("type");
    app.press(".composer input", "Enter").await.expect("send");
    app.wait_for_exact_text(".message.own:not(.pending) .body", text, DEFAULT_WAIT).await.expect("message sent");
}

#[tokio::test]
async fn messages_from_others_are_announced_and_mentions_flagged() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login(&mut bob, "bob").await;
    bob.open_room("random").await.expect("bob reads #random");

    let mut alice = App::launch(&hs).await.expect("launch alice");
    login(&mut alice, "alice").await;
    alice.open_room("general").await.expect("alice in #general");

    let pid = std::process::id();
    let plain = format!("notify ping {pid}");
    send(&mut alice, &plain).await;
    bob.wait_for_text(".notification-log li", &plain, DEFAULT_WAIT).await.expect("announced");
    let flagged = bob
        .eval(&format!(
            "return [...document.querySelectorAll('.notification-log li')].find(li => li.textContent.includes({})).dataset.mention",
            serde_json::to_string(&plain).expect("json")
        ))
        .await
        .expect("flag");
    assert_eq!(flagged.as_str(), Some("false"), "a plain message is not a mention");
    bob.wait_for_text(".notification-log li", "alice: ", DEFAULT_WAIT).await.expect("sender named");

    let mention = format!("@bob look here {pid}");
    send(&mut alice, &mention).await;
    bob.wait_for_js(
        &format!(
            "return [...document.querySelectorAll('.notification-log li')].some(li => li.textContent.includes({}) && li.dataset.mention === 'true')",
            serde_json::to_string(&mention).expect("json")
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("mention flagged");

    // The inbox lists the mention and opens the room.
    bob.click(".titlebar .inbox-button").await.expect("open inbox");
    bob.wait_for_text(".inbox-mention", &mention, DEFAULT_WAIT).await.expect("mention in inbox");
    bob.click_text(".inbox-mention", &mention).await.expect("open mention");
    bob.wait_for_text(".chat-header h2", "general", DEFAULT_WAIT).await.expect("jumped to #general");
    assert_eq!(bob.count(".inbox-panel").await.expect("count"), 0, "inbox closed");
}
