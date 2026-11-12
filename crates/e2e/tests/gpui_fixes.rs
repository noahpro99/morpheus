//! Visual rules found by eye in the GPUI app (task 19): the side panel runs
//! to the bottom of the chat, the command palette is laid out evenly, and
//! pictures keep their shape. They hold on Dioxus too where the selectors exist.

#![allow(clippy::expect_used)]

mod common;

use morpheus_e2e::{App, Bounds, DEFAULT_WAIT, api_as, new_account};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

/// The side pane ends where the chat body does (the composer's bottom edge,
/// give or take its own padding), and never past the window.
async fn assert_pane_reaches_bottom(app: &mut App, what: &str) {
    let pane: Bounds = app
        .bounds(".side-pane")
        .await
        .expect("bounds")
        .unwrap_or_else(|| panic!("{what}: no .side-pane"));
    let body = app
        .bounds(".shell-body")
        .await
        .expect("bounds")
        .expect("shell body");
    let window = app.bounds(".shell").await.expect("bounds").expect("shell");
    eprintln!("{what}: pane {pane:?} body {body:?} window {window:?}");
    let pane_bottom = pane.y + pane.h;
    let body_bottom = body.y + body.h;
    // The wrapper pads the pane by 8 px at the bottom.
    assert!(
        (body_bottom - 8.0 - pane_bottom).abs() <= 3.0,
        "{what}: the pane ends at {pane_bottom}, the chat body at {body_bottom}"
    );
    assert!(
        pane_bottom <= window.y + window.h + 1.0,
        "{what}: the pane runs past the window"
    );
}

#[tokio::test]
async fn the_side_panel_runs_to_the_bottom_of_the_chat() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let user = new_account(&hs, "sidepane").expect("account");
    let room = api_as(
        &hs,
        &user,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": "tiny" })),
    )
    .expect("a room");
    let room = room["room_id"]
        .as_str()
        .expect("room id")
        .replace('!', "%21")
        .replace(':', "%3A");
    let root = api_as(
        &hs,
        &user,
        "PUT",
        &format!("rooms/{room}/send/m.room.message/root"),
        Some(serde_json::json!({ "msgtype": "m.text", "body": "findable pangolin" })),
    )
    .expect("root");
    let root = root["event_id"].as_str().expect("root id").to_owned();
    api_as(
        &hs,
        &user,
        "PUT",
        &format!("rooms/{room}/send/m.room.message/reply"),
        Some(serde_json::json!({
            "msgtype": "m.text", "body": "a reply",
            "m.relates_to": { "rel_type": "m.thread", "event_id": root,
                "is_falling_back": true, "m.in_reply_to": { "event_id": root } }
        })),
    )
    .expect("reply");

    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    common::login(&mut app, &user, &format!("{user}-password")).await;
    app.open_room("tiny").await.expect("open room");

    // Members of a room with one member.
    app.wait_for(".members .member", DEFAULT_WAIT)
        .await
        .expect("members");
    assert_pane_reaches_bottom(&mut app, "a room with one member").await;

    // Search.
    app.click(".chat-header .search-toggle").await.expect("search");
    app.wait_for(".search-panel input[name=search]", DEFAULT_WAIT)
        .await
        .expect("search panel");
    assert_pane_reaches_bottom(&mut app, "search, nothing typed").await;
    app.type_into(".search-panel input[name=search]", "pangolin")
        .await
        .expect("type");
    app.wait_for(".search-panel .search-hit", DEFAULT_WAIT)
        .await
        .expect("a hit");
    assert_pane_reaches_bottom(&mut app, "search with a hit").await;

    // A thread.
    app.click(".chat-header .search-toggle").await.ok();
    app.wait_for(".thread-chip", DEFAULT_WAIT).await.expect("chip");
    app.click_text(".thread-chip", "repl").await.expect("thread");
    app.wait_for(".thread-panel .message", DEFAULT_WAIT)
        .await
        .expect("thread panel");
    assert_pane_reaches_bottom(&mut app, "a thread").await;
}

