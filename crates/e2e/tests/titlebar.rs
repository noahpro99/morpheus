//! The custom title bar: scope title, back/forward through visited rooms,
//! the inbox of unread rooms, help, and window controls.

// Desktop layout (member column beside the chat, window controls): phone runs keep it on desktop.
#![allow(clippy::expect_used)]

use std::time::Duration;

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login(app: &mut App, user: &str) {
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", user)
        .await
        .expect("username");
    app.type_into(
        "form.login input[name=password]",
        &format!("{user}-password"),
    )
    .await
    .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("room listed");
}

#[tokio::test]
async fn titlebar_title_history_help_and_controls() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login(&mut app, "alice").await;

    // The bar sits above everything and names the scope.
    app.wait_for_text(".titlebar .titlebar-title", "Rooms", DEFAULT_WAIT)
        .await
        .expect("title is the scope");
    app.wait_for(
        ".titlebar .window-controls button[title=Close]",
        DEFAULT_WAIT,
    )
    .await
    .expect("window controls on desktop");
    app.wait_for(".rail-item[title=\"Engineering\"]", DEFAULT_WAIT)
        .await
        .expect("the space on the rail");
    app.click(".rail-item[title=\"Engineering\"]")
        .await
        .expect("space");
    app.wait_for_text(".titlebar-title", "Engineering", DEFAULT_WAIT)
        .await
        .expect("title follows the space");
    app.click(".rail-item[title=\"Direct messages\"]")
        .await
        .expect("dms");
    app.wait_for_text(".titlebar-title", "Direct messages", DEFAULT_WAIT)
        .await
        .expect("title for DMs");

    // Back and forward walk the visited rooms, switching scope as needed.
    app.open_room("general").await.expect("general");
    app.open_room("random").await.expect("random");
    // Into the space by hand (open_room would hop scopes, each of which
    // auto-selects a room and would count as a visit).
    app.click(".rail-item[title=\"Engineering\"]")
        .await
        .expect("space");
    app.wait_for_text(".room", "dev", DEFAULT_WAIT)
        .await
        .expect("dev listed");
    app.click_text(".room", "dev").await.expect("dev");
    app.wait_for_text(".chat-header h2", "dev", DEFAULT_WAIT)
        .await
        .expect("dev open");
    app.click(".nav-button[title^=Back]").await.expect("back");
    app.wait_for_text(".chat-header h2", "random", DEFAULT_WAIT)
        .await
        .expect("back to random");
    app.wait_for(".rail-item.selected[title=\"Rooms\"]", DEFAULT_WAIT)
        .await
        .expect("scope followed back");
    app.click(".nav-button[title^=Back]")
        .await
        .expect("back again");
    app.wait_for_text(".chat-header h2", "general", DEFAULT_WAIT)
        .await
        .expect("back to general");
    app.click(".nav-button[title^=Forward]")
        .await
        .expect("forward");
    app.wait_for_text(".chat-header h2", "random", DEFAULT_WAIT)
        .await
        .expect("forward to random");
    app.click(".nav-button[title^=Forward]")
        .await
        .expect("forward again");
    app.wait_for_text(".chat-header h2", "dev", DEFAULT_WAIT)
        .await
        .expect("forward to dev");
    app.wait_for(".rail-item.selected[title=\"Engineering\"]", DEFAULT_WAIT)
        .await
        .expect("scope followed forward");
    app.wait_for(
        ".nav-button[title^=Forward]:disabled",
        Duration::from_secs(5),
    )
    .await
    .expect("nothing further forward");

    // Help opens a dialog; Escape closes it.
    app.click(".help-button").await.expect("help");
    app.wait_for_text(".help-dialog", "Ctrl+K", DEFAULT_WAIT)
        .await
        .expect("help dialog");
    app.press(".help-dialog", "Escape").await.expect("escape");
    app.wait_for_count(".help-dialog", 0, DEFAULT_WAIT)
        .await
        .expect("help closed");
}

#[tokio::test]
async fn inbox_lists_unread_rooms_and_opens_them() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch_desktop(&hs).await.expect("launch alice");
    login(&mut alice, "alice").await;
    alice.open_room("general").await.expect("general");

    let mut bob = App::launch_desktop(&hs).await.expect("launch bob");
    login(&mut bob, "bob").await;
    bob.open_room("random").await.expect("bob in random");
    bob.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer");
    let ping = format!("inbox ping {}", std::process::id());
    bob.type_into(".composer .composer-input", &ping)
        .await
        .expect("type");
    bob.press(".composer .composer-input", "Enter")
        .await
        .expect("send");

    alice
        .wait_for(".inbox-badge", DEFAULT_WAIT)
        .await
        .expect("inbox badge");
    alice.click(".inbox-button").await.expect("open inbox");
    alice
        .wait_for_text(".inbox-panel .inbox-item", "random", DEFAULT_WAIT)
        .await
        .expect("random listed");
    // Other rooms may be unread too (the shared test account): the count now.
    let unread_before = alice
        .count(".inbox-panel .inbox-item")
        .await
        .expect("inbox items") as i64;
    alice
        .click_text(".inbox-item", "random")
        .await
        .expect("open from inbox");
    alice
        .wait_for_text(".chat-header h2", "random", DEFAULT_WAIT)
        .await
        .expect("random opened");
    alice
        .wait_for_count(".inbox-panel", 0, DEFAULT_WAIT)
        .await
        .expect("inbox closed");
    alice
        .wait_for_text(".message .body", &ping, DEFAULT_WAIT)
        .await
        .expect("bob's ping visible");
    // Reading the room takes it off the inbox's count.
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let badge = alice.text(".inbox-badge").await.expect("badge");
        let now = badge.and_then(|b| b.trim().parse::<i64>().ok());
        if now.is_none_or(|n| n < unread_before) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the badge counts one fewer once the room is read (still {now:?}, was {unread_before})"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn alt_left_and_right_walk_the_rooms_visited() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login(&mut app, "alice").await;
    app.open_room("general").await.expect("general");
    app.open_room("random").await.expect("random");
    app.key_combo("body", "ArrowLeft", false, true, false)
        .await
        .expect("alt+left");
    app.wait_for_text(".chat-header h2", "general", DEFAULT_WAIT)
        .await
        .expect("back to general");
    app.key_combo("body", "ArrowRight", false, true, false)
        .await
        .expect("alt+right");
    app.wait_for_text(".chat-header h2", "random", DEFAULT_WAIT)
        .await
        .expect("forward to random");
}

/// The inbox sits where help used to, help right of it, next to the window
/// controls.
#[tokio::test]
async fn the_inbox_button_comes_before_help() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login(&mut app, "alice").await;
    app.wait_for(".titlebar .help-button", DEFAULT_WAIT).await.expect("help");
    let inbox = app.bounds(".titlebar .inbox-button").await.expect("bounds").expect("inbox");
    let help = app.bounds(".titlebar .help-button").await.expect("bounds").expect("help");
    let controls = app.bounds(".titlebar .window-controls").await.expect("bounds").expect("controls");
    assert!(inbox.right() <= help.x + 0.5, "inbox {inbox:?} is not left of help {help:?}");
    assert!(help.right() <= controls.x + 4.0, "help {help:?} is not next to the controls {controls:?}");
}
