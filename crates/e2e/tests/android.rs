//! The mobile app on an Android emulator (or a plugged-in phone): sign in,
//! move between the single-column panes, send a message, and read the
//! member list. Skipped unless `MORPHEUS_E2E_ANDROID=1`; `just e2e-android`
//! builds the app with the test bridge, installs it and runs this.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn enabled() -> bool {
    std::env::var_os("MORPHEUS_E2E_ANDROID").is_some()
}

/// Puts a finger down on the first room and keeps it there; records
/// vibrations in `window.__buzz` and leaves the matching lift in `__lift`.
const HOLD_ROOM_JS: &str = "window.__buzz = []; navigator.vibrate = ms => { window.__buzz.push(ms); return true; }; \
    const el = document.querySelector('.room'); const r = el.getBoundingClientRect(); \
    const t = new Touch({ identifier: 1, target: el, clientX: r.left + 30, clientY: r.top + r.height / 2 }); \
    window.__lift = new TouchEvent('touchend', { bubbles: true, cancelable: true, changedTouches: [t] }); \
    el.dispatchEvent(new TouchEvent('touchstart', { bubbles: true, cancelable: true, touches: [t], targetTouches: [t], changedTouches: [t] })); \
    return true;";

async fn pane(app: &mut App, expected: &str) {
    app.wait_for_js(
        &format!("return document.querySelector('.shell').dataset.pane === '{expected}'"),
        DEFAULT_WAIT,
    )
    .await
    .unwrap_or_else(|e| panic!("pane {expected}: {e}"));
}

async fn shot(app: &mut App, name: &str) {
    // The emulator paints in software; let the frame catch up with the DOM.
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    if let Some(path) = app.screenshot(name).await.expect("screenshot") {
        eprintln!("screenshot: {}", path.display());
    }
}

