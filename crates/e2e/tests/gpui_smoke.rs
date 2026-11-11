//! The GPUI app's bridge answers selector requests: the tagged brand label
//! is found, and its text and count come back. Runs only with
//! `MORPHEUS_E2E_UI=gpui` and `MORPHEUS_E2E_BIN` set to a `morpheus-gpui`
//! built with `--features e2e`; needs no homeserver.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

#[tokio::test]
async fn the_gpui_app_serves_tagged_elements() {
    if std::env::var("MORPHEUS_E2E_UI").as_deref() != Ok("gpui") {
        eprintln!("MORPHEUS_E2E_UI is not gpui; skipping");
        return;
    }
    // No homeserver: the app does not talk to one yet.
    let mut app = App::launch("http://127.0.0.1:1").await.expect("launch app");
    app.wait_for(".app h1.brand", DEFAULT_WAIT)
        .await
        .expect("brand label");
    assert_eq!(
        app.text(".app h1.brand").await.expect("text").as_deref(),
        Some("Morpheus")
    );
    assert_eq!(app.count(".app h1.brand").await.expect("count"), 1);
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
