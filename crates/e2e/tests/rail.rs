//! Rail layout: drag a server onto another to make a folder, open it, move a
//! server out through the menu, rename and ungroup the folder, reorder by
//! dropping between badges. The layout is stored in account data.

#![allow(clippy::expect_used)]

use std::time::Duration;

mod common;

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

/// Discord's rail pill: none beside a read server, a small dot beside one
/// with unread messages, a medium bar while the pointer is over a server,
/// the full-height bar beside the server you are in.
#[tokio::test]
async fn the_rail_pill_grows_on_hover_and_is_tall_when_selected() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let user = new_account(&hs, "pill").expect("account");
    let enc = |id: &str| id.replace('!', "%21").replace(':', "%3A");
    api_as(&hs, &user, "POST", "createRoom", Some(serde_json::json!({ "name": "lobby" }))).expect("lobby");
    let mut last_channel = String::new();
    for (server, channel) in [("Pread", "p-read"), ("Punread", "p-unread")] {
        let space = api_as(
            &hs,
            &user,
            "POST",
            "createRoom",
            Some(serde_json::json!({ "name": server, "creation_content": { "type": "m.space" } })),
        )
        .expect("space");
        let space = space["room_id"].as_str().expect("id").to_owned();
        let room = api_as(&hs, &user, "POST", "createRoom", Some(serde_json::json!({ "name": channel })))
            .expect("channel");
        let room = room["room_id"].as_str().expect("id").to_owned();
        api_as(
            &hs,
            &user,
            "PUT",
            &format!("rooms/{}/state/m.space.child/{room}", enc(&space)),
            Some(serde_json::json!({ "via": ["localhost"] })),
        )
        .expect("child");
        last_channel = room;
    }
    let mut app = App::launch(&hs).await.expect("launch app");
    if !app.is_gpui() {
        eprintln!("the pill's hover is only driven in the GPUI app; skipping");
        return;
    }
    login(&mut app, &user).await;
    let read = ".rail-item.space-badge[title=\"Pread\"]";
    let unread = ".rail-item.space-badge[title=\"Punread\"]";
    app.wait_for(read, DEFAULT_WAIT).await.expect("server");
    app.wait_for(unread, DEFAULT_WAIT).await.expect("server");
    // Someone else writes in the second server's channel.
    let room = enc(&last_channel);
    api_as(
        &hs,
        &user,
        "POST",
        &format!("rooms/{room}/invite"),
        Some(serde_json::json!({ "user_id": "@bob:localhost" })),
    )
    .expect("invite bob");
    api_as(&hs, "bob", "POST", &format!("rooms/{room}/join"), Some(serde_json::json!({}))).expect("bob joins");
    morpheus_e2e::send_as(&hs, "bob", &last_channel, "pill unread").expect("bob writes");

    let height = |m: Option<morpheus_e2e::Bounds>| m.map_or(0.0, |m| m.h);
    // Wait for the small dot beside the unread server.
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let h = height(pill_of(&mut app, unread).await);
        if h > 0.0 && h <= 10.0 {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "a small dot beside the unread server (now {h})");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    // Read, not selected, not hovered: nothing. (Pointer parked on the logo.)
    app.hover(".rail-item[title=\"Rooms\"]").await.expect("park");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(height(pill_of(&mut app, read).await) < 1.0, "no pill beside a read server");

    // Hover: a medium bar, between the dot and the full bar.
    app.hover(read).await.expect("hover");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let h = height(pill_of(&mut app, read).await);
        if (14.0..=28.0).contains(&h) {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "a medium bar while hovered (now {h})");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // An unread server grows too.
    app.hover(unread).await.expect("hover unread");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        (14.0..=28.0).contains(&height(pill_of(&mut app, unread).await)),
        "a hovered unread server has the medium bar"
    );

    // Selected: the tall bar, and it stays tall under the pointer.
    app.click(read).await.expect("open the server");
    tokio::time::sleep(Duration::from_millis(400)).await;
    let tall = height(pill_of(&mut app, ".rail-item.selected").await);
    assert!(tall >= 32.0, "the server you are in has a tall bar (now {tall})");
    // Not hovered any more, still tall; the other server is back to a dot.
    app.hover(".rail-item[title=\"Rooms\"]").await.expect("away");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(height(pill_of(&mut app, ".rail-item.selected").await) >= 32.0, "still tall");
    assert!(height(pill_of(&mut app, read).await) >= 32.0, "the selected one is the server");
    let h = height(pill_of(&mut app, unread).await);
    assert!(h > 0.0 && h <= 10.0, "an unread server is a dot again (now {h})");
}