#[tokio::test]
async fn phone_signs_in_navigates_and_sends() {
    if !enabled() {
        eprintln!("MORPHEUS_E2E_ANDROID not set; skipping the Android test");
        return;
    }
    let mut app = App::launch_android().await.expect("launch on Android");

    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    shot(&mut app, "android-login").await;
    app.type_into("form.login input[name=username]", "alice")
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", "alice-password")
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("rooms listed");

    // A phone is narrow: one pane at a time, starting on the room list.
    app.wait_for(".shell.narrow", DEFAULT_WAIT)
        .await
        .expect("narrow layout on a phone");
    pane(&mut app, "rooms").await;
    shot(&mut app, "android-rooms").await;

    // Press and hold is right-click, with a buzz; lifting keeps the menu.
    app.eval(HOLD_ROOM_JS).await.expect("touch down on a room");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("holding opens the room menu");
    app.eval("document.dispatchEvent(window.__lift); return true;")
        .await
        .expect("lift");
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    let state = app
        .eval("return [window.__buzz.length, !!document.querySelector('.context-menu')];")
        .await
        .expect("state");
    assert_eq!(
        state,
        serde_json::json!([1, true]),
        "one buzz, menu still open after lifting"
    );
    shot(&mut app, "android-hold-menu").await;
    app.key_combo("body", "Escape", false, false, false)
        .await
        .expect("close menu");
    app.wait_for_js(
        "return !document.querySelector('.context-menu')",
        DEFAULT_WAIT,
    )
    .await
    .expect("menu closed");

    // Into a room, send, see it echoed.
    app.click_text(".room", "general")
        .await
        .expect("open general");
    pane(&mut app, "chat").await;
    app.wait_for(".composer input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
    let text = format!("from the phone {}", std::process::id());
    app.type_into(".composer input", &text).await.expect("type");
    app.press(".composer input", "Enter").await.expect("send");
    app.wait_for_exact_text(".message.own:not(.pending) .body", &text, DEFAULT_WAIT)
        .await
        .expect("sent and echoed");
    shot(&mut app, "android-chat").await;

    // Members are their own pane; back buttons walk out again.
    app.tap(".chat-header .member-toggle")
        .await
        .expect("members");
    pane(&mut app, "side").await;
    app.wait_for(".side-pane .member", DEFAULT_WAIT)
        .await
        .expect("members listed");
    app.wait_for_js(
        "const m = document.querySelector('.side-pane .member'); return !!m && m.getBoundingClientRect().height > 0;",
        DEFAULT_WAIT,
    )
    .await
    .expect("members visible");
    shot(&mut app, "android-members").await;
    app.click(".side-pane .pane-back")
        .await
        .expect("back from members");
    pane(&mut app, "chat").await;
    app.click(".chat-header .pane-back")
        .await
        .expect("back to rooms");
    pane(&mut app, "rooms").await;
}

/// A real homeserver over HTTPS: typing matrix.org (no need to leave the
/// field) offers single sign-on, and nothing fails on the way. Needs internet.
/// Android checks certificates through the platform, which the app has to
/// set up itself; without that the probe panicked and no SSO button showed.
#[tokio::test]
async fn phone_offers_single_sign_on_for_matrix_org() {
    if !enabled() {
        eprintln!("MORPHEUS_E2E_ANDROID not set; skipping the Android test");
        return;
    }
    let mut app = App::launch_android().await.expect("launch on Android");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=homeserver]", "matrix.org")
        .await
        .expect("homeserver");
    app.wait_for_text("form.login button", "single sign-on", DEFAULT_WAIT)
        .await
        .expect("SSO offered for matrix.org");
    shot(&mut app, "android-login-sso").await;
    let error = app
        .eval("return document.querySelector('form.login .error')?.textContent ?? null;")
        .await
        .expect("error text");
    assert_eq!(
        error,
        serde_json::Value::Null,
        "login screen shows an error"
    );

    // Tapping it hands the sign-in page to the phone's browser (an Android
    // VIEW intent), and the app waits for the browser to come back.
    let _ = std::process::Command::new("adb")
        .args(["logcat", "-c"])
        .status();
    app.click_text("form.login button", "single sign-on")
        .await
        .expect("tap SSO");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let log = std::process::Command::new("adb")
            .args(["logcat", "-d", "-s", "ActivityTaskManager:I"])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
            .unwrap_or_default();
        if log.contains("act=android.intent.action.VIEW") {
            break;
        }
        let error = app
            .eval("return document.querySelector('form.login .error')?.textContent ?? null;")
            .await
            .unwrap_or_default();
        assert_eq!(error, serde_json::Value::Null, "tapping SSO failed");
        assert!(
            std::time::Instant::now() < deadline,
            "no browser opened for single sign-on"
        );
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    // Back to the app (the browser stays open behind it).
    let _ = std::process::Command::new("adb")
        .args([
            "shell",
            "am",
            "start",
            "-n",
            "dev.morpheus.mobile/dev.dioxus.main.MainActivity",
        ])
        .status();
}

/// Android's back key (the real one, through `adb`), after a person's
/// pause rather than the instant the last thing appeared.
fn back_key() {
    std::thread::sleep(std::time::Duration::from_millis(300));
    let _ = std::process::Command::new("adb")
        .args(["shell", "input", "keyevent", "KEYCODE_BACK"])
        .status();
}

/// Whether the on-screen keyboard is up (it takes the next back press).
fn keyboard_shown() -> bool {
    std::thread::sleep(std::time::Duration::from_millis(300));
    std::process::Command::new("adb")
        .args(["shell", "dumpsys", "input_method"])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).contains("mIsInputViewShown=true"))
        .unwrap_or(false)
}

async fn gone(app: &mut App, selector: &str, what: &str) {
    app.wait_for_js(
        &format!(
            "return !document.querySelector({});",
            serde_json::to_string(selector).expect("json")
        ),
        DEFAULT_WAIT,
    )
    .await
    .unwrap_or_else(|e| panic!("back closes {what}: {e}"));
}