#[tokio::test]
async fn the_member_list_of_a_dm_and_a_big_room_run_to_the_bottom() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    common::login(&mut app, "alice", "alice-password").await;
    app.open_room("crowd").await.expect("open room");
    app.wait_for(".members .member", DEFAULT_WAIT)
        .await
        .expect("members");
    assert_pane_reaches_bottom(&mut app, "a big room").await;

    app.open_room("bob").await.expect("a DM");
    app.wait_for(".composer", DEFAULT_WAIT).await.expect("composer");
    app.wait_for(".side-pane", DEFAULT_WAIT).await.expect("pane");
    assert_pane_reaches_bottom(&mut app, "a DM").await;
}

#[tokio::test]
async fn the_side_panel_runs_to_the_bottom_at_other_window_sizes() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    for (w, h) in [(1000, 520), (1700, 1000)] {
        let mut app = App::launch_sized(&hs, w, h).await.expect("launch app");
        common::login(&mut app, "alice", "alice-password").await;
        app.open_room("crowd").await.expect("open room");
        app.wait_for(".members .member", DEFAULT_WAIT)
            .await
            .expect("members");
        assert_pane_reaches_bottom(&mut app, &format!("crowd at {w}x{h}")).await;
        app.open_room("general").await.expect("open room");
        app.wait_for(".members .member", DEFAULT_WAIT)
            .await
            .expect("members");
        assert_pane_reaches_bottom(&mut app, &format!("general at {w}x{h}")).await;
    }
}

#[tokio::test]
async fn a_long_thread_in_a_short_window_stays_inside_the_pane() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let user = new_account(&hs, "longthread").expect("account");
    let room = api_as(
        &hs,
        &user,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": "long" })),
    )
    .expect("a room");
    let room = room["room_id"]
        .as_str()
        .expect("room id")
        .replace('!', "%21")
        .replace(':', "%3A");
    let long = "a long root that goes on and on ".repeat(12);
    let root = api_as(
        &hs,
        &user,
        "PUT",
        &format!("rooms/{room}/send/m.room.message/root"),
        Some(serde_json::json!({ "msgtype": "m.text", "body": long })),
    )
    .expect("root");
    let root = root["event_id"].as_str().expect("root id").to_owned();
    for n in 0..6 {
        api_as(
            &hs,
            &user,
            "PUT",
            &format!("rooms/{room}/send/m.room.message/r{n}"),
            Some(serde_json::json!({
                "msgtype": "m.text", "body": format!("reply number {n}, with some words in it"),
                "m.relates_to": { "rel_type": "m.thread", "event_id": root,
                    "is_falling_back": true, "m.in_reply_to": { "event_id": root } }
            })),
        )
        .expect("reply");
    }
    let mut app = App::launch_sized(&hs, 1000, 520).await.expect("launch app");
    common::login(&mut app, &user, &format!("{user}-password")).await;
    app.open_room("long").await.expect("open room");
    app.wait_for(".thread-chip", DEFAULT_WAIT).await.expect("chip");
    app.click_text(".thread-chip", "repl").await.expect("thread");
    app.wait_for(".thread-panel .message", DEFAULT_WAIT)
        .await
        .expect("thread panel");
    assert_pane_reaches_bottom(&mut app, "a long thread").await;
    let panel = app.bounds(".thread-panel").await.expect("b").expect("panel");
    let pane = app.bounds(".side-pane").await.expect("b").expect("pane");
    assert!(panel.y + panel.h <= pane.y + pane.h + 1.0, "the panel spills out of the pane");
}

