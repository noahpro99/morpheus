//! Encrypted history on a new device: once it is unlocked with the recovery
//! key, messages sent before it existed are fetched from the key backup and
//! read, rather than staying as loading placeholders.

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
    app.wait_for(".shell", DEFAULT_WAIT).await.expect("shell");
}

#[tokio::test]
async fn the_recovery_key_unlocks_old_messages_on_a_new_device() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let user = new_account(&hs, "history").expect("account");
    api_as(
        &hs,
        &user,
        "POST",
        "createRoom",
        Some(serde_json::json!({
            "name": "secrets",
            "initial_state": [{
                "type": "m.room.encryption",
                "state_key": "",
                "content": { "algorithm": "m.megolm.v1.aes-sha2" }
            }]
        })),
    )
    .expect("an encrypted room");

    // The first device: backed up, and writes in the room.
    let mut first = App::launch(&hs).await.expect("first device");
    login(&mut first, &user).await;
    first
        .wait_for_text(".backup-prompt", "Set up Secure Backup", DEFAULT_WAIT)
        .await
        .expect("offered Secure Backup");
    first
        .click_text(".backup-prompt button", "Set up")
        .await
        .expect("set up Secure Backup");
    first
        .wait_for(".recovery-key-new code", DEFAULT_WAIT)
        .await
        .expect("the recovery key");
    let key = first
        .text(".recovery-key-new code")
        .await
        .expect("key")
        .unwrap_or_default()
        .trim()
        .to_owned();
    first.press("body", "Escape").await.expect("close");
    first.open_room("secrets").await.expect("open the room");
    first
        .wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer");
    first
        .type_into(".composer .composer-input", "from before the new device")
        .await
        .expect("type");
    first
        .press(".composer .composer-input", "Enter")
        .await
        .expect("send");
    first
        .wait_for_text(".timeline", "from before the new device", DEFAULT_WAIT)
        .await
        .expect("sent");
    // Its key reaches the backup on the server.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let version = api_as(&hs, &user, "GET", "room_keys/version", None).expect("backup version");
        let version = version["version"].as_str().unwrap_or_default().to_owned();
        let keys = api_as(
            &hs,
            &user,
            "GET",
            &format!("room_keys/keys?version={version}"),
            None,
        )
        .expect("backed-up keys");
        if keys["rooms"]
            .as_object()
            .is_some_and(|rooms| !rooms.is_empty())
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the first device never backed up the room's key (version {version}: {keys})"
        );
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    drop(first);

    // A new device, unlocked with the recovery key.
    let mut second = App::launch(&hs).await.expect("second device");
    login(&mut second, &user).await;
    // Opened before it is unlocked: the message is locked, then read.
    second.open_room("secrets").await.expect("open the room");
    second
        .wait_for(".timeline .undecrypted", DEFAULT_WAIT)
        .await
        .expect("the old message is locked at first");
    second.click(".settings-button").await.expect("settings");
    second
        .wait_for(".settings-nav", DEFAULT_WAIT)
        .await
        .expect("settings open");
    second
        .click_text(".settings-nav button", "Security & Privacy")
        .await
        .expect("Security & Privacy");
    second
        .wait_for(".recovery-form input", DEFAULT_WAIT)
        .await
        .expect("the recovery key field");
    second
        .type_into(".recovery-form input", &key)
        .await
        .expect("type the key");
    second.submit(".recovery-form").await.expect("submit");
    second
        .wait_for_text(".status-list", "This device is verified", DEFAULT_WAIT)
        .await
        .expect("unlocked");
    second
        .press("body", "Escape")
        .await
        .expect("close settings");

    second.open_room("secrets").await.expect("open the room");
    second
        .wait_for_text(".timeline", "from before the new device", DEFAULT_WAIT)
        .await
        .unwrap_or_else(|err| panic!("the old message is read, not left as a placeholder: {err}"));
}

/// The recovery key Settings shows just after making one.
async fn new_key_shown(app: &mut App) -> String {
    app.wait_for(".recovery-key-new code", DEFAULT_WAIT)
        .await
        .expect("the recovery key");
    app.text(".recovery-key-new code")
        .await
        .expect("key")
        .unwrap_or_default()
        .trim()
        .to_owned()
}

