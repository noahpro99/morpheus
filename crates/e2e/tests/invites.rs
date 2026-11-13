//! Pending invites: listed in the sidebar and the inbox, counted on the DM
//! rail button, shown as a card in the chat column; accept joins, decline
//! leaves.

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

/// Right-click the joined room row whose name contains `name`.
async fn room_menu(app: &mut App, name: &str) {
    app.right_click_text(".room:not(.invite-row)", name)
        .await
        .expect("right-click room");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("room menu");
}

async fn invite_bob(app: &mut App, room: &str) {
    room_menu(app, room).await;
    app.click_text(".context-item", "Invite people")
        .await
        .expect("invite");
    app.wait_for(".invite-dialog input[name=user]", DEFAULT_WAIT)
        .await
        .expect("invite dialog");
    app.type_into(".invite-dialog input[name=user]", "@bob:localhost")
        .await
        .expect("user");
    app.submit(".invite-dialog").await.expect("send invite");
    app.wait_for_text(".invite-dialog .ok", "Invited @bob:localhost", DEFAULT_WAIT)
        .await
        .expect("invited");
    app.press(".invite-dialog", "Escape").await.expect("escape");
}

#[tokio::test]
async fn invites_are_listed_declined_and_accepted() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let _clean = reset_rooms_named(&hs, &["alice", "bob"], "e2e room invite");
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login(&mut alice, "alice").await;
    alice.open_room("general").await.expect("open general");
    let name = format!("e2e room invite {}", std::process::id());
    alice.click(".create-room-button").await.expect("plus");
    alice
        .wait_for(".create-room-dialog input[name=name]", DEFAULT_WAIT)
        .await
        .expect("dialog");
    alice
        .type_into(".create-room-dialog input[name=name]", &name)
        .await
        .expect("name");
    alice.submit(".create-room-dialog").await.expect("create");
    alice
        .wait_for_text(".chat-header h2", &name, DEFAULT_WAIT)
        .await
        .expect("new room opened");

    let mut bob = App::launch(&hs).await.expect("launch bob");
    login(&mut bob, "bob").await;
    // Nothing pending before the invite.
    bob.wait_for_text_gone(".invite-row", &name, DEFAULT_WAIT)
        .await
        .expect("no invite yet");

    invite_bob(&mut alice, &name).await;

    // Sidebar row with the inviter, rail badge, card in the chat column.
    bob.wait_for_text(".invite-row", &name, DEFAULT_WAIT)
        .await
        .expect("invite listed");
    bob.wait_for_text(".invite-row", "from alice", DEFAULT_WAIT)
        .await
        .expect("inviter shown");
    bob.wait_for(".rail-dms .rail-badge", DEFAULT_WAIT)
        .await
        .expect("dm badge counts the invite");
    bob.click_text(".invite-row", &name)
        .await
        .expect("open invite");
    bob.wait_for_text(".invite-card h2", &name, DEFAULT_WAIT)
        .await
        .expect("invite card");
    bob.wait_for_text(".invite-card", "alice invited you", DEFAULT_WAIT)
        .await
        .expect("card text");
    assert_eq!(
        bob.count(".composer").await.expect("count"),
        0,
        "no composer for an invite"
    );
    // The inbox lists it too.
    bob.click(".titlebar .inbox-button")
        .await
        .expect("open inbox");
    bob.wait_for_text(".inbox-invite", &name, DEFAULT_WAIT)
        .await
        .expect("inbox invite");
    bob.press(".inbox-panel", "Escape")
        .await
        .expect("close inbox");

    // Decline from the card: the row and the card go away.
    bob.click(".invite-card .invite-decline")
        .await
        .expect("decline");
    bob.wait_for_text_gone(".invite-row", &name, DEFAULT_WAIT)
        .await
        .expect("invite gone");
    bob.wait_for_count(".invite-card", 0, DEFAULT_WAIT)
    .await
    .expect("card gone");

    // Invite again and accept from the sidebar row.
    invite_bob(&mut alice, &name).await;
    bob.wait_for_text(".invite-row", &name, DEFAULT_WAIT)
        .await
        .expect("invited again");
    bob.click_within(".invite-row", &name, ".invite-accept")
        .await
        .expect("accept");
    bob.wait_for_text_gone(".invite-row", &name, DEFAULT_WAIT)
        .await
        .expect("no longer pending");
    bob.open_room(&name).await.expect("joined room opens");
    bob.wait_for(".composer .composer-input", DEFAULT_WAIT)
        .await
        .expect("composer for the joined room");
}

/// Alice's friends list: exactly `users`, until the guard drops.
fn befriend(hs: &str, users: &[&str]) -> morpheus_e2e::Cleanup {
    let guard = morpheus_e2e::reset_friends(hs, "alice");
    morpheus_e2e::api_as(
        hs,
        "alice",
        "PUT",
        "user/@alice:localhost/account_data/dev.morpheus.friends",
        Some(serde_json::json!({ "users": users })),
    )
    .expect("friends set");
    guard
}

