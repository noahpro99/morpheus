//! The spaces rail through the real window: selecting a space filters the
//! room list down to that space's rooms, Home shows ungrouped rooms, and the
//! sidebar header follows the selection.
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
    let js = format!(
        "return [...document.querySelectorAll('.room-name')].some(el => el.textContent.includes({}));",
        serde_json::to_string(name).expect("string always serialises")
    );
    app.eval(&js)
        .await
        .expect("eval")
        .as_bool()
        .unwrap_or(false)
}

#[tokio::test]
async fn space_badge_filters_the_room_list() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login(&mut app).await;

    // Home: #general and #random are ungrouped and show up; #dev lives
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
        Some("Morpheus".to_string()),
        "home header reads Morpheus"
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
    app.click_text(".room", "dev").await.expect("open dev");
    app.wait_for_text(".chat-header h2", "dev", DEFAULT_WAIT)
        .await
        .expect("dev header");

    // Back to Home: #general and #random return, #dev disappears again.
    app.click(".rail-item[title=\"Home\"]")
        .await
        .expect("click home");
    app.wait_for_text(".sidebar-header h2", "Morpheus", DEFAULT_WAIT)
        .await
        .expect("header back to home");
    app.wait_for_text(".room", "general", DEFAULT_WAIT)
        .await
        .expect("general back");
    assert!(!has_room(&mut app, "dev").await, "dev hidden again at home");
}
