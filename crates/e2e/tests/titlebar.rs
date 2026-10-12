//! The custom title bar: scope title, back/forward through visited rooms,
//! the inbox of unread rooms, help, and window controls.

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
    app.type_into("form.login input[name=password]", &format!("{user}-password"))
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
    let mut app = App::launch(&hs).await.expect("launch app");
    login(&mut app, "alice").await;

    // The bar sits above everything and names the scope.
    app.wait_for_text(".titlebar .titlebar-title", "Rooms", DEFAULT_WAIT)
        .await
        .expect("title is the scope");
    app.wait_for(".titlebar .window-controls button[title=Close]", DEFAULT_WAIT)
        .await
        .expect("window controls on desktop");
    app.click(".rail-item[title=\"Engineering\"]").await.expect("space");
    app.wait_for_text(".titlebar-title", "Engineering", DEFAULT_WAIT)
        .await
        .expect("title follows the space");
    app.click(".rail-item[title=\"Direct messages\"]").await.expect("dms");
    app.wait_for_text(".titlebar-title", "Direct messages", DEFAULT_WAIT)
        .await
        .expect("title for DMs");

    // Back and forward walk the visited rooms, switching scope as needed.
    app.open_room("general").await.expect("general");
    app.open_room("random").await.expect("random");
    // Into the space by hand (open_room would hop scopes, each of which
    // auto-selects a room and would count as a visit).
    app.click(".rail-item[title=\"Engineering\"]").await.expect("space");
    app.wait_for_text(".room", "dev", DEFAULT_WAIT).await.expect("dev listed");
    app.click_text(".room", "dev").await.expect("dev");
    app.wait_for_text(".chat-header h2", "dev", DEFAULT_WAIT).await.expect("dev open");
    app.click(".nav-button[title=Back]").await.expect("back");
    app.wait_for_text(".chat-header h2", "random", DEFAULT_WAIT).await.expect("back to random");
    app.wait_for(".rail-item.selected[title=\"Rooms\"]", DEFAULT_WAIT).await.expect("scope followed back");
    app.click(".nav-button[title=Back]").await.expect("back again");
    app.wait_for_text(".chat-header h2", "general", DEFAULT_WAIT).await.expect("back to general");
    app.click(".nav-button[title=Forward]").await.expect("forward");
    app.wait_for_text(".chat-header h2", "random", DEFAULT_WAIT).await.expect("forward to random");
    app.click(".nav-button[title=Forward]").await.expect("forward again");
    app.wait_for_text(".chat-header h2", "dev", DEFAULT_WAIT).await.expect("forward to dev");
    app.wait_for(".rail-item.selected[title=\"Engineering\"]", DEFAULT_WAIT).await.expect("scope followed forward");
    app.wait_for(".nav-button[title=Forward]:disabled", Duration::from_secs(5)).await.expect("nothing further forward");

    // Help opens a dialog; Escape closes it.
    app.click(".help-button").await.expect("help");
    app.wait_for_text(".help-dialog", "Ctrl+K", DEFAULT_WAIT).await.expect("help dialog");
    app.press(".help-dialog", "Escape").await.expect("escape");
    app.wait_for_js("return document.querySelector('.help-dialog') === null;", DEFAULT_WAIT)
        .await
        .expect("help closed");
}

#[tokio::test]
async fn inbox_lists_unread_rooms_and_opens_them() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login(&mut alice, "alice").await;
    alice.open_room("general").await.expect("general");

    let mut bob = App::launch(&hs).await.expect("launch bob");
    login(&mut bob, "bob").await;
    bob.open_room("random").await.expect("bob in random");
    bob.wait_for(".composer input:not([disabled])", DEFAULT_WAIT).await.expect("composer");
    let ping = format!("inbox ping {}", std::process::id());
    bob.type_into(".composer input", &ping).await.expect("type");
    bob.press(".composer input", "Enter").await.expect("send");

    alice.wait_for(".inbox-badge", DEFAULT_WAIT).await.expect("inbox badge");
    alice.click(".inbox-button").await.expect("open inbox");
    alice.wait_for_text(".inbox-panel .inbox-item", "random", DEFAULT_WAIT).await.expect("random listed");
    alice.click_text(".inbox-item", "random").await.expect("open from inbox");
    alice.wait_for_text(".chat-header h2", "random", DEFAULT_WAIT).await.expect("random opened");
    alice.wait_for_js("return document.querySelector('.inbox-panel') === null;", DEFAULT_WAIT)
        .await
        .expect("inbox closed");
    alice.wait_for_text(".message .body", &ping, DEFAULT_WAIT).await.expect("bob's ping visible");
    // Reading the room clears it from the inbox.
    alice
        .wait_for_js("return document.querySelector('.inbox-badge') === null;", DEFAULT_WAIT)
        .await
        .expect("badge clears once the room is read");
}
