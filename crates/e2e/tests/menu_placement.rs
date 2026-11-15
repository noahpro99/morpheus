//! Menus and flyouts stay inside the window (flipping sideways or upward
//! when there is no room), and every row of a menu stretches to the menu's
//! full width so its highlight reaches the right edge.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, Bounds, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login_and_open(app: &mut App, room: &str) {
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
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("room listed");
    app.open_room(room).await.expect("open room");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
}

fn inside(inner: &Bounds, window: &Bounds) -> bool {
    inner.x >= window.x - 0.5
        && inner.y >= window.y - 0.5
        && inner.right() <= window.right() + 0.5
        && inner.bottom() <= window.bottom() + 0.5
}

async fn assert_menus_inside(app: &mut App, what: &str) {
    let window = app
        .bounds(".shell")
        .await
        .expect("shell")
        .expect("shell bounds");
    let menus = app.all_bounds(".context-menu").await.expect("menus");
    assert!(
        menus.len() >= 2,
        "{what}: a menu and its flyout are open: {menus:?}"
    );
    for menu in &menus {
        assert!(
            inside(menu, &window),
            "{what}: menu {menu:?} leaves the window {window:?}"
        );
    }
}

async fn send(app: &mut App, text: &str) {
    app.type_into(".composer .composer-input", text)
        .await
        .expect("type");
    app.press(".composer .composer-input", "Enter")
        .await
        .expect("send");
    app.wait_for_exact_text(".message.own:not(.pending) .body", text, DEFAULT_WAIT)
        .await
        .expect("message sent");
}

#[tokio::test]
async fn the_profile_menu_and_its_flyout_stay_inside_the_window() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_sized(&hs, 800, 500).await.expect("launch app");
    login_and_open(&mut app, "general").await;
    app.click(".profile-chip").await.expect("chip");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("menu");
    app.click_text(
        ".context-menu > .context-item.has-submenu",
        "Switch account",
    )
    .await
    .expect("open Switch account");
    app.wait_for(".context-submenu .context-item", DEFAULT_WAIT)
        .await
        .expect("flyout");
    assert_menus_inside(&mut app, "profile").await;
}

#[tokio::test]
async fn a_tall_flyout_at_the_bottom_stays_inside_the_window() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_sized(&hs, 800, 560).await.expect("launch app");
    login_and_open(&mut app, "general").await;
    let text = format!("menu placement {}", std::process::id());
    send(&mut app, &text).await;
    app.right_click_text(".message.own", &text)
        .await
        .expect("right-click");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("menu");
    app.click_text(".context-menu > .context-item.has-submenu", "Forward to")
        .await
        .expect("open Forward to");
    app.wait_for(".context-submenu .context-item", DEFAULT_WAIT)
        .await
        .expect("flyout");
    assert_menus_inside(&mut app, "message").await;
}

#[tokio::test]
async fn every_row_of_a_menu_spans_the_menu_width() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;
    let text = format!("menu widths {}", std::process::id());
    send(&mut app, &text).await;
    app.right_click_text(".message.own", &text)
        .await
        .expect("right-click");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("menu");
    app.click_text(".context-menu > .context-item.has-submenu", "Forward to")
        .await
        .expect("open Forward to");
    app.wait_for(".context-submenu .context-item", DEFAULT_WAIT)
        .await
        .expect("flyout");
    let submenu = app
        .bounds(".context-submenu")
        .await
        .expect("b")
        .expect("flyout bounds");
    let rows = app
        .all_bounds(".context-submenu .context-item")
        .await
        .expect("rows");
    assert!(rows.len() >= 2, "several rows: {rows:?}");
    for row in &rows {
        // The panel's padding (6) and border (1) on each side.
        assert!(
            (row.w - (submenu.w - 14.0)).abs() <= 1.0,
            "row {row:?} should fill the flyout {submenu:?}"
        );
    }
    let menu = app
        .bounds(".context-menu:not(.context-submenu)")
        .await
        .expect("b")
        .expect("menu bounds");
    let top = app
        .all_bounds(".context-menu:not(.context-submenu) > .context-item")
        .await
        .expect("rows");
    for row in &top {
        assert!(
            (row.w - (menu.w - 14.0)).abs() <= 1.0,
            "row {row:?} should fill the menu {menu:?}"
        );
    }
}
