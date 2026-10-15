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
    assert!(
        !app.eval(&visible(".chat"))
            .await
            .expect("eval")
            .as_bool()
            .unwrap_or(true),
        "chat hidden on the list pane"
    );
    assert!(
        app.eval(&visible(".sidebar"))
            .await
            .expect("eval")
            .as_bool()
            .unwrap_or(false),
        "sidebar visible"
    );

    // Tapping a room brings the chat forward, with a back button.
    app.click_text(".room", "general")
        .await
        .expect("open general");
    pane(&mut app, "chat").await;
    app.wait_for(".chat-header .pane-back", DEFAULT_WAIT)
        .await
        .expect("back button");
    assert!(
        !app.eval(&visible(".sidebar"))
            .await
            .expect("eval")
            .as_bool()
            .unwrap_or(true),
        "sidebar hidden on the chat pane"
    );
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