/// The command palette (Ctrl+K): centred, rows of one height with their
/// picture and text on the row's middle line, the list under the input and
/// as wide as the panel.
#[tokio::test]
async fn the_palette_is_laid_out_evenly() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    common::login(&mut app, "alice", "alice-password").await;
    app.open_room("general").await.expect("open room");
    app.shortcut(".shell", "k", true).await.expect("ctrl+k");
    app.wait_for(".switcher input", DEFAULT_WAIT)
        .await
        .expect("palette");
    app.wait_for(".switcher-item .avatar", DEFAULT_WAIT)
        .await
        .expect("a room row");
    // Mix in the actions and settings rows too.
    app.type_into(".switcher input", "e").await.expect("type");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    if let Ok(secs) = std::env::var("FIXES_HOLD") {
        tokio::time::sleep(std::time::Duration::from_secs(secs.parse().unwrap_or(0))).await;
    }

    let window = app.bounds(".shell").await.expect("b").expect("shell");
    let panel = app.bounds(".switcher").await.expect("b").expect("panel");
    let input = app.bounds(".switcher input").await.expect("b").expect("input");
    let list = app
        .bounds(".switcher-results")
        .await
        .expect("b")
        .expect("list");
    let rows = app.all_bounds(".switcher-item").await.expect("rows");
    eprintln!("window {window:?}\npanel {panel:?}\ninput {input:?}\nlist {list:?}");
    assert!(rows.len() >= 4, "rows to compare: {}", rows.len());

    // Centred, and as wide as Dioxus draws it: 520, or 90% of a narrow window.
    let gap_left = panel.x - window.x;
    let gap_right = window.x + window.w - (panel.x + panel.w);
    assert!((gap_left - gap_right).abs() <= 2.0, "not centred: {gap_left} / {gap_right}");
    assert!((panel.w - 520.0f64.min(window.w * 0.9)).abs() <= 2.0, "width {}", panel.w);
    // Its top is 15% down the window.
    assert!(
        (panel.y - window.y - window.h * 0.15).abs() <= 3.0,
        "top {} in a window {} tall",
        panel.y,
        window.h
    );
    // The input sits on top of the list, both as wide as the panel.
    assert!(input.y >= panel.y - 1.0 && input.y + input.h <= list.y + 1.0, "input above the list");
    assert!((list.x - panel.x).abs() <= 1.0 && (list.w - panel.w).abs() <= 1.0, "list as wide as the panel");
    assert!(input.x >= panel.x - 1.0 && input.x + input.w <= panel.x + panel.w + 1.0, "input inside the panel");
    // The input band is as tall as Dioxus draws it (14 px, one line, 14 px), not a tall box.
    assert!(list.y - panel.y <= 58.0, "the input band is {} tall", list.y - panel.y);
    // Rows: one height, one left edge, one width, no gaps beyond a pixel.
    for (i, row) in rows.iter().enumerate() {
        assert!((row.h - rows[0].h).abs() <= 1.0, "row {i} is {} tall, row 0 {}", row.h, rows[0].h);
        assert!((row.x - rows[0].x).abs() <= 1.0 && (row.w - rows[0].w).abs() <= 1.0, "row {i} is out of line");
        if i > 0 {
            let gap = row.y - (rows[i - 1].y + rows[i - 1].h);
            assert!(gap.abs() <= 1.0, "a gap of {gap} before row {i}");
        }
    }
    assert!(rows[0].x > list.x && rows[0].x + rows[0].w < list.x + list.w, "rows are inset in the list");
    // Pictures, icons and names on the middle line of their row.
    for sel in [".switcher-item .avatar", ".switcher-item svg.icon", ".switcher-item .name", ".switcher-item .kind"] {
        let found = app.all_bounds(sel).await.expect("bounds");
        for b in found {
            let mid = b.y + b.h / 2.0;
            let row = rows
                .iter()
                .find(|r| mid >= r.y && mid <= r.y + r.h)
                .unwrap_or_else(|| panic!("{sel} {b:?} is in no row"));
            assert!(
                (mid - (row.y + row.h / 2.0)).abs() <= 2.0,
                "{sel} is off the row's middle line: {b:?} in {row:?}"
            );
        }
    }
}

