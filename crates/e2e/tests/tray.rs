//! Closing the window keeps Morpheus running (it lives on in the tray):
//! the process stays, messages that arrive meanwhile are announced, and the
//! window comes back. A hidden webview suspends its JavaScript, so the
//! driver reopens the window through the bridge before looking at the DOM.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login(app: &mut App, user: &str) {
    app.wait_for("form.login", DEFAULT_WAIT).await.expect("login form");
    app.type_into("form.login input[name=username]", user).await.expect("username");
    app.type_into("form.login input[name=password]", &format!("{user}-password")).await.expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms listed");
}

#[tokio::test]
async fn closing_the_window_keeps_syncing_and_announcing() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login(&mut bob, "bob").await;
    bob.open_room("random").await.expect("bob in #random");

    // Close through the title bar: the window hides, the process stays.
    // Deferred: the webview stops answering the moment it hides, so the
    // eval must return before the click lands.
    bob.eval("setTimeout(() => document.querySelector('.titlebar button.close').click(), 50); return true;").await.expect("close");
    eprintln!("[tray] closed");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    assert!(bob.is_running(), "closing the window must not quit the app");

    let mut alice = App::launch(&hs).await.expect("launch alice");
    login(&mut alice, "alice").await;
    alice.open_room("general").await.expect("alice in #general");
    alice.wait_for(".composer input:not([disabled])", DEFAULT_WAIT).await.expect("composer ready");
    let text = format!("tray ping {}", std::process::id());
    alice.type_into(".composer input", &text).await.expect("type");
    alice.press(".composer input", "Enter").await.expect("send");
    alice.wait_for_exact_text(".message.own:not(.pending) .body", &text, DEFAULT_WAIT).await.expect("sent");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Reopen (what the tray does) and find the message announced meanwhile.
    eprintln!("[tray] showing");
    bob.show_window().await.expect("show window");
    eprintln!("[tray] shown");
    bob.wait_for_text(".notification-log li", &text, DEFAULT_WAIT).await.expect("announced while hidden");
    bob.wait_for(".composer input", DEFAULT_WAIT).await.expect("still usable");
}