/// The invite dialog just opened lists the friends; the filter narrows them;
/// clicking bob's row invites him (the row says so); a typed full ID works
/// for someone who is not a friend.
async fn use_the_friend_list(alice: &mut App, bob: &mut App, name: &str, what: &str) {
    alice
        .wait_for(".invite-dialog .friend-picker", DEFAULT_WAIT)
        .await
        .unwrap_or_else(|e| panic!("{what}: no friends list in the dialog: {e}"));
    alice
        .wait_for_count(".invite-dialog .friend-pick", 1, DEFAULT_WAIT)
        .await
        .unwrap_or_else(|e| panic!("{what}: bob (the one friend) not listed: {e}"));
    alice
        .wait_for_text(".invite-dialog .friend-pick", "bob", DEFAULT_WAIT)
        .await
        .unwrap_or_else(|e| panic!("{what}: {e}"));
    // The filter.
    alice
        .type_into(".invite-dialog input[name=user]", "zzz")
        .await
        .expect("filter");
    alice
        .wait_for_count(".invite-dialog .friend-pick", 0, DEFAULT_WAIT)
        .await
        .unwrap_or_else(|e| panic!("{what}: the filter did not narrow the list: {e}"));
    alice
        .type_into(".invite-dialog input[name=user]", "BO")
        .await
        .expect("filter");
    alice
        .wait_for_count(".invite-dialog .friend-pick", 1, DEFAULT_WAIT)
        .await
        .unwrap_or_else(|e| panic!("{what}: the filter hides bob for \"BO\": {e}"));
    // Invite from the list.
    alice
        .click(".invite-dialog .friend-pick")
        .await
        .expect("click bob");
    alice
        .wait_for(".invite-dialog .friend-pick[data-state=sent]", DEFAULT_WAIT)
        .await
        .unwrap_or_else(|e| panic!("{what}: the row is not marked invited: {e}"));
    bob.wait_for_text(".invite-row", name, DEFAULT_WAIT)
        .await
        .unwrap_or_else(|e| panic!("{what}: bob got no invite: {e}"));
    // A typed ID of a non-friend still works.
    alice
        .type_into(".invite-dialog input[name=user]", "@carol:localhost")
        .await
        .expect("a full id");
    alice.submit(".invite-dialog").await.expect("submit");
    alice
        .wait_for_text(".invite-dialog .ok", "Invited @carol:localhost", DEFAULT_WAIT)
        .await
        .unwrap_or_else(|e| panic!("{what}: a typed ID did not invite: {e}"));
    alice.press(".invite-dialog", "Escape").await.expect("escape");
}

/// Every invite dialog has the searchable list of friends: the room's and
/// the server's.
#[tokio::test]
async fn invite_dialogs_list_friends_and_invite_from_the_list() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let _clean = reset_rooms_named(&hs, &["alice", "bob", "carol"], "e2e room invite");
    let _friends = befriend(&hs, &["@bob:localhost"]);
    let mut alice = App::launch(&hs).await.expect("launch alice");
    let mut bob = App::launch(&hs).await.expect("launch bob");
    if !alice.is_gpui() {
        eprintln!("GPUI only");
        return;
    }
    login(&mut alice, "alice").await;
    login(&mut bob, "bob").await;
    alice.open_room("general").await.expect("open general");

    // A room.
    let room = format!("e2e room invite {}", std::process::id());
    alice.click(".create-room-button").await.expect("plus");
    alice
        .wait_for(".create-room-dialog input[name=name]", DEFAULT_WAIT)
        .await
        .expect("dialog");
    alice
        .type_into(".create-room-dialog input[name=name]", &room)
        .await
        .expect("name");
    alice.submit(".create-room-dialog").await.expect("create");
    alice
        .wait_for_text(".chat-header h2", &room, DEFAULT_WAIT)
        .await
        .expect("new room opened");
    room_menu(&mut alice, &room).await;
    alice
        .click_text(".context-item", "Invite people")
        .await
        .expect("invite");
    use_the_friend_list(&mut alice, &mut bob, &room, "room").await;

    // A server.
    let server = format!("e2e room invite srv {}", std::process::id());
    alice.click(".rail-add").await.expect("rail plus");
    alice
        .wait_for(".create-space-dialog input[name=name]", DEFAULT_WAIT)
        .await
        .expect("dialog");
    alice
        .type_into(".create-space-dialog input[name=name]", &server)
        .await
        .expect("name");
    alice.submit(".create-space-dialog").await.expect("create");
    alice
        .wait_for_text(".sidebar-header h2", &server, DEFAULT_WAIT)
        .await
        .expect("new server scoped");
    alice
        .click(".sidebar-header h2.server-name")
        .await
        .expect("server menu");
    alice
        .click_text(".context-item", "Invite to server")
        .await
        .expect("invite to server");
    use_the_friend_list(&mut alice, &mut bob, &server, "server").await;
}
