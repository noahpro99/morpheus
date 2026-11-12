//! Rail layout: drag a server onto another to make a folder, open it, move a
//! server out through the menu, rename and ungroup the folder, reorder by
//! dropping between badges. The layout is stored in account data.

#![allow(clippy::expect_used)]

use std::time::Duration;

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
    app.wait_for(".shell", DEFAULT_WAIT)
        .await
        .expect("signed in");
}

async fn create_server(app: &mut App, name: &str) {
    app.click(".rail-add").await.expect("rail plus");
    app.wait_for(".create-space-dialog input[name=name]", DEFAULT_WAIT)
        .await
        .expect("dialog");
    app.type_into(".create-space-dialog input[name=name]", name)
        .await
        .expect("name");
    app.submit(".create-space-dialog").await.expect("create");
    app.wait_for(&format!(".rail-item[title=\"{name}\"]"), DEFAULT_WAIT)
        .await
        .expect("badge");
}

/// Press `from`, move the pointer onto `to` (its middle, or just inside its
/// top edge with `before`) and, unless `hold`, let go (a drag the app sees
/// as real pointer input, in either UI).
async fn drag_to(app: &mut App, from: &str, to: &str, before: bool, hold: bool) {
    let a = app.bounds(from).await.expect("from").expect("a source badge");
    let b = app.bounds(to).await.expect("to").expect("a target badge");
    let y = if before { b.y + 3.0 } else { b.y + b.h / 2.0 };
    let (dx, dy) = (
        (b.centre_x() - a.centre_x()).round() as i32,
        (y - (a.y + a.h / 2.0)).round() as i32,
    );
    app.drag_hold(from, dx, dy).await.expect("drag");
    if !hold {
        app.release().await.expect("let go");
    }
}

async fn drag(app: &mut App, from: &str, to: &str) {
    drag_to(app, from, to, false, false).await;
}

async fn drag_before(app: &mut App, from: &str, to: &str) {
    drag_to(app, from, to, true, false).await;
}

/// The titles of the rail's servers and folders, top-level, in order (a
/// folder has none).
async fn top_level(app: &mut App) -> Vec<String> {
    app.attrs(".rail > .rail-item.space-badge, .rail > .rail-folder", "title")
        .await
        .expect("titles")
        .into_iter()
        .map(Option::unwrap_or_default)
        .collect()
}

/// The titles of the rail's server badges (not in folders), in order.
async fn badge_titles(app: &mut App) -> Vec<String> {
    app.attrs(".rail > .rail-item.space-badge", "title")
        .await
        .expect("titles")
        .into_iter()
        .map(Option::unwrap_or_default)
        .collect()
}

/// Wait until `want` holds of the top-level entries' titles.
async fn wait_top_level(app: &mut App, what: &str, want: impl Fn(&[String]) -> bool) {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let titles = top_level(app).await;
        if want(&titles) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{what}: the rail's top level is {titles:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn servers_drag_into_folders_and_reorder() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    // A new account: no folders left behind by earlier runs, and nothing to
    // clean up afterwards.
    let user = new_account(&hs, "rail").expect("account");
    let mut app = App::launch(&hs).await.expect("launch app");
    login(&mut app, &user).await;
    let pid = std::process::id();
    let (a, b) = (format!("srv a {pid}"), format!("srv b {pid}"));
    create_server(&mut app, &a).await;
    create_server(&mut app, &b).await;

    // Drop b onto a: a folder appears holding both; open it to see them.
    drag(
        &mut app,
        &format!(".rail-item[title=\"{b}\"]"),
        &format!(".rail-item[title=\"{a}\"]"),
    )
    .await;
    // Both left the top level (into the new folder).
    wait_top_level(&mut app, "a folder of the two", |titles| {
        !titles.contains(&a) && !titles.contains(&b)
    })
    .await;
    // The new layout is saved to the account and comes back: let it settle.
    tokio::time::sleep(Duration::from_secs(1)).await;
    app.wait_for(".rail-folder .folder-badge", DEFAULT_WAIT)
        .await
        .expect("folder created");
    let folder_name = format!("Work {pid}");
    app.right_click(".rail-folder .folder-badge")
        .await
        .expect("folder menu");
    app.click_text(".context-item", "Rename folder")
        .await
        .expect("rename");
    app.wait_for(".rename-folder-dialog input[name=name]", DEFAULT_WAIT)
        .await
        .expect("dialog");
    app.type_into(".rename-folder-dialog input[name=name]", &folder_name)
        .await
        .expect("name");
    app.submit(".rename-folder-dialog").await.expect("save");
    let badge = format!(".rail-folder .folder-badge[title=\"{folder_name}\"]");
    app.wait_for(&badge, DEFAULT_WAIT).await.expect("renamed");
    app.click(&badge).await.expect("open folder");
    app.wait_for(
        &format!(".rail-folder.open .folder-items .rail-item[title=\"{a}\"]"),
        DEFAULT_WAIT,
    )
    .await
    .expect("a inside");
    app.wait_for(
        &format!(".rail-folder.open .folder-items .rail-item[title=\"{b}\"]"),
        DEFAULT_WAIT,
    )
    .await
    .expect("b inside");

    // Move b out through the server menu.
    app.right_click(&format!(".folder-items .rail-item[title=\"{b}\"]"))
        .await
        .expect("server menu");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("menu");
    app.click_submenu("Move to", "No folder")
        .await
        .expect("move out");
    app.wait_for(&format!(".rail > .rail-item[title=\"{b}\"]"), DEFAULT_WAIT)
        .await
        .expect("b is a top-level badge again");

    // Reorder: let go of b just above the first server, so it comes first.
    drag_before(
        &mut app,
        &format!(".rail-item[title=\"{b}\"]"),
        ".rail > [data-index=\"0\"]",
    )
    .await;
    wait_top_level(&mut app, "b first", |titles| titles.first() == Some(&b)).await;

    // The layout survives a relaunch (it lives in account data).
    let mut app = app.relaunch().await.expect("relaunch");
    app.wait_for(".shell", DEFAULT_WAIT)
        .await
        .expect("restored");
    app.wait_for(&badge, DEFAULT_WAIT)
        .await
        .expect("folder remembered");
    wait_top_level(&mut app, "order remembered", |titles| titles.first() == Some(&b)).await;

    // Ungroup: the folder goes, its servers stay.
    app.right_click(&badge).await.expect("folder menu");
    app.click_text(".context-item", "Ungroup")
        .await
        .expect("ungroup");
    app.wait_for_count(&badge, 0, DEFAULT_WAIT)
        .await
        .expect("folder gone");
    app.wait_for(&format!(".rail > .rail-item[title=\"{a}\"]"), DEFAULT_WAIT)
        .await
        .expect("a is a badge again");
}

