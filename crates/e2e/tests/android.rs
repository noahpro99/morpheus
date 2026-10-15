//! The mobile app on an Android emulator (or a plugged-in phone): sign in,
//! move between the single-column panes, send a message, and read the
//! member list. Skipped unless `MORPHEUS_E2E_ANDROID=1`; `just e2e-android`
//! builds the app with the test bridge, installs it and runs this.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn enabled() -> bool {
    std::env::var_os("MORPHEUS_E2E_ANDROID").is_some()
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
    app.click(".chat-header .member-toggle")
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
