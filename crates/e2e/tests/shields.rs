//! In encrypted rooms, a message from a session its sender never verified
//! carries a shield whose tooltip says why.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login(app: &mut App, user: &str) {
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", user)
        .await
        .expect("username");
    app.type_into(
        "form.login input[name=password]",
        &format!("{user}-password"),
    )
    .await
    .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for_text(".room", "encrypted", DEFAULT_WAIT)
        .await
        .expect("rooms");
    app.open_room("encrypted").await.expect("open #encrypted");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer");
}

#[tokio::test]
async fn unverified_sessions_get_a_shield() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    // Bob first, so alice's message is encrypted for this session of his.
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login(&mut bob, "bob").await;
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login(&mut alice, "alice").await;

    let text = format!("from a new session {}", std::process::id());
    alice
        .type_into(".composer .composer-input", &text)
        .await
        .expect("type");
    alice
        .press(".composer .composer-input", "Enter")
        .await
        .expect("send");

    // The shield on the message that has her text, once it arrives.
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    let titles = loop {
        let found = bob
            .attrs_within(".message", &text, ".msg-shield", "title")
            .await
            .expect("shields");
        if !found.is_empty() {
            break found;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "a shield on alice's message"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    };
    let title = titles[0].clone().unwrap_or_default();
    assert!(
        title.contains("device") || title.contains("verified"),
        "the shield explains itself: {title:?}"
    );
}