// ---- Calls on the rail (task 39) ------------------------------------------

/// Bob joins `channel`'s call: through the switcher to its page, then Join.
async fn bob_joins_voice(bob: &mut App, channel: &str) {
    bob.key_combo("body", "k", true, false, false).await.expect("ctrl+k");
    bob.wait_for(".switcher input", DEFAULT_WAIT).await.expect("switcher");
    bob.type_into(".switcher input", channel).await.expect("filter");
    bob.wait_for_text(".switcher-item.active", channel, DEFAULT_WAIT).await.expect("offered");
    bob.press(".switcher input", "Enter").await.expect("pick");
    bob.wait_for(".voice-page-join .join-voice", DEFAULT_WAIT).await.expect("join page");
    bob.click(".voice-page-join .join-voice").await.expect("join");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT).await.expect("connected");
}

/// A badge with a green circle and a speaker at the icon's top right, ringed
/// in the rail's colour; a pixel check where the session can be captured.
async fn check_call_badge(app: &mut App, icon: &str) {
    let sel = format!("{icon} .rail-call-badge");
    app.wait_for(&sel, DEFAULT_WAIT).await.unwrap_or_else(|e| panic!("no call badge on {icon}: {e}"));
    let badge = app.bounds(&sel).await.expect("b").expect("the call badge");
    let owner = app.bounds(icon).await.expect("b").expect("the icon");
    assert!((16.0..=24.0).contains(&badge.w) && (badge.w - badge.h).abs() < 0.5, "a small circle: {badge:?}");
    assert!(
        badge.centre_x() > owner.x + owner.w * 0.6 && badge.y + badge.h / 2.0 < owner.y + owner.h * 0.4,
        "at the icon's top right: {badge:?} on {owner:?}"
    );
    assert_eq!(app.count(&format!("{sel} svg")).await.expect("count"), 1, "a speaker icon in it");
    tokio::time::sleep(Duration::from_millis(400)).await;
    let Some(shot) = capture_of(app) else { return };
    let cx = badge.centre_x();
    let green = pixel(&shot, cx, badge.y + 5.0);
    assert!(
        green.1 > green.0 + 50 && green.1 > green.2 + 20,
        "the circle is not green at its top: {green:?}"
    );
    let ring = pixel(&shot, cx, badge.y + 1.0);
    let rail = app.bounds(".rail").await.expect("b").expect("rail");
    let behind = pixel(&shot, rail.x + 8.0, badge.y + badge.h + 3.0);
    assert!(
        (i32::from(ring.0) - i32::from(behind.0)).abs() + (i32::from(ring.1) - i32::from(behind.1)).abs()
            + (i32::from(ring.2) - i32::from(behind.2)).abs() < 16,
        "the ring is not the rail's colour: ring {ring:?}, rail {behind:?}"
    );
}

fn capture_of(app: &App) -> Option<(Vec<u8>, usize, usize)> {
    common::capture(app)
}

fn pixel(shot: &(Vec<u8>, usize, usize), x: f64, y: f64) -> (u8, u8, u8) {
    common::pixel_at(shot, x, y)
}

