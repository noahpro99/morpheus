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

async fn send(app: &mut App, text: String) {
    let sel = ".composer .composer-input";
    app.type_into(sel, &text).await.expect("type");
    app.press(sel, "Enter").await.expect("send");
}

/// The shield sits at the right of its message and takes no height: a message
/// with one is as tall as the same message without.
#[tokio::test]
async fn the_shield_sits_at_the_right_and_takes_no_height() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut bob = App::launch(&hs).await.expect("launch bob");
    if !bob.is_gpui() {
        eprintln!("measured in the GPUI app only; skipping");
        return;
    }
    login(&mut bob, "bob").await;
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login(&mut alice, "alice").await;

    // Same length, so the same one line each; alice's carry a shield for bob,
    // his own do not. Alternating senders gives each a header.
    let pid = std::process::id();
    let texts = [
        format!("shield row {pid} a"),
        format!("shield row {pid} b"),
        format!("shield row {pid} c"),
    ];
    send(&mut alice, texts[0].clone()).await;
    bob.wait_for_text(".message", &texts[0], DEFAULT_WAIT).await.expect("a arrives");
    send(&mut bob, texts[1].clone()).await;
    bob.wait_for_text(".message", &texts[1], DEFAULT_WAIT).await.expect("b sent");
    send(&mut alice, texts[2].clone()).await;
    bob.wait_for_text(".message", &texts[2], DEFAULT_WAIT).await.expect("c arrives");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let shielded = bob
        .bounds_containing(".message", &texts[0])
        .await
        .expect("bounds")
        .expect("a's row");
    let plain = bob
        .bounds_containing(".message", &texts[1])
        .await
        .expect("bounds")
        .expect("b's row");
    let again = bob
        .bounds_containing(".message", &texts[2])
        .await
        .expect("bounds")
        .expect("c's row");
    let shield = bob
        .bounds_within(".message", &texts[0], ".msg-shield")
        .await
        .expect("shield")
        .expect("a shield on alice's message");
    let body = bob
        .bounds_within(".message", &texts[0], ".body")
        .await
        .expect("body")
        .expect("the text");
    // Whatever else a row holds, a shield adds nothing: the row is its header plus its text, as a row of bob's is.
    let mut extras = Vec::new();
    for (text, row) in [(&texts[1], plain), (&texts[2], again)] {
        let meta = bob.bounds_within(".message", text, ".message-meta").await.expect("meta").expect("header");
        let body = bob.bounds_within(".message", text, ".body").await.expect("body").expect("text");
        extras.push(row.h - meta.h - body.h);
    }
    assert!(extras[1] < 8.0, "the shield has no line of its own: the row is {} taller than its header and text", extras[1]);
    assert!(
        (extras[0] - extras[1]).abs() < 1.0,
        "a row with a shield is no taller than one without (row minus header minus text: bob's {}, alice's {}): {plain:?} {again:?}",
        extras[0],
        extras[1]
    );
    assert!(shield.x > body.x + 100.0, "the shield is right of the text: {shield:?} {body:?}");
    assert!(
        shield.centre_x() > shielded.x + shielded.w / 2.0,
        "in the right half of the row: {shield:?} {shielded:?}"
    );
    assert!(
        shield.y >= shielded.y && shield.bottom() <= shielded.bottom(),
        "inside the row: {shield:?} {shielded:?}"
    );
}
