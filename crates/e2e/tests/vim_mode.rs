//! Vim mode (Settings > Appearance > Keyboard): off by default and kept; the
//! message box is modal (insert and normal, with an indicator) and Enter in
//! normal mode sends; with no box focused j and k move a selection through the
//! messages, r replies to it, / searches; off, everything is as it was. GPUI
//! app only.

#![allow(clippy::expect_used)]

use std::time::Duration;

use morpheus_e2e::{App, DEFAULT_WAIT};

mod common;

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

const INPUT: &str = ".composer .composer-input";
const MODE: &str = ".composer .vim-mode";

async fn start(hs: &str) -> Option<App> {
    let mut app = App::launch(hs).await.expect("launch");
    if !app.is_gpui() {
        eprintln!("vim mode is in the GPUI app only; skipping");
        return None;
    }
    common::login(&mut app, "alice", "alice-password").await;
    app.open_room("general").await.expect("general");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
    Some(app)
}

/// Settings > Appearance, open.
async fn open_keyboard_settings(app: &mut App) {
    app.wait_for(".settings-button", DEFAULT_WAIT)
        .await
        .expect("gear");
    for _ in 0..5 {
        app.click(".settings-button").await.expect("gear");
        if app
            .wait_for(".settings-nav", Duration::from_secs(3))
            .await
            .is_ok()
        {
            break;
        }
    }
    app.wait_for_text(".settings-nav button", "Appearance", DEFAULT_WAIT)
        .await
        .expect("an Appearance page");
    app.click_text(".settings-nav button", "Appearance")
        .await
        .expect("page");
    app.wait_for(".settings-page input[name=vim-mode]", DEFAULT_WAIT)
        .await
        .expect("the Vim mode switch");
}

