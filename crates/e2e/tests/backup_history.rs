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
        .eval("return document.querySelector('.recovery-key-new code').textContent.trim();")
        .await
        .expect("key")
        .as_str()
        .unwrap_or_default()
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
