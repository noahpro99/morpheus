//! The GPUI app's bridge answers selector requests: the sign-in heading
//! is found, and its text and count come back. Runs only with
//! `just e2e` (the desktop app built with the e2e feature); needs no homeserver.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

#[tokio::test]
async fn the_gpui_app_serves_tagged_elements() {
    if !morpheus_e2e::gpui() {
        eprintln!("run against the web or a phone; skipping");
        return;
    }
    // No homeserver: the app does not talk to one yet.
    let mut app = App::launch("http://127.0.0.1:1").await.expect("launch app");
    app.wait_for(".app form.login h1", DEFAULT_WAIT)
        .await
        .expect("sign-in heading");
    assert_eq!(
        app.text(".app form.login h1")
            .await
            .expect("text")
            .as_deref(),
        Some("Welcome back")
    );
    assert_eq!(app.count(".app form.login h1").await.expect("count"), 1);
    assert_eq!(app.count(".nothing-like-this").await.expect("count"), 0);
    // A selector outside the subset is an error, not a silent miss.
    let err = app.count("h1:first-child").await.expect_err("unsupported");
    assert!(
        format!("{err:#}").contains("unsupported selector"),
        "{err:#}"
    );
    // Raw JavaScript cannot run in the GPUI app.
    let err = app.eval("return 1").await.expect_err("no js");
    assert!(format!("{err:#}").contains("porting"), "{err:#}");
}