/// A row's picture or icon is a gap (8 px, as Dioxus) away from its name.
#[tokio::test]
async fn palette_rows_keep_a_gap_between_picture_and_text() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    common::login(&mut app, "alice", "alice-password").await;
    app.open_room("general").await.expect("open room");
    app.shortcut(".shell", "k", true).await.expect("ctrl+k");
    app.wait_for(".switcher-item .avatar", DEFAULT_WAIT)
        .await
        .expect("a room row");
    app.type_into(".switcher input", "e").await.expect("type");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    for lead in [".switcher-item .avatar", ".switcher-item svg.icon"] {
        let leads = app.all_bounds(lead).await.expect("leads");
        assert!(!leads.is_empty(), "no {lead}");
        let names = app.all_bounds(".switcher-item .name, .switcher-item .action-label").await.expect("names");
        for l in leads {
            let mid = l.y + l.h / 2.0;
            let name = names
                .iter()
                .find(|n| mid >= n.y && mid <= n.y + n.h)
                .unwrap_or_else(|| panic!("no name beside {lead} {l:?}"));
            let gap = name.x - (l.x + l.w);
            assert!((gap - 8.0).abs() <= 2.0, "{lead}: {gap} px between it and the name");
        }
    }
}

const WIDE_PNG: &[u8] = include_bytes!("../fixtures/wide.png");
const TALL_PNG: &[u8] = include_bytes!("../fixtures/tall.png");

fn room_of(hs: &str, owner: &str, guest: &str, name: &str) -> String {
    let made = api_as(
        hs,
        owner,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": name, "preset": "public_chat" })),
    )
    .expect("a room");
    let room = made["room_id"].as_str().expect("room id").to_owned();
    api_as(
        hs,
        guest,
        "POST",
        &format!("join/{}", room.replace('!', "%21").replace(':', "%3A")),
        Some(serde_json::json!({})),
    )
    .expect("join");
    room
}

/// Pictures in the timeline keep their shape: the drawn box has the
/// picture's own proportions however wide the timeline is (a narrow one
/// shrinks it), and the picture fills it without being stretched.
#[tokio::test]
async fn pictures_keep_their_proportions_in_a_narrow_and_a_wide_window() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (ann, ben) = (
        new_account(&hs, "shpann").expect("account"),
        new_account(&hs, "shpben").expect("account"),
    );
    room_of(&hs, &ann, &ben, "shapes");
    let rooms = api_as(&hs, &ben, "GET", "joined_rooms", None).expect("rooms");
    let room = rooms["joined_rooms"][0].as_str().expect("room").to_owned();
    morpheus_e2e::send_image_sized_as(&hs, &ben, &room, WIDE_PNG, "wide.png", None, (800, 400))
        .expect("a wide picture");
    morpheus_e2e::send_image_sized_as(&hs, &ben, &room, TALL_PNG, "tall.png", None, (300, 600))
        .expect("a tall picture");

    for (w, h) in [(1500, 900), (760, 800)] {
        let mut app = App::launch_sized(&hs, w, h).await.expect("launch app");
        common::login(&mut app, &ann, &format!("{ann}-password")).await;
        app.open_room("shapes").await.expect("open room");
        for (name, ratio) in [("wide.png", 2.0), ("tall.png", 0.5)] {
            let sel = format!(".message img.image[alt=\"{name}\"]");
            app.wait_for(&sel, DEFAULT_WAIT).await.expect("the picture");
            if let Ok(secs) = std::env::var("FIXES_HOLD") {
                tokio::time::sleep(std::time::Duration::from_secs(secs.parse().unwrap_or(0))).await;
            }
            let b = app.bounds(&sel).await.expect("b").expect("picture");
            let timeline = app.bounds(".timeline").await.expect("b").expect("timeline");
            eprintln!("{w}x{h} {name}: {b:?} in a timeline {} wide", timeline.w);
            assert!(
                (b.w / b.h - ratio).abs() <= ratio * 0.04,
                "{name} at {w}x{h} is drawn {}x{}: not {ratio}:1",
                b.w,
                b.h
            );
            assert!(b.x + b.w <= timeline.x + timeline.w + 1.0, "{name} sticks out of the timeline");
            let fit = app.attrs(&sel, "data-fit").await.expect("fit");
            assert!(
                fit.iter().all(|f| matches!(f.as_deref(), Some("contain" | "cover"))),
                "{name} is drawn with fit {fit:?}"
            );
        }
        // A shrunk picture leaves no empty band under it: the next one starts
        // right below (the row's height is the box's, as it is drawn).
        let wide = app.bounds(".message img.image[alt=\"wide.png\"]").await.expect("b").expect("wide");
        let tall = app.bounds(".message img.image[alt=\"tall.png\"]").await.expect("b").expect("tall");
        assert!(
            tall.y - (wide.y + wide.h) <= 60.0,
            "{} px of empty space between the pictures at {w}x{h}",
            tall.y - (wide.y + wide.h)
        );
    }
}

