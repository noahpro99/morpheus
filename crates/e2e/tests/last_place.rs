//! Quitting the app completely and starting it again opens where you were:
//! the same room, in its server (or the DMs).

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT, api_as, new_account};

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
        .expect("rooms listed");
}

/// A fresh account in three rooms, so the one left open is not simply the
/// first in the list.
fn account_with_rooms(hs: &str) -> String {
    let user = new_account(hs, "place").expect("account");
    for name in ["aardvark", "middle", "zebra"] {
        api_as(
            hs,
            &user,
            "POST",
            "createRoom",
            Some(serde_json::json!({ "name": name })),
        )
        .expect("a room");
    }
    user
}

#[tokio::test]
async fn reopening_the_app_returns_to_the_last_room() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let user = account_with_rooms(&hs);
    let mut app = App::launch(&hs).await.expect("launch");
    login(&mut app, &user).await;
    app.wait_for_text(".room", "zebra", DEFAULT_WAIT)
        .await
        .expect("all rooms listed");
    // Whichever room the app opened first, go to a different one.
    let first = app
        .text(".chat-header h2")
        .await
        .expect("first room")
        .unwrap_or_default();
    let target = if first.trim() == "middle" {
        "zebra"
    } else {
        "middle"
    };
    app.open_room(target).await.expect("open the room");
    app.wait_for_text(".chat-header h2", target, DEFAULT_WAIT)
        .await
        .expect("room open");
    // A moment, as a person would take, before quitting.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let mut app = app.relaunch().await.expect("relaunch");
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("rooms listed again");
    app.wait_for_text(".chat-header h2", target, DEFAULT_WAIT)
        .await
        .expect("the last room reopened");
    // And it stays there once the list has settled.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let shown = app
        .text(".chat-header h2")
        .await
        .expect("room")
        .unwrap_or_default();
    assert_eq!(shown.trim(), target, "moved off the last room");
}

/// A fresh account in two servers: "Xserver" (x-one, x-two) and "Yserver"
/// (y-one).
fn account_with_two_servers(hs: &str) -> String {
    let user = new_account(hs, "scopes").expect("account");
    let enc = |id: &str| id.replace('!', "%21").replace(':', "%3A");
    // One room outside the servers, so Rooms has a row for `login` to wait for.
    api_as(hs, &user, "POST", "createRoom", Some(serde_json::json!({ "name": "lobby" }))).expect("lobby");
    for (server, channels) in [("Xserver", &["x-one", "x-two"][..]), ("Yserver", &["y-one"][..])] {
        let space = api_as(
            hs,
            &user,
            "POST",
            "createRoom",
            Some(serde_json::json!({ "name": server, "creation_content": { "type": "m.space" } })),
        )
        .expect("space");
        let space = space["room_id"].as_str().expect("id").to_owned();
        for name in channels {
            let room = api_as(
                hs,
                &user,
                "POST",
                "createRoom",
                Some(serde_json::json!({ "name": name })),
            )
            .expect("channel");
            let room = room["room_id"].as_str().expect("id").to_owned();
            api_as(
                hs,
                &user,
                "PUT",
                &format!("rooms/{}/state/m.space.child/{room}", enc(&space)),
                Some(serde_json::json!({ "via": ["localhost"] })),
            )
            .expect("child");
        }
    }
    user
}

async fn header_is(app: &mut App, name: &str, why: &str) {
    app.wait_for_text(".chat-header h2", name, DEFAULT_WAIT)
        .await
        .unwrap_or_else(|e| panic!("{why}: {e}"));
}

/// As Discord: a server's badge opens the channel that was open when you
/// left it, also after the DMs and after quitting the app.
#[tokio::test]
async fn clicking_a_server_returns_to_the_channel_last_open_in_it() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let user = account_with_two_servers(&hs);
    let mut app = App::launch(&hs).await.expect("launch");
    login(&mut app, &user).await;
    if !app.is_gpui() {
        // The Dioxus rail only lists the server; its Ctrl+Alt+Right does the jump.
        eprintln!("skipped: the Dioxus rail badges do not reopen a server's channel");
        return;
    }
    let x = ".rail-item[title=\"Xserver\"]";
    let y = ".rail-item[title=\"Yserver\"]";
    let dms = ".rail-item[title=\"Direct messages\"]";
    app.wait_for(x, DEFAULT_WAIT).await.expect("X badge");
    app.wait_for(y, DEFAULT_WAIT).await.expect("Y badge");

    app.click(x).await.expect("X");
    app.open_room("x-two").await.expect("x-two");
    app.click(y).await.expect("Y");
    app.open_room("y-one").await.expect("y-one");
    app.click(dms).await.expect("DMs");
    app.wait_for_text(".sidebar-header h2", "Direct messages", DEFAULT_WAIT)
        .await
        .expect("DMs listed");

    app.click(x).await.expect("X again");
    header_is(&mut app, "x-two", "X reopens the channel left open in it").await;
    app.click(y).await.expect("Y again");
    header_is(&mut app, "y-one", "Y reopens its own channel").await;
    // Clicking the server you are in leaves the channel as it is.
    app.click(y).await.expect("Y once more");
    header_is(&mut app, "y-one", "still there").await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // After quitting the app: it opens in Y (where it was), and X still
    // knows its channel.
    let mut app = app.relaunch().await.expect("relaunch");
    app.wait_for(x, DEFAULT_WAIT).await.expect("X badge again");
    header_is(&mut app, "y-one", "the last room reopened").await;
    app.click(x).await.expect("X after restart");
    header_is(&mut app, "x-two", "X reopens its channel after a restart").await;
}

/// As Discord: a server never visited opens its top channel, the first one
/// listed in its sidebar (not a voice channel, which a click would join).
#[tokio::test]
async fn a_server_never_visited_opens_its_first_listed_channel() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let user = new_account(&hs, "topchan").expect("account");
    let enc = |id: &str| id.replace('!', "%21").replace(':', "%3A");
    api_as(&hs, &user, "POST", "createRoom", Some(serde_json::json!({ "name": "lobby" }))).expect("lobby");
    let space = api_as(
        &hs,
        &user,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": "Zserver", "creation_content": { "type": "m.space" } })),
    )
    .expect("space");
    let space = space["room_id"].as_str().expect("id").to_owned();
    // Listed in this order: a voice channel, then z-first, then z-second.
    for (name, voice, order) in [("z-call", true, "a"), ("z-first", false, "b"), ("z-second", false, "c")] {
        let content = if voice {
            serde_json::json!({ "name": name, "creation_content": { "type": "org.matrix.msc3417.call" } })
        } else {
            serde_json::json!({ "name": name })
        };
        let room = api_as(&hs, &user, "POST", "createRoom", Some(content)).expect("channel");
        let room = room["room_id"].as_str().expect("id").to_owned();
        api_as(
            &hs,
            &user,
            "PUT",
            &format!("rooms/{}/state/m.space.child/{room}", enc(&space)),
            Some(serde_json::json!({ "via": ["localhost"], "order": order })),
        )
        .expect("child");
    }
    let mut app = App::launch(&hs).await.expect("launch");
    login(&mut app, &user).await;
    if !app.is_gpui() {
        eprintln!("skipped: the Dioxus rail badges do not open a server's channel");
        return;
    }
    let z =".rail-item[title=\"Zserver\"]";
    app.wait_for(z, DEFAULT_WAIT).await.expect("Z badge");
    app.click(z).await.expect("Z");
    app.wait_for_text(".room", "z-second", DEFAULT_WAIT).await.expect("Z listed");
    header_is(&mut app, "z-first", "a server never visited opens its top channel").await;
}