/// The rail marks where you are and what is waiting, as Discord's does: a
/// white bar beside the scope you are in, and a white dot beside any other
/// with unread messages.
/// The mark (a bar or a dot) beside a badge: where it is.
async fn pill_of(app: &mut App, badge: &str) -> Option<morpheus_e2e::Bounds> {
    app.bounds(&format!("{badge} .rail-pill")).await.ok().flatten()
}

#[tokio::test]
async fn the_rail_marks_the_current_scope_and_unread_ones() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login(&mut app, "alice").await;
    // Over in the DMs; then a message for alice in #random (under Rooms).
    app.click(".rail-item[title=\"Direct messages\"]")
        .await
        .expect("dms");
    let random = api_as(
        &hs,
        "bob",
        "GET",
        "directory/room/%23random:localhost",
        None,
    )
    .expect("random's id");
    let random = random["room_id"]
        .as_str()
        .expect("id")
        .replace('!', "%21")
        .replace(':', "%3A");
    let text = format!("rail dot {}", std::process::id());
    api_as(
        &hs,
        "bob",
        "PUT",
        &format!("rooms/{random}/send/m.room.message/{}", text.len()),
        Some(serde_json::json!({ "msgtype": "m.text", "body": text })),
    )
    .expect("bob writes");

    let selected = ".rail-item.selected";
    let rooms = ".rail-item[title=\"Rooms\"]";
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let tall = pill_of(&mut app, selected).await.is_some_and(|m| m.h >= 32.0);
        let dot = pill_of(&mut app, rooms).await.is_some_and(|m| m.h > 0.0 && m.h <= 10.0);
        if tall && dot {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "a tall bar beside the scope you are in and a dot beside Rooms, which has an unread message"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    for selector in [selected, rooms] {
        let pill = pill_of(&mut app, selector).await.expect("mark");
        let item = app.bounds(selector).await.expect("badge").expect("its badge");
        let colours = app
            .colours_of(&format!("{selector} .rail-pill"), "background")
            .await
            .expect("colour");
        assert_eq!(colours, vec!["#ffffff".to_owned()], "{selector}: not white");
        assert!(
            pill.right() <= item.x,
            "{selector}: not to the left of its badge: {pill:?} {item:?}"
        );
    }
}

/// Servers (and the rail's other badges) are rounded squares, not circles.
#[tokio::test]
async fn rail_badges_are_rounded_squares() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login(&mut app, "alice").await;
    app.wait_for(".rail-item.space-badge", DEFAULT_WAIT)
        .await
        .expect("a server");
    // Each badge, and the picture inside it: [title, corner radius, width].
    let round = if app.is_gpui() {
        // The badges note the corner radius they are drawn with.
        let badges = ".rail > .rail-item, .rail-folder .rail-item";
        let radii = app.attrs(badges, "data-radius").await.expect("radii");
        let boxes = app.all_bounds(badges).await.expect("boxes");
        radii
            .into_iter()
            .zip(boxes)
            .filter_map(|(r, b)| Some((r?.parse::<f64>().ok()?, b.w)))
            .filter(|(r, w)| *r >= w * 0.45 || *r < w * 0.2)
            .map(|(r, w)| serde_json::json!([r, w]))
            .collect::<Vec<_>>()
            .into()
    } else {
        app.eval(
            "return [...document.querySelectorAll('.rail > .rail-item, .rail-folder .rail-item, .rail-item .avatar')] \
               .filter(el => !el.matches('.rail-add')) \
               .map(el => [el.title || el.className, parseFloat(getComputedStyle(el).borderTopLeftRadius), el.getBoundingClientRect().width]) \
               .filter(([, r, w]) => r >= w * 0.45 || r < w * 0.2);",
        )
        .await
        .expect("badges")
    };
    assert_eq!(
        round,
        serde_json::json!([]),
        "badges that are circles (or square)"
    );
}

