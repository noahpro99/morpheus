//! Friends: people you keep close (a private list in account data, as
//! Matrix has no friendship). Added from a person's menu, listed on the
//! Friends page at the top of the DMs, with a way to message them.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT, reset_friends};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn member_menu(app: &mut App, member: &str) {
    app.wait_for_text(".members .member", member, DEFAULT_WAIT)
        .await
        .expect("member listed");
    app.right_click_text(".members .member", member)
        .await
        .expect("right-click member");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("menu");
}

async fn open_friends(app: &mut App) {
    app.click(".rail-dms").await.expect("DMs");
    app.wait_for(".sidebar .friends-row", DEFAULT_WAIT)
        .await
        .expect("Friends row");
    app.click(".sidebar .friends-row").await.expect("Friends");
    app.wait_for(".friends-page", DEFAULT_WAIT)
        .await
        .expect("Friends page");
}

#[tokio::test]
async fn friends_are_added_listed_messaged_and_removed() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let _clean = reset_friends(&hs, "carol");
    let mut app = App::launch(&hs).await.expect("launch app");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", "carol")
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", "carol-password")
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    app.open_room("general").await.expect("open general");

    member_menu(&mut app, "bob").await;
    app.click_text(".context-menu .context-item", "Add friend")
        .await
        .expect("Add friend");

    open_friends(&mut app).await;
    app.wait_for_text(".friends-page .friend", "bob", DEFAULT_WAIT)
        .await
        .expect("bob listed");

    // Kept in the account.
    let mut app = app.relaunch().await.expect("relaunch");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    open_friends(&mut app).await;
    app.wait_for_text(".friends-page .friend", "bob", DEFAULT_WAIT)
        .await
        .expect("bob still listed");

    // Message opens the DM with them (the row's Message button is the same
    // click target as the row; the button is checked to be there).
    app.wait_for_text(".friends-page .friend button", "Message", DEFAULT_WAIT)
        .await
        .expect("a Message button");
    app.click(".friends-page .friend .friend-name")
        .await
        .expect("Message bob");
    app.wait_for(".chat-header", DEFAULT_WAIT)
        .await
        .expect("a chat");
    app.wait_for_count(".friends-page", 0, DEFAULT_WAIT)
        .await
        .expect("left the Friends page");

    // Remove.
    open_friends(&mut app).await;
    app.wait_for_text(".friends-page .friend", "bob", DEFAULT_WAIT)
        .await
        .expect("bob listed again");
    app.click_text(".friends-page .friend button", "Remove")
        .await
        .expect("Remove");
    app.wait_for_text_gone(".friends-page .friend", "bob", DEFAULT_WAIT)
        .await
        .expect("bob gone");
}

#[tokio::test]
async fn new_message_by_username_opens_a_dm() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let _clean = reset_friends(&hs, "carol");
    let mut app = App::launch(&hs).await.expect("launch app");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", "carol")
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", "carol-password")
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    open_friends(&mut app).await;

    // No "Add Friend" (Matrix has no friend requests): New Message instead.
    assert_eq!(app.count(".add-friend-tab").await.expect("count"), 0);
    app.click(".friends-header .new-message-tab")
        .await
        .expect("New Message");
    app.wait_for("form.new-message input", DEFAULT_WAIT)
        .await
        .expect("the form");
    // Someone who does not exist: said so.
    app.type_into("form.new-message input", "nobody-here")
        .await
        .expect("type");
    app.submit("form.new-message").await.expect("send");
    app.wait_for_text(
        ".new-message .new-message-result.error",
        "nobody-here",
        DEFAULT_WAIT,
    )
    .await
    .expect("an unknown name is refused");
    if let Some(path) = app.screenshot("new-message").await.expect("screenshot") {
        eprintln!("screenshot: {}", path.display());
    }
    // A bare username: the DM with them opens, ready to type in.
    app.type_into("form.new-message input", "alice")
        .await
        .expect("type");
    app.submit("form.new-message").await.expect("send");
    app.wait_for_count(".friends-page", 0, DEFAULT_WAIT)
        .await
        .expect("left the Friends page");
    app.wait_for_text(".chat-header h2", "alice", DEFAULT_WAIT)
        .await
        .unwrap_or_else(|e| panic!("the DM with alice opens: {e}"));
    assert!(
        !app.texts(".chat-header h2")
            .await
            .expect("titles")
            .iter()
            .any(|t| t.contains("Empty Room")),
        "the DM is named for the person, not left unnamed"
    );
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("ready to type");
}

