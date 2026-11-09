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

/// The open menu is a sheet: full width, on the bottom edge of the page.
const SHEET_JS: &str = "const m = document.querySelector('.context-menu.sheet'); if (!m) return false; \
    const r = m.getBoundingClientRect(), s = document.querySelector('.shell').getBoundingClientRect(); \
    return Math.abs(r.left - s.left) < 1 && Math.abs(r.right - s.right) < 1 && Math.abs(r.bottom - s.bottom) < 2;";

/// A menu row with `label` that is actually on screen (submenus are in the
/// page before they open).
fn shown_item(label: &str) -> String {
    format!(
        "return [...document.querySelectorAll('.context-menu .context-item')].some(e => e.textContent.includes({}) && e.offsetParent !== null && e.getBoundingClientRect().height > 0);",
        serde_json::to_string(label).expect("json")
    )
}

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

    // On a phone the menu is a full-width sheet on the bottom edge, and a
    // submenu opens in place (no hover on a touch screen), with a way back.
    app.wait_for_js(SHEET_JS, DEFAULT_WAIT)
        .await
        .expect("the menu is a bottom sheet");
    app.tap_text(".context-menu.sheet .context-item", "Notifications")
        .await
        .expect("tap Notifications");
    app.wait_for_js(&shown_item("Only mentions"), DEFAULT_WAIT)
        .await
        .expect("the submenu opens in the sheet");
    app.tap(".context-menu.sheet .sheet-back")
        .await
        .expect("back to the menu");
    app.wait_for_js(&shown_item("Mark as"), DEFAULT_WAIT)
        .await
        .expect("back at the main list");
    // Dragging the sheet down by its handle dismisses it.
    let top = app
        .eval("return document.querySelector('.context-menu.sheet').getBoundingClientRect().top;")
        .await
        .expect("sheet top")
        .as_f64()
        .expect("number");
    let (width, height) = app.page_size().await.expect("page size");
    app.swipe((width / 2.0, top + 10.0), (width / 2.0, height - 4.0), 250)
        .await
        .expect("swipe the sheet down");
    app.wait_for_js(
        "return !document.querySelector('.context-menu')",
        DEFAULT_WAIT,
    )
    .await
    .expect("a swipe down closes the sheet");
    // It closes wherever the finger starts: on the dimmed page above it, or
    // on one of its items.
    for (start, from) in [("the backdrop", "above"), ("an item", "item")] {
        app.right_click(".room").await.expect("open the room menu");
        app.wait_for_js(SHEET_JS, DEFAULT_WAIT)
            .await
            .expect("the menu is a bottom sheet");
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let y = app
            .eval(&format!(
                "const s = document.querySelector('.context-menu.sheet').getBoundingClientRect(); \
                 const i = [...document.querySelectorAll('.context-menu.sheet .context-item')][0].getBoundingClientRect(); \
                 return '{from}' === 'above' ? s.top / 2 : i.top + i.height / 2;"
            ))
            .await
            .expect("start point")
            .as_f64()
            .expect("number");
        app.swipe((width / 2.0, y), (width / 2.0, height - 4.0), 250)
            .await
            .expect("swipe down");
        app.wait_for_js(
            "return !document.querySelector('.context-menu')",
            DEFAULT_WAIT,
        )
        .await
        .unwrap_or_else(|e| panic!("a swipe down from {start} closes the sheet: {e}"));
    }

    // Into a room, send, see it echoed.
    app.click_text(".room", "general")
        .await
        .expect("open general");
    pane(&mut app, "chat").await;
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
    let text = format!("from the phone {}", std::process::id());
    app.type_into(".composer .composer-input", &text)
        .await
        .expect("type");
    app.press(".composer .composer-input", "Enter")
        .await
        .expect("send");
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
    // Swiping right steps back a pane: members to the chat, the chat to
    // the room list (as the back buttons do).
    let (width, height) = app.page_size().await.expect("page size");
    // Clear of the screen edges, where Android's own back gesture takes the touch.
    let inset = width * 0.15;
    app.swipe((inset, height / 2.0), (width - inset, height / 2.0), 200)
        .await
        .expect("swipe right from members");
    pane(&mut app, "chat").await;
    // Back again, slowly: half-way, the chat has moved with the finger and
    // the room list shows in the gap it left.
    let mid = width / 2.0;
    app.drag_and_hold((inset, height / 2.0), (mid, height / 2.0))
        .await
        .expect("drag half-way");
    let during = app
        .eval(&format!(
            "const c = new DOMMatrix(getComputedStyle(document.querySelector('.chat')).transform); \
             const under = document.elementFromPoint({x}, {y}); \
             return [Math.round(c.m41), !!(under && under.closest('.sidebar, .rail'))];",
            x = mid / 2.0,
            y = height / 2.0
        ))
        .await
        .expect("mid-drag state");
    let left = during[0].as_f64().expect("left");
    assert!(
        (mid - inset - 40.0..=mid - inset + 40.0).contains(&left),
        "the chat follows the finger: it is at {left}px, the finger moved {}px",
        mid - inset
    );
    assert_eq!(
        during[1],
        serde_json::json!(true),
        "the room list shows underneath"
    );
    app.lift((mid, height / 2.0)).await.expect("lift");
    pane(&mut app, "rooms").await;

    // From the list, swiping the other way brings that channel back.
    app.swipe((width - inset, height / 2.0), (inset, height / 2.0), 200)
        .await
        .expect("swipe left on the list");
    pane(&mut app, "chat").await;
    app.wait_for_text(".chat-header h2", "general", DEFAULT_WAIT)
        .await
        .expect("back in #general");

    // A leftward drag on the chat pulls the member list in under the finger,
    // with the chat still underneath; letting go opens it.
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    app.drag_and_hold((width - inset, height / 2.0), (mid, height / 2.0))
        .await
        .expect("drag left half-way");
    let during = app
        .eval(&format!(
            "const p = document.querySelector('.side-pane'); if (!p) return null; \
             const m = new DOMMatrix(getComputedStyle(p).transform); \
             const under = document.elementFromPoint({x}, {y}); \
             return [Math.round(m.m41), !!(under && under.closest('.chat'))];",
            x = mid / 2.0,
            y = height / 2.0
        ))
        .await
        .expect("mid-drag state");
    let offset = during[0].as_f64().expect("side pane offset");
    let travelled = width - inset - mid;
    assert!(
        (width - travelled - 40.0..=width - travelled + 40.0).contains(&offset),
        "the member list follows the finger: it is {offset}px in, the finger moved {travelled}px"
    );
    assert_eq!(
        during[1],
        serde_json::json!(true),
        "the chat shows underneath"
    );
    // Where the list sits on every frame from here: after the finger lifts
    // it only travels on, never back to the edge to slide in again.
    app.eval(
        "window.__lefts = []; const until = performance.now() + 900; \
         const look = () => { const p = document.querySelector('.side-pane'); \
           if (p) window.__lefts.push(Math.round(p.getBoundingClientRect().left)); \
           if (performance.now() < until) requestAnimationFrame(look); }; \
         requestAnimationFrame(look); return true;",
    )
    .await
    .expect("watch the list");
    app.lift((mid, height / 2.0)).await.expect("lift");
    pane(&mut app, "side").await;
    app.wait_for(".side-pane .member", DEFAULT_WAIT)
        .await
        .expect("members open");
    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    let lefts: Vec<f64> =
        serde_json::from_value(app.eval("return window.__lefts;").await.expect("positions"))
            .expect("numbers");
    let back = lefts.windows(2).find(|pair| pair[1] > pair[0] + 2.0);
    assert!(
        back.is_none(),
        "the member list went back towards the edge ({back:?}) to slide in again: {lefts:?}"
    );
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
        .map(|out| String::from_utf8_lossy(&out.stdout).contains("mInputShown=true"))
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

    // Taps do not count as hovers: the app's styles keep no :hover rules on
    // a device that cannot hover.
    let hover_rules = app
        .eval("const walk = list => [...list.cssRules].flatMap(r => r.cssRules && !r.selectorText ? walk(r) : [r]); \
               return [...document.styleSheets].flatMap(s => { try { return walk(s); } catch (_) { return []; } }) \
               .filter(r => (r.selectorText || '').includes(':hover')).length;")
        .await
        .expect("hover rules");
    assert_eq!(
        hover_rules,
        serde_json::json!(0),
        "hover styles on a touch screen"
    );

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
    // From a page, back steps to the list of pages (as its back arrow
    // does), then out of Settings.
    let pages = app
        .eval("return [...document.querySelectorAll('.settings-nav button.settings-nav-item')].map(b => b.textContent.trim());")
        .await
        .expect("pages");
    for page in pages
        .as_array()
        .expect("pages")
        .iter()
        .filter_map(|p| p.as_str())
    {
        app.tap_text(".settings-nav button.settings-nav-item", page)
            .await
            .expect("a settings page");
        app.wait_for_text(".settings-page h2", page, DEFAULT_WAIT)
            .await
            .expect("page shown");
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        back_key();
        gone(&mut app, ".settings-page", &format!("the {page} page")).await;
        app.wait_for(".settings .settings-nav", DEFAULT_WAIT)
            .await
            .unwrap_or_else(|e| panic!("back from {page} at the list of pages: {e}"));
    }
    // The page's own back arrow does the same.
    app.tap_text(".settings-nav button.settings-nav-item", "Appearance")
        .await
        .expect("a settings page");
    app.wait_for_text(".settings-page h2", "Appearance", DEFAULT_WAIT)
        .await
        .expect("page shown");
    app.tap(".settings-page .settings-back")
        .await
        .expect("the back arrow");
    gone(&mut app, ".settings-page", "the page (back arrow)").await;
    back_key();
    gone(&mut app, ".settings", "settings").await;
    pane(&mut app, "rooms").await;

    // Into a room; back from the member pane returns to the chat.
    app.tap_text(".room", "general")
        .await
        .expect("open general");
    pane(&mut app, "chat").await;
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
    // Opening a room does not raise the keyboard over it (Discord does not).
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    assert!(!keyboard_shown(), "opening a room raised the keyboard");
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
    app.type_into(".composer .composer-input", &text)
        .await
        .expect("type");
    app.press(".composer .composer-input", "Enter")
        .await
        .expect("send");
    app.wait_for_exact_text(".message.own:not(.pending) .body", &text, DEFAULT_WAIT)
        .await
        .expect("sent");
    app.press(".composer .composer-input", "ArrowUp")
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

