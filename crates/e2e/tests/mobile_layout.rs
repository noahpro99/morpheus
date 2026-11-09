//! Phone-width layout: one pane at a time (rooms, chat, side), with back
//! buttons and Escape acting as back. Driven on the desktop build with a
//! phone-sized window, which the mobile app shares.

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
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("rooms listed");
}

fn visible(selector: &str) -> String {
    format!(
        "const el = document.querySelector({}); return !!el && el.offsetParent !== null;",
        serde_json::to_string(selector).expect("json")
    )
}

/// Which pane is in front at the middle of the app: "chat", "side" or "list".
/// Panes stack on a phone, so "shown" means on top, not merely present.
const FRONT_JS: &str = "const s = document.querySelector('.shell').getBoundingClientRect(); \
    const el = document.elementFromPoint(s.left + s.width * 0.75, s.top + s.height / 2); \
    if (!el) return null; \
    if (el.closest('.side-pane')) return 'side'; \
    if (el.closest('.chat')) return 'chat'; \
    if (el.closest('.sidebar, .rail')) return 'list'; \
    return el.className;";

async fn front(app: &mut App, expected: &str) {
    app.wait_for_js(
        &format!("return ({{ f: () => {{ {FRONT_JS} }} }}).f() === '{expected}';"),
        DEFAULT_WAIT,
    )
    .await
    .unwrap_or_else(|e| panic!("{expected} in front: {e}"));
}
async fn pane(app: &mut App, expected: &str) {
    app.wait_for_js(
        &format!("return document.querySelector('.shell').dataset.pane === '{expected}'"),
        DEFAULT_WAIT,
    )
    .await
    .unwrap_or_else(|e| panic!("pane {expected}: {e}"));
}

#[tokio::test]
async fn narrow_window_shows_one_pane_at_a_time() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_sized(&hs, 420, 800)
        .await
        .expect("launch phone-sized app");
    login(&mut app, "alice").await;
    app.wait_for(".shell.narrow", DEFAULT_WAIT)
        .await
        .expect("narrow layout");

    // Starts on the room list; the chat is not shown.
    pane(&mut app, "rooms").await;
    front(&mut app, "list").await;
    // The chat waits off to the right, ready to slide in over the list.
    app.wait_for_js(
        // The transform, not the measured box: WebKitGTK under fractional
        // display scaling reports transformed boxes scaled.
        "const c = document.querySelector('.chat'), cs = getComputedStyle(c); \
         return new DOMMatrix(cs.transform).m41 >= c.offsetWidth - 1 && cs.position === 'absolute' && parseFloat(cs.transitionDuration) > 0;",
        DEFAULT_WAIT,
    )
    .await
    .expect("chat parked to the right, layered, with a slide transition");

    // Menus are full-width bottom sheets at phone width.
    app.right_click(".room").await.expect("right-click a room");
    app.wait_for_js(
        "const m = document.querySelector('.context-menu.sheet'); if (!m) return false; const r = m.getBoundingClientRect(), s = document.querySelector('.shell').getBoundingClientRect(); return Math.abs(r.left - s.left) < 1 && Math.abs(r.right - s.right) < 1 && Math.abs(r.bottom - s.bottom) < 2;",
        DEFAULT_WAIT,
    )
    .await
    .expect("the room menu is a bottom sheet");
    app.key_combo("body", "Escape", false, false, false)
        .await
        .expect("close the sheet");
    app.wait_for_js(
        "return !document.querySelector('.context-menu');",
        DEFAULT_WAIT,
    )
    .await
    .expect("sheet closed");

    // Tapping a room brings the chat forward, with a back button.
    app.click_text(".room", "general")
        .await
        .expect("open general");
    pane(&mut app, "chat").await;
    app.wait_for(".chat-header .pane-back", DEFAULT_WAIT)
        .await
        .expect("back button");
    // Slid over the list, which stays underneath.
    front(&mut app, "chat").await;
    app.wait_for_js(
        "const c = document.querySelector('.chat'); \
         return new DOMMatrix(getComputedStyle(c).transform).m41 === 0 && !!document.querySelector('.sidebar');",
        DEFAULT_WAIT,
    )
    .await
    .expect("chat over the list");
    // The member list waits just off the right edge, for a leftward swipe.
    app.wait_for_js(
        "const p = document.querySelector('.side-pane.parked'); if (!p) return false; \
         return new DOMMatrix(getComputedStyle(p).transform).m41 >= p.offsetWidth - 1 && !!p.querySelector('.members');",
        DEFAULT_WAIT,
    )
    .await
    .expect("members parked beside the chat");
    app.click(".chat-header .pane-back").await.expect("back");
    pane(&mut app, "rooms").await;

    // The member list is its own pane with a back bar.
    app.click_text(".room", "general")
        .await
        .expect("open general again");
    pane(&mut app, "chat").await;
    app.click(".chat-header .member-toggle")
        .await
        .expect("members");
    pane(&mut app, "side").await;
    app.wait_for(".side-pane .members", DEFAULT_WAIT)
        .await
        .expect("members pane");
    // Shown, not just present: a medium-width rule once hid it here.
    app.wait_for_js(&visible(".side-pane .member"), DEFAULT_WAIT)
        .await
        .expect("members visible");
    app.click(".side-pane .pane-back")
        .await
        .expect("back from members");
    pane(&mut app, "chat").await;

    // Escape acts as the back key.
    app.key_combo("body", "Escape", false, false, false)
        .await
        .expect("escape");
    pane(&mut app, "rooms").await;

    if let Some(path) = app.screenshot("mobile-rooms").await.expect("screenshot") {
        eprintln!("screenshot: {}", path.display());
    }
}

#[tokio::test]
async fn wide_window_keeps_every_column() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login(&mut app, "alice").await;
    app.open_room("general").await.expect("open general");
    app.wait_for_js(
        "return document.querySelector('.shell').dataset.pane === 'all'",
        DEFAULT_WAIT,
    )
    .await
    .expect("all panes");
    assert_eq!(app.count(".shell.narrow").await.expect("count"), 0);
    assert_eq!(
        app.count(".pane-back").await.expect("count"),
        0,
        "no back buttons on a desktop"
    );
}

/// However many servers, the profile card stays on screen at the bottom of
/// a phone: the server rail scrolls rather than pushing it off.
#[tokio::test]
async fn the_profile_card_stays_on_screen_with_many_servers() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    // Short, so alice's servers fill the rail.
    let mut app = App::launch_sized(&hs, 400, 400).await.expect("launch");
    login(&mut app, "alice").await;
    app.wait_for(".user-pill", DEFAULT_WAIT)
        .await
        .expect("card");
    let at = app
        .eval(
            "const p = document.querySelector('.user-pill').getBoundingClientRect(); \
             const s = document.querySelector('.shell').getBoundingClientRect(); \
             return [Math.round(p.top), Math.round(p.bottom), Math.round(s.bottom)];",
        )
        .await
        .expect("where");
    let (top, bottom, end) = (
        at[0].as_f64().unwrap_or(0.0),
        at[1].as_f64().unwrap_or(0.0),
        at[2].as_f64().unwrap_or(0.0),
    );
    assert!(
        top < end && bottom <= end + 1.0,
        "the card is off the bottom: {at}"
    );
}
