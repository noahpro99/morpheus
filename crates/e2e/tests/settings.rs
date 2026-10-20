//! Settings modal: encryption status, recovery key, device list, and emoji
//! verification between two of the user's own devices.

#![allow(clippy::expect_used)]

use std::time::Duration;

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login(app: &mut App, user: &str, password: &str) {
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
    app.wait_for(".shell", DEFAULT_WAIT)
        .await
        .expect("shell after login");
}

async fn open_settings(app: &mut App) {
    app.click(".settings-button").await.expect("gear button");
    app.wait_for(".settings", DEFAULT_WAIT)
        .await
        .expect("settings modal");
    // A phone starts on the list of pages; a desktop on My Account.
    if app.is_phone() && app.count(".settings-page").await.unwrap_or(0) == 0 {
        app.click_text(".settings-nav button", "My Account")
            .await
            .expect("My Account");
    }
}

/// Settings' Security & Privacy page: encryption, recovery, devices.
async fn open_security(app: &mut App) {
    open_settings(app).await;
    app.click_text(".settings-nav button", "Security & Privacy")
        .await
        .expect("Security & Privacy");
    app.wait_for(
        ".settings-page .recovery-form, .settings-page .verification",
        DEFAULT_WAIT,
    )
    .await
    .expect("security page");
}

#[tokio::test]
async fn settings_shows_devices_and_rejects_a_bad_recovery_key() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login(&mut app, "carol", "carol-password").await;
    open_settings(&mut app).await;

    app.wait_for_text(".settings-user", "@carol:localhost", DEFAULT_WAIT)
        .await
        .expect("account");
    app.click_text(".settings-nav button", "Security & Privacy")
        .await
        .expect("Security & Privacy");
    app.wait_for_text(".status-list", "not verified", DEFAULT_WAIT)
        .await
        .expect("unverified status");
    app.wait_for_text(".device", "this device", DEFAULT_WAIT)
        .await
        .expect("current device listed");

    app.type_into(".recovery-form input", "definitely not a recovery key")
        .await
        .expect("type key");
    app.submit(".recovery-form").await.expect("submit key");
    app.wait_for(".settings .error", DEFAULT_WAIT)
        .await
        .expect("recovery error shown");

    app.click(".settings-header .ghost").await.expect("close");
    app.wait_for_js(
        "return document.querySelector('.settings') === null;",
        Duration::from_secs(5),
    )
    .await
    .expect("modal closed");
}

#[tokio::test]
async fn emoji_verification_between_two_devices() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    // Two app instances logged in as the same user are two devices.
    let mut a = App::launch(&hs).await.expect("launch A");
    login(&mut a, "bob", "bob-password").await;
    let mut b = App::launch(&hs).await.expect("launch B");
    login(&mut b, "bob", "bob-password").await;
    // Let both finish their first sync so A picks B as the most recent device.
    a.wait_for(".room", DEFAULT_WAIT).await.expect("A synced");
    b.wait_for(".room", DEFAULT_WAIT).await.expect("B synced");

    open_security(&mut a).await;
    a.click_text(".settings button", "Verify with another")
        .await
        .expect("start verification");
    a.wait_for_text(
        ".verification",
        "Waiting for your other device",
        DEFAULT_WAIT,
    )
    .await
    .expect("A waiting");

    // B gets the request pushed into its settings dialog.
    b.wait_for_text(".verification", "wants to verify", DEFAULT_WAIT)
        .await
        .expect("B sees request");
    b.click_text(".verification button", "Accept")
        .await
        .expect("B accepts");

    a.wait_for(".emoji-grid", DEFAULT_WAIT)
        .await
        .expect("A emoji");
    b.wait_for(".emoji-grid", DEFAULT_WAIT)
        .await
        .expect("B emoji");
    let names =
        "return [...document.querySelectorAll('.emoji-name')].map(e => e.textContent).join(',');";
    let a_names = a.eval(names).await.expect("A names");
    let b_names = b.eval(names).await.expect("B names");
    assert_eq!(a_names, b_names, "both devices show the same emoji");

    a.click_text(".verification button", "They match")
        .await
        .expect("A confirms");
    b.click_text(".verification button", "They match")
        .await
        .expect("B confirms");
    a.wait_for_text(".verification", "Verified", DEFAULT_WAIT)
        .await
        .expect("A done");
    b.wait_for_text(".verification", "Verified", DEFAULT_WAIT)
        .await
        .expect("B done");
    // Success needs no dismissing: no Close button, and the panel steps
    // aside by itself, leaving the device shown as verified.
    assert_eq!(
        a.count(".verification button").await.expect("buttons"),
        0,
        "a finished verification offers no buttons"
    );
    a.wait_for_js(
        "return !document.querySelector('.verification');",
        DEFAULT_WAIT,
    )
    .await
    .expect("the finished panel goes away by itself");

    // The device list refreshes: the other device now shows as verified.
    a.wait_for_js(
        "return [...document.querySelectorAll('.device')].some(d => !d.textContent.includes('this device') && d.textContent.includes('verified') && !d.textContent.includes('unverified'));",
        DEFAULT_WAIT,
    )
    .await
    .expect("A lists B as verified");

    if let Some(path) = a.screenshot("verification").await.expect("screenshot") {
        eprintln!("screenshot: {}", path.display());
    }
}