/// Run `adb shell` and return what it printed.
fn adb_shell(args: &[&str]) -> String {
    std::process::Command::new("adb")
        .arg("shell")
        .args(args)
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default()
}

/// Push notifications end to end: the app registers with the phone's
/// UnifiedPush app (Sunup) and with the homeserver, and a message sent while
/// it is in the background shows as an Android notification naming the room.
/// Needs Sunup on the phone and internet (the homeserver pushes through
/// UnifiedPush's public gateway to Mozilla's push service), so it runs only
/// with `MORPHEUS_E2E_PUSH=1`.
#[tokio::test]
async fn phone_gets_push_notifications_in_the_background() {
    if !enabled() || std::env::var_os("MORPHEUS_E2E_PUSH").is_none() {
        eprintln!(
            "MORPHEUS_E2E_ANDROID and MORPHEUS_E2E_PUSH not both set; skipping the push test"
        );
        return;
    }
    let hs = std::env::var("MORPHEUS_TEST_HOMESERVER").expect("MORPHEUS_TEST_HOMESERVER for bob");
    let mut alice = App::launch_android_granting(&["android.permission.POST_NOTIFICATIONS"])
        .await
        .expect("launch on Android");
    alice
        .wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    alice
        .type_into("form.login input[name=username]", "alice")
        .await
        .expect("username");
    alice
        .type_into("form.login input[name=password]", "alice-password")
        .await
        .expect("password");
    alice.submit("form.login").await.expect("submit");
    alice
        .wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("rooms listed");

    // The app registered with Sunup and then with the homeserver.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    loop {
        let status = adb_shell(&[
            "run-as",
            "dev.morpheus.mobile",
            "cat",
            "files/morpheus/push_status",
        ]);
        if status.contains("registered via") {
            eprintln!("push status: {}", status.trim());
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "push never registered; status: {status:?}"
        );
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    // The notification names the room from the list the app keeps.
    let names = adb_shell(&[
        "run-as",
        "dev.morpheus.mobile",
        "cat",
        "files/morpheus/room_names.json",
    ]);
    assert!(names.contains("general"), "room names not written: {names}");

    // Leave the app; bob posts in #general from the desktop.
    let _ = adb_shell(&["input", "keyevent", "KEYCODE_HOME"]);
    let mut bob = App::launch(&hs).await.expect("launch bob");
    bob.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("bob login form");
    bob.type_into("form.login input[name=username]", "bob")
        .await
        .expect("username");
    bob.type_into("form.login input[name=password]", "bob-password")
        .await
        .expect("password");
    bob.submit("form.login").await.expect("submit");
    bob.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("bob rooms");
    bob.open_room("general").await.expect("bob in general");
    bob.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer");
    let text = format!("push ping {}", std::process::id());
    bob.type_into(".composer .composer-input", &text)
        .await
        .expect("type");
    bob.press(".composer .composer-input", "Enter")
        .await
        .expect("send");
    let sent = std::time::Instant::now();

    // Delivery goes homeserver → gateway → Mozilla → Sunup; usually seconds,
    // but a push service may hold messages until its next reconnect.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(240);
    loop {
        let shown = adb_shell(&["dumpsys", "notification", "--noredact"]);
        let ours = shown.split("NotificationRecord(").any(|record| {
            // Who wrote what, in which room (the app is still running, so it
            // has decrypted the message; a push alone carries only ids).
            record.contains("pkg=dev.morpheus.mobile")
                && record.contains("general")
                && record.contains("bob")
                && record.contains(&text)
        });
        if ours {
            eprintln!("notification after {:?}", sent.elapsed());
            // With bob's picture beside it, as on the desktop.
            let record = shown
                .split("NotificationRecord(")
                .find(|record| record.contains("pkg=dev.morpheus.mobile") && record.contains(&text))
                .unwrap_or_default();
            assert!(
                record.contains("android.largeIcon=Icon"),
                "no sender's picture on the notification: {}",
                record.chars().take(1500).collect::<String>()
            );
            break;
        }
        if std::time::Instant::now() >= deadline {
            let records: Vec<&str> = shown
                .split("NotificationRecord(")
                .filter(|record| record.contains("dev.morpheus.mobile"))
                .collect();
            panic!(
                "no Morpheus notification for #general; {} Morpheus records, dump {} bytes: {:?}",
                records.len(),
                shown.len(),
                records
                    .first()
                    .map(|r| r.chars().take(600).collect::<String>())
            );
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    // Tapping the notification opens Morpheus on that room.
    let _ = adb_shell(&["cmd", "statusbar", "expand-notifications"]);
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let _ = adb_shell(&["uiautomator", "dump", "/sdcard/morpheus-ui.xml"]);
    let screen = adb_shell(&["cat", "/sdcard/morpheus-ui.xml"]);
    // The notification's title node ("bob in general") sits inside its row;
    // the row is what takes the tap, so tap the middle of that row.
    let bounds_of = |node: &str| -> Option<[i64; 4]> {
        let raw = node.split("bounds=\"").nth(1)?.split('"').next()?;
        let numbers: Vec<i64> = raw
            .split(|c: char| !c.is_ascii_digit())
            .filter(|part| !part.is_empty())
            .filter_map(|part| part.parse().ok())
            .collect();
        Some([
            *numbers.first()?,
            *numbers.get(1)?,
            *numbers.get(2)?,
            *numbers.get(3)?,
        ])
    };
    let nodes: Vec<&str> = screen.split("<node ").collect();
    let title = nodes
        .iter()
        .find(|node| node.contains(" in general\"") && node.contains("android:id/title"))
        .and_then(|node| bounds_of(node))
        .unwrap_or_else(|| panic!("no Morpheus notification in the shade"));
    let row = nodes
        .iter()
        .filter(|node| node.contains("expandableNotificationRow"))
        .filter_map(|node| bounds_of(node))
        .find(|b| b[1] <= title[1] && title[3] <= b[3])
        .expect("the notification row");
    let (x, y) = ((row[0] + row[2]) / 2, (row[1] + row[3]) / 2);
    eprintln!("tapping the notification row at {x},{y}");
    shot(&mut alice, "android-shade").await;
    let _ = adb_shell(&["input", "tap", &x.to_string(), &y.to_string()]);
    alice
        .wait_for_js(
            "return document.querySelector('.shell').dataset.pane === 'chat' && document.querySelector('.chat-header h2')?.textContent.includes('general');",
            DEFAULT_WAIT,
        )
        .await
        .expect("tapping the notification opens #general");

    // With the app closed (Android killed it in the background), a push still
    // shows who wrote what: the push service decrypts it itself.
    let _ = adb_shell(&["input", "keyevent", "KEYCODE_HOME"]);
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    // Killed as Android reclaims memory (a force-stop would block pushes).
    let pid = adb_shell(&["pidof", "dev.morpheus.mobile"]);
    let _ = adb_shell(&["run-as", "dev.morpheus.mobile", "kill", "-9", pid.trim()]);
    let _ = adb_shell(&["cmd", "notification", "cancel_all"]);
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    assert!(
        adb_shell(&["pidof", "dev.morpheus.mobile"])
            .trim()
            .is_empty(),
        "the app should not be running"
    );
    let cold_text = format!("while you were away {}", std::process::id());
    bob.type_into(".composer .composer-input", &cold_text)
        .await
        .expect("type");
    bob.press(".composer .composer-input", "Enter")
        .await
        .expect("send");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    loop {
        let shown = adb_shell(&["dumpsys", "notification", "--noredact"]);
        if shown
            .split("NotificationRecord(")
            .any(|record| record.contains("pkg=dev.morpheus.mobile") && record.contains(&cold_text))
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no notification with the message text while the app was closed"
        );
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    // A call rings through too: bob calls alice in their DM, and the phone
    // shows who is calling, on the calls channel (it rings like a phone).
    bob.click(".rail-item[title=\"Direct messages\"]")
        .await
        .expect("bob's DMs");
    bob.open_room("alice").await.expect("bob in the DM");
    bob.click(".chat-header .call-button")
        .await
        .expect("bob calls");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    loop {
        let shown = adb_shell(&["dumpsys", "notification", "--noredact"]);
        if shown.split("NotificationRecord(").any(|record| {
            record.contains("pkg=dev.morpheus.mobile")
                && record.contains("bob is calling")
                && record.contains("channel=calls")
        }) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no ringing notification for bob's call"
        );
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    // Opening the app answers for the phone's ringtone: it stops (the app
    // rings in its own way), rather than looping on behind it.
    let _ = adb_shell(&[
        "am",
        "start",
        "-n",
        "dev.morpheus.mobile/dev.dioxus.main.MainActivity",
    ]);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while adb_shell(&["dumpsys", "notification", "--noredact"])
        .split("NotificationRecord(")
        .any(|record| {
            record.contains("pkg=dev.morpheus.mobile") && record.contains("channel=calls")
        })
    {
        assert!(
            std::time::Instant::now() < deadline,
            "the ringing notification stayed (and rang on) with the app open"
        );
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    bob.click(".voice-panel .hang-up")
        .await
        .expect("bob hangs up");
}

/// The attach button opens Android's file picker, and a picked file is
/// uploaded to the room (the picker's answer is stood in for by handing the
/// file input a file, as the picker would).
#[tokio::test]
async fn phone_attach_button_picks_and_uploads_a_file() {
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
    app.wait_for_text(".room", "random", DEFAULT_WAIT)
        .await
        .expect("random listed");
    app.tap_text(".room", "random").await.expect("open random");
    pane(&mut app, "chat").await;
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");

    // A real tap: Android only shows its picker for one.
    app.tap(".composer .attach-button")
        .await
        .expect("tap attach");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let top = adb_shell(&["dumpsys", "activity", "activities"]);
        let resumed = top
            .lines()
            .find(|line| line.contains("ResumedActivity"))
            .unwrap_or_default()
            .to_owned();
        if resumed.contains("documentsui") || resumed.contains("photopicker") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no file picker opened: {resumed}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    back_key();
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    let name = format!("phone-note-{}.txt", std::process::id());
    app.eval(&format!(
        "const input = document.getElementById('morpheus-file-input'); \
         const dt = new DataTransfer(); dt.items.add(new File(['from the phone'], {name}, {{ type: 'text/plain' }})); \
         input.files = dt.files; input.dispatchEvent(new Event('change')); return true;",
        name = serde_json::to_string(&name).expect("json")
    ))
    .await
    .expect("hand the input a file");
    app.wait_for_text(".attachment-card .attachment-name", &name, DEFAULT_WAIT)
        .await
        .expect("the picked file is uploaded");
}

/// Typing on a phone: tapping the message box raises the keyboard, and the
/// box (and the newest message) stay visible above it.
#[tokio::test]
async fn phone_keyboard_keeps_the_message_box_visible() {
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
    app.wait_for_text(".room", "general", DEFAULT_WAIT)
        .await
        .expect("general listed");
    app.tap_text(".room", "general")
        .await
        .expect("open general");
    pane(&mut app, "chat").await;
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
    let before = app
        .eval("return innerHeight;")
        .await
        .expect("height")
        .as_f64()
        .expect("number");

    app.tap(".composer .composer-input")
        .await
        .expect("tap the message box");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while !keyboard_shown() {
        assert!(std::time::Instant::now() < deadline, "no keyboard");
    }
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    let state = app
        .eval(
            "const vv = window.visualViewport; const visible = vv ? vv.height + vv.offsetTop : innerHeight; \
             const box = document.querySelector('.composer .composer-input').getBoundingClientRect(); \
             const last = [...document.querySelectorAll('.message')].pop()?.getBoundingClientRect(); \
             return [innerHeight, visible, box.bottom, last ? last.bottom : 0];",
        )
        .await
        .expect("layout");
    let [inner, visible, box_bottom, last_bottom] =
        [0, 1, 2, 3].map(|i| state[i].as_f64().expect("number"));
    assert!(
        inner < before - 100.0,
        "the page did not shrink for the keyboard ({before} → {inner})"
    );
    assert!(
        box_bottom <= visible + 1.0,
        "the message box is under the keyboard ({box_bottom} > {visible})"
    );
    assert!(
        last_bottom <= box_bottom,
        "the newest message is hidden ({last_bottom} > {box_bottom})"
    );
    shot(&mut app, "android-keyboard").await;
}

/// The last call audio route Android logged (`updateCommunicationRoute`).
fn last_route() -> String {
    adb_shell(&["dumpsys", "audio"])
        .lines()
        .rfind(|line| line.contains("updateCommunicationRoute"))
        .unwrap_or_default()
        .to_owned()
}

/// On a phone a call plays from the loudspeaker (as Discord's voice
/// channels do); the voice panel switches it to the earpiece and back.
#[tokio::test]
async fn phone_calls_use_the_loudspeaker_and_switch_to_the_earpiece() {
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
    open_without_joining(&mut app, "hangout").await;
    app.click(".chat-header .call-button").await.expect("call");
    app.click(".chat-header .pane-back")
        .await
        .expect("back to the list");
    app.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("in the call");

    // Loudspeaker to start with.
    app.wait_for(".voice-panel .speaker-button.active", DEFAULT_WAIT)
        .await
        .expect("speaker on");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while !last_route().contains("speaker") {
        assert!(
            std::time::Instant::now() < deadline,
            "audio not on the loudspeaker: {}",
            last_route()
        );
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }

    // To the earpiece.
    app.click(".voice-panel .speaker-button")
        .await
        .expect("earpiece");
    app.wait_for(".voice-panel .speaker-button:not(.active)", DEFAULT_WAIT)
        .await
        .expect("earpiece shown");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while last_route().contains("speaker") {
        assert!(
            std::time::Instant::now() < deadline,
            "audio still on the loudspeaker: {}",
            last_route()
        );
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
    app.click(".voice-panel .hang-up").await.expect("hang up");
}

/// What Android logged about the app crashing since the crash log was cleared.
fn crash_log() -> String {
    adb_shell(&["logcat", "-b", "crash", "-d"])
}

/// Starting a call on a phone, with its real microphone and speakers (not
/// the test tone): the app stays up and the call connects.
#[tokio::test]
async fn phone_starts_a_call_with_the_real_microphone_without_crashing() {
    if !enabled() {
        eprintln!("MORPHEUS_E2E_ANDROID not set; skipping the Android test");
        return;
    }
    let _ = adb_shell(&["logcat", "-b", "crash", "-c"]);
    let mut app = App::launch_android_with_real_audio()
        .await
        .expect("launch on Android");
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
    open_without_joining(&mut app, "hangout").await;
    app.click(".chat-header .call-button").await.expect("call");
    // Give a crash time to happen: the microphone opens as the call connects.
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    let crashed = crash_log();
    assert!(
        !crashed.contains("dev.morpheus.mobile"),
        "the app crashed starting a call:\n{crashed}"
    );
    app.tap(".call-screen .call-minimize")
        .await
        .expect("down from the call screen (the app is still up)");
    app.click(".chat-header .pane-back")
        .await
        .expect("back to the list");
    app.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("in the call");
    shot(&mut app, "android-call-list").await;
    app.tap_text(".room", "hangout")
        .await
        .expect("back into the room");
    pane(&mut app, "chat").await;
    shot(&mut app, "android-call-room").await;
    app.click(".chat-header .pane-back")
        .await
        .expect("back to the list");
    app.click(".voice-panel .hang-up").await.expect("hang up");
    app.wait_for_js(
        "return !document.querySelector('.voice-panel');",
        DEFAULT_WAIT,
    )
    .await
    .expect("out of the call");
    let crashed = crash_log();
    assert!(
        !crashed.contains("dev.morpheus.mobile"),
        "the app crashed in or after the call:\n{crashed}"
    );
}

/// The phone's message box: the keyboard's Enter starts a new line, the send
/// button beside the emoji button sends, and going back to the list puts
/// the keyboard away.
#[tokio::test]
async fn phone_enter_is_a_new_line_the_send_button_sends_and_back_hides_the_keyboard() {
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
    app.wait_for_text(".room", "general", DEFAULT_WAIT)
        .await
        .expect("general listed");
    app.tap_text(".room", "general")
        .await
        .expect("open general");
    pane(&mut app, "chat").await;
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
    // The send button sits right after the emoji button.
    let order = app
        .eval("return [...document.querySelector('.composer-row').children].map(e => e.className.split(' ')[0]);")
        .await
        .expect("order");
    let order: Vec<String> = serde_json::from_value(order).expect("names");
    let emoji = order
        .iter()
        .position(|c| c == "emoji-button")
        .expect("emoji button");
    assert_eq!(
        order.get(emoji + 1).map(String::as_str),
        Some("send-button"),
        "{order:?}"
    );

    app.tap(".composer .composer-input")
        .await
        .expect("tap the box");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while !keyboard_shown() {
        assert!(std::time::Instant::now() < deadline, "no keyboard");
    }
    // Typed on the keyboard, with its Enter between the lines.
    let run = std::process::id();
    let _ = adb_shell(&["input", "text", &format!("first{run}")]);
    let _ = adb_shell(&["input", "keyevent", "KEYCODE_ENTER"]);
    let _ = adb_shell(&["input", "text", "second"]);
    // The keyboard's own suggestions may reword what `input text` types
    // (it learns from every run): what matters is the two lines.
    if let Err(err) = app
        .wait_for_js(
            "const v = document.querySelector('.composer .composer-input').value; \
             return v.startsWith('f') && v.endsWith('\\nsecond');",
            DEFAULT_WAIT,
        )
        .await
    {
        let value = app
            .eval("return [document.querySelector('.composer .composer-input').value, document.activeElement && document.activeElement.className];")
            .await;
        shot(&mut app, "android-enter-failed").await;
        panic!("Enter made a new line, and nothing was sent: {err}; the box holds {value:?}");
    }
    let first = app
        .eval("return document.querySelector('.composer .composer-input').value.split('\\n')[0];")
        .await
        .expect("the first line");
    let first = first.as_str().expect("text").to_owned();
    app.tap(".composer .send-button").await.expect("send");
    app.wait_for_js(
        &format!(
            "return [...document.querySelectorAll('.message .body')].some(b => b.textContent.includes({}) && b.textContent.includes('second'));",
            serde_json::to_string(&first).expect("json")
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("both lines sent as one message");
    assert!(keyboard_shown(), "sending keeps the keyboard up");

    // Back to the list: the keyboard goes.
    app.tap(".chat-header .pane-back").await.expect("back");
    pane(&mut app, "rooms").await;
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while keyboard_shown() {
        assert!(
            std::time::Instant::now() < deadline,
            "the keyboard stayed up over the room list"
        );
    }
}

/// After the phone app has been in the background (Android freezes it),
/// coming back to a chat shows new messages as they arrive.
#[tokio::test]
async fn phone_back_from_the_background_still_gets_messages_live() {
    if !enabled() {
        eprintln!("MORPHEUS_E2E_ANDROID not set; skipping the Android test");
        return;
    }
    let Ok(hs) = std::env::var("MORPHEUS_TEST_HOMESERVER") else {
        return;
    };
    let mut phone = App::launch_android().await.expect("launch on Android");
    phone
        .wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    phone
        .type_into("form.login input[name=username]", "alice")
        .await
        .expect("username");
    phone
        .type_into("form.login input[name=password]", "alice-password")
        .await
        .expect("password");
    phone.submit("form.login").await.expect("submit");
    phone
        .wait_for_text(".room", "general", DEFAULT_WAIT)
        .await
        .expect("general listed");
    phone
        .tap_text(".room", "general")
        .await
        .expect("open general");
    pane(&mut phone, "chat").await;

    let mut bob = App::launch_desktop(&hs).await.expect("launch bob");
    bob.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    bob.type_into("form.login input[name=username]", "bob")
        .await
        .expect("username");
    bob.type_into("form.login input[name=password]", "bob-password")
        .await
        .expect("password");
    bob.submit("form.login").await.expect("submit");
    bob.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    bob.open_room("general").await.expect("open general");
    bob.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer");
    let run = std::process::id();
    let send = async |bob: &mut App, text: &str| {
        bob.type_into(".composer .composer-input", text)
            .await
            .expect("type");
        bob.press(".composer .composer-input", "Enter")
            .await
            .expect("send");
    };
    send(&mut bob, &format!("before {run}")).await;
    phone
        .wait_for_text(".message .body", &format!("before {run}"), DEFAULT_WAIT)
        .await
        .expect("live before the break");

    // Home, a while away (long enough for Android to freeze the app and the
    // sync request to lapse), then back.
    let _ = adb_shell(&["input", "keyevent", "KEYCODE_HOME"]);
    tokio::time::sleep(std::time::Duration::from_secs(45)).await;
    send(&mut bob, &format!("while away {run}")).await;
    let _ = adb_shell(&[
        "am",
        "start",
        "-n",
        "dev.morpheus.mobile/dev.dioxus.main.MainActivity",
    ]);
    phone
        .wait_for_text(".message .body", &format!("while away {run}"), DEFAULT_WAIT)
        .await
        .expect("what came while away shows on return");
    for n in 0..3 {
        let text = format!("after {n} {run}");
        send(&mut bob, &text).await;
        phone
            .wait_for_text(".message .body", &text, DEFAULT_WAIT)
            .await
            .unwrap_or_else(|e| panic!("message {n} arrives live after the break: {e}"));
    }
}

/// A call on a phone fills the screen, as Discord's does: a tile for each
/// person in it, and mute, speaker, and hang up along the bottom. Back (or
/// the minimize button) shrinks it to a bar above the message box; the bar
/// or the header's call button brings it back.
#[tokio::test]
async fn phone_call_fills_the_screen_and_minimizes_to_a_bar() {
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
    open_without_joining(&mut app, "hangout").await;
    app.tap(".chat-header .call-button").await.expect("call");

    // Full screen: the room's name, a tile for alice, the controls.
    app.wait_for_text(".call-screen .call-screen-title", "hangout", DEFAULT_WAIT)
        .await
        .expect("the call screen");
    app.wait_for_text(".call-screen .call-screen-state", "Connected", DEFAULT_WAIT)
        .await
        .expect("connected");
    app.wait_for_text(".call-screen .call-tile", "alice", DEFAULT_WAIT)
        .await
        .expect("a tile for alice");
    for control in [".mute-button", ".speaker-button", ".hang-up"] {
        app.wait_for(
            &format!(".call-screen .call-controls {control}"),
            DEFAULT_WAIT,
        )
        .await
        .unwrap_or_else(|e| panic!("{control} on the call screen: {e}"));
    }
    let covered = app
        .eval(
            "const r = document.querySelector('.call-screen').getBoundingClientRect(); \
             return r.top <= 0 && r.left <= 0 && r.width >= innerWidth && r.height >= innerHeight;",
        )
        .await
        .expect("measure");
    assert_eq!(
        covered,
        serde_json::json!(true),
        "the call fills the screen"
    );
    shot(&mut app, "android-call-screen").await;

    // Mute from the call screen.
    app.tap(".call-screen .mute-button").await.expect("mute");
    app.wait_for(".call-screen .mute-button.active", DEFAULT_WAIT)
        .await
        .expect("muted");
    app.tap(".call-screen .mute-button").await.expect("unmute");
    app.wait_for(".call-screen .mute-button:not(.active)", DEFAULT_WAIT)
        .await
        .expect("unmuted");

    // Back minimizes to a bar in the chat, still in the call.
    back_key();
    gone(&mut app, ".call-screen", "the call screen").await;
    pane(&mut app, "chat").await;
    app.wait_for_text(".chat .call-bar", "hangout", DEFAULT_WAIT)
        .await
        .expect("the call bar");
    shot(&mut app, "android-call-bar").await;
    app.tap(".chat .call-bar")
        .await
        .expect("open the call again");
    app.wait_for(".call-screen", DEFAULT_WAIT)
        .await
        .expect("the call screen again");
    app.tap(".call-screen .call-minimize")
        .await
        .expect("minimize");
    gone(&mut app, ".call-screen", "the call screen").await;
    // The header's call button opens it rather than hanging up.
    app.tap(".chat-header .call-button")
        .await
        .expect("call button");
    app.wait_for(".call-screen", DEFAULT_WAIT)
        .await
        .expect("the call screen from the header");

    app.tap(".call-screen .hang-up").await.expect("hang up");
    gone(&mut app, ".call-screen", "the call screen").await;
    app.wait_for_js(
        "return !document.querySelector('.voice-panel') && !document.querySelector('.call-bar');",
        DEFAULT_WAIT,
    )
    .await
    .expect("out of the call");
}

/// The middle of the first element matching `selector` whose text has
/// `needle`, in page coordinates.
async fn middle_of(app: &mut App, selector: &str, needle: &str) -> (f64, f64) {
    let at = app
        .eval(&format!(
            "const el = [...document.querySelectorAll({})].find(el => el.textContent.includes({})); \
             if (!el) return null; el.scrollIntoView({{ block: 'center' }}); \
             const r = el.getBoundingClientRect(); return [r.left + Math.min(r.width / 2, 40), r.top + r.height / 2];",
            serde_json::to_string(selector).expect("json"),
            serde_json::to_string(needle).expect("json")
        ))
        .await
        .expect("measure");
    let at = at
        .as_array()
        .unwrap_or_else(|| panic!("no {selector} with {needle:?}"));
    (at[0].as_f64().expect("x"), at[1].as_f64().expect("y"))
}

/// A real finger held down at `at` for a second, then lifted.
async fn hold(app: &mut App, at: (f64, f64)) {
    app.drag_and_hold(at, at).await.expect("touch down");
    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    app.lift(at).await.expect("lift");
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
}

/// Press and hold opens a message's menu, so messages cannot be selected
/// directly on a touch screen: the menu's Select text shows the message
/// where holding selects it, with the system's handles and Copy.
#[tokio::test]
async fn phone_select_text_lets_a_message_be_selected() {
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
    app.wait_for_text(".room", "general", DEFAULT_WAIT)
        .await
        .expect("general listed");
    app.tap_text(".room", "general")
        .await
        .expect("open general");
    pane(&mut app, "chat").await;
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
    let text = format!("select these words {}", std::process::id());
    app.type_into(".composer .composer-input", &text)
        .await
        .expect("type");
    app.press(".composer .composer-input", "Enter")
        .await
        .expect("send");
    app.wait_for_exact_text(".message.own:not(.pending) .body", &text, DEFAULT_WAIT)
        .await
        .expect("sent");
    back_key(); // the keyboard
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let at = middle_of(&mut app, ".message .body", &text).await;
    hold(&mut app, at).await;
    app.wait_for_js(&shown_item("Select text"), DEFAULT_WAIT)
        .await
        .expect("Select text in the message menu");
    app.tap_text(".context-menu .context-item", "Select text")
        .await
        .expect("Select text");
    app.wait_for_text(".select-text .selectable", &text, DEFAULT_WAIT)
        .await
        .expect("the message, to select from");

    // Holding there selects words, as anywhere else on the phone.
    let at = middle_of(&mut app, ".select-text .selectable", &text).await;
    hold(&mut app, at).await;
    let selected = app
        .eval("return [String(getSelection()), !!document.querySelector('.context-menu')];")
        .await
        .expect("selection");
    shot(&mut app, "android-select-text").await;
    let words = selected[0].as_str().unwrap_or_default();
    assert!(
        !words.trim().is_empty() && text.contains(words.trim()),
        "holding selected nothing from the message: {selected}"
    );
    assert_eq!(
        selected[1],
        serde_json::json!(false),
        "no menu over the selection"
    );

    // Back closes it (after clearing the selection, if the system takes one press for that).
    back_key();
    if app.count(".select-text").await.expect("count") > 0 {
        back_key();
    }
    gone(&mut app, ".select-text", "the select text sheet").await;
}

/// Someone's profile on a phone is a bottom sheet: it rises from the bottom
/// edge across the full width, their picture sits over the banner (not
/// under it), and a downward swipe puts it away.
#[tokio::test]
async fn phone_profile_is_a_bottom_sheet() {
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
    app.wait_for_text(".room", "general", DEFAULT_WAIT)
        .await
        .expect("general listed");
    app.tap_text(".room", "general")
        .await
        .expect("open general");
    pane(&mut app, "chat").await;
    app.wait_for(".message .avatar-button", DEFAULT_WAIT)
        .await
        .expect("a sender's picture");
    app.eval(
        "const b = [...document.querySelectorAll('.message .avatar-button')].pop(); \
         b.scrollIntoView({ block: 'center' }); return true;",
    )
    .await
    .expect("scroll to a sender");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    app.eval(
        "[...document.querySelectorAll('.message .avatar-button')].pop().click(); return true;",
    )
    .await
    .expect("open the profile");
    app.wait_for(".profile-card", DEFAULT_WAIT)
        .await
        .expect("the profile");
    // It slides up: give that time to finish before measuring.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let shape = app
        .eval(
            "const c = document.querySelector('.profile-card'); const r = c.getBoundingClientRect(); \
             const a = c.querySelector('.profile-body .avatar').getBoundingClientRect(); \
             const banner = c.querySelector('.profile-banner').getBoundingClientRect(); \
             const x = a.left + a.width / 2, y = (a.top + banner.bottom) / 2; \
             const top = document.elementFromPoint(x, y); \
             return [c.classList.contains('sheet'), Math.round(r.left), Math.round(innerWidth - r.right), \
                     Math.round(innerHeight - r.bottom), a.top < banner.bottom, \
                     !!(top && top.closest('.avatar'))];",
        )
        .await
        .expect("measure");
    shot(&mut app, "android-profile-sheet").await;
    assert_eq!(
        shape[0],
        serde_json::json!(true),
        "a sheet, not a floating card: {shape}"
    );
    assert_eq!(
        (&shape[1], &shape[2], &shape[3]),
        (
            &serde_json::json!(0),
            &serde_json::json!(0),
            &serde_json::json!(0)
        ),
        "edge to edge, on the bottom edge: {shape}"
    );
    assert_eq!(
        shape[4],
        serde_json::json!(true),
        "the picture overlaps the banner: {shape}"
    );
    assert_eq!(
        shape[5],
        serde_json::json!(true),
        "the picture is over the banner, not under: {shape}"
    );

    // Swiping it down puts it away.
    let (width, height) = app.page_size().await.expect("size");
    let top = app
        .eval("return document.querySelector('.profile-card').getBoundingClientRect().top;")
        .await
        .expect("top")
        .as_f64()
        .expect("number");
    app.swipe((width / 2.0, top + 10.0), (width / 2.0, height - 4.0), 250)
        .await
        .expect("swipe down");
    gone(&mut app, ".profile-card", "the profile sheet").await;
}

/// A slow link to the homeserver: a proxy on `port` in front of it whose
/// requests wait `delay` each while `slow` is set. (The emulator's own
/// `network delay` leaves traffic to the host alone.)
fn slow_proxy(
    port: u16,
    delay: std::time::Duration,
) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
    use std::sync::atomic::Ordering;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let slow = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let listener = std::net::TcpListener::bind(("127.0.0.1", port)).expect("proxy port");
    listener.set_nonblocking(true).expect("nonblocking");
    let listener = tokio::net::TcpListener::from_std(listener).expect("listener");
    let flag = slow.clone();
    tokio::spawn(async move {
        while let Ok((client, _)) = listener.accept().await {
            let Ok(server) = tokio::net::TcpStream::connect(("127.0.0.1", 8008)).await else {
                continue;
            };
            let (mut client_read, mut client_write) = client.into_split();
            let (mut server_read, mut server_write) = server.into_split();
            let flag = flag.clone();
            tokio::spawn(async move {
                let mut buf = vec![0; 64 * 1024];
                while let Ok(n) = client_read.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    if flag.load(Ordering::Relaxed) {
                        tokio::time::sleep(delay).await;
                    }
                    if server_write.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            });
            tokio::spawn(async move {
                let _ = tokio::io::copy(&mut server_read, &mut client_write).await;
            });
        }
    });
    slow
}

/// Sending shows the message at once, greyed until the server has it, then
/// as sent: no wait for the network before it appears, even a slow one.
#[tokio::test]
async fn phone_sent_messages_show_at_once_even_on_a_slow_network() {
    if !enabled() {
        eprintln!("MORPHEUS_E2E_ANDROID not set; skipping the Android test");
        return;
    }
    let mut app = App::launch_android().await.expect("launch on Android");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    // Signed in through the proxy, to slow the link down later.
    let slow = slow_proxy(18008, std::time::Duration::from_millis(1500));
    app.type_into("form.login input[name=homeserver]", "http://10.0.2.2:18008")
        .await
        .expect("homeserver");
    app.type_into("form.login input[name=username]", "alice")
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", "alice-password")
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for_text(".room", "general", DEFAULT_WAIT)
        .await
        .expect("general listed");
    app.tap_text(".room", "general")
        .await
        .expect("open general");
    pane(&mut app, "chat").await;
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");

    slow.store(true, std::sync::atomic::Ordering::Relaxed);
    let text = format!("right away {}", std::process::id());
    app.type_into(".composer .composer-input", &text)
        .await
        .expect("type");
    // When the send button is pressed, and when the message first shows.
    app.eval(&format!(
        "window.__shown = null; const text = {}; \
         document.querySelector('.send-button').addEventListener('click', () => {{ window.__pressed = performance.now(); }}, {{ capture: true, once: true }}); \
         const seen = () => [...document.querySelectorAll('.message.own .body')].some(b => b.textContent.includes(text)); \
         new MutationObserver((_, o) => {{ if (seen()) {{ window.__shown = performance.now(); o.disconnect(); }} }}) \
           .observe(document.body, {{ subtree: true, childList: true, characterData: true }}); \
         return true;",
        serde_json::to_string(&text).expect("json")
    ))
    .await
    .expect("watch");
    app.tap(".send-button").await.expect("send");
    app.wait_for_js("return window.__shown !== null;", DEFAULT_WAIT)
        .await
        .expect("the message shows");
    let waited = app
        .eval("return window.__shown - window.__pressed;")
        .await
        .expect("timing")
        .as_f64()
        .expect("number");
    let pending = app
        .eval(&format!(
            "return [...document.querySelectorAll('.message.own')].some(m => m.classList.contains('pending') && m.textContent.includes({}));",
            serde_json::to_string(&text).expect("json")
        ))
        .await
        .expect("pending");
    shot(&mut app, "android-sending").await;
    assert!(
        waited < 500.0,
        "the message took {waited:.0} ms to show after pressing send"
    );
    assert_eq!(
        pending,
        serde_json::json!(true),
        "greyed while the server has not got it"
    );
    slow.store(false, std::sync::atomic::Ordering::Relaxed);
    app.wait_for_exact_text(".message.own:not(.pending) .body", &text, DEFAULT_WAIT)
        .await
        .expect("then shown as sent");
}

/// The emulator's network switched off (Wi-Fi and mobile data) for as long
/// as this lives.
struct NoNetwork;

impl NoNetwork {
    fn new() -> Self {
        let _ = adb_shell(&["svc", "wifi", "disable"]);
        let _ = adb_shell(&["svc", "data", "disable"]);
        Self
    }
}

impl Drop for NoNetwork {
    fn drop(&mut self) {
        let _ = adb_shell(&["svc", "wifi", "enable"]);
        let _ = adb_shell(&["svc", "data", "enable"]);
    }
}

/// A phone loses its network now and then (a tunnel, Wi-Fi to mobile data).
/// Syncing failed then and never started again, so new messages only came
/// with a restart of the app; now it carries on once the network is back.
#[tokio::test]
async fn phone_gets_messages_live_again_after_losing_the_network() {
    if !enabled() {
        eprintln!("MORPHEUS_E2E_ANDROID not set; skipping the Android test");
        return;
    }
    let Ok(hs) = std::env::var("MORPHEUS_TEST_HOMESERVER") else {
        return;
    };
    let mut phone = App::launch_android().await.expect("launch on Android");
    phone
        .wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    phone
        .type_into("form.login input[name=username]", "alice")
        .await
        .expect("username");
    phone
        .type_into("form.login input[name=password]", "alice-password")
        .await
        .expect("password");
    phone.submit("form.login").await.expect("submit");
    phone
        .wait_for_text(".room", "general", DEFAULT_WAIT)
        .await
        .expect("general listed");
    phone
        .tap_text(".room", "general")
        .await
        .expect("open general");
    pane(&mut phone, "chat").await;

    let mut bob = App::launch_desktop(&hs).await.expect("launch bob");
    bob.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    bob.type_into("form.login input[name=username]", "bob")
        .await
        .expect("username");
    bob.type_into("form.login input[name=password]", "bob-password")
        .await
        .expect("password");
    bob.submit("form.login").await.expect("submit");
    bob.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    bob.open_room("general").await.expect("open general");
    bob.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer");
    let run = std::process::id();
    let send = async |bob: &mut App, text: &str| {
        bob.type_into(".composer .composer-input", text)
            .await
            .expect("type");
        bob.press(".composer .composer-input", "Enter")
            .await
            .expect("send");
    };
    send(&mut bob, &format!("online {run}")).await;
    phone
        .wait_for_text(".message .body", &format!("online {run}"), DEFAULT_WAIT)
        .await
        .expect("live to start with");

    // Long enough for the sync request under way to fail.
    let offline = NoNetwork::new();
    tokio::time::sleep(std::time::Duration::from_secs(40)).await;
    let errors = phone.log_tail(400);
    drop(offline);
    eprintln!(
        "sync while offline:\n{}",
        errors
            .lines()
            .filter(|l| l.contains("sync"))
            .take(20)
            .collect::<Vec<_>>()
            .join("\n")
    );

    for n in 0..2 {
        let text = format!("back online {n} {run}");
        send(&mut bob, &text).await;
        phone
            .wait_for_text(".message .body", &text, std::time::Duration::from_secs(60))
            .await
            .unwrap_or_else(|e| panic!("message {n} arrives live once the network is back: {e}"));
    }
}

/// The call screen goes down with a downward swipe, to its bar above the
/// message box (still in the call). Sideways swipes do nothing there: they
/// moved the panes hidden underneath (the member list slid in behind it).
#[tokio::test]
async fn phone_call_screen_swipes_down_and_not_sideways() {
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
    open_without_joining(&mut app, "hangout").await;
    app.tap(".chat-header .call-button").await.expect("call");
    app.wait_for_text(".call-screen .call-screen-state", "Connected", DEFAULT_WAIT)
        .await
        .expect("the call screen");
    let (width, height) = app.page_size().await.expect("size");

    // Sideways, either way, across the middle (from the very edge it is
    // Android's own back gesture, which may close it): the call screen
    // stays, and nothing moves under it.
    for (from, to) in [
        ((width * 0.85, height / 2.0), (width * 0.15, height / 2.0)),
        ((width * 0.15, height / 2.0), (width * 0.85, height / 2.0)),
    ] {
        app.swipe(from, to, 200).await.expect("swipe sideways");
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        assert_eq!(
            app.count(".call-screen").await.expect("count"),
            1,
            "a sideways swipe closed the call screen"
        );
        pane(&mut app, "chat").await;
        assert_eq!(
            app.eval("return !!document.querySelector('.side-pane:not(.parked)');")
                .await
                .expect("side pane"),
            serde_json::json!(false),
            "the member list slid in under the call screen"
        );
    }

    // Down: the bar, still in the call.
    app.swipe(
        (width / 2.0, height * 0.3),
        (width / 2.0, height - 10.0),
        250,
    )
    .await
    .expect("swipe down");
    gone(&mut app, ".call-screen", "the call screen").await;
    app.wait_for_text(".chat .call-bar", "hangout", DEFAULT_WAIT)
        .await
        .expect("down to the bar, still in the call");

    app.tap(".chat .call-bar").await.expect("open again");
    app.wait_for(".call-screen .hang-up", DEFAULT_WAIT)
        .await
        .expect("the call screen again");
    let shifted = app
        .eval("return getComputedStyle(document.querySelector('.call-screen')).transform;")
        .await
        .expect("transform");
    assert!(
        matches!(
            shifted.as_str(),
            Some("none") | Some("matrix(1, 0, 0, 1, 0, 0)")
        ),
        "it opened again where the swipe left it: {shifted}"
    );
    app.tap(".call-screen .hang-up").await.expect("hang up");
}

/// A call ringing the phone, with the app open, is heard: the ringtone
/// really plays (this run has real audio).
#[tokio::test]
async fn phone_ring_is_heard_with_the_app_open() {
    if !enabled() {
        eprintln!("MORPHEUS_E2E_ANDROID not set; skipping the Android test");
        return;
    }
    let Ok(hs) = std::env::var("MORPHEUS_TEST_HOMESERVER") else {
        return;
    };
    let mut phone = App::launch_android_with_real_audio()
        .await
        .expect("launch on Android");
    phone
        .wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    phone
        .type_into("form.login input[name=username]", "alice")
        .await
        .expect("username");
    phone
        .type_into("form.login input[name=password]", "alice-password")
        .await
        .expect("password");
    phone.submit("form.login").await.expect("submit");
    phone.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");

    let mut bob = App::launch_desktop(&hs).await.expect("launch bob");
    bob.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    bob.type_into("form.login input[name=username]", "bob")
        .await
        .expect("username");
    bob.type_into("form.login input[name=password]", "bob-password")
        .await
        .expect("password");
    bob.submit("form.login").await.expect("submit");
    bob.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    bob.click(".rail-item[title=\"Direct messages\"]")
        .await
        .expect("bob's DMs");
    bob.open_room("alice").await.expect("bob in the DM");
    bob.click(".chat-header .call-button")
        .await
        .expect("bob calls");
    phone
        .wait_for_text(".incoming-call", "bob", DEFAULT_WAIT)
        .await
        .expect("the phone is rung");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    let plays: Vec<String> = loop {
        let plays: Vec<String> = serde_json::from_value(
            phone
                .eval("return window.morpheusSoundPlays ?? [];")
                .await
                .expect("sounds"),
        )
        .unwrap_or_default();
        if plays.iter().any(|p| p.starts_with("ring: ")) {
            break plays;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no ringtone tried: {plays:?}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    };
    bob.click(".voice-panel .hang-up")
        .await
        .expect("bob hangs up");
    assert!(
        plays.iter().any(|p| p == "ring: played")
            && !plays
                .iter()
                .any(|p| p.starts_with("ring: ") && p != "ring: played"),
        "the ringtone did not play: {plays:?}"
    );
}

/// A phone in a call turns its camera on, and the others see it: the
/// camera button is there, and a desktop gets the phone's picture (the
/// emulator's camera films a virtual room). Real camera and microphone.
#[tokio::test]
async fn phone_camera_shows_on_the_desktop() {
    if !enabled() {
        eprintln!("MORPHEUS_E2E_ANDROID not set; skipping the Android test");
        return;
    }
    let Ok(hs) = std::env::var("MORPHEUS_TEST_HOMESERVER") else {
        return;
    };
    let mut phone = App::launch_android_with_real_media()
        .await
        .expect("launch on Android");
    phone
        .wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    phone
        .type_into("form.login input[name=username]", "alice")
        .await
        .expect("username");
    phone
        .type_into("form.login input[name=password]", "alice-password")
        .await
        .expect("password");
    phone.submit("form.login").await.expect("submit");
    phone.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    open_without_joining(&mut phone, "hangout").await;
    phone.tap(".chat-header .call-button").await.expect("call");
    phone
        .wait_for_text(".call-screen .call-screen-state", "Connected", DEFAULT_WAIT)
        .await
        .expect("connected");

    let mut bob = App::launch_desktop(&hs).await.expect("launch bob");
    bob.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    bob.type_into("form.login input[name=username]", "bob")
        .await
        .expect("username");
    bob.type_into("form.login input[name=password]", "bob-password")
        .await
        .expect("password");
    bob.submit("form.login").await.expect("submit");
    bob.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    open_without_joining(&mut bob, "hangout").await;
    bob.click(".chat-header .call-button")
        .await
        .expect("bob joins");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("bob connected");

    // The phone has a camera button, and turns it on.
    phone
        .wait_for(".call-screen .camera-button:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("a camera button on the phone");
    phone
        .tap(".call-screen .camera-button")
        .await
        .expect("camera on");
    phone
        .wait_for(".call-screen .camera-button.active", DEFAULT_WAIT)
        .await
        .expect("the camera is on");

    // Bob sees a real picture from it: loaded, and not one flat colour.
    let picture = "const t = [...document.querySelectorAll('.call-stage .video-tile')].find(t => t.dataset.user === '@alice:localhost'); \
         const i = t && t.querySelector('img.video'); \
         if (!i || !i.complete || !i.naturalWidth) return false; \
         const c = document.createElement('canvas'); c.width = 32; c.height = 32; \
         const g = c.getContext('2d'); g.drawImage(i, 0, 0, 32, 32); \
         const d = g.getImageData(0, 0, 32, 32).data; let lo = 255, hi = 0; \
         for (let p = 0; p < d.length; p += 4) { const v = (d[p] + d[p + 1] + d[p + 2]) / 3; lo = Math.min(lo, v); hi = Math.max(hi, v); } \
         return hi - lo > 30;";
    bob.wait_for_js(picture, DEFAULT_WAIT)
        .await
        .expect("bob sees the phone's camera");
    // Upright: the sensor is mounted turned (90° on the emulator, as on
    // most phones), so a 640×480 picture stands as portrait, taller than
    // wide, on both sides. The phone's own preview showed it sideways.
    let shape = "const t = [...document.querySelectorAll('.video-tile')].find(t => t.dataset.user === '@alice:localhost'); \
         const i = t && t.querySelector('img.video'); \
         return i && i.naturalWidth ? [i.naturalWidth, i.naturalHeight] : null;";
    // Frames replace the picture as they come: wait for a loaded one.
    bob.wait_for_js(
        &format!("return (() => {{ {shape} }})() !== null;"),
        DEFAULT_WAIT,
    )
    .await
    .expect("a loaded frame");
    let seen = bob.eval(shape).await.expect("bob's picture");
    phone
        .wait_for_js(
            "const t = document.querySelector('.call-screen .video-tile.self'); const i = t && t.querySelector('img.video'); return !!(i && i.naturalWidth);",
            DEFAULT_WAIT,
        )
        .await
        .expect("the phone's own preview");
    let own = phone
        .eval(
            "const i = document.querySelector('.call-screen .video-tile.self img.video'); return [i.naturalWidth, i.naturalHeight];",
        )
        .await
        .expect("the phone's preview");
    let portrait = |shape: &serde_json::Value| shape[1].as_f64() > shape[0].as_f64();
    assert!(
        portrait(&seen),
        "bob sees the phone's picture lying down: {seen}"
    );
    assert!(
        portrait(&own),
        "the phone's own preview is lying down: {own}"
    );
    if let Some(path) = bob
        .screenshot("phone-camera-on-desktop")
        .await
        .expect("screenshot")
    {
        eprintln!("screenshot: {}", path.display());
    }
    shot(&mut phone, "android-camera-on").await;

    phone
        .tap(".call-screen .camera-button")
        .await
        .expect("camera off");
    bob.wait_for_js(
        "return !document.querySelector('.call-stage .video-tile');",
        DEFAULT_WAIT,
    )
    .await
    .expect("off again for bob");
    phone.tap(".call-screen .hang-up").await.expect("hang up");
    bob.click(".voice-panel .hang-up")
        .await
        .expect("bob hangs up");
}

/// Seconds in a CSS time list's first entry ("0.24s, 0.24s" → 0.24).
fn first_seconds(value: &serde_json::Value) -> f64 {
    let text = value.as_str().unwrap_or_default();
    let first = text.split(',').next().unwrap_or_default().trim();
    first
        .strip_suffix("ms")
        .and_then(|ms| ms.parse::<f64>().ok().map(|ms| ms / 1000.0))
        .or_else(|| first.strip_suffix('s').and_then(|s| s.parse().ok()))
        .unwrap_or(f64::MAX)
}

/// Swipes feel quick: panes slide in 180 ms, a swipe let go finishes in
/// 140 ms, and sheets rise in 150 ms (they were 240, 180 and 180–220).
#[tokio::test]
async fn phone_swipe_animations_are_quick() {
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
    app.wait_for_text(".room", "general", DEFAULT_WAIT)
        .await
        .expect("general listed");
    app.tap_text(".room", "general")
        .await
        .expect("open general");
    pane(&mut app, "chat").await;

    // The panes' slide.
    let slide = app
        .eval("return getComputedStyle(document.querySelector('.chat')).transitionDuration;")
        .await
        .expect("slide");
    assert!(first_seconds(&slide) <= 0.18, "panes slide in {slide}");

    // A swipe back let go: how long the pane takes to finish going.
    app.eval(
        "window.__settle = null; const chat = document.querySelector('.chat'); \
         const look = () => { const t = chat.style.transition; \
           if (t && t.includes('transform')) window.__settle = t; else requestAnimationFrame(look); }; \
         requestAnimationFrame(look); return true;",
    )
    .await
    .expect("watch");
    let (width, height) = app.page_size().await.expect("size");
    app.drag_and_hold((width * 0.2, height / 2.0), (width * 0.7, height / 2.0))
        .await
        .expect("drag");
    app.lift((width * 0.7, height / 2.0)).await.expect("lift");
    app.wait_for_js(
        "return window.__settle !== null && !window.__settle.includes('none');",
        DEFAULT_WAIT,
    )
    .await
    .expect("the swipe finishes");
    let settle = app.eval("return window.__settle;").await.expect("settle");
    let settle_s = settle
        .as_str()
        .unwrap_or_default()
        .split_whitespace()
        .find_map(|w| w.strip_suffix("ms").and_then(|ms| ms.parse::<f64>().ok()))
        .unwrap_or(f64::MAX)
        / 1000.0;
    assert!(settle_s <= 0.14, "a let-go swipe finishes in {settle}");
    pane(&mut app, "rooms").await;

    // A menu sheet's rise.
    app.eval(HOLD_ROOM_JS).await.expect("hold a room");
    app.wait_for(".context-menu.sheet", DEFAULT_WAIT)
        .await
        .expect("the room menu");
    let rise = app
        .eval("return getComputedStyle(document.querySelector('.context-menu.sheet')).animationDuration;")
        .await
        .expect("rise");
    assert!(first_seconds(&rise) <= 0.15, "sheets rise in {rise}");
}

/// Tapping a server in the rail shows no square: the web view's tap
/// highlight (a rectangle, whatever the shape) and a focus ring are off;
/// the selected pill is the feedback.
#[tokio::test]
async fn tapping_a_server_flashes_no_square() {
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
    app.wait_for(".rail-item[title=\"Engineering\"]", DEFAULT_WAIT)
        .await
        .expect("a server in the rail");
    app.tap(".rail-item[title=\"Engineering\"]")
        .await
        .expect("tap the server");
    app.wait_for(".rail-item.selected[title=\"Engineering\"]", DEFAULT_WAIT)
        .await
        .expect("selected");
    let looks = app
        .eval("const el = document.querySelector('.rail-item[title=\"Engineering\"]'); const s = getComputedStyle(el); \
               return [s.webkitTapHighlightColor, s.outlineStyle === 'none' || s.outlineWidth === '0px'];")
        .await
        .expect("styles");
    assert_eq!(
        looks,
        serde_json::json!(["rgba(0, 0, 0, 0)", true]),
        "no tap highlight, no focus ring"
    );
}

/// Quitting the app completely (swiped away, or stopped) and starting it
/// again opens the chat you were in, in front, not the room list.
#[tokio::test]
async fn phone_reopens_the_last_chat_after_quitting() {
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
    app.open_room("random").await.expect("open random");
    pane(&mut app, "chat").await;
    app.wait_for_text(".chat-header h2", "random", DEFAULT_WAIT)
        .await
        .expect("in #random");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Stopped completely (`relaunch` force-stops it), then started again.
    let mut app = app.relaunch().await.expect("start again");
    app.wait_for(".shell", DEFAULT_WAIT)
        .await
        .expect("signed in still");
    app.wait_for_js(
        "return document.querySelector('.shell').dataset.pane === 'chat'",
        DEFAULT_WAIT,
    )
    .await
    .expect("the chat in front after starting again");
    app.wait_for_text(".chat-header h2", "random", DEFAULT_WAIT)
        .await
        .expect("back in #random");
}

/// Open a room through the switcher (Ctrl+K), which shows a voice channel
/// without joining its call (tapping its row joins), so a test can start or
/// join from the call button. Calls belong to voice channels and DMs.
async fn open_without_joining(app: &mut App, room: &str) {
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    app.key_combo("body", "k", true, false, false)
        .await
        .expect("ctrl+k");
    app.wait_for(".switcher input", DEFAULT_WAIT)
        .await
        .expect("switcher");
    app.type_into(".switcher input", room)
        .await
        .expect("filter");
    app.wait_for_text(".switcher-item.active", room, DEFAULT_WAIT)
        .await
        .expect("room offered");
    app.press(".switcher input", "Enter").await.expect("pick");
    app.wait_for_text(".chat-header h2", room, DEFAULT_WAIT)
        .await
        .expect("room open");
}

/// Change avatar on a phone opens the system's picker, and the picture
/// picked there becomes your avatar.
#[tokio::test]
async fn phone_changes_the_avatar_through_the_system_picker() {
    if !enabled() {
        eprintln!("MORPHEUS_E2E_ANDROID not set; skipping the Android test");
        return;
    }
    // A picture to pick, in the phone's Downloads.
    let png: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8,
        0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    let name = format!("avatar-{}.png", std::process::id());
    let local = std::env::temp_dir().join(&name);
    std::fs::write(&local, png).expect("picture");
    let _ = std::process::Command::new("adb")
        .args([
            "push",
            &local.to_string_lossy(),
            &format!("/sdcard/Download/{name}"),
        ])
        .status();
    let _ = adb_shell(&[
        "am",
        "broadcast",
        "-a",
        "android.intent.action.MEDIA_SCANNER_SCAN_FILE",
        "-d",
        &format!("file:///sdcard/Download/{name}"),
    ]);

    let mut app = App::launch_android().await.expect("launch on Android");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", "carol")
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", "carol-password")
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    app.tap(".user-pill .profile-chip")
        .await
        .expect("account menu");
    app.tap_text(".context-item", "Edit profile")
        .await
        .expect("edit profile");
    app.wait_for(".profile-editor", DEFAULT_WAIT)
        .await
        .expect("editor");
    app.tap_text(".profile-editor button", "Change avatar")
        .await
        .expect("change avatar");

    // The system picker comes up; pick the picture by its name.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let spot = loop {
        let _ = adb_shell(&["uiautomator", "dump", "/sdcard/morpheus-ui.xml"]);
        let screen = adb_shell(&["cat", "/sdcard/morpheus-ui.xml"]);
        // By its name (a file picker), or the newest photo (Android's photo
        // picker shows pictures without names; ours was just added).
        let found = screen
            .split("<node ")
            .find(|n| n.contains(&name) || n.contains("content-desc=\"Photo"))
            .and_then(|node| {
                let raw = node.split("bounds=\"").nth(1)?.split('"').next()?;
                let n: Vec<i64> = raw
                    .split(|c: char| !c.is_ascii_digit())
                    .filter(|p| !p.is_empty())
                    .filter_map(|p| p.parse().ok())
                    .collect();
                Some(((n.first()? + n.get(2)?) / 2, (n.get(1)? + n.get(3)?) / 2))
            });
        if let Some(spot) = found {
            break spot;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no system picker showing {name}"
        );
        // The picker may open on Recent: look in Downloads.
        if let Some(downloads) = screen
            .split("<node ")
            .find(|n| n.contains("text=\"Downloads\""))
            && let Some(raw) = downloads
                .split("bounds=\"")
                .nth(1)
                .and_then(|s| s.split('"').next())
        {
            let n: Vec<i64> = raw
                .split(|c: char| !c.is_ascii_digit())
                .filter(|p| !p.is_empty())
                .filter_map(|p| p.parse().ok())
                .collect();
            if n.len() == 4 {
                let _ = adb_shell(&[
                    "input",
                    "tap",
                    &((n[0] + n[2]) / 2).to_string(),
                    &((n[1] + n[3]) / 2).to_string(),
                ]);
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    };
    let _ = adb_shell(&["input", "tap", &spot.0.to_string(), &spot.1.to_string()]);
    app.wait_for_text(".profile-editor", "Avatar updated", DEFAULT_WAIT)
        .await
        .expect("the picked picture is the avatar");
}

/// Swiping left across the profile card at the bottom goes to the DMs.
#[tokio::test]
async fn phone_swiping_the_profile_card_left_goes_to_the_dms() {
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
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    app.tap(".rail-item[title=\"Rooms\"]").await.expect("rooms");
    app.wait_for(".rail-item.selected[title=\"Rooms\"]", DEFAULT_WAIT)
        .await
        .expect("on Rooms");
    let card = app
        // Along its upper part: the bottom edge of the screen is Android's own
        // (a sideways swipe there switches apps).
        .eval("const r = document.querySelector('.user-pill .user-row').getBoundingClientRect(); return [r.left, r.right, r.top + 3];")
        .await
        .expect("card");
    let (left, right, y) = (
        card[0].as_f64().expect("left"),
        card[1].as_f64().expect("right"),
        card[2].as_f64().expect("y"),
    );
    let inset = (right - left) * 0.25;
    app.swipe((right - inset, y), (left + inset, y), 200)
        .await
        .expect("swipe left across the card");
    app.wait_for(
        ".rail-item.selected[title=\"Direct messages\"]",
        DEFAULT_WAIT,
    )
    .await
    .expect("in the DMs");
}