/// Phone chrome and the back key: no desktop title bar, a tall pill that
/// buzzes when tapped, and back closing whatever is open (menus, dialogs,
/// cards, an edit, the side pane), then stepping chat → rooms, then leaving
/// the app.
#[tokio::test]
async fn phone_back_key_closes_and_steps_back() {
    if !enabled() {
        eprintln!("MORPHEUS_E2E_ANDROID not set; skipping the Android test");
        return;
    }
    let mut app = App::launch_android().await.expect("launch on Android");
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
        .expect("rooms listed");
    pane(&mut app, "rooms").await;

    // No desktop title bar on a phone; the pill is a thumb-sized target.
    assert_eq!(
        app.count(".titlebar").await.expect("count"),
        0,
        "title bar on a phone"
    );
    let pill = app
        .eval("return document.querySelector('.user-pill').getBoundingClientRect().height;")
        .await
        .expect("pill")
        .as_f64()
        .expect("number");
    assert!(pill >= 60.0, "pill is {pill}px tall");

    // Tapping your pill buzzes and opens the account menu; back closes it.
    app.eval("window.__buzz = []; navigator.vibrate = ms => { window.__buzz.push(ms); return true; }; return true;")
        .await
        .expect("stub vibrate");
    app.tap(".user-pill .profile-chip").await.expect("tap pill");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("account menu");
    let buzzes = app
        .eval("return window.__buzz.length;")
        .await
        .expect("buzz");
    assert_eq!(buzzes, serde_json::json!(1), "tapping the pill buzzes");
    back_key();
    gone(&mut app, ".context-menu", "the account menu").await;

    // Settings (a modal) closes on back.
    app.tap(".user-pill .settings-button")
        .await
        .expect("settings");
    app.wait_for(".settings", DEFAULT_WAIT)
        .await
        .expect("settings open");
    back_key();
    gone(&mut app, ".settings", "settings").await;
    pane(&mut app, "rooms").await;

    // Into a room; back from the member pane returns to the chat.
    app.tap_text(".room", "general")
        .await
        .expect("open general");
    pane(&mut app, "chat").await;
    app.wait_for(".composer input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
    app.tap(".chat-header .member-toggle")
        .await
        .expect("members");
    pane(&mut app, "side").await;
    back_key();
    pane(&mut app, "chat").await;

    // A profile card from a message avatar closes on back.
    app.eval("const v = [...document.querySelectorAll('.message .avatar-button')].find(b => { const r = b.getBoundingClientRect(); return r.top > 80 && r.bottom < innerHeight - 120; }); v.setAttribute('data-avatar-visible', ''); return true;")
        .await
        .expect("a visible avatar");
    app.tap("[data-avatar-visible]").await.expect("avatar");
    app.wait_for(".profile-card", DEFAULT_WAIT)
        .await
        .expect("card");
    back_key();
    gone(&mut app, ".profile-card", "the profile card").await;
    pane(&mut app, "chat").await;

    // Back cancels an edit before it leaves the room.
    let text = format!("back key {}", std::process::id());
    app.type_into(".composer input", &text).await.expect("type");
    app.press(".composer input", "Enter").await.expect("send");
    app.wait_for_exact_text(".message.own:not(.pending) .body", &text, DEFAULT_WAIT)
        .await
        .expect("sent");
    app.press(".composer input", "ArrowUp")
        .await
        .expect("edit last");
    app.wait_for(".composer-banner", DEFAULT_WAIT)
        .await
        .expect("editing");
    // Editing raises the keyboard; Android spends the first back on closing it.
    if keyboard_shown() {
        back_key();
    }
    back_key();
    gone(&mut app, ".composer-banner", "the edit").await;
    pane(&mut app, "chat").await;

    // Back from the chat goes to the room list, then out of the app.
    back_key();
    pane(&mut app, "rooms").await;
    back_key();
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let top = std::process::Command::new("adb")
            .args(["shell", "dumpsys", "activity", "activities"])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
            .unwrap_or_default();
        let resumed = top
            .lines()
            .find(|line| line.contains("ResumedActivity"))
            .unwrap_or_default()
            .to_owned();
        if !resumed.contains("dev.morpheus.mobile") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "back on the room list should leave the app: {resumed}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}
