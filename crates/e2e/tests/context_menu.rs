//! Right-click menus: messages, rooms and members get the styled menu, the
//! menu acts (reply, react), closes on Escape or a click outside, and the
//! webview's native menu never appears anywhere in the shell.

#![allow(clippy::expect_used)]

use std::time::Duration;

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
        .expect("room listed");
    app.open_room(room).await.expect("open room");
    app.wait_for(".composer input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
}

async fn menu_closed(app: &mut App) {
    app.wait_for_js(
        "return document.querySelector('.context-menu') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("menu closed");
}

#[tokio::test]
async fn message_menu_replies_reacts_and_offers_edit_delete() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;

    // A fresh own message so edit/delete apply and no reaction exists yet.
    let text = format!("context menu test {}", std::process::id());
    app.type_into(".composer input", &text).await.expect("type");
    app.press(".composer input", "Enter").await.expect("send");
    app.wait_for_exact_text(".message.own:not(.pending) .body", &text, DEFAULT_WAIT)
        .await
        .expect("message sent");
    let mine = "[...document.querySelectorAll('.message.own')].at(-1)";
    let select_mine = format!(
        "{mine}.dispatchEvent(new MouseEvent('contextmenu', {{ bubbles: true, cancelable: true, clientX: 400, clientY: 300 }}));"
    );

    app.eval(&select_mine)
        .await
        .expect("right-click own message");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("menu opens");
    for label in [
        "Reply",
        "Reply in thread",
        "Edit message",
        "Copy text",
        "Copy message link",
        "Delete message",
    ] {
        app.wait_for_text(".context-item", label, Duration::from_secs(2))
            .await
            .unwrap_or_else(|e| panic!("menu item {label}: {e}"));
    }
    assert_eq!(
        app.count(".context-reaction").await.expect("count"),
        3,
        "quick reactions row"
    );

    // Escape closes it.
    app.press(".context-menu", "Escape").await.expect("escape");
    menu_closed(&mut app).await;

    // React from the menu.
    app.eval(&select_mine).await.expect("right-click again");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("menu");
    app.click_text(".context-reaction", "👍")
        .await
        .expect("thumbs up");
    menu_closed(&mut app).await;
    app.wait_for_js(
        &format!("return {mine}.querySelector('.reaction.mine') !== null;"),
        DEFAULT_WAIT,
    )
    .await
    .expect("reaction landed on the message");

    // Reply from the menu puts the composer in reply mode.
    app.eval(&select_mine).await.expect("right-click again");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("menu");
    app.click_text(".context-item", "Reply")
        .await
        .expect("reply");
    menu_closed(&mut app).await;
    app.wait_for(".composer-banner", DEFAULT_WAIT)
        .await
        .expect("composer shows the reply banner");

    // Someone else's message: no edit or delete. Recent rows may all be
    // ours, so scroll up until another sender shows.
    app.reveal_element(".message:not(.own):not(.system)", DEFAULT_WAIT)
        .await
        .expect("another sender's message");
    app.right_click(".message:not(.own):not(.system)")
        .await
        .expect("right-click other");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("menu");
    let labels = app
        .eval("return [...document.querySelectorAll('.context-item')].map(e => e.textContent).join('|');")
        .await
        .expect("labels");
    let labels = labels.as_str().unwrap_or_default().to_owned();
    assert!(
        labels.contains("Reply") && !labels.contains("Delete") && !labels.contains("Edit"),
        "got {labels}"
    );
    // A click outside closes it.
    app.click(".context-backdrop").await.expect("click outside");
    menu_closed(&mut app).await;
}

#[tokio::test]
async fn room_and_member_menus_and_no_native_menu() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;

    app.right_click(".room").await.expect("right-click room");
    app.wait_for_text(".context-item", "Copy link", DEFAULT_WAIT)
        .await
        .expect("room menu");
    app.wait_for_text(".context-item.danger", "Leave room", DEFAULT_WAIT)
        .await
        .expect("leave is marked dangerous");
    app.press(".context-menu", "Escape").await.expect("escape");
    menu_closed(&mut app).await;

    app.wait_for(".members .member", DEFAULT_WAIT)
        .await
        .expect("members listed");
    app.right_click(".members .member")
        .await
        .expect("right-click member");
    app.wait_for_text(".context-item", "Copy user ID", DEFAULT_WAIT)
        .await
        .expect("member menu");
    app.click_text(".context-item", "Profile")
        .await
        .expect("view profile");
    app.wait_for(".profile-card", DEFAULT_WAIT)
        .await
        .expect("profile opened from the menu");
    menu_closed(&mut app).await;
    app.press("body", "Escape").await.expect("close profile");

    // Right-clicking dead space never shows the webview's own menu.
    let prevented = app
        .eval(
            "const ev = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 200, clientY: 60 }); \
             document.querySelector('.sidebar-header').dispatchEvent(ev); return ev.defaultPrevented;",
        )
        .await
        .expect("dispatch");
    assert_eq!(
        prevented.as_bool(),
        Some(true),
        "native context menu suppressed"
    );
    assert_eq!(
        app.count(".context-menu").await.expect("count"),
        0,
        "and no custom menu for dead space"
    );
}
