//! The window has no system title bar on any desktop (the app draws its
//! own), so every screen needs one of the app's: the sign-in screen too,
//! with somewhere to drag the window and its minimize, maximize and close.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

#[tokio::test]
async fn the_sign_in_screen_has_a_bar_to_drag_and_close() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("sign-in screen");
    let bar = app
        .bounds("header.titlebar")
        .await
        .expect("read the bar")
        .expect("the sign-in screen has a title bar");
    let page = app
        .bounds("body")
        .await
        .expect("the page")
        .expect("page bounds");
    assert_eq!(bar.y, 0.0, "at the top: {bar:?}");
    assert!(
        bar.h >= 24.0 && (bar.w - page.w).abs() < 1.0,
        "across the window, tall enough to grab: {bar:?} in {page:?}"
    );
    let controls = app
        .attrs("header.titlebar .window-controls button", "title")
        .await
        .expect("the buttons");
    assert_eq!(
        controls,
        vec![
            Some("Minimize".to_owned()),
            Some("Maximize".to_owned()),
            Some("Close".to_owned())
        ],
        "with the window's buttons"
    );
}