/// Avatars are square and filled by the picture (cropped, not squeezed or
/// letterboxed), whatever the picture's proportions.
#[tokio::test]
async fn avatars_are_square_and_filled_by_a_non_square_picture() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (ann, ben) = (
        new_account(&hs, "avtann").expect("account"),
        new_account(&hs, "avtben").expect("account"),
    );
    room_of(&hs, &ann, &ben, "faces");
    morpheus_e2e::set_avatar_as(&hs, &ben, WIDE_PNG).expect("an avatar");
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    common::login(&mut app, &ann, &format!("{ann}-password")).await;
    app.open_room("faces").await.expect("open room");
    let sel = format!(".members .member[title=\"@{ben}:localhost\"] img.avatar");
    app.wait_for(&sel, DEFAULT_WAIT).await.expect("his picture");
    let b = app.bounds(&sel).await.expect("b").expect("avatar");
    assert!((b.w - b.h).abs() <= 1.0, "the avatar is {}x{}", b.w, b.h);
    assert_eq!(
        app.attrs(&sel, "data-fit").await.expect("fit"),
        vec![Some("cover".to_owned())],
        "a wide picture should fill the circle, cropped"
    );
}

/// A wide picture: a red square in the middle, blue on either side.
const FRAMED_PNG: &[u8] = include_bytes!("../fixtures/framed.png");

#[tokio::test]
async fn a_wide_picture_is_cropped_to_fill_a_round_avatar() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (ann, ben) = (
        new_account(&hs, "rndann").expect("account"),
        new_account(&hs, "rndben").expect("account"),
    );
    room_of(&hs, &ann, &ben, "round faces");
    morpheus_e2e::set_avatar_as(&hs, &ben, FRAMED_PNG).expect("an avatar");
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    common::login(&mut app, &ann, &format!("{ann}-password")).await;
    app.open_room("round faces").await.expect("open room");
    let sel = format!(".members .member[title=\"@{ben}:localhost\"] img.avatar");
    app.wait_for(&sel, DEFAULT_WAIT).await.expect("his picture");
    // Let the picture load and draw.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let b = app.bounds(&sel).await.expect("b").expect("avatar");
    let Some((pixels, w, h)) = common::capture(&app) else {
        eprintln!("no grim capture here; skipping the pixel check");
        return;
    };
    let pixel = |x: f64, y: f64| {
        let (x, y) = (x.round() as usize, y.round() as usize);
        assert!(x < w && y < h, "({x}, {y}) is outside the {w}x{h} capture");
        let i = (y * w + x) * 3;
        (pixels[i], pixels[i + 1], pixels[i + 2])
    };
    let red = |p: (u8, u8, u8)| p.0 > 180 && p.1 < 80 && p.2 < 80;
    let blue = |p: (u8, u8, u8)| p.2 > 180 && p.0 < 80 && p.1 < 80;
    // Inside the circle, edge to edge: the middle of the picture (red), its
    // blue sides cropped away.
    let (cx, cy) = (b.x + b.w / 2.0, b.y + b.h / 2.0);
    for (x, y) in [(cx, cy), (b.x + 3.0, cy), (b.right() - 3.0, cy), (cx, b.y + 3.0), (cx, b.y + b.h - 3.0)] {
        let p = pixel(x, y);
        assert!(red(p), "inside the circle at ({x}, {y}) is {p:?}, not the picture's red middle");
    }
    // The box's corners are outside the circle: neither part of the picture.
    for (x, y) in [(b.x + 2.0, b.y + 2.0), (b.right() - 2.0, b.y + 2.0), (b.x + 2.0, b.y + b.h - 2.0), (b.right() - 2.0, b.y + b.h - 2.0)] {
        let p = pixel(x, y);
        assert!(!red(p) && !blue(p), "the avatar's corner at ({x}, {y}) is {p:?}: the picture is not cut to a circle");
    }
}

