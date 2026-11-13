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
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
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
    app.open_room(room).await.expect("open room");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
}

#[tokio::test]
async fn typing_and_editing_focus_the_message_box() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;

    // Typing with nothing focused lands in the box.
    app.unfocus().await.expect("blur");
    app.key_combo("body", "h", false, false, false)
        .await
        .expect("type h");
    app.wait_for_focus(".composer .composer-input", DEFAULT_WAIT)
        .await
        .expect("composer focused by typing");

    // Editing a message of your own focuses the box with the text in it.
    let text = format!("focus {}", std::process::id());
    app.type_into(".composer .composer-input", &text)
        .await
        .expect("type");
    app.press(".composer .composer-input", "Enter")
        .await
        .expect("send");
    app.wait_for_exact_text(".message.own:not(.pending) .body", &text, DEFAULT_WAIT)
        .await
        .expect("sent");
    app.unfocus().await.expect("blur");
    app.right_click_exact(".message.own .body", &text)
        .await
        .expect("menu");
    app.wait_for_text(".context-item", "Edit", DEFAULT_WAIT)
        .await
        .expect("menu built");
    app.click_text(".context-item", "Edit").await.expect("edit");
    app.wait_for_text(".composer-banner", "Editing", DEFAULT_WAIT)
        .await
        .expect("editing");
    app.wait_for_focus(".composer .composer-input", DEFAULT_WAIT)
        .await
        .expect("composer focused for the edit");
    app.wait_for_value(".composer .composer-input", &text, DEFAULT_WAIT)
        .await
        .expect("draft prefilled");
    app.press(".composer .composer-input", "Escape")
        .await
        .expect("cancel edit");
}

#[tokio::test]
async fn palette_runs_settings_and_pill_matches_composer_margin() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;

    app.key_combo("body", "k", true, false, false)
        .await
        .expect("ctrl+k");
    app.wait_for(".switcher input", DEFAULT_WAIT)
        .await
        .expect("palette");
    app.type_into(".switcher input", "sett")
        .await
        .expect("filter");
    app.wait_for_text(".switcher-item.active", "Settings", DEFAULT_WAIT)
        .await
        .expect("settings offered");
    app.press(".switcher input", "Enter").await.expect("pick");
    app.wait_for(".settings", DEFAULT_WAIT)
        .await
        .expect("settings opened");
    app.key_combo("body", "Escape", false, false, false)
        .await
        .expect("close settings");
    app.wait_for_count(".settings", 0, DEFAULT_WAIT)
        .await
        .expect("settings closed");

    // The pill's right margin equals the message box's left padding.
    let bar = app.bounds(".user-bar").await.expect("bar").expect("the user bar");
    let pill = app.bounds(".user-pill").await.expect("pill").expect("the pill");
    let composer = app.bounds(".composer").await.expect("composer").expect("the composer");
    let row = app
        .bounds(".composer .composer-row")
        .await
        .expect("row")
        .expect("the box's row");
    let (right, left) = (bar.right() - pill.right(), row.x - composer.x);
    assert_eq!(right, left, "pill right gap {right} vs composer left gap {left}");
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
    app.wait_for(".member", DEFAULT_WAIT)
        .await
        .expect("members");
    app.click(".member")
        .await
        .expect("open card from the member list");
    app.wait_for(".profile-card", DEFAULT_WAIT)
        .await
        .expect("card");
    let window = app.bounds("body").await.expect("window").expect("the window");
    let card = app.bounds(".profile-card").await.expect("card").expect("the card");
    let member = app.bounds(".member").await.expect("member").expect("the member row");
    assert!(
        card.x > 0.0 && card.right() <= member.centre_x() + 1.0 && card.bottom() <= window.h && card.y >= 0.0,
        "card left of where the member row was clicked (its middle), inside the window: {card:?} {member:?}"
    );
    app.key_combo("body", "Escape", false, false, false)
        .await
        .expect("close card");
    app.wait_for_count(".profile-card", 0, DEFAULT_WAIT)
        .await
        .expect("card closed");

    // A sender avatar in the timeline sits at the left: the card opens to its right.
    // (Join and settings rows left by other tests can fill the view: scroll to one.)
    app.reveal_element(".message:not(.system) .avatar-button", DEFAULT_WAIT)
        .await
        .expect("an avatar in view");
    app.click(".message:not(.system) .avatar-button")
        .await
        .expect("open card from a message");
    app.wait_for(".profile-card", DEFAULT_WAIT)
        .await
        .expect("card");
    let card = app.bounds(".profile-card").await.expect("card").expect("the card");
    let avatar = app
        .bounds(".message:not(.system) .avatar-button")
        .await
        .expect("avatar")
        .expect("the avatar");
    assert!(
        card.x >= avatar.centre_x() - 1.0 && card.right() <= window.w && card.bottom() <= window.h,
        "card right of the avatar, inside the window: {card:?} {avatar:?}"
    );
}

/// Pasting with nothing focused goes to the message box (focused for you),
/// as typing does.
#[tokio::test]
async fn pasting_anywhere_goes_to_the_message_box() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;
    if app.is_gpui() {
        eprintln!("a text paste with nothing focused is not ported to GPUI (no clipboard in the bridge); skipping");
        return;
    }
    let text = format!("pasted from afar {}", std::process::id());
    let state = app
        .eval(&format!(
            "document.activeElement && document.activeElement.blur(); \
             const dt = new DataTransfer(); dt.setData('text/plain', {}); \
             document.querySelector('.timeline').dispatchEvent(new ClipboardEvent('paste', {{ bubbles: true, cancelable: true, clipboardData: dt }})); \
             await new Promise(r => setTimeout(r, 300)); \
             const box = document.querySelector('.composer .composer-input'); \
             return [box.value, document.activeElement === box];",
            serde_json::to_string(&text).expect("json")
        ))
        .await
        .expect("paste");
    assert_eq!(
        state,
        serde_json::json!([text, true]),
        "the paste landed in the box, focused"
    );
}
