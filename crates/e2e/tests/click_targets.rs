//! Clickable areas fill their natural box, not just the glyph or text in
//! it: a bar's buttons run the bar's height, a row is clickable to its
//! edges. (GPUI only: the Dioxus app is not being changed.)

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login(hs: &str, user: &str) -> Option<App> {
    let mut app = App::launch(hs).await.expect("launch");
    if !app.is_gpui() {
        return None;
    }
    app.wait_for("form.login", DEFAULT_WAIT).await.expect("login form");
    app.type_into("form.login input[name=username]", user).await.expect("username");
    app.type_into("form.login input[name=password]", &format!("{user}-password"))
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    Some(app)
}

async fn b(app: &mut App, sel: &str) -> morpheus_e2e::Bounds {
    app.bounds(sel).await.expect("bounds").unwrap_or_else(|| panic!("no {sel}"))
}

/// Clicks `bar`, over `target`'s middle horizontally, `inset` px inside its
/// top (or bottom) edge: where a button as tall as the bar still is.
async fn click_bar_edge(app: &mut App, bar: &str, target: &str, top: bool, inset: f64) {
    let (bar_b, t) = (b(app, bar).await, b(app, target).await);
    let ox = (t.x + t.w / 2.0) - (bar_b.x + bar_b.w / 2.0);
    let oy = if top { inset - bar_b.h / 2.0 } else { bar_b.h / 2.0 - inset - 1.0 };
    app.click_at(bar, ox as f32, oy as f32, 1).await.expect("click the edge");
}

async fn as_tall_as(app: &mut App, bar: &str, target: &str) {
    let (bar_b, t) = (b(app, bar).await, b(app, target).await);
    assert!(t.h >= bar_b.h - 3.0, "{target} should run the height of {bar}: {t:?} in {bar_b:?}");
}

