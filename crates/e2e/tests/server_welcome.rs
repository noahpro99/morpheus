//! The server welcome screen: shown in the main area the first time a server
//! is opened (and when you join one), with its picture, name, topic, member
//! count and channels and a "Start chatting" button; reopenable from the
//! server menu as "Server overview".

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT, reset_rooms_named};

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

async fn create_server(app: &mut App, server: &str, channel: &str) {
    app.click(".rail-add").await.expect("rail plus");
    app.type_into(".create-space-dialog input[name=name]", server)
        .await
        .expect("name");
    app.submit(".create-space-dialog").await.expect("create");
    app.wait_for_text(".sidebar-header h2", server, DEFAULT_WAIT)
        .await
        .expect("server scoped");
    app.click(".create-room-button").await.expect("plus");
    app.type_into(".create-room-dialog input[name=name]", channel)
        .await
        .expect("channel name");
    app.submit(".create-room-dialog")
        .await
        .expect("create channel");
    app.wait_for_text(".chat-header h2", channel, DEFAULT_WAIT)
        .await
        .expect("channel opened");
}

#[tokio::test]
async fn server_overview_from_the_menu_shows_the_welcome_and_start_chatting_opens_a_channel() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let _clean = reset_rooms_named(&hs, &["alice"], "e2e welcome");
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login(&mut app, "alice").await;
    let server = format!("e2e welcome a {}", std::process::id());
    create_server(&mut app, &server, "lobby").await;
    // A server you made yourself opens straight into its channel.
    assert_eq!(app.count(".server-welcome").await.expect("count"), 0);

    app.click(".sidebar-header h2.server-name")
        .await
        .expect("name");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("menu");
    app.click_text(".context-item", "Server overview")
        .await
        .expect("overview item");
    app.wait_for_text(".server-welcome h2", &server, DEFAULT_WAIT)
        .await
        .expect("welcome shows the name");
    app.wait_for_text(".server-welcome", "1 member", DEFAULT_WAIT)
        .await
        .expect("member count");
    app.wait_for_text(".server-welcome .welcome-channel", "lobby", DEFAULT_WAIT)
        .await
        .expect("channel listed");
    assert_eq!(app.count(".composer").await.expect("count"), 0);

    app.click_text(".server-welcome button", "Start chatting")
        .await
        .expect("start chatting");
    app.wait_for_count(".server-welcome", 0, DEFAULT_WAIT)
        .await
        .expect("welcome gone");
    app.wait_for_text(".chat-header h2", "lobby", DEFAULT_WAIT)
        .await
        .expect("first text channel open");
}

#[tokio::test]
async fn a_server_you_join_welcomes_you_once() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let _clean = reset_rooms_named(&hs, &["alice", "bob"], "e2e welcome");
    let mut alice = App::launch_desktop(&hs).await.expect("launch alice");
    login(&mut alice, "alice").await;
    let server = format!("e2e welcome b {}", std::process::id());
    create_server(&mut alice, &server, "hall").await;

    let mut bob = App::launch_desktop(&hs).await.expect("launch bob");
    login(&mut bob, "bob").await;

    alice
        .right_click(&format!(".rail-item[title=\"{server}\"]"))
        .await
        .expect("badge menu");
    alice
        .click_text(".context-item", "Invite to server")
        .await
        .expect("invite");
    alice
        .type_into(".invite-dialog input[name=user]", "@bob:localhost")
        .await
        .expect("user");
    alice.submit(".invite-dialog").await.expect("send");
    alice
        .wait_for_text(".invite-dialog .ok", "Invited @bob:localhost", DEFAULT_WAIT)
        .await
        .expect("invited");
    alice
        .press(".invite-dialog", "Escape")
        .await
        .expect("escape");

    bob.click(".rail-dms").await.expect("DMs");
    bob.wait_for_text(".invite-row", &server, DEFAULT_WAIT)
        .await
        .expect("invite listed");
    bob.click_within(".invite-row", &server, ".invite-accept")
        .await
        .expect("accept");
    let badge = format!(".rail-item[title=\"{server}\"]");
    bob.wait_for(&badge, DEFAULT_WAIT).await.expect("badge");

    // First open: the welcome, with the channel listed.
    bob.click(&badge).await.expect("open the server");
    bob.wait_for_text(".server-welcome h2", &server, DEFAULT_WAIT)
        .await
        .expect("welcome");
    bob.wait_for_text(".server-welcome .welcome-channel", "hall", DEFAULT_WAIT)
        .await
        .expect("channel listed");
    bob.click_text(".server-welcome button", "Start chatting")
        .await
        .expect("start chatting");
    bob.wait_for_count(".server-welcome", 0, DEFAULT_WAIT)
        .await
        .expect("welcome gone");

    // Not again: leave and come back.
    bob.click(".rail-dms").await.expect("DMs");
    bob.click(&badge).await.expect("open again");
    bob.wait_for_text(".sidebar-header h2", &server, DEFAULT_WAIT)
        .await
        .expect("in the server");
    assert_eq!(
        bob.count(".server-welcome").await.expect("count"),
        0,
        "the welcome shows once"
    );
}
