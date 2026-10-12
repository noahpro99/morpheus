//! Focus and placement details: typing anywhere goes to the message box,
//! starting an edit focuses it, the command palette runs actions, the
//! profile card opens beside the avatar on the side nearer the middle, and
//! the user pill keeps the message box's margin.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login_and_open(app: &mut App, room: &str) {
    app.wait_for("form.login", DEFAULT_WAIT).await.expect("login form");
    app.type_into("form.login input[name=username]", "alice").await.expect("username");
    app.type_into("form.login input[name=password]", "alice-password").await.expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms listed");
    app.open_room(room).await.expect("open room");
    app.wait_for(".composer input:not([disabled])", DEFAULT_WAIT).await.expect("composer ready");
}

const COMPOSER_FOCUSED: &str = "return document.activeElement === document.querySelector('.composer input')";

#[tokio::test]
async fn typing_and_editing_focus_the_message_box() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;

    // Typing with nothing focused lands in the box.
    app.eval("document.activeElement.blur(); document.body.focus();").await.expect("blur");
    app.eval("document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'h', bubbles: true }))").await.expect("type h");
    app.wait_for_js(COMPOSER_FOCUSED, DEFAULT_WAIT).await.expect("composer focused by typing");

    // Editing a message of your own focuses the box with the text in it.
    let text = format!("focus {}", std::process::id());
    app.type_into(".composer input", &text).await.expect("type");
    app.press(".composer input", "Enter").await.expect("send");
    app.wait_for_exact_text(".message.own:not(.pending) .body", &text, DEFAULT_WAIT).await.expect("sent");
    app.eval("document.activeElement.blur()").await.expect("blur");
    let row = format!(
        "[...document.querySelectorAll('.message.own')].find(m => m.querySelector('.body')?.textContent === {})",
        serde_json::to_string(&text).expect("json")
    );
    app.eval(&format!("{row}.dispatchEvent(new MouseEvent('contextmenu', {{ bubbles: true, cancelable: true, clientX: 420, clientY: 320 }}));")).await.expect("menu");
    app.wait_for_text(".context-item", "Edit", DEFAULT_WAIT).await.expect("menu built");
    app.click_text(".context-item", "Edit").await.expect("edit");
    app.wait_for_text(".composer-banner", "Editing", DEFAULT_WAIT).await.expect("editing");
    app.wait_for_js(COMPOSER_FOCUSED, DEFAULT_WAIT).await.expect("composer focused for the edit");
    app.wait_for_js(&format!("return document.querySelector('.composer input').value === {}", serde_json::to_string(&text).expect("json")), DEFAULT_WAIT).await.expect("draft prefilled");
    app.press(".composer input", "Escape").await.expect("cancel edit");
}

#[tokio::test]
async fn palette_runs_settings_and_pill_matches_composer_margin() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;

    app.key_combo("body", "k", true, false, false).await.expect("ctrl+k");
    app.wait_for(".switcher input", DEFAULT_WAIT).await.expect("palette");
    app.type_into(".switcher input", "sett").await.expect("filter");
    app.wait_for_text(".switcher-item.active", "Settings", DEFAULT_WAIT).await.expect("settings offered");
    app.press(".switcher input", "Enter").await.expect("pick");
    app.wait_for(".settings", DEFAULT_WAIT).await.expect("settings opened");
    app.key_combo("body", "Escape", false, false, false).await.expect("close settings");
    app.wait_for_js("return !document.querySelector('.settings')", DEFAULT_WAIT).await.expect("settings closed");

    // The pill's right margin equals the message box's left padding.
    let gaps = app
        .eval("const bar = document.querySelector('.user-bar'), c = document.querySelector('.composer'); \
               return [parseFloat(getComputedStyle(bar).paddingRight), parseFloat(getComputedStyle(c).paddingLeft)];")
        .await
        .expect("gaps");
    let gaps = gaps.as_array().expect("array");
    assert_eq!(gaps[0], gaps[1], "pill right gap {} vs composer left gap {}", gaps[0], gaps[1]);
}

#[tokio::test]
async fn profile_card_opens_beside_the_avatar_towards_the_middle() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    // Wide enough for the member list (a tiled test window is narrower).
    let mut app = App::launch_sized(&hs, 1280, 800).await.expect("launch app");
    login_and_open(&mut app, "general").await;

    // A member row sits at the right edge: the card opens to its left.
    app.wait_for(".member", DEFAULT_WAIT).await.expect("members");
    app.click(".member").await.expect("open card from the member list");
    app.wait_for(".profile-card", DEFAULT_WAIT).await.expect("card");
    app.wait_for_js(
        "const c = document.querySelector('.profile-card').getBoundingClientRect(), m = document.querySelector('.member').getBoundingClientRect(); \
         return c.left > 0 && c.right <= m.left + 1 && c.bottom <= innerHeight && c.top >= 0;",
        DEFAULT_WAIT,
    )
    .await
    .expect("card left of the member row, inside the window");
    app.key_combo("body", "Escape", false, false, false).await.expect("close card");
    app.wait_for_js("return !document.querySelector('.profile-card')", DEFAULT_WAIT).await.expect("card closed");

    // A sender avatar in the timeline sits at the left: the card opens to its right.
    app.click(".message:not(.system) .avatar-button").await.expect("open card from a message");
    app.wait_for(".profile-card", DEFAULT_WAIT).await.expect("card");
    app.wait_for_js(
        "const c = document.querySelector('.profile-card').getBoundingClientRect(), a = document.querySelector('.message:not(.system) .avatar-button').getBoundingClientRect(); \
         return c.left >= a.right - 1 && c.right <= innerWidth && c.bottom <= innerHeight;",
        DEFAULT_WAIT,
    )
    .await
    .expect("card right of the avatar, inside the window");
}