/// Regression: the step signal of a running verification used to be owned by
/// the settings modal's scope, so closing the modal dropped it and reopening
/// panicked the app ("called `Result::unwrap()` on an `Err` value: Dropped").
#[tokio::test]
async fn verification_survives_closing_and_reopening_settings() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut a = App::launch(&hs).await.expect("launch A");
    login(&mut a, "bob", "bob-password").await;
    let mut b = App::launch(&hs).await.expect("launch B");
    login(&mut b, "bob", "bob-password").await;
    a.wait_for(".room", DEFAULT_WAIT).await.expect("A synced");
    b.wait_for(".room", DEFAULT_WAIT).await.expect("B synced");

    open_security(&mut a).await;
    a.click_text(".settings button", "Verify with another")
        .await
        .expect("start verification");
    a.wait_for_text(
        ".verification",
        "Waiting for your other device",
        DEFAULT_WAIT,
    )
    .await
    .expect("A waiting");

    // Close the modal mid-flow, then reopen it: the flow must still be there.
    a.click(".settings-header button[title=Close]")
        .await
        .expect("close settings");
    a.wait_for_js(
        "return document.querySelector('.settings') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("settings closed");
    open_settings(&mut a).await;
    a.wait_for_text(
        ".verification",
        "Waiting for your other device",
        DEFAULT_WAIT,
    )
    .await
    .expect("A still waiting after reopening");

    // The reopened panel keeps following the flow.
    b.wait_for_text(".verification", "wants to verify", DEFAULT_WAIT)
        .await
        .expect("B sees request");
    b.click_text(".verification button", "Accept")
        .await
        .expect("B accepts");
    a.wait_for(".emoji-grid", DEFAULT_WAIT)
        .await
        .expect("A emoji after reopening");
    b.wait_for(".emoji-grid", DEFAULT_WAIT)
        .await
        .expect("B emoji");

    a.click_text(".verification button", "They match")
        .await
        .expect("A confirms");
    b.click_text(".verification button", "They match")
        .await
        .expect("B confirms");
    a.wait_for_text(".verification", "Verified", DEFAULT_WAIT)
        .await
        .expect("A done");
    b.wait_for_text(".verification", "Verified", DEFAULT_WAIT)
        .await
        .expect("B done");
}

/// An account with no key backup can set one up from Settings: the recovery
/// key is shown once to keep, and backup reads as connected.
#[tokio::test]
async fn new_account_sets_up_recovery_from_settings() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let user = morpheus_e2e::new_account(&hs, "recovery").expect("new account");
    let mut app = App::launch(&hs).await.expect("launch app");
    login(&mut app, &user, &format!("{user}-password")).await;
    open_security(&mut app).await;
    app.wait_for_text(".status-list", "No key backup", DEFAULT_WAIT)
        .await
        .expect("no backup yet");
    app.click_text(".settings-page button", "Set up recovery")
        .await
        .expect("set up recovery");
    app.wait_for(".recovery-key-new code", DEFAULT_WAIT)
        .await
        .expect("the new key is shown");
    let key = app
        .eval("return document.querySelector('.recovery-key-new code').textContent.trim();")
        .await
        .expect("key")
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(
        key.split_whitespace().count() >= 8,
        "a recovery key: {key:?}"
    );
    app.wait_for_text(".status-list", "Key backup connected", DEFAULT_WAIT)
        .await
        .expect("backup connected");
    assert_eq!(
        app.count(".settings-page button.setup-recovery")
            .await
            .expect("count"),
        0,
        "no second set-up offered"
    );
}