/// A server (or a folder holding it) with someone in a voice channel's call
/// shows a green speaker badge at its icon's top right; it goes when the call
/// ends.
#[tokio::test]
async fn a_server_with_a_call_going_shows_a_green_speaker_badge() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let ann = new_account(&hs, "railcall").expect("account");
    let pid = std::process::id();
    let (a, b, channel) = (format!("rc a {pid}"), format!("rc b {pid}"), format!("rc voice {pid}"));
    let space = |name: &str| {
        api_as(&hs, &ann, "POST", "createRoom", Some(serde_json::json!({
            "name": name, "preset": "public_chat", "creation_content": { "type": "m.space" },
        })))
        .expect("a space")["room_id"].as_str().expect("id").to_owned()
    };
    let (sa, _sb) = (space(&a), space(&b));
    let voice = api_as(&hs, &ann, "POST", "createRoom", Some(serde_json::json!({
        "name": channel, "preset": "public_chat",
        "creation_content": { "type": "org.matrix.msc3417.call" },
        "power_level_content_override": { "events": { "org.matrix.msc3401.call.member": 0 } },
        "initial_state": [{ "type": "m.space.parent", "state_key": sa, "content": { "via": ["localhost"], "canonical": true } }],
        "invite": ["@bob:localhost"],
    })))
    .expect("a voice channel")["room_id"].as_str().expect("id").to_owned();
    let enc = |id: &str| id.replace('!', "%21").replace(':', "%3A");
    api_as(&hs, &ann, "PUT", &format!("rooms/{}/state/m.space.child/{}", enc(&sa), enc(&voice)),
        Some(serde_json::json!({ "via": ["localhost"] }))).expect("a child");
    api_as(&hs, "bob", "POST", &format!("rooms/{}/join", enc(&voice)), Some(serde_json::json!({}))).expect("bob joins");

    // Bob first: the later window is on top, and the pixel check sees ann's.
    let mut bob = App::launch_desktop(&hs).await.expect("launch bob");
    let mut app = App::launch(&hs).await.expect("launch ann");
    if !app.is_gpui() {
        return;
    }
    login(&mut app, &ann).await;
    let (badge_a, badge_b) = (format!(".rail-item[title=\"{a}\"]"), format!(".rail-item[title=\"{b}\"]"));
    app.wait_for(&badge_a, DEFAULT_WAIT).await.expect("server a");
    app.wait_for(&badge_b, DEFAULT_WAIT).await.expect("server b");
    assert_eq!(app.count(".rail-call-badge").await.expect("count"), 0, "a badge with no call");

    // Bob starts a call in the channel: the badge is on its server only.
    login(&mut bob, "bob").await;
    bob_joins_voice(&mut bob, &channel).await;
    check_call_badge(&mut app, &badge_a).await;
    assert_eq!(app.count(&format!("{badge_b} .rail-call-badge")).await.expect("count"), 0, "the other server has one");
    assert_eq!(app.count(".rail-call-badge").await.expect("count"), 1, "one badge");
    // Hung up: gone.
    bob.click(".voice-panel .hang-up").await.expect("hang up");
    app.wait_for_count(".rail-call-badge", 0, DEFAULT_WAIT).await.expect("the badge goes with the call");

    // The two servers in a folder: the folder carries it.
    drag(&mut app, &badge_b, &badge_a).await;
    wait_top_level(&mut app, "a folder of the two", |t| !t.contains(&a) && !t.contains(&b)).await;
    app.wait_for(".rail-folder", DEFAULT_WAIT).await.expect("folder");
    bob.click(".voice-page-join .join-voice").await.expect("join again");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT).await.expect("connected");
    check_call_badge(&mut app, ".rail-folder .folder-badge").await;
    bob.click(".voice-panel .hang-up").await.expect("hang up");
    app.wait_for_count(".rail-call-badge", 0, DEFAULT_WAIT).await.expect("gone from the folder too");
}

