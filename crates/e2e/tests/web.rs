//! The web app (app.morpheuschat.org): Morpheus in a browser tab. No window
//! bar (the browser has its own), and a way to the desktop app always in
//! view. Runs with `MORPHEUS_E2E_WEB=1` (`just e2e-web`).

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    if !morpheus_e2e::web::enabled() {
        eprintln!("MORPHEUS_E2E_WEB not set; skipping web test");
        return None;
    }
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn sign_in(app: &mut App, user: &str) {
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

/// The "Get Morpheus Desktop" button: at the top, inside the page, with its
/// icon, linking to the download page.
const DESKTOP_LINK: &str = "const a = document.querySelector('.get-desktop'); if (!a) return 'missing'; \
     const r = a.getBoundingClientRect(); \
     if (r.width === 0 || r.top < 0 || r.top > 60 || r.right > innerWidth || r.left < 0) return 'out of view: ' + JSON.stringify(r); \
     if (!a.querySelector('svg')) return 'no icon'; \
     if (a.textContent.trim() !== 'Get Morpheus Desktop') return 'says ' + a.textContent.trim(); \
     if (!a.href.startsWith('https://morpheuschat.org/')) return 'goes to ' + a.href; \
     return 'ok';";

#[tokio::test]
async fn the_web_app_has_no_window_bar_and_offers_the_desktop_app() {
    let Some(hs) = homeserver() else { return };
    let mut app = App::launch(&hs).await.expect("launch the web app");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    assert_eq!(
        app.eval(DESKTOP_LINK).await.expect("link"),
        "ok",
        "on the sign-in page"
    );
    sign_in(&mut app, "alice").await;
    assert_eq!(
        app.count(".titlebar, .window-controls")
            .await
            .expect("count"),
        0,
        "no window bar: the browser is the window"
    );
    assert_eq!(
        app.eval(DESKTOP_LINK).await.expect("link"),
        "ok",
        "signed in"
    );
    // Always in view: a dialog open does not cover it.
    app.click(".settings-button").await.expect("settings");
    app.wait_for(".settings", DEFAULT_WAIT)
        .await
        .expect("settings open");
    let on_top = app
        .eval("const a = document.querySelector('.get-desktop'), r = a.getBoundingClientRect(); return document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2)?.closest('.get-desktop') === a;")
        .await
        .expect("on top");
    assert_eq!(on_top, true, "the link is on top of the settings");
}

#[tokio::test]
async fn on_a_phone_sized_page_the_desktop_link_stays_in_view() {
    let Some(hs) = homeserver() else { return };
    let mut app = App::launch_sized(&hs, 400, 800)
        .await
        .expect("launch the web app");
    sign_in(&mut app, "alice").await;
    assert_eq!(app.eval(DESKTOP_LINK).await.expect("link"), "ok");
}

#[tokio::test]
async fn signing_in_and_chatting_work_and_a_reload_stays_signed_in() {
    let Some(hs) = homeserver() else { return };
    let mut app = App::launch(&hs).await.expect("launch the web app");
    sign_in(&mut app, "alice").await;
    app.open_room("general").await.expect("open general");
    app.wait_for_text(".chat-header h2", "general", DEFAULT_WAIT)
        .await
        .expect("in #general");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("the message box ready");
    let text = format!("hello from the web {}", std::process::id());
    app.type_into(".composer .composer-input", &text)
        .await
        .expect("type");
    app.press(".composer .composer-input", "Enter")
        .await
        .expect("send");
    app.wait_for_exact_text(".message .body", &text, DEFAULT_WAIT)
        .await
        .expect("the message is in the room");
    app.eval("location.reload(); return true;")
        .await
        .expect("reload");
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("still signed in after a reload");
}

/// The web bar: the desktop link in the middle, Help and Inbox at the right
/// (and they work), nothing else.
#[tokio::test]
async fn the_web_bar_centres_the_desktop_link_with_help_and_inbox_at_the_right() {
    let Some(hs) = homeserver() else { return };
    let mut app = App::launch(&hs).await.expect("launch the web app");
    sign_in(&mut app, "alice").await;
    let layout = app
        .eval(
            "const a = document.querySelector('.get-desktop').getBoundingClientRect(); \
             const centred = Math.abs(a.left + a.width / 2 - innerWidth / 2) < 4; \
             const at = sel => { const el = document.querySelector(sel); if (!el) return null; const r = el.getBoundingClientRect(); return { top: r.top, bottom: r.bottom, left: r.left, right: r.right }; }; \
             const help = at('.web-actions .help-button'), inbox = at('.web-actions .inbox-button'); \
             const bar = document.querySelector('.web-bar').getBoundingClientRect(); \
             const inBar = r => !!r && r.top >= bar.top && r.bottom <= bar.bottom + 1; \
             return { centred, help: inBar(help) && help.left > innerWidth * 0.75, inbox: inBar(inbox) && inbox.left > innerWidth * 0.75, \
               others: document.querySelectorAll('.web-actions button').length };",
        )
        .await
        .expect("layout");
    assert_eq!(
        layout,
        serde_json::json!({ "centred": true, "help": true, "inbox": true, "others": 2 }),
        "the web bar's layout"
    );
    app.click(".web-actions .inbox-button")
        .await
        .expect("inbox");
    app.wait_for(".inbox-panel", DEFAULT_WAIT)
        .await
        .expect("the inbox opens");
    app.press("body", "Escape").await.expect("close");
    app.click(".web-actions .help-button").await.expect("help");
    app.wait_for(".help-dialog", DEFAULT_WAIT)
        .await
        .expect("help opens");
}

/// Scroll bars in the web app are thin and dark, as the desktop's are, not
/// the browser's grey ones.
#[tokio::test]
async fn web_scroll_bars_are_thin_and_dark() {
    let Some(hs) = homeserver() else { return };
    let mut app = App::launch(&hs).await.expect("launch the web app");
    sign_in(&mut app, "alice").await;
    app.open_room("general").await.expect("open general");
    let style = app
        .eval(
            "return ['.timeline', '.room-list', '.members'].map(sel => document.querySelector(sel)).filter(Boolean).map(el => { const s = getComputedStyle(el); return [s.scrollbarWidth, s.scrollbarColor]; });",
        )
        .await
        .expect("style");
    let style = style.as_array().expect("array").clone();
    assert!(!style.is_empty(), "scroll areas found");
    for area in style {
        assert_eq!(area[0], "thin", "thin: {area}");
        assert_ne!(area[1], "auto", "dark, not the browser's: {area}");
    }
}
