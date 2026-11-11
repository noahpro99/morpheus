//! The spaces rail through the real window: selecting a space filters the
//! room list down to that space's rooms, Rooms shows ungrouped rooms, DMs get
//! their own button, and the sidebar header follows the selection.
//!
//! `dev/homeserver/seed.sh` puts #dev inside the "Engineering" space and
//! leaves #general and #random outside any space, so the three rooms give
//! us both sides of the filter to check.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login(app: &mut App) {
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
    app.wait_for(".shell", DEFAULT_WAIT)
        .await
        .expect("shell after login");
}

/// Whether any `.room-name` currently reads `name`. Unlike `wait_for_text`
/// this doesn't wait, so it can assert absence without racing a timeout.
async fn has_room(app: &mut App, name: &str) -> bool {
    app.texts(".room-name")
        .await
        .expect("room names")
        .iter()
        .any(|text| text.contains(name))
}

#[tokio::test]
async fn space_badge_filters_the_room_list() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login(&mut app).await;

    // Rooms: #general and #random are ungrouped and show up; #dev lives
    // inside the "Engineering" space and does not.
    app.wait_for_text(".room", "general", DEFAULT_WAIT)
        .await
        .expect("general at home");
    app.wait_for_text(".room", "random", DEFAULT_WAIT)
        .await
        .expect("random at home");
    assert!(!has_room(&mut app, "dev").await, "dev hidden at home");
    assert_eq!(
        app.text(".sidebar-header h2").await.expect("header"),
        Some("Rooms".to_string()),
        "rooms header reads Rooms"
    );
    assert_eq!(
        app.count(".rail-item.selected")
            .await
            .expect("selected badge count"),
        1,
        "exactly one rail badge is selected"
    );

    // The space has its own badge in the rail.
    app.wait_for(".rail-item[title=\"Engineering\"]", DEFAULT_WAIT)
        .await
        .expect("engineering badge");

    // Select it: only #dev should be listed, and the header follows.
    app.click(".rail-item[title=\"Engineering\"]")
        .await
        .expect("click engineering");
    app.wait_for_text(".sidebar-header h2", "Engineering", DEFAULT_WAIT)
        .await
        .expect("header follows selection");
    app.wait_for_text(".room", "dev", DEFAULT_WAIT)
        .await
        .expect("dev inside its space");
    assert!(
        !has_room(&mut app, "general").await,
        "general hidden while a space is selected"
    );
    assert!(
        !has_room(&mut app, "random").await,
        "random hidden while a space is selected"
    );
    assert_eq!(
        app.count(".rail-item.selected[title=\"Engineering\"]")
            .await
            .expect("selected badge"),
        1,
        "the Engineering badge is highlighted while selected"
    );

    // Opening #dev works like any other room.
    app.open_room("dev").await.expect("open dev");
    app.wait_for_text(".chat-header h2", "dev", DEFAULT_WAIT)
        .await
        .expect("dev header");

    // Back to Rooms: #general and #random return, #dev disappears again.
    app.click(".rail-item[title=\"Rooms\"]")
        .await
        .expect("click home");
    app.wait_for_text(".sidebar-header h2", "Rooms", DEFAULT_WAIT)
        .await
        .expect("header back to home");
    app.wait_for_text(".room", "general", DEFAULT_WAIT)
        .await
        .expect("general back");
    assert!(!has_room(&mut app, "dev").await, "dev hidden again at home");
}

/// Regression: room clicks must change the open room, never the space scope.
/// (The two context signals once shared a type and collided.)
#[tokio::test]
async fn clicking_rooms_keeps_the_rooms_scope() {
    let Some(hs) = std::env::var("MORPHEUS_TEST_HOMESERVER").ok() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
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
    app.wait_for_text(".room", "random", DEFAULT_WAIT)
        .await
        .expect("rooms listed");

    for room in ["general", "random", "general"] {
        app.open_room(room).await.expect("click room");
        app.wait_for_text(".chat-header h2", room, DEFAULT_WAIT)
            .await
            .expect("room opened");
        app.wait_for(
            ".rail-item.selected[title=\"Rooms\"]",
            std::time::Duration::from_secs(5),
        )
        .await
        .expect("still in the Rooms scope");
        app.wait_for_text(".room", "random", std::time::Duration::from_secs(5))
            .await
            .expect("with all rooms listed");
    }
}

/// The rail splits direct messages from rooms: the DMs button lists only
/// DMs (with the other person's avatar, small), the Rooms button lists only
/// ungrouped rooms, and the footer shows a display name rather than a Matrix ID.
#[tokio::test]
async fn dms_button_lists_direct_messages() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login(&mut app).await;
    app.wait_for_text(".room", "general", DEFAULT_WAIT)
        .await
        .expect("rooms listed first");
    assert!(!has_room(&mut app, "bob").await, "the DM is not a room");

    app.click(".rail-item[title=\"Direct messages\"]")
        .await
        .expect("DMs button");
    app.wait_for_text(".sidebar-header h2", "Direct messages", DEFAULT_WAIT)
        .await
        .expect("header follows the rail");
    app.wait_for_text(".room.dm", "bob", DEFAULT_WAIT)
        .await
        .expect("the DM with bob is listed");
    assert!(
        !has_room(&mut app, "general").await,
        "rooms stay out of DMs"
    );
    // Bob's profile photo stands in for the room avatar, at sidebar size.
    app.wait_for_js(
        "const a = document.querySelector('.room.dm img.avatar'); return a && a.clientHeight <= 24;",
        DEFAULT_WAIT,
    )
    .await
    .expect("small DM avatar");

    app.open_room("bob").await.expect("open the DM");

    // Footer: display name, no Matrix ID.
    let user = app
        .text(".profile-chip .user")
        .await
        .expect("footer text")
        .unwrap_or_default();
    assert!(
        !user.contains('@'),
        "footer shows a display name, got {user:?}"
    );
    assert!(!user.is_empty(), "footer has a name");
}