/// A call in a direct message marks the DMs button.
#[tokio::test]
async fn a_dm_call_marks_the_dms_button() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut bob = App::launch_desktop(&hs).await.expect("launch bob");
    let mut alice = App::launch(&hs).await.expect("launch alice");
    if !alice.is_gpui() {
        return;
    }
    login(&mut alice, "alice").await;
    login(&mut bob, "bob").await;
    bob.click(".rail-item[title=\"Direct messages\"]").await.expect("dms");
    bob.open_room("alice").await.expect("the dm");
    assert_eq!(alice.count(".rail-dms .rail-call-badge").await.expect("count"), 0, "no call yet");
    bob.wait_for(".chat-header .call-button", DEFAULT_WAIT).await.expect("call button");
    bob.click(".chat-header .call-button").await.expect("call");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT).await.expect("connected");
    check_call_badge(&mut alice, ".rail-dms").await;
    bob.click(".voice-panel .hang-up").await.expect("hang up");
    alice.wait_for_count(".rail-call-badge", 0, DEFAULT_WAIT).await.expect("gone");
}

/// The pixel (RGB) at `x`, `y` of the screen, if it can be captured.
fn screen_pixel(x: f64, y: f64) -> Option<[u8; 3]> {
    let out = std::process::Command::new("grim")
        .args(["-s", "1", "-t", "ppm", "-g", &format!("{},{} 1x1", x.round(), y.round()), "-"])
        .output()
        .ok()?;
    let n = out.stdout.len();
    out.stdout.get(n.checked_sub(3)?..).map(|p| [p[0], p[1], p[2]])
}

/// The DMs badge starts where the top bar does, and the panel above the
/// room list is really rounded at its top left corner (the rail shows in it).
#[tokio::test]
async fn the_dms_icon_lines_up_with_the_top_bar_and_the_panel_is_rounded() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch alice");
    if !app.is_gpui() {
        return;
    }
    login(&mut app, "alice").await;
    app.open_room("general").await.expect("general");
    app.wait_for(".chat-header", DEFAULT_WAIT).await.expect("a room open");
    let dms = app.bounds(".rail-dms").await.expect("bounds").expect("the DMs badge");
    let bar = app.bounds(".chat-header").await.expect("bounds").expect("the top bar");
    let head = app.bounds(".sidebar-header").await.expect("bounds").expect("the panel");
    assert!((dms.y - bar.y).abs() <= 1.0, "the DMs badge {dms:?} does not start where the top bar {bar:?} does");
    assert!((head.y - bar.y).abs() <= 1.0, "the panel {head:?} and the top bar {bar:?} differ");
    let radius: f64 = app
        .attr(".sidebar", "data-radius-tl")
        .await
        .expect("attr")
        .and_then(|r| r.parse().ok())
        .unwrap_or(0.0);
    assert!(radius >= 8.0, "the panel's top left corner has a radius of {radius}");
    let can_capture = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
        && !std::process::Command::new("pgrep").args(["-x", "hyprlock"]).status().is_ok_and(|s| s.success());
    let at = app.pid().and_then(|pid| {
        let out = std::process::Command::new("hyprctl").args(["clients", "-j"]).output().ok()?;
        let clients: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
        let w = clients.as_array()?.iter().find(|c| c["pid"].as_u64() == Some(u64::from(pid)))?.clone();
        Some((w["at"][0].as_f64()?, w["at"][1].as_f64()?))
    });
    if let (true, Some((wx, wy))) = (can_capture, at) {
        let (corner, rail, panel) = (
            screen_pixel(wx + head.x + 1.0, wy + head.y + 1.0),
            screen_pixel(wx + head.x - 4.0, wy + head.y + 1.0),
            screen_pixel(wx + head.x + 3.0, wy + head.y + 30.0),
        );
        eprintln!("corner {corner:?}, rail {rail:?}, panel {panel:?}");
        assert!(corner.is_some() && corner == rail, "the corner {corner:?} is not the rail's {rail:?}");
        assert!(corner != panel, "the corner {corner:?} is drawn as the panel {panel:?}");
    } else {
        eprintln!("screen locked or not Hyprland: no pixel check");
    }
}
