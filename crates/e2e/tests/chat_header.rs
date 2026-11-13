//! The bar over a chat, as Discord lays it out: it runs from the chat to
//! the window's right edge, with the side panel (members, a thread, search)
//! under it as a card; a call button only where a call belongs (DMs and
//! voice channels), at the right beside the pins and the search bar.

#![allow(clippy::expect_used)]

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
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
}

/// The window's size: the app's root on GPUI, the page's size on Dioxus.
async fn window_size(app: &mut App) -> (f64, f64) {
    if app.is_gpui() {
        let b = app.bounds("body").await.expect("body").expect("a body");
        (b.w, b.h)
    } else {
        app.page_size().await.expect("page size")
    }
}

#[tokio::test]
async fn the_top_bar_spans_to_the_right_over_a_side_card() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch");
    login(&mut app, "alice").await;
    app.open_room("general").await.expect("general");
    app.wait_for(".side-pane .members", DEFAULT_WAIT)
        .await
        .expect("members shown");
    let (width, height) = window_size(&mut app).await;
    let header = app.bounds(".chat-header").await.expect("header").expect("a header");
    let side = app.bounds(".side-pane").await.expect("side").expect("a side pane");
    assert!(
        header.right() >= width - 1.0,
        "the top bar stops short of the right edge: header {header:?}, window {width}"
    );
    assert!(
        side.y >= header.bottom(),
        "the side panel is not under the top bar: {side:?} vs {header:?}"
    );
    assert!(
        side.right() < width && side.bottom() < height,
        "the side panel is not inset like a card: {side:?}, window {width}x{height}"
    );
    // Rounded on all four corners (the GPUI app notes the radius it draws).
    let radius = if app.is_gpui() {
        app.attr(".side-pane", "data-radius")
            .await
            .expect("radius")
            .and_then(|r| r.parse::<f64>().ok())
            .unwrap_or(0.0)
    } else {
        app.eval(
            "const cs = getComputedStyle(document.querySelector('.side-pane')); \
             return Math.min(...[cs.borderTopLeftRadius, cs.borderTopRightRadius, cs.borderBottomRightRadius, cs.borderBottomLeftRadius].map(parseFloat));",
        )
        .await
        .expect("radii")
        .as_f64()
        .unwrap_or(0.0)
    };
    assert!(radius >= 6.0, "the side panel is not rounded on all four corners: {radius}");
}

#[tokio::test]
async fn a_call_button_only_where_calls_belong() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch");
    login(&mut app, "alice").await;

    // A text channel: no call button.
    app.open_room("general").await.expect("general");
    app.wait_for_text(".chat-header h2", "general", DEFAULT_WAIT)
        .await
        .expect("in general");
    assert_eq!(
        app.count(".chat-header .call-button").await.expect("count"),
        0,
        "a call button in a text channel"
    );

    // A DM: the call button at the right, by the pins, then the search bar.
    app.click(".rail-item[title=\"Direct messages\"]")
        .await
        .expect("direct messages");
    app.open_room("bob").await.expect("the dm");
    app.wait_for(".chat-header .call-button", DEFAULT_WAIT)
        .await
        .expect("a call button in a DM");
    let call = app.bounds(".chat-header > .call-button").await.expect("call").expect("call button");
    let pin = app.bounds(".chat-header > .pin-toggle").await.expect("pin").expect("pin button");
    let search = app.bounds(".chat-header > .search-bar").await.expect("search").expect("search");
    let header = app.bounds(".chat-header").await.expect("header").expect("header");
    // Buttons across the bar: the call button, then the pins right after it.
    assert!(
        call.right() <= pin.x + 1.0 && pin.x - call.right() < 24.0,
        "the call button is not beside the pins: {call:?} {pin:?}"
    );
    assert!(
        search.x >= pin.right() - 1.0 && search.x >= call.right() - 1.0,
        "the search bar is not at the right end: {search:?}"
    );
    assert!(search.w >= 140.0, "search is a button, not a bar: {search:?}");
    assert!(
        header.right() - search.right() <= 24.0,
        "the search bar is not at the top right: {search:?} in {header:?}"
    );
}

async fn menu_opens_and_closes(app: &mut App, what: &str) {
    let label = if app.count(".chat-header .room-head .avatar").await.expect("count") > 0 { "Close DM" } else { "Edit room" };
    app.wait_for_text(".context-menu .context-item", label, DEFAULT_WAIT)
        .await
        .unwrap_or_else(|e| panic!("{what} should open the room menu: {e}"));
    app.press(".context-menu", "Escape").await.expect("escape");
    app.wait_for_count(".context-menu", 0, DEFAULT_WAIT).await.expect("menu closed");
}

/// The icon (a channel's #, a DM's picture) and the name are one button, as
/// tall as the bar: the whole of it opens the room's menu, its edges too.
async fn head_button_is_one_big_target(app: &mut App, icon: &str) {
    let bar = app.bounds(".chat-header").await.expect("b").expect("bar");
    let head = app.bounds(".chat-header .room-head").await.expect("b").expect("the head button");
    assert!(head.h >= bar.h - 2.0, "as tall as the bar: {head:?} in {bar:?}");
    assert!(head.x <= bar.x + 12.0, "at the bar's left: {head:?}");
    let icon_b = app.bounds(&format!(".chat-header .room-head {icon}")).await.expect("b").expect("icon");
    assert!(icon_b.right() <= head.right() && icon_b.x >= head.x, "the icon is inside it");
    let name = app.bounds(".chat-header .room-head .room-title").await.expect("b").expect("name");
    assert!(name.x >= icon_b.right(), "the name is right of the icon: {name:?} {icon_b:?}");
    app.click(&format!(".chat-header .room-head {icon}")).await.expect("click the icon");
    menu_opens_and_closes(app, "the icon").await;
    app.click(".chat-header .room-head .room-title").await.expect("click the name");
    menu_opens_and_closes(app, "the name").await;
    // The corners of the box, well off the glyphs.
    let (dx, dy) = (head.w / 2.0 - 2.0, head.h / 2.0 - 2.0);
    for (ox, oy) in [(-dx, -dy), (dx, -dy), (-dx, dy), (dx, dy)] {
        app.click_at(".chat-header .room-head", ox as f32, oy as f32, 1).await.expect("click the edge");
        menu_opens_and_closes(app, &format!("the button's edge ({ox}, {oy})")).await;
    }
}

#[tokio::test]
async fn a_channels_hash_and_name_are_one_big_menu_button() {
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
    app.wait_for(".chat-header .room-title", DEFAULT_WAIT).await.expect("title");
    head_button_is_one_big_target(&mut app, ".hash").await;
}

#[tokio::test]
async fn a_dms_picture_and_name_are_one_big_menu_button() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch");
    if !app.is_gpui() {
        return;
    }
    login(&mut app, "alice").await;
    app.click(".rail-dms").await.expect("DMs");
    app.wait_for(".room.dm .room-name", DEFAULT_WAIT).await.expect("a DM");
    app.click(".room.dm").await.expect("open it");
    app.wait_for(".chat-header .room-title", DEFAULT_WAIT).await.expect("title");
    let avatar = app.bounds(".chat-header .avatar").await.expect("b").expect("the DM's avatar");
    assert!((avatar.w - 24.0).abs() < 1.0, "a 24 px picture: {avatar:?}");
    head_button_is_one_big_target(&mut app, ".avatar").await;
}