/// An account with more rooms than the first page of the room list (200)
/// sees all of them, the oldest too (task 21: the list stopped at the 200
/// most recent, so a voice channel's row vanished once a test account had
/// collected enough rooms, and with it its participants).
#[tokio::test]
async fn an_account_with_hundreds_of_rooms_lists_them_all() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let user = new_account(&hs, "manyrooms").expect("account");
    let make = |name: String| {
        api_as(
            &hs,
            &user,
            "POST",
            "createRoom",
            Some(serde_json::json!({ "name": name })),
        )
        .expect("a room");
    };
    make("the oldest".to_owned());
    for n in 0..215 {
        make(format!("filler {n}"));
    }
    let mut app = App::launch(&hs).await.expect("launch");
    common::login(&mut app, &user, &format!("{user}-password")).await;
    let started = std::time::Instant::now();
    loop {
        let names = app.texts(".room .room-name").await.unwrap_or_default();
        if names.iter().any(|n| n == "the oldest") {
            break;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "the oldest room never listed ({} rooms shown)",
            names.len()
        );
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

async fn alice_in_general(hs: &str) -> App {
    let mut app = App::launch_desktop(hs).await.expect("launch app");
    common::login(&mut app, "alice", "alice-password").await;
    app.open_room("general").await.expect("open room");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
    app
}

async fn open_bobs_card(app: &mut App) {
    app.click_text(".members .member", "bob").await.expect("click member");
    app.wait_for_text(".profile-card .profile-id", "@bob:localhost", DEFAULT_WAIT)
        .await
        .expect("card for bob");
    // Drawn, and the pointer's glide done.
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
}

/// Rounded panels are rounded at every corner (task 22): a child that paints
/// its own background at an edge (the settings sidebar, a profile's banner)
/// must not show square corners over the panel's rounded ones.
#[tokio::test]
async fn the_settings_modal_is_rounded_at_all_four_corners() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = alice_in_general(&hs).await;
    app.click(".settings-button").await.expect("gear");
    app.wait_for(".settings .settings-nav", DEFAULT_WAIT).await.expect("settings");
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    let b = app.bounds(".settings").await.expect("b").expect("modal");
    let Some(square) = common::square_corners(&app, &b, [true; 4]) else {
        eprintln!("no grim capture here; skipping the pixel check");
        return;
    };
    assert!(square.is_empty(), "the settings modal's corners drawn square: {square:?}");
}

#[tokio::test]
async fn the_profile_popup_is_rounded_at_its_top_corners() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = alice_in_general(&hs).await;
    open_bobs_card(&mut app).await;
    let b = app.bounds(".profile-card").await.expect("b").expect("card");
    let Some(square) = common::square_corners(&app, &b, [true, true, false, false]) else {
        eprintln!("no grim capture here; skipping the pixel check");
        return;
    };
    assert!(square.is_empty(), "the popup's top corners drawn square: {square:?}");
}

#[tokio::test]
async fn the_full_profile_is_rounded_at_all_four_corners() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = alice_in_general(&hs).await;
    open_bobs_card(&mut app).await;
    app.click(".profile-card .card-button.more").await.expect("more");
    app.click_text(".context-item", "View full profile").await.expect("full");
    app.wait_for(".profile-modal .profile-id", DEFAULT_WAIT).await.expect("full profile");
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    let b = app.bounds(".profile-modal").await.expect("b").expect("modal");
    let Some(square) = common::square_corners(&app, &b, [true; 4]) else {
        eprintln!("no grim capture here; skipping the pixel check");
        return;
    };
    assert!(square.is_empty(), "the full profile's corners drawn square: {square:?}");
}