/// Dragging a server is plain: the pointer turns into a grabbing hand (no
/// picture of the badge follows it), a white line shows where it will land,
/// nothing else moves or spaces out, and letting go puts it there.
#[tokio::test]
async fn dragging_a_server_shows_a_line_where_it_lands() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let user = new_account(&hs, "drag").expect("account");
    let mut app = App::launch(&hs).await.expect("launch app");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", &user)
        .await
        .expect("username");
    app.type_into(
        "form.login input[name=password]",
        &format!("{user}-password"),
    )
    .await
    .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".shell", DEFAULT_WAIT)
        .await
        .expect("signed in");
    for name in ["one", "two", "three"] {
        create_server(&mut app, name).await;
    }
    assert_eq!(
        badge_titles(&mut app).await,
        ["three", "two", "one"],
        "new servers go on top"
    );
    assert_eq!(
        app.count(".rail [draggable=true]").await.expect("count"),
        0,
        "badges use the browser's drag (which drags a picture of them)"
    );
    let tops = |bounds: Vec<morpheus_e2e::Bounds>| -> Vec<i64> {
        bounds.iter().map(|b| b.y.round() as i64).collect()
    };
    let before = tops(app.all_bounds(".rail > .rail-item").await.expect("tops"));

    // New servers go on top: hold "three" just above "one".
    drag_to(
        &mut app,
        ".rail-item[title=\"three\"]",
        ".rail-item[title=\"one\"]",
        true,
        true,
    )
    .await;
    let line = app
        .bounds(".rail-drop-line")
        .await
        .expect("line")
        .expect("a line shows where it lands");
    let two = app
        .bounds(".rail-item[title=\"two\"]")
        .await
        .expect("two")
        .expect("two");
    let one = app
        .bounds(".rail-item[title=\"one\"]")
        .await
        .expect("one")
        .expect("one");
    assert_eq!(cursor(&mut app).await, "grabbing", "the pointer grabs");
    assert!(
        line.h > 0.0 && line.h <= 4.0 && line.w >= 24.0,
        "a thin line: {line:?}"
    );
    assert_eq!(line_colour(&mut app).await, "#ffffff", "a white line");
    assert!(
        line.y >= two.y + two.h - 2.0 && line.y + line.h <= one.y + 2.0,
        "between two and one: {line:?}, {two:?}, {one:?}"
    );
    assert_eq!(
        tops(app.all_bounds(".rail > .rail-item").await.expect("tops")),
        before,
        "the badges moved while dragging"
    );

    // Let go: "three" lands between two and one.
    app.release().await.expect("let go");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while badge_titles(&mut app).await != ["two", "three", "one"] {
        assert!(
            std::time::Instant::now() < deadline,
            "moved: {:?}",
            badge_titles(&mut app).await
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        app.count(".rail-drop-line").await.expect("count"),
        0,
        "the line goes"
    );
    assert_eq!(cursor(&mut app).await, "auto", "the pointer is back");
}

/// The drop line's colour as `#rrggbb` (the page's computed style, or what the
/// GPUI app notes on the line; the line is fixed in the page, which
/// `colours_of` skips).
async fn line_colour(app: &mut App) -> String {
    if app.is_gpui() {
        return app
            .colours_of(".rail-drop-line", "background")
            .await
            .expect("colour")
            .into_iter()
            .next()
            .unwrap_or_default();
    }
    let rgb = app
        .eval("const line = document.querySelector('.rail-drop-line'); return line ? getComputedStyle(line).backgroundColor : null;")
        .await
        .expect("colour");
    match rgb.as_str() {
        Some("rgb(255, 255, 255)") => "#ffffff".into(),
        other => format!("{other:?}"),
    }
}

/// The pointer's look: the page's `cursor` (Dioxus), or what the GPUI app
/// notes on its body (`data-cursor`: grabbing while something is dragged).
async fn cursor(app: &mut App) -> String {
    if app.is_gpui() {
        return app
            .attr("body.app", "data-cursor")
            .await
            .expect("cursor")
            .unwrap_or_default();
    }
    app.eval("return getComputedStyle(document.body).cursor;")
        .await
        .expect("cursor")
        .as_str()
        .unwrap_or_default()
        .to_owned()
}
