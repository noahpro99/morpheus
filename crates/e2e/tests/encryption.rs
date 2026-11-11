//! Whether a room is end-to-end encrypted shows in its top bar, in channels
//! and DMs alike, and the room's privacy settings turn encryption on (Matrix
//! rooms cannot turn it off again, so it asks first and says so).

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT, api_as, new_account};

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
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
}

/// The badge in the top bar: `on`, `off`, or missing.
async fn badge(app: &mut App) -> String {
    let on = ".chat-header .e2e-badge.on[title=\"End-to-end encrypted\"]";
    let off = ".chat-header .e2e-badge.off[title=\"Not encrypted\"]";
    if app.count(on).await.expect("badge") > 0 {
        "on:End-to-end encrypted".into()
    } else if app.count(off).await.expect("badge") > 0 {
        "off:Not encrypted".into()
    } else {
        "missing".into()
    }
}

#[tokio::test]
async fn the_top_bar_shows_whether_a_room_is_encrypted() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch");
    login(&mut app, "alice").await;

    app.open_room("encrypted").await.expect("encrypted");
    app.wait_for(".chat-header .e2e-badge", DEFAULT_WAIT)
        .await
        .expect("a badge");
    let shown = badge(&mut app).await;
    assert!(
        shown.starts_with("on:") && shown.contains("End-to-end encrypted"),
        "#encrypted: {shown}"
    );

    app.open_room("general").await.expect("general");
    app.wait_for_text(".chat-header h2", "general", DEFAULT_WAIT)
        .await
        .expect("in general");
    app.wait_for(".chat-header .e2e-badge.off", DEFAULT_WAIT)
        .await
        .expect("an off badge");
    let shown = badge(&mut app).await;
    assert!(shown.contains("Not encrypted"), "#general: {shown}");

    // DMs too.
    app.click(".rail-item[title=\"Direct messages\"]")
        .await
        .expect("dms");
    app.open_room("bob").await.expect("the dm");
    app.wait_for(".chat-header .e2e-badge", DEFAULT_WAIT)
        .await
        .expect("a badge in a DM");
}

#[tokio::test]
async fn a_room_can_turn_on_encryption_for_good() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let user = new_account(&hs, "lock").expect("account");
    api_as(
        &hs,
        &user,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": "open plans" })),
    )
    .expect("an unencrypted room");
    let mut app = App::launch(&hs).await.expect("launch");
    login(&mut app, &user).await;
    app.open_room("open plans").await.expect("the room");
    app.wait_for(".chat-header .e2e-badge.off", DEFAULT_WAIT)
        .await
        .expect("not encrypted yet");

    // The badge opens the room's privacy settings.
    app.click(".chat-header .e2e-badge").await.expect("badge");
    app.wait_for(".privacy-dialog .encryption-setting", DEFAULT_WAIT)
        .await
        .expect("an encryption setting");
    app.click(".privacy-dialog .turn-on-encryption")
        .await
        .expect("turn on");
    // It asks first: this cannot be undone.
    app.wait_for_text(
        ".privacy-dialog .encryption-setting",
        "cannot be turned off",
        DEFAULT_WAIT,
    )
    .await
    .expect("warned");
    app.click(".privacy-dialog .confirm-encryption")
        .await
        .expect("confirm");
    app.wait_for(".chat-header .e2e-badge.on", DEFAULT_WAIT)
        .await
        .expect("encrypted now");
    app.wait_for_count(".privacy-dialog .turn-on-encryption", 0, DEFAULT_WAIT)
        .await
        .expect("no way to turn it off");
    app.wait_for_text(
        ".privacy-dialog .encryption-setting",
        "End-to-end encrypted",
        DEFAULT_WAIT,
    )
    .await
    .expect("says it is on");
}