/// The side pane's own views (members, search, a thread) fill it with the
/// pane's colour: at its rounded corners that must not show as a square.
#[tokio::test]
async fn the_side_pane_is_rounded_at_all_four_corners() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = alice_in_general(&hs).await;
    app.wait_for(".side-pane .members", DEFAULT_WAIT).await.expect("members");
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    let b = app.bounds(".side-pane").await.expect("b").expect("pane");
    let Some(square) = common::square_corners(&app, &b, [true; 4]) else {
        eprintln!("no grim capture here; skipping the pixel check");
        return;
    };
    assert!(square.is_empty(), "the side pane's corners drawn square: {square:?}");
}

async fn open_full_profile(app: &mut App) {
    open_bobs_card(app).await;
    app.click(".profile-card .card-button.more").await.expect("more");
    app.click_text(".context-item", "View full profile").await.expect("full");
    app.wait_for(".profile-modal .profile-id", DEFAULT_WAIT).await.expect("full profile");
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
}

/// The full profile is a modal with margins, centred across and not run to
/// the bottom of the window (it was stretched to it); in a short window it
/// stops short of the bottom and its content scrolls.
#[tokio::test]
async fn the_full_profile_is_a_centred_modal_with_a_height_limit() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = alice_in_general(&hs).await;
    if !app.is_gpui() {
        eprintln!("skipped: the Dioxus modal has no height limit");
        return;
    }
    open_full_profile(&mut app).await;
    let window = app.bounds(".shell").await.expect("b").expect("shell");
    let m = app.bounds(".profile-modal").await.expect("b").expect("modal");
    eprintln!("modal {m:?} in {window:?}");
    assert!(m.h < window.h * 0.8, "the modal is {} tall in a window {} tall: stretched", m.h, window.h);
    assert!(m.y > window.y + 40.0, "no margin above: {m:?}");
    assert!(m.bottom() < window.bottom() - 24.0, "no margin below: {m:?}");
    assert!((m.centre_x() - window.centre_x()).abs() < 2.0, "not centred across: {m:?} in {window:?}");

    // A short window: the modal keeps its margin below and scrolls.
    drop(app);
    let mut app = App::launch_sized(&hs, 1000, 480).await.expect("launch small");
    common::login(&mut app, "alice", "alice-password").await;
    app.open_room("general").await.expect("open room");
    open_full_profile(&mut app).await;
    let window = app.bounds(".shell").await.expect("b").expect("shell");
    let m = app.bounds(".profile-modal").await.expect("b").expect("modal");
    eprintln!("short window: modal {m:?} in {window:?}");
    assert!(m.bottom() <= window.bottom() - 16.0, "runs to the bottom of a short window: {m:?} in {window:?}");
    assert!(m.h > 100.0, "squeezed away: {m:?}");
}

/// Ann's window with a message from Ben in a room (task 22, item D).
async fn a_message_from_ben(hs: &str, name: &str) -> (App, String) {
    let (ann, ben) = (
        new_account(hs, "hovann").expect("account"),
        new_account(hs, "hovben").expect("account"),
    );
    let room = room_of(hs, &ann, &ben, name);
    morpheus_e2e::send_as(hs, &ben, &room, "hello over there").expect("send");
    let mut app = App::launch_desktop(hs).await.expect("launch app");
    common::login(&mut app, &ann, &format!("{ann}-password")).await;
    app.open_room(name).await.expect("open room");
    app.wait_for_text(".message .body", "hello over there", DEFAULT_WAIT)
        .await
        .expect("his message");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    (app, ben)
}

fn bright(p: (u8, u8, u8)) -> bool {
    u32::from(p.0) + u32::from(p.1) + u32::from(p.2) > 450
}

