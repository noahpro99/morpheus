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
        .eval(
            "const bar = document.querySelector('header.titlebar');\
             if (!bar) return null;\
             const r = bar.getBoundingClientRect();\
             return { top: r.top, height: r.height, width: r.width, page: innerWidth,\
               controls: [...bar.querySelectorAll('.window-controls button')].map(b => b.title) };",
        )
        .await
        .expect("read the bar");
    assert!(!bar.is_null(), "the sign-in screen has a title bar");
    assert_eq!(bar["top"], 0.0, "at the top: {bar}");
    assert!(
        bar["height"].as_f64().unwrap_or(0.0) >= 24.0
            && bar["width"].as_f64() == bar["page"].as_f64(),
        "across the window, tall enough to grab: {bar}"
    );
    assert_eq!(
        bar["controls"],
        serde_json::json!(["Minimize", "Maximize", "Close"]),
        "with the window's buttons"
    );
}