/// The member list and pins buttons and the lock fill the bar's height, so
/// a click at the bar's top or bottom edge over them works.
#[tokio::test]
async fn the_bars_buttons_are_as_tall_as_the_bar() {
    let Some(hs) = homeserver() else { return };
    let Some(mut app) = login(&hs, "alice").await else { return };
    app.open_room("general").await.expect("general");
    app.wait_for(".chat-header .e2e-badge", DEFAULT_WAIT).await.expect("badge");
    let bar = ".chat-header";
    for target in [".member-toggle", ".pin-toggle", ".e2e-badge"] {
        as_tall_as(&mut app, bar, &format!(".chat-header {target}")).await;
    }
    for top in [true, false] {
        let toggle = ".chat-header .member-toggle";
        let was = app.count(".chat-header .member-toggle.active").await.expect("count");
        click_bar_edge(&mut app, bar, toggle, top, 3.0).await;
        let deadline = std::time::Instant::now() + DEFAULT_WAIT;
        while app.count(".chat-header .member-toggle.active").await.expect("count") == was {
            assert!(std::time::Instant::now() < deadline, "member toggle edge click (top {top}) did nothing");
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
    click_bar_edge(&mut app, bar, ".chat-header .pin-toggle", true, 3.0).await;
    app.wait_for(".pinned-dialog", DEFAULT_WAIT).await.expect("the pins button's top edge opens the pins");
    app.press(".pinned-dialog", "Escape").await.expect("close");
    app.wait_for_count(".pinned-dialog", 0, DEFAULT_WAIT).await.expect("closed");
    click_bar_edge(&mut app, bar, ".chat-header .e2e-badge", false, 3.0).await;
    app.wait_for(".privacy-dialog", DEFAULT_WAIT).await.expect("the lock's bottom edge opens privacy");
}

#[tokio::test]
async fn a_dms_call_button_is_as_tall_as_the_bar() {
    let Some(hs) = homeserver() else { return };
    let Some(mut app) = login(&hs, "alice").await else { return };
    app.click(".rail-dms").await.expect("DMs");
    app.wait_for(".room.dm", DEFAULT_WAIT).await.expect("a DM");
    app.click(".room.dm").await.expect("open it");
    app.wait_for(".chat-header .call-button", DEFAULT_WAIT).await.expect("call button");
    as_tall_as(&mut app, ".chat-header", ".chat-header .call-button").await;
}

/// The search box's ✕ runs the box's height and to its right edge.
#[tokio::test]
async fn the_search_clear_button_fills_its_corner() {
    let Some(hs) = homeserver() else { return };
    let Some(mut app) = login(&hs, "alice").await else { return };
    app.open_room("general").await.expect("general");
    let input = ".chat-header input[name=search]";
    app.click(input).await.expect("focus");
    app.type_into(input, "okapi").await.expect("query");
    app.wait_for(".chat-header .search-clear", DEFAULT_WAIT).await.expect("the clear button");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        // The box widens over 150 ms: wait for it to rest.
        let w1 = b(&mut app, ".chat-header .search-bar").await.w;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        if (w1 - b(&mut app, ".chat-header .search-bar").await.w).abs() < 0.5 {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "never settled");
    }
    as_tall_as(&mut app, ".chat-header .search-bar", ".chat-header .search-clear").await;
    let bar = b(&mut app, ".chat-header .search-bar").await;
    let x = b(&mut app, ".chat-header .search-clear").await;
    assert!(x.right() >= bar.right() - 2.0, "runs to the box's right edge: {x:?} in {bar:?}");
    // The bottom-right corner of the box clears it.
    app.click_at(".chat-header .search-bar", (bar.w / 2.0 - 2.0) as f32, (bar.h / 2.0 - 2.0) as f32, 1)
        .await
        .expect("click the corner");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while !app.value(input).await.expect("value").unwrap_or_default().is_empty() {
        assert!(std::time::Instant::now() < deadline, "the box's corner did not clear it");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// A server's name runs the sidebar header's height and out to the + button;
/// the + runs the height too.
#[tokio::test]
async fn the_server_name_fills_the_sidebar_header() {
    let Some(hs) = homeserver() else { return };
    let Some(mut app) = login(&hs, "alice").await else { return };
    app.wait_for(".rail-item[title=\"Projects\"]", DEFAULT_WAIT).await.expect("the Projects badge");
    app.click(".rail-item[title=\"Projects\"]").await.expect("Projects");
    app.wait_for(".sidebar-header h2.server-name", DEFAULT_WAIT).await.expect("server name");
    as_tall_as(&mut app, ".sidebar-header", ".sidebar-header .create-room-button").await;
    let h = b(&mut app, ".sidebar-header").await;
    let plus = b(&mut app, ".sidebar-header .create-room-button").await;
    // The left padding, and the gap before the +, open the menu too.
    for x in [h.x + 4.0, plus.x - 6.0] {
        for y in [h.y + 3.0, h.y + h.h - 5.0] {
            app.click_at(".sidebar-header", (x - (h.x + h.w / 2.0)) as f32, (y - (h.y + h.h / 2.0)) as f32, 1)
                .await
                .expect("click");
            app.wait_for(".context-menu", DEFAULT_WAIT)
                .await
                .unwrap_or_else(|e| panic!("no server menu from ({x}, {y}): {e}"));
            app.press(".context-menu", "Escape").await.expect("escape");
            app.wait_for_count(".context-menu", 0, DEFAULT_WAIT).await.expect("closed");
        }
    }
}

/// The title bar's inbox button runs the bar's height.
#[tokio::test]
async fn the_title_bars_buttons_are_as_tall_as_the_bar() {
    let Some(hs) = homeserver() else { return };
    let Some(mut app) = login(&hs, "alice").await else { return };
    for target in [".inbox-button", ".help-button", ".nav-button"] {
        as_tall_as(&mut app, ".titlebar", &format!(".titlebar {target}")).await;
    }
    click_bar_edge(&mut app, ".titlebar", ".titlebar .inbox-button", true, 2.0).await;
    app.wait_for(".inbox-panel", DEFAULT_WAIT).await.expect("the inbox button's top edge opens the inbox");
}

/// Rows are clickable to their corners.
#[tokio::test]
async fn rows_are_clickable_to_their_corners() {
    let Some(hs) = homeserver() else { return };
    let Some(mut app) = login(&hs, "alice").await else { return };
    app.open_room("general").await.expect("general");
    app.wait_for(".side-pane .member", DEFAULT_WAIT).await.expect("members");
    let row = b(&mut app, ".side-pane .member").await;
    for (ox, oy) in [(-1.0, -1.0), (1.0, 1.0)] {
        app.click_at(".side-pane .member", (ox * (row.w / 2.0 - 2.0)) as f32, (oy * (row.h / 2.0 - 2.0)) as f32, 1)
            .await
            .expect("click the corner");
        app.wait_for(".profile-card", DEFAULT_WAIT).await.expect("a member row's corner opens the card");
        app.press(".profile-card", "Escape").await.expect("escape");
        app.wait_for_count(".profile-card", 0, DEFAULT_WAIT).await.expect("closed");
    }
    app.open_room("random").await.expect("random");
    app.wait_for(".room.selected", DEFAULT_WAIT).await.expect("selected");
    let row = b(&mut app, ".room:not(.selected):not(.dm)").await;
    app.click_at(".room:not(.selected):not(.dm)", (row.w / 2.0 - 2.0) as f32, (row.h / 2.0 - 2.0) as f32, 1)
        .await
        .expect("click the corner");
    app.wait_for_count(".room.selected", 1, DEFAULT_WAIT).await.expect("one selected");
}
