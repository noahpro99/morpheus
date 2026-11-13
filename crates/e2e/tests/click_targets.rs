//! Buttons are their natural size, as in Discord: the hit area is the visible
//! button (a 22-34 px box, not a bar's whole height), a click just outside it
//! does nothing, and hovering one only brightens it (no background). Rows
//! stay clickable to their corners. (GPUI only: the Dioxus app is not being
//! changed.)

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

/// A button of a natural size: 22-34 px high, 22 to `max_w` wide.
async fn natural(app: &mut App, sel: &str, max_w: f64) -> morpheus_e2e::Bounds {
    let t = b(app, sel).await;
    assert!(
        (22.0..=34.0).contains(&t.h) && (22.0..=max_w).contains(&t.w),
        "{sel} is not a natural button size (22-34 high, 22-{max_w} wide): {t:?}"
    );
    t
}

/// Clicks `bar` at `dx`, `dy` from its centre.
async fn click_in(app: &mut App, bar: &str, dx: f64, dy: f64) {
    app.click_at(bar, dx as f32, dy as f32, 1).await.expect("click");
}

/// A point `dy_from_top` px under `bar`'s top edge, over `target`'s middle: in the
/// bar but outside a button that is smaller than the bar.
async fn click_bar_top(app: &mut App, bar: &str, target: &str, dy_from_top: f64) {
    let (bar_b, t) = (b(app, bar).await, b(app, target).await);
    click_in(app, bar, (t.x + t.w / 2.0) - (bar_b.x + bar_b.w / 2.0), dy_from_top - bar_b.h / 2.0).await;
}

