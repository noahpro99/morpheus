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