async fn open_security(app: &mut App) {
    app.click(".settings-button").await.expect("settings");
    app.wait_for(".settings-nav", DEFAULT_WAIT)
        .await
        .expect("settings open");
    app.click_text(".settings-nav button", "Security & Privacy")
        .await
        .expect("Security & Privacy");
}

/// As in Element: a lost or unsaved recovery key is replaced from a signed-in
/// device; the new key unlocks history on a new device, and the old one no
/// longer does.
#[tokio::test]
async fn a_new_recovery_key_replaces_the_old_one() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let user = new_account(&hs, "rekey").expect("account");
    api_as(
        &hs,
        &user,
        "POST",
        "createRoom",
        Some(serde_json::json!({
            "name": "secrets",
            "initial_state": [{
                "type": "m.room.encryption",
                "state_key": "",
                "content": { "algorithm": "m.megolm.v1.aes-sha2" }
            }]
        })),
    )
    .expect("an encrypted room");

    let mut first = App::launch(&hs).await.expect("first device");
    login(&mut first, &user).await;
    first
        .wait_for_text(".backup-prompt", "Set up Secure Backup", DEFAULT_WAIT)
        .await
        .expect("offered Secure Backup");
    first
        .click_text(".backup-prompt button", "Set up")
        .await
        .expect("set up Secure Backup");
    let old_key = new_key_shown(&mut first).await;
    first.press("body", "Escape").await.expect("close");

    // Later, from Settings: a new key.
    open_security(&mut first).await;
    first
        .wait_for(".change-recovery-key", DEFAULT_WAIT)
        .await
        .expect("a Change recovery key button");
    first
        .click(".change-recovery-key")
        .await
        .expect("change the key");
    let new_key = new_key_shown(&mut first).await;
    assert!(
        !new_key.is_empty() && new_key != old_key,
        "a different key: {new_key}"
    );
    first.press("body", "Escape").await.expect("close");

    first.open_room("secrets").await.expect("open the room");
    first
        .wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer");
    first
        .type_into(".composer .composer-input", "after the new key")
        .await
        .expect("type");
    first
        .press(".composer .composer-input", "Enter")
        .await
        .expect("send");
    first
        .wait_for_text(".timeline", "after the new key", DEFAULT_WAIT)
        .await
        .expect("sent");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let version = api_as(&hs, &user, "GET", "room_keys/version", None).expect("backup version");
        let version = version["version"].as_str().unwrap_or_default().to_owned();
        let keys = api_as(
            &hs,
            &user,
            "GET",
            &format!("room_keys/keys?version={version}"),
            None,
        )
        .expect("backed-up keys");
        if keys["rooms"]
            .as_object()
            .is_some_and(|rooms| !rooms.is_empty())
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "never backed up ({keys})"
        );
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    drop(first);

    let mut second = App::launch(&hs).await.expect("second device");
    login(&mut second, &user).await;
    open_security(&mut second).await;
    second
        .wait_for(".recovery-form input", DEFAULT_WAIT)
        .await
        .expect("the recovery key field");
    // The old key no longer works.
    second
        .type_into(".recovery-form input", &old_key)
        .await
        .expect("type the old key");
    second.submit(".recovery-form").await.expect("submit");
    second
        .wait_for(".settings-page .error", DEFAULT_WAIT)
        .await
        .expect("the old key is refused");
    second
        .type_into(".recovery-form input", &new_key)
        .await
        .expect("type the new key");
    second.submit(".recovery-form").await.expect("submit");
    second
        .wait_for_text(".status-list", "This device is verified", DEFAULT_WAIT)
        .await
        .expect("unlocked with the new key");
    second
        .press("body", "Escape")
        .await
        .expect("close settings");
    second.open_room("secrets").await.expect("open the room");
    second
        .wait_for_text(".timeline", "after the new key", DEFAULT_WAIT)
        .await
        .unwrap_or_else(|err| panic!("history unlocks with the new key: {err}"));
}