async fn set_vim(app: &mut App, on: bool) {
    open_keyboard_settings(app).await;
    let sel = ".settings-page input[name=vim-mode]";
    if app.checked(sel).await.expect("checked")[0] != on {
        app.click(sel).await.expect("flip");
        let deadline = std::time::Instant::now() + DEFAULT_WAIT;
        while app.checked(sel).await.expect("checked")[0] != on {
            assert!(
                std::time::Instant::now() < deadline,
                "vim mode did not flip"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    app.press("body", "Escape").await.expect("close settings");
    app.wait_for_count(".settings", 0, DEFAULT_WAIT)
        .await
        .expect("closed");
}

async fn wait_for_message(app: &mut App, needle: &str) {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let texts = app.texts(".message:not(.pending)").await.expect("texts");
        if texts.iter().any(|t| t.contains(needle)) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no message with {needle:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn send(app: &mut App, text: &str) {
    app.type_into(INPUT, text).await.expect("type");
    app.press(INPUT, "Enter").await.expect("send");
    wait_for_message(app, text).await;
}

/// Leave the box with Escape (from insert mode twice), as a vim user does.
async fn leave_box(app: &mut App) {
    // Escape goes to whatever has the keys, without moving the focus.
    for _ in 0..4 {
        if app
            .count(".composer-input[data-focused]")
            .await
            .expect("count")
            == 0
        {
            return;
        }
        app.press("body", "Escape").await.expect("escape");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    app.wait_for_count(".composer-input[data-focused]", 0, DEFAULT_WAIT)
        .await
        .expect("the box let go");
}

#[tokio::test]
async fn vim_mode_is_off_by_default_and_the_choice_is_kept() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let Some(mut app) = start(&hs).await else {
        return;
    };
    open_keyboard_settings(&mut app).await;
    assert!(
        !app.checked(".settings-page input[name=vim-mode]")
            .await
            .expect("checked")[0],
        "off by default"
    );
    app.press("body", "Escape").await.expect("close settings");
    app.click(INPUT).await.expect("focus the box");
    assert_eq!(
        app.count(MODE).await.expect("count"),
        0,
        "no mode shown while off"
    );

    set_vim(&mut app, true).await;
    let mut app = app.relaunch().await.expect("relaunch");
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("signed in again");
    open_keyboard_settings(&mut app).await;
    assert!(
        app.checked(".settings-page input[name=vim-mode]")
            .await
            .expect("checked")[0],
        "kept over a restart"
    );
    set_vim(&mut app, false).await;
}

#[tokio::test]
async fn the_message_box_is_modal_with_vim_mode_on() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let Some(mut app) = start(&hs).await else {
        return;
    };
    set_vim(&mut app, true).await;
    let pid = std::process::id();

    // Focusing the box starts in insert mode: typing is as before.
    app.click(INPUT).await.expect("focus the box");
    app.wait_for_text(MODE, "INSERT", DEFAULT_WAIT)
        .await
        .expect("insert mode");
    app.type_into(INPUT, &format!("alpha beta {pid}"))
        .await
        .expect("type");
    app.wait_for_value(INPUT, &format!("alpha beta {pid}"), DEFAULT_WAIT)
        .await
        .expect("typed");

    // Esc: normal mode. dw takes a word, x a letter.
    app.press(INPUT, "Escape").await.expect("normal");
    app.wait_for_text(MODE, "NORMAL", DEFAULT_WAIT)
        .await
        .expect("normal mode");
    app.keys(INPUT, "0dw").await.expect("dw");
    app.wait_for_value(INPUT, &format!("beta {pid}"), DEFAULT_WAIT)
        .await
        .expect("dw");
    app.keys(INPUT, "x").await.expect("x");
    app.wait_for_value(INPUT, &format!("eta {pid}"), DEFAULT_WAIT)
        .await
        .expect("x");

    // A then typing: insert at the end of the line; Esc back.
    app.keys(INPUT, "A!").await.expect("A!");
    app.wait_for_text(MODE, "INSERT", DEFAULT_WAIT)
        .await
        .expect("insert again");
    app.wait_for_value(INPUT, &format!("eta {pid}!"), DEFAULT_WAIT)
        .await
        .expect("A then typing");
    app.press(INPUT, "Escape").await.expect("normal");
    app.wait_for_text(MODE, "NORMAL", DEFAULT_WAIT)
        .await
        .expect("normal mode");

    // u undoes the insert, Ctrl+R (not the app's reload) does it again.
    app.keys(INPUT, "u").await.expect("u");
    app.wait_for_value(INPUT, &format!("eta {pid}"), DEFAULT_WAIT)
        .await
        .expect("undone");
    app.key_combo(INPUT, "r", true, false, false)
        .await
        .expect("ctrl+r");
    app.wait_for_value(INPUT, &format!("eta {pid}!"), DEFAULT_WAIT)
        .await
        .expect("redone");

    // Visual: select a word and yank, then put.
    app.keys(INPUT, "0vey$p").await.expect("yank and put");
    app.wait_for_value(INPUT, &format!("eta {pid}!eta"), DEFAULT_WAIT)
        .await
        .expect("yanked and put");
    app.keys(INPUT, "u").await.expect("undo the put");

    // Enter in normal mode sends, as Discord's Enter does.
    app.press(INPUT, "Enter").await.expect("send");
    wait_for_message(&mut app, &format!("eta {pid}!")).await;
    app.wait_for_value(INPUT, "", DEFAULT_WAIT)
        .await
        .expect("box emptied");
    app.wait_for_text(MODE, "NORMAL", DEFAULT_WAIT)
        .await
        .expect("still normal");

    // Esc with nothing to close lets go of the box.
    app.press(INPUT, "Escape").await.expect("blur");
    app.wait_for_count(".composer-input[data-focused]", 0, DEFAULT_WAIT)
        .await
        .expect("the box let go");

    set_vim(&mut app, false).await;
}

#[tokio::test]
async fn the_timeline_moves_by_keys_with_vim_mode_on() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let Some(mut app) = start(&hs).await else {
        return;
    };
    set_vim(&mut app, true).await;
    let pid = std::process::id();
    let (one, two) = (format!("vim one {pid}"), format!("vim two {pid}"));
    app.click(INPUT).await.expect("focus the box");
    send(&mut app, &one).await;
    send(&mut app, &two).await;
    leave_box(&mut app).await;

    // The newest message on screen is ours (the list has caught up).
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while !app
        .texts(".message")
        .await
        .expect("texts")
        .last()
        .is_some_and(|t| t.contains(&two))
    {
        assert!(
            std::time::Instant::now() < deadline,
            "the newest message is not ours"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // k selects the newest message, then moves up; j back down.
    let selected = async |app: &mut App| app.texts(".message.vim-selected").await.expect("texts");
    app.keys("body", "k").await.expect("k");
    app.wait_for_text(".message.vim-selected", &two, DEFAULT_WAIT)
        .await
        .expect("newest selected");
    app.keys("body", "k").await.expect("k");
    app.wait_for_text(".message.vim-selected", &one, DEFAULT_WAIT)
        .await
        .expect("one up");
    assert_eq!(selected(&mut app).await.len(), 1, "one selection");
    app.keys("body", "j").await.expect("j");
    app.wait_for_text(".message.vim-selected", &two, DEFAULT_WAIT)
        .await
        .expect("one down");

    // gg and G: the top and the newest.
    app.keys("body", "gg").await.expect("gg");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while selected(&mut app).await.iter().any(|t| t.contains(&two)) {
        assert!(
            std::time::Instant::now() < deadline,
            "gg stayed on the newest"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    app.keys("body", "G").await.expect("G");
    app.wait_for_text(".message.vim-selected", &two, DEFAULT_WAIT)
        .await
        .expect("G");

    // Ctrl+U scrolls (it does not toggle the member list, as without vim mode).
    let members = app.count(".members").await.expect("count");
    app.key_combo("body", "u", true, false, false)
        .await
        .expect("ctrl+u");
    app.key_combo("body", "d", true, false, false)
        .await
        .expect("ctrl+d");
    assert_eq!(
        app.count(".members").await.expect("count"),
        members,
        "Ctrl+U left the members alone"
    );

    // e edits the selected (own) message: the box holds it.
    app.keys("body", "e").await.expect("e");
    app.wait_for_text(".composer-banner", "Editing message", DEFAULT_WAIT)
        .await
        .expect("edit banner");
    app.wait_for_value(INPUT, &two, DEFAULT_WAIT)
        .await
        .expect("the box holds the message");
    app.press(INPUT, "Escape").await.expect("normal");
    app.press(INPUT, "Escape").await.expect("cancel the edit");
    app.wait_for_count(".composer-banner", 0, DEFAULT_WAIT)
        .await
        .expect("edit cancelled");
    leave_box(&mut app).await;

    // r replies to the selected message: the banner, and a reply with its quote.
    app.keys("body", "kk").await.expect("kk");
    app.wait_for_text(".message.vim-selected", &one, DEFAULT_WAIT)
        .await
        .expect("one selected");
    app.keys("body", "r").await.expect("r");
    app.wait_for_text(".composer-banner", "Replying to", DEFAULT_WAIT)
        .await
        .expect("reply banner");
    app.wait_for_count(".message.vim-selected", 0, DEFAULT_WAIT)
        .await
        .expect("selection let go");
    let reply = format!("vim reply {pid}");
    send(&mut app, &reply).await;
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let texts = app.texts(".message:not(.pending)").await.expect("texts");
        if texts.iter().any(|t| t.contains(&reply) && t.contains(&one)) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the reply did not quote {one:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    leave_box(&mut app).await;

    // dd asks before deleting.
    app.keys("body", "k").await.expect("k");
    app.wait_for(".message.vim-selected", DEFAULT_WAIT)
        .await
        .expect("selected");
    app.keys("body", "dd").await.expect("dd");
    app.wait_for(".confirm-dialog", DEFAULT_WAIT)
        .await
        .expect("asks first");
    app.press("body", "Escape")
        .await
        .expect("close the question");
    app.wait_for_count(".confirm-dialog", 0, DEFAULT_WAIT)
        .await
        .expect("closed");

    // + opens the reaction picker.
    app.wait_for(".message.vim-selected", DEFAULT_WAIT)
        .await
        .expect("still selected after the question");
    app.keys("body", "+").await.expect("+");
    app.wait_for(".emoji-picker", DEFAULT_WAIT)
        .await
        .expect("picker");
    app.press("body", "Escape").await.expect("close the picker");
    app.wait_for_count(".emoji-picker", 0, DEFAULT_WAIT)
        .await
        .expect("closed");

    // i focuses the box in insert mode, and types nothing itself.
    app.keys("body", "i").await.expect("i");
    app.wait_for_text(MODE, "INSERT", DEFAULT_WAIT)
        .await
        .expect("insert mode");
    app.wait_for(".composer-input[data-focused]", DEFAULT_WAIT)
        .await
        .expect("focused");
    assert_eq!(
        app.value(INPUT).await.expect("value").unwrap_or_default(),
        ""
    );
    leave_box(&mut app).await;

    // / goes to the search box.
    app.keys("body", "/").await.expect("/");
    app.wait_for(".search-bar input[name=search][data-focused]", DEFAULT_WAIT)
        .await
        .expect("search focused");
    app.press(".search-bar input[name=search]", "Escape")
        .await
        .expect("leave search");

    set_vim(&mut app, false).await;
}

#[tokio::test]
async fn turning_vim_mode_off_restores_ordinary_typing() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let Some(mut app) = start(&hs).await else {
        return;
    };
    set_vim(&mut app, true).await;
    app.click(INPUT).await.expect("focus the box");
    app.wait_for_text(MODE, "INSERT", DEFAULT_WAIT)
        .await
        .expect("insert mode");
    app.press(INPUT, "Escape").await.expect("normal");
    app.wait_for_text(MODE, "NORMAL", DEFAULT_WAIT)
        .await
        .expect("normal mode");

    set_vim(&mut app, false).await;
    app.click(INPUT).await.expect("focus the box");
    assert_eq!(app.count(MODE).await.expect("count"), 0, "no mode shown");
    app.type_into(INPUT, "dw ok").await.expect("type");
    app.wait_for_value(INPUT, "dw ok", DEFAULT_WAIT)
        .await
        .expect("typing is typing");
    app.press(INPUT, "Escape").await.expect("escape");
    app.wait_for_value(INPUT, "dw ok", DEFAULT_WAIT)
        .await
        .expect("the draft stays");
    app.type_into(INPUT, "").await.expect("clear");

    // Nothing focused: typing goes to the box, and no message is selected.
    app.unfocus().await.expect("unfocus");
    app.keys("body", "k").await.expect("k");
    app.wait_for_value(INPUT, "k", DEFAULT_WAIT)
        .await
        .expect("typed into the box");
    assert_eq!(app.count(".message.vim-selected").await.expect("count"), 0);
    app.type_into(INPUT, "").await.expect("clear");
}

/// The shortcuts dialog lists the vim keys, greyed out while vim mode is off,
/// and has the Vim mode switch at its bottom: flipping it there turns vim mode
/// on for the message box (and back off).
#[tokio::test]
async fn the_shortcuts_dialog_lists_the_vim_keys_with_a_switch() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let Some(mut app) = start(&hs).await else {
        return;
    };
    app.key_combo("body", "/", true, false, false)
        .await
        .expect("ctrl+/");
    app.wait_for(".help-dialog", DEFAULT_WAIT)
        .await
        .expect("shortcuts dialog");

    // Bigger than before, and still inside the window.
    let window = app.bounds("body").await.expect("body").expect("a window");
    let dialog = app
        .bounds(".help-dialog")
        .await
        .expect("b")
        .expect("a dialog");
    assert!(dialog.w >= 800.0, "the dialog is too narrow: {dialog:?}");
    assert!(
        dialog.h >= f64::min(600.0, window.h - 40.0),
        "the dialog is too short: {dialog:?}"
    );
    assert!(
        dialog.bottom() <= window.h,
        "the dialog does not fit: {dialog:?} in {window:?}"
    );

    // The vim keys are listed (the message box's and the messages'), off.
    app.wait_for(".help-dialog .vim-keys.off .help-row", DEFAULT_WAIT)
        .await
        .expect("vim keys, greyed");
    assert_eq!(
        app.count(".help-dialog .vim-keys.on").await.expect("count"),
        0
    );
    let rows = app
        .texts(".help-dialog .vim-keys .help-row")
        .await
        .expect("rows");
    assert!(rows.len() >= 10, "vim rows: {rows:?}");
    for want in ["dd", "gg", "Ctrl+R"] {
        assert!(
            rows.iter().any(|r| r.contains(want)),
            "no vim row for {want}: {rows:?}"
        );
    }
    // The switch is below the list and off.
    let sw = ".help-dialog input[name=help-vim-mode]";
    assert!(
        !app.checked(sw).await.expect("checked")[0],
        "off by default"
    );
    let switch = app.bounds(sw).await.expect("b").expect("the switch");
    assert!(
        switch.y > dialog.bottom() - 90.0 && switch.bottom() <= dialog.bottom(),
        "the switch is not at the bottom: {switch:?} in {dialog:?}"
    );

    // Turning it on lights the keys up, and the message box is modal.
    app.click(sw).await.expect("flip");
    app.wait_for(".help-dialog .vim-keys.on .help-row", DEFAULT_WAIT)
        .await
        .expect("keys no longer greyed");
    assert_eq!(
        app.count(".help-dialog .vim-keys.off")
            .await
            .expect("count"),
        0
    );
    assert!(app.checked(sw).await.expect("checked")[0], "now on");
    app.press("body", "Escape").await.expect("close");
    app.wait_for_count(".help-dialog", 0, DEFAULT_WAIT)
        .await
        .expect("closed");
    app.click(INPUT).await.expect("focus the box");
    app.wait_for_text(MODE, "INSERT", DEFAULT_WAIT)
        .await
        .expect("vim mode is on in the box");

    // And off again from the dialog.
    leave_box(&mut app).await;
    app.key_combo("body", "/", true, false, false)
        .await
        .expect("ctrl+/");
    app.wait_for(".help-dialog .vim-keys.on", DEFAULT_WAIT)
        .await
        .expect("open again");
    app.click(sw).await.expect("flip back");
    app.wait_for(".help-dialog .vim-keys.off", DEFAULT_WAIT)
        .await
        .expect("greyed again");
    app.press("body", "Escape").await.expect("close");
    app.click(INPUT).await.expect("focus the box");
    assert_eq!(
        app.count(MODE).await.expect("count"),
        0,
        "no mode shown while off"
    );
}