/// On the Friends page the whole row is the Message button, as in Discord:
/// clicking a friend's name opens the DM; Remove still only removes.
#[tokio::test]
async fn a_friends_whole_row_opens_the_dm() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let _clean = reset_friends(&hs, "carol");
    let mut app = App::launch(&hs).await.expect("launch app");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", "carol")
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", "carol-password")
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    app.open_room("general").await.expect("open general");
    member_menu(&mut app, "bob").await;
    app.click_text(".context-menu .context-item", "Add friend")
        .await
        .expect("Add friend");
    open_friends(&mut app).await;
    app.wait_for_text(".friends-page .friend", "bob", DEFAULT_WAIT)
        .await
        .expect("bob listed");
    app.click(".friends-page .friend .friend-name")
        .await
        .expect("click the row");
    app.wait_for_count(".friends-page", 0, DEFAULT_WAIT)
        .await
        .expect("left the Friends page");
    app.wait_for(".chat-header", DEFAULT_WAIT)
        .await
        .expect("the row opened the DM");

    open_friends(&mut app).await;
    app.wait_for_text(".friends-page .friend", "bob", DEFAULT_WAIT)
        .await
        .expect("bob listed again");
    app.click_text(".friends-page .friend button", "Remove")
        .await
        .expect("Remove");
    app.wait_for_text_gone(".friends-page .friend", "bob", DEFAULT_WAIT)
        .await
        .expect("bob gone");
    assert_eq!(
        app.count(".friends-page").await.expect("count"),
        1,
        "Remove didn't also open the DM"
    );
}

/// DMs in the list are tall rows, as in Discord (42px, a 32px picture),
/// and the Friends row matches them.
#[tokio::test]
async fn dm_rows_are_tall() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let _clean = reset_friends(&hs, "alice");
    let mut app = App::launch(&hs).await.expect("launch app");
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
    app.wait_for(".rail-dms", DEFAULT_WAIT).await.expect("rail");
    app.click(".rail-dms").await.expect("DMs");
    app.wait_for(".sidebar .room.dm", DEFAULT_WAIT)
        .await
        .expect("a DM listed");
    let mut sizes = Vec::new();
    for selector in [
        ".sidebar .room.dm",
        ".sidebar .room.dm .avatar",
        ".sidebar .friends-row",
    ] {
        sizes.push(
            app.bounds(selector)
                .await
                .expect("bounds")
                .unwrap_or_else(|| panic!("no {selector}"))
                .h,
        );
    }
    assert!(sizes[0] >= 42.0, "a DM row is 42px tall: {sizes:?}");
    assert!(sizes[1] >= 32.0, "with a 32px picture: {sizes:?}");
    assert!(sizes[2] >= 42.0, "and so is Friends: {sizes:?}");
}

/// The top of the DMs sidebar is an inset, lighter "Command palette" button with
/// the shortcut (Ctrl+K here) that opens the palette, in place of a "Direct
/// messages" heading.
#[tokio::test]
async fn the_dms_sidebar_starts_with_the_command_palette_button() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch alice");
    if !app.is_gpui() {
        return;
    }
    app.wait_for("form.login", DEFAULT_WAIT).await.expect("login form");
    app.type_into("form.login input[name=username]", "alice").await.expect("username");
    app.type_into("form.login input[name=password]", "alice-password").await.expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".rail-dms", DEFAULT_WAIT).await.expect("rail");
    app.click(".rail-dms").await.expect("DMs");
    app.wait_for(".sidebar-header .palette-button", DEFAULT_WAIT)
        .await
        .expect("the palette button is in the sidebar's top bar");
    assert_eq!(
        app.count(".sidebar-header h2").await.expect("count"),
        0,
        "no heading text in the top bar of the DMs"
    );
    assert!(
        app.texts(".sidebar-header").await.expect("texts").iter().all(|t| !t.contains("Direct messages")),
        "the old heading text is gone"
    );
    let header = app.bounds(".sidebar-header").await.expect("bounds").expect("header");
    let text = app.text(".sidebar .palette-button").await.expect("text").unwrap_or_default();
    assert!(text.contains("Command palette") && text.contains("Ctrl+K"), "button text {text:?}");
    let button = app.bounds(".sidebar .palette-button").await.expect("bounds").expect("button");
    let friends = app.bounds(".sidebar .friends-row").await.expect("bounds").expect("friends");
    let sidebar = app.bounds(".sidebar").await.expect("bounds").expect("sidebar");
    assert!(
        button.y >= header.y && button.bottom() <= header.bottom() + 0.5,
        "the button {button:?} is not inside the top bar {header:?}"
    );
    assert!(button.bottom() <= friends.y + 0.5, "the button {button:?} is not above Friends {friends:?}");
    assert!(
        button.x > sidebar.x + 4.0 && button.right() < sidebar.right() - 4.0,
        "the button {button:?} is not inset in the sidebar {sidebar:?}"
    );
    let bg = app.colours_of(".sidebar .palette-button", "background").await.expect("bg");
    let side = app.colours_of(".sidebar", "background").await.expect("bg");
    assert!(
        bg.first().is_some_and(|c| !c.is_empty()) && bg.first() != side.first(),
        "the button's background {bg:?} does not stand out from the sidebar's {side:?}"
    );
    app.click(".sidebar .palette-button").await.expect("click");
    app.wait_for(".switcher input", DEFAULT_WAIT).await.expect("the palette opened");
}