/// The bar's buttons are boxes of their own size: the member toggle works from
/// its corners, not from the bar's top or bottom edge beside it.
#[tokio::test]
async fn the_bars_buttons_are_natural_sized_boxes() {
    let Some(hs) = homeserver() else { return };
    let Some(mut app) = login(&hs, "alice").await else { return };
    app.open_room("general").await.expect("general");
    app.wait_for(".chat-header .e2e-badge", DEFAULT_WAIT).await.expect("badge");
    natural(&mut app, ".chat-header .e2e-badge", 40.0).await;
    let members = natural(&mut app, ".chat-header .member-toggle", 70.0).await;
    natural(&mut app, ".chat-header .pin-toggle", 70.0).await;
    let bar = b(&mut app, ".chat-header").await;
    assert!(members.h < bar.h - 8.0, "the button is bar-height: {members:?} in {bar:?}");

    // Beside the button, at the bar's top: nothing happens.
    let toggle = ".chat-header .member-toggle";
    let was = app.count(".chat-header .member-toggle.active").await.expect("count");
    click_bar_top(&mut app, ".chat-header", toggle, 2.0).await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(
        app.count(".chat-header .member-toggle.active").await.expect("count"),
        was,
        "a click above the button (in the bar) toggled the member list"
    );
    // Inside its corners: it works.
    for (dx, dy) in [(-1.0, -1.0), (1.0, 1.0)] {
        let was = app.count(".chat-header .member-toggle.active").await.expect("count");
        click_in(&mut app, toggle, dx * (members.w / 2.0 - 2.0), dy * (members.h / 2.0 - 2.0)).await;
        let deadline = std::time::Instant::now() + DEFAULT_WAIT;
        while app.count(".chat-header .member-toggle.active").await.expect("count") == was {
            assert!(std::time::Instant::now() < deadline, "the button's corner ({dx}, {dy}) did nothing");
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
    let pins = b(&mut app, ".chat-header .pin-toggle").await;
    click_in(&mut app, ".chat-header .pin-toggle", pins.w / 2.0 - 2.0, pins.h / 2.0 - 2.0).await;
    app.wait_for(".pinned-dialog", DEFAULT_WAIT).await.expect("the pins button's corner opens the pins");
    app.press(".pinned-dialog", "Escape").await.expect("close");
    app.wait_for_count(".pinned-dialog", 0, DEFAULT_WAIT).await.expect("closed");
    let lock = b(&mut app, ".chat-header .e2e-badge").await;
    click_in(&mut app, ".chat-header .e2e-badge", -(lock.w / 2.0 - 2.0), lock.h / 2.0 - 2.0).await;
    app.wait_for(".privacy-dialog", DEFAULT_WAIT).await.expect("the lock's corner opens privacy");
}

#[tokio::test]
async fn a_dms_call_button_is_a_natural_box() {
    let Some(hs) = homeserver() else { return };
    let Some(mut app) = login(&hs, "alice").await else { return };
    app.click(".rail-dms").await.expect("DMs");
    app.wait_for(".room.dm", DEFAULT_WAIT).await.expect("a DM");
    app.click(".room.dm").await.expect("open it");
    app.wait_for(".chat-header .call-button", DEFAULT_WAIT).await.expect("call button");
    natural(&mut app, ".chat-header .call-button", 44.0).await;
}

/// The search box's ✕ is a small square inside the box (not a box-high strip
/// out to its edge), and its own corners clear the box.
#[tokio::test]
async fn the_search_clear_button_is_a_small_square() {
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
    let x = b(&mut app, ".chat-header .search-clear").await;
    let bar = b(&mut app, ".chat-header .search-bar").await;
    assert!((18.0..=26.0).contains(&x.h) && (18.0..=28.0).contains(&x.w), "the ✕ is {x:?}");
    assert!(x.h < bar.h, "the ✕ runs the whole box height: {x:?} in {bar:?}");
    assert!(x.right() <= bar.right() + 0.5, "the ✕ sticks out of the box: {x:?} in {bar:?}");
    // Beside it, inside the box (at the box's top edge): nothing is cleared.
    click_bar_top(&mut app, ".chat-header .search-bar", ".chat-header .search-clear", 1.0).await;
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert_eq!(app.value(input).await.expect("value").unwrap_or_default(), "okapi", "the box edge cleared it");
    // Its own corner clears it.
    click_in(&mut app, ".chat-header .search-clear", x.w / 2.0 - 2.0, x.h / 2.0 - 2.0).await;
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while !app.value(input).await.expect("value").unwrap_or_default().is_empty() {
        assert!(std::time::Instant::now() < deadline, "the ✕'s corner did not clear the box");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// In a server's sidebar header: the + is ~26 px with an 18 px icon, the
/// server's name is a bar-less 24-34 px high button that opens the server menu.
#[tokio::test]
async fn the_sidebar_headers_buttons_are_natural() {
    let Some(hs) = homeserver() else { return };
    let Some(mut app) = login(&hs, "alice").await else { return };
    app.wait_for(".rail-item[title=\"Projects\"]", DEFAULT_WAIT).await.expect("the Projects badge");
    app.click(".rail-item[title=\"Projects\"]").await.expect("Projects");
    app.wait_for(".sidebar-header h2.server-name", DEFAULT_WAIT).await.expect("server name");
    let plus = natural(&mut app, ".sidebar-header .create-room-button", 32.0).await;
    assert!(plus.w >= 24.0 && plus.h >= 24.0, "the + is a bit small: {plus:?}");
    let name = natural(&mut app, ".sidebar-header h2.server-name", 400.0).await;
    let head = b(&mut app, ".sidebar-header").await;
    assert!(name.h < head.h - 8.0, "the name is header-high: {name:?} in {head:?}");
    // The name's corners open the menu; the header's top edge beside it does not.
    click_bar_top(&mut app, ".sidebar-header", ".sidebar-header h2.server-name", 2.0).await;
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert_eq!(app.count(".context-menu").await.expect("count"), 0, "the header's edge opened the menu");
    for (dx, dy) in [(-1.0, -1.0), (1.0, 1.0)] {
        click_in(&mut app, ".sidebar-header h2.server-name", dx * (name.w / 2.0 - 2.0), dy * (name.h / 2.0 - 2.0)).await;
        app.wait_for(".context-menu", DEFAULT_WAIT)
            .await
            .unwrap_or_else(|e| panic!("no server menu from the name's corner ({dx}, {dy}): {e}"));
        app.press(".context-menu", "Escape").await.expect("escape");
        app.wait_for_count(".context-menu", 0, DEFAULT_WAIT).await.expect("closed");
    }
}

/// The title bar's buttons are boxes of their own.
#[tokio::test]
async fn the_title_bars_buttons_are_natural_sized() {
    let Some(hs) = homeserver() else { return };
    let Some(mut app) = login(&hs, "alice").await else { return };
    for target in [".inbox-button", ".help-button"] {
        natural(&mut app, &format!(".titlebar {target}"), 34.0).await;
    }
    for t in app.all_bounds(".titlebar .nav-button").await.expect("nav buttons") {
        assert!((22.0..=34.0).contains(&t.h) && (22.0..=34.0).contains(&t.w), "a nav button is {t:?}");
    }
    // A click just above the inbox button (still in the bar) opens nothing.
    click_bar_top(&mut app, ".titlebar", ".titlebar .inbox-button", 1.0).await;
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert_eq!(app.count(".inbox-panel").await.expect("count"), 0, "the bar's edge opened the inbox");
    let inbox = b(&mut app, ".titlebar .inbox-button").await;
    click_in(&mut app, ".titlebar .inbox-button", inbox.w / 2.0 - 2.0, -(inbox.h / 2.0 - 2.0)).await;
    app.wait_for(".inbox-panel", DEFAULT_WAIT).await.expect("the inbox button's corner opens the inbox");
}

async fn colour(app: &mut App, sel: &str) -> String {
    app.colours_of(sel, "color").await.expect("colours").into_iter().next().unwrap_or_default()
}

/// Hovering these buttons paints no background: the icon (or name) only
/// turns brighter, and goes back.
#[tokio::test]
async fn hovering_the_enlarged_buttons_only_brightens_them() {
    let Some(hs) = homeserver() else { return };
    let Some(mut app) = login(&hs, "alice").await else { return };
    app.wait_for(".rail-item[title=\"Projects\"]", DEFAULT_WAIT).await.expect("the Projects badge");
    app.click(".rail-item[title=\"Projects\"]").await.expect("Projects");
    app.wait_for(".sidebar-header h2.server-name", DEFAULT_WAIT).await.expect("server name");
    for sel in [
        ".sidebar-header .create-room-button",
        ".sidebar-header h2.server-name",
        ".titlebar .help-button",
        ".titlebar .inbox-button",
    ] {
        app.hover(".titlebar-title").await.expect("away");
        let rest = colour(&mut app, sel).await;
        assert!(!rest.is_empty(), "{sel} notes no colour");
        app.hover(sel).await.expect("hover");
        let over = colour(&mut app, sel).await;
        let bg = app.colours_of(sel, "background").await.expect("bg").into_iter().next().unwrap_or_default();
        assert_eq!(bg, "", "{sel} paints a background when hovered: {bg}");
        assert_ne!(rest, over, "{sel} does not brighten when hovered ({rest})");
        app.hover(".titlebar-title").await.expect("away");
        assert_eq!(colour(&mut app, sel).await, rest, "{sel} keeps its hover colour");
    }
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
