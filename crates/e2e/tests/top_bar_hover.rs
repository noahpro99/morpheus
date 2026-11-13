//! Hovering the top bar's buttons (the room's name and the buttons to its
//! right) paints no background, as in Discord: only the glyph or text turns
//! from muted to the normal colour. (GPUI only.)

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login(app: &mut App, user: &str) {
    app.wait_for("form.login", DEFAULT_WAIT).await.expect("login form");
    app.type_into("form.login input[name=username]", user).await.expect("username");
    app.type_into("form.login input[name=password]", &format!("{user}-password"))
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
}

async fn first(app: &mut App, sel: &str, prop: &str) -> String {
    app.colours_of(sel, prop).await.expect("colours").into_iter().next().unwrap_or_default()
}

/// Hover `sel` after the pointer was elsewhere: no background, a brighter colour.
async fn check(app: &mut App, sel: &str) {
    app.hover(".composer .composer-input").await.expect("away");
    let rest = first(app, sel, "color").await;
    assert!(!rest.is_empty(), "{sel} notes no colour");
    app.hover(sel).await.expect("hover");
    let bg = first(app, sel, "background").await;
    let over = first(app, sel, "color").await;
    assert_eq!(bg, "", "{sel} paints a background when hovered: {bg}");
    assert_ne!(rest, over, "{sel} does not change colour when hovered ({rest})");
    app.hover(".composer .composer-input").await.expect("away");
    assert_eq!(first(app, sel, "color").await, rest, "{sel} keeps the hover colour");
}

#[tokio::test]
async fn hovering_the_top_bar_only_brightens_it() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch");
    if !app.is_gpui() {
        return;
    }
    login(&mut app, "alice").await;
    app.open_room("general").await.expect("general");
    app.wait_for(".chat-header .room-head", DEFAULT_WAIT).await.expect("head");
    // The member list starts open (its button is lit): close it first.
    app.click(".chat-header .member-toggle").await.expect("members");
    for sel in [
        ".chat-header .room-head",
        ".chat-header .e2e-badge",
        ".chat-header .member-toggle",
        ".chat-header .pin-toggle",
    ] {
        check(&mut app, sel).await;
    }
    // The search box's ✕ (shown once there is text).
    app.type_into(".chat-header input[name=search]", "hello").await.expect("type");
    app.wait_for(".chat-header .search-clear", DEFAULT_WAIT).await.expect("clear");
    check(&mut app, ".chat-header .search-clear").await;
    app.click(".chat-header .search-clear").await.expect("clear");

    // A DM: its picture and name, and the call button.
    app.click(".rail-dms").await.expect("DMs");
    app.wait_for(".room.dm .room-name", DEFAULT_WAIT).await.expect("a DM");
    app.click(".room.dm").await.expect("open it");
    app.wait_for(".chat-header .call-button", DEFAULT_WAIT).await.expect("call button");
    check(&mut app, ".chat-header .room-head").await;
    check(&mut app, ".chat-header .call-button").await;
}
