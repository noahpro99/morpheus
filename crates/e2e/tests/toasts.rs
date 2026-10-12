//! Failed background actions surface as toasts (bottom-right), which
//! dismiss on click.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

#[tokio::test]
async fn a_forbidden_pin_shows_a_toast_that_dismisses_on_click() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    // bob is a plain member of #general: pinning needs moderator power.
    app.type_into("form.login input[name=username]", "bob")
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", "bob-password")
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("rooms listed");
    app.open_room("general").await.expect("open general");
    app.wait_for(".composer input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");

    // Pin a message of bob's own, once it has an event id.
    let text = format!("toast {}", std::process::id());
    app.type_into(".composer input", &text).await.expect("type");
    app.press(".composer input", "Enter").await.expect("send");
    app.wait_for_exact_text(".message.own:not(.pending) .body", &text, DEFAULT_WAIT)
        .await
        .expect("message sent");
    let row = format!(
        "[...document.querySelectorAll('.message.own')].find(m => m.querySelector('.body')?.textContent === {})",
        serde_json::to_string(&text).expect("json")
    );
    app.eval(&format!(
        "{row}.dispatchEvent(new MouseEvent('contextmenu', {{ bubbles: true, cancelable: true, clientX: 420, clientY: 320 }}));"
    ))
    .await
    .expect("right-click");
    app.wait_for_text(".context-item", "Pin message", DEFAULT_WAIT)
        .await
        .expect("menu built");
    app.click_text(".context-item", "Pin message")
        .await
        .expect("pin");
    app.wait_for_text(".toast", "Pin failed", DEFAULT_WAIT)
        .await
        .expect("toast shown");
    app.click(".toast").await.expect("dismiss");
    app.wait_for_js("return !document.querySelector('.toast')", DEFAULT_WAIT)
        .await
        .expect("toast gone");
}
