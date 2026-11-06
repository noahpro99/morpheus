//! Keyboard shortcuts (Discord's set): Ctrl+/ for the list, Alt+arrows for
//! rooms and scopes, Alt+Left/Right for history, Ctrl+I/U/P/, for panels,
//! Up in an empty composer to edit, Esc to jump to the newest message.

#![allow(clippy::expect_used)]

use std::time::Duration;

use morpheus_e2e::{App, DEFAULT_WAIT};

/// Holds once the timeline has more history than fits on screen.
const SCROLLABLE: &str = "const t = document.querySelector('.timeline'); return !!t && t.scrollHeight > t.clientHeight + 600;";

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
        .expect("room listed");
    app.open_room(room).await.expect("open room");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
}

async fn header(app: &mut App) -> String {
    app.text(".chat-header h2")
        .await
        .expect("header")
        .unwrap_or_default()
}

#[tokio::test]
async fn shortcuts_navigate_rooms_scopes_history_and_panels() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;

    // Ctrl+/ lists the shortcuts; Esc closes it.
    app.key_combo("body", "/", true, false, false)
        .await
        .expect("ctrl+/");
    app.wait_for_text(".help-dialog", "Keyboard shortcuts", DEFAULT_WAIT)
        .await
        .expect("shortcuts dialog");
    app.press("body", "Escape").await.expect("escape");
    app.wait_for_js(
        "return document.querySelector('.help-dialog') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("closed");

    // Alt+Down / Alt+Up step through the sidebar order.
    let before = header(&mut app).await;
    app.key_combo("body", "ArrowDown", false, true, false)
        .await
        .expect("alt+down");
    app.wait_for_js(
        &format!(
            "return document.querySelector('.chat-header h2')?.textContent !== {};",
            serde_json::to_string(&before).expect("json")
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("moved to the next room");
    app.key_combo("body", "ArrowUp", false, true, false)
        .await
        .expect("alt+up");
    app.wait_for_text(".chat-header h2", &before, DEFAULT_WAIT)
        .await
        .expect("back to the first room");

    // Ctrl+Alt+Down moves the rail scope; Alt+Left goes back in history.
    app.key_combo("body", "ArrowDown", true, true, false)
        .await
        .expect("ctrl+alt+down");
    app.wait_for(".rail-item.selected[title=\"Engineering\"]", DEFAULT_WAIT)
        .await
        .expect("next scope");
    app.wait_for_text(".chat-header h2", "dev", DEFAULT_WAIT)
        .await
        .expect("first room of the space");
    app.key_combo("body", "ArrowLeft", false, true, false)
        .await
        .expect("alt+left");
    app.wait_for_text(".chat-header h2", &before, DEFAULT_WAIT)
        .await
        .expect("history back");
    app.wait_for(".rail-item.selected[title=\"Rooms\"]", DEFAULT_WAIT)
        .await
        .expect("scope followed");

    // Panels: Ctrl+I inbox, Ctrl+U members, Ctrl+P pins, Ctrl+, settings.
    app.key_combo("body", "i", true, false, false)
        .await
        .expect("ctrl+i");
    app.wait_for(".inbox-panel", DEFAULT_WAIT)
        .await
        .expect("inbox");
    app.press("body", "Escape").await.expect("escape");
    app.key_combo("body", "u", true, false, false)
        .await
        .expect("ctrl+u");
    app.wait_for_js(
        "return document.querySelector('.members') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("members hidden");
    app.key_combo("body", "u", true, false, false)
        .await
        .expect("ctrl+u again");
    app.wait_for(".members", DEFAULT_WAIT)
        .await
        .expect("members back");
    app.key_combo("body", "p", true, false, false)
        .await
        .expect("ctrl+p");
    app.wait_for(".pinned-dialog", DEFAULT_WAIT)
        .await
        .expect("pins");
    app.press("body", "Escape").await.expect("escape");
    app.key_combo("body", ",", true, false, false)
        .await
        .expect("ctrl+,");
    app.wait_for(".settings", DEFAULT_WAIT)
        .await
        .expect("settings");
    app.press("body", "Escape").await.expect("escape");
    app.wait_for_js(
        "return document.querySelector('.settings') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("settings closed");
}

#[tokio::test]
async fn up_edits_last_message_and_escape_jumps_to_bottom() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;

    let text = format!("edit me {}", std::process::id());
    app.type_into(".composer .composer-input", &text)
        .await
        .expect("type");
    app.press(".composer .composer-input", "Enter")
        .await
        .expect("send");
    app.wait_for_exact_text(".message.own:not(.pending) .body", &text, DEFAULT_WAIT)
        .await
        .expect("sent");

    // Up in the empty box edits it: the banner shows and the text is back in the box.
    app.key_combo(".composer .composer-input", "ArrowUp", false, false, false)
        .await
        .expect("up");
    app.wait_for_text(".composer-banner", "Editing message", DEFAULT_WAIT)
        .await
        .expect("edit mode");
    app.wait_for_js(
        &format!(
            "return document.querySelector('.composer .composer-input').value === {};",
            serde_json::to_string(&text).expect("json")
        ),
        Duration::from_secs(5),
    )
    .await
    .expect("draft holds the message");
    app.press(".composer .composer-input", "Escape")
        .await
        .expect("cancel edit");

    // Scroll up, then Esc with nothing open jumps back to the newest message.
    // A tall window can show the whole first page; load enough history to scroll.
    app.reveal("a scrollable timeline", SCROLLABLE, DEFAULT_WAIT)
        .await
        .expect("history");
    app.eval("document.querySelector('.timeline').scrollTop = -2000;")
        .await
        .expect("scroll up");
    app.wait_for(".jump-to-bottom", DEFAULT_WAIT)
        .await
        .expect("scrolled away");
    app.press("body", "Escape").await.expect("escape");
    // On a phone Escape is the back key: it returns to the room list instead.
    let phone = app
        .eval("return !!document.querySelector('.shell.narrow');")
        .await
        .expect("layout");
    let expected = if phone == true {
        "return document.querySelector('.shell').dataset.pane === 'rooms';"
    } else {
        "return document.querySelector('.jump-to-bottom') === null;"
    };
    app.wait_for_js(expected, DEFAULT_WAIT)
        .await
        .expect("back at the bottom (desktop) or the room list (phone)");
}

/// Ctrl+R reloads the app, as in Discord: the interface is built afresh
/// (same account, no sign-in) and works.
#[tokio::test]
async fn ctrl_r_reloads_the_app() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "random").await;
    // A mark on the current interface, gone once it is rebuilt.
    app.eval("document.querySelector('.shell').dataset.beforeReload = 'yes'; return true;")
        .await
        .expect("mark");
    app.key_combo("body", "r", true, false, false)
        .await
        .expect("ctrl+r");
    app.wait_for(".shell:not([data-before-reload])", DEFAULT_WAIT)
        .await
        .expect("rebuilt");
    assert_eq!(
        app.count("form.login").await.expect("count"),
        0,
        "still signed in"
    );
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("rooms again");
    app.open_room("general")
        .await
        .expect("works after reloading");
}

/// Ctrl+Shift+M mutes and unmutes; Ctrl+Shift+D deafens (which mutes too)
/// and undeafens, leaving you muted if you were before.
#[tokio::test]
async fn ctrl_shift_m_mutes_and_ctrl_shift_d_deafens() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch");
    login_and_open(&mut app, "general").await;
    let state = "return [document.querySelector('.user-bar .mute-button').classList.contains('active'), \
                         document.querySelector('.user-bar .deafen-button').classList.contains('active')];";
    async fn press(app: &mut App, key: &str) {
        // Shift makes the letter a capital, as a real keyboard does.
        app.key_combo("body", key, true, false, true).await.expect("shortcut");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let mut expect = async |app: &mut App, muted: bool, deafened: bool, why: &str| {
        assert_eq!(app.eval(state).await.expect("state"), serde_json::json!([muted, deafened]), "{why}");
    };
    expect(&mut app, false, false, "starts heard and hearing").await;
    press(&mut app, "M").await;
    expect(&mut app, true, false, "Ctrl+Shift+M mutes").await;
    press(&mut app, "M").await;
    expect(&mut app, false, false, "and unmutes").await;

    press(&mut app, "D").await;
    expect(&mut app, true, true, "Ctrl+Shift+D deafens, muted too").await;
    press(&mut app, "D").await;
    expect(&mut app, false, false, "undeafened: unmuted again").await;

    press(&mut app, "M").await;
    press(&mut app, "D").await;
    expect(&mut app, true, true, "muted, then deafened").await;
    press(&mut app, "D").await;
    expect(&mut app, true, false, "undeafened: still muted, as before").await;
    press(&mut app, "M").await;
    expect(&mut app, false, false, "back as it was").await;
}