/// The longest run of bright pixels along any row of `b`.
fn longest_bright_run(shot: &(Vec<u8>, usize, usize), b: &Bounds) -> usize {
    let mut best = 0;
    for y in (b.y.floor() as usize)..(b.bottom().ceil() as usize) {
        let mut run = 0;
        for x in (b.x.floor() as usize)..(b.right().ceil() as usize) {
            if bright(common::pixel_at(shot, x as f64, y as f64)) {
                run += 1;
                best = best.max(run);
            } else {
                run = 0;
            }
        }
    }
    best
}

#[tokio::test]
async fn hovering_a_sender_name_underlines_it() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (mut app, _) = a_message_from_ben(&hs, "underlines").await;
    if !app.is_gpui() {
        eprintln!("skipped: hover cannot be driven on the web build");
        return;
    }
    let b = app.bounds(".message .sender").await.expect("b").expect("sender");
    // Away from it: no line under the name (letters alone never run that long).
    app.hover(".composer-input").await.expect("away");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let Some(shot) = common::capture(&app) else {
        eprintln!("no grim capture here; skipping the pixel check");
        return;
    };
    let before = longest_bright_run(&shot, &b);
    app.hover(".message .sender").await.expect("over it");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let shot = common::capture(&app).expect("capture");
    let after = longest_bright_run(&shot, &b);
    eprintln!("longest line in the name: {before} px away, {after} px hovered (name is {} wide)", b.w);
    assert!(before < (b.w * 0.5) as usize, "a line under the name without hovering: {before}");
    assert!(after as f64 >= b.w * 0.8, "no underline when hovered: {after} of {}", b.w);
    // And away again, it goes.
    app.hover(".composer-input").await.expect("away");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let shot = common::capture(&app).expect("capture");
    assert!(longest_bright_run(&shot, &b) < (b.w * 0.5) as usize, "the underline stayed");
}

#[tokio::test]
async fn an_avatar_grows_its_shadow_when_hovered_and_sinks_when_pressed() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (mut app, _) = a_message_from_ben(&hs, "pressable").await;
    if !app.is_gpui() {
        eprintln!("skipped: hover cannot be driven on the web build");
        return;
    }
    let b = app.bounds(".message .avatar-button").await.expect("b").expect("avatar");
    // The pointer on the name keeps the row's hover colour in both pictures:
    // only the avatar's own shadow differs.
    let sample = |shot: &(Vec<u8>, usize, usize)| {
        let (x, y) = (b.x - 5.0, b.y + b.h / 2.0 + 4.0);
        let p = common::pixel_at(shot, x, y);
        u32::from(p.0) + u32::from(p.1) + u32::from(p.2)
    };
    app.hover(".message .sender").await.expect("on the name");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    if let Some(shot) = common::capture(&app) {
        let rest = sample(&shot);
        app.hover(".message .avatar-button").await.expect("on the avatar");
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let over = sample(&common::capture(&app).expect("capture"));
        eprintln!("beside the avatar: {rest} resting, {over} hovered (sum of r+g+b)");
        assert!(over + 6 <= rest, "the shadow did not grow: {rest} at rest, {over} hovered");
    } else {
        eprintln!("no grim capture here; skipping the shadow pixels");
    }

    // Pressed: a pixel lower; let go: back.
    let at = app.bounds(".message .avatar-button").await.expect("b").expect("avatar");
    app.drag_hold(".message .avatar-button", 0, 0).await.expect("press");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let down = app.bounds(".message .avatar-button").await.expect("b").expect("avatar");
    assert!(
        (down.y - at.y - 1.0).abs() < 0.3 && (down.x - at.x).abs() < 0.3,
        "pressed: {down:?}, not one pixel below {at:?}"
    );
    app.release().await.expect("let go");
    app.press("body", "Escape").await.ok();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let up = app.bounds(".message .avatar-button").await.expect("b").expect("avatar");
    assert!((up.y - at.y).abs() < 0.3, "released: {up:?}, not back at {at:?}");
}
