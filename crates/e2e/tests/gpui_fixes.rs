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
    app.click(".chat-header input[name=search]").await.expect("search");
    app.type_into(".chat-header input[name=search]", "pangolin")
        .await
        .expect("type");
    app.press(".chat-header input[name=search]", "Enter")
        .await
        .expect("search");
    app.wait_for(".search-panel .search-hit", DEFAULT_WAIT)
        .await
        .expect("a hit");
    assert_pane_reaches_bottom(&mut app, "search with a hit").await;

    // A thread.
    app.press(".chat-header input[name=search]", "Escape").await.ok();
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
    let (ann, ben) = (
        new_account(&hs, "shortann").expect("account"),
        new_account(&hs, "shortben").expect("account"),
    );
    room_of(&hs, &ann, &ben, "short room");
    let mut app = App::launch_sized(&hs, 1000, 480).await.expect("launch small");
    common::login(&mut app, &ann, &format!("{ann}-password")).await;
    app.open_room("short room").await.expect("open room");
    app.wait_for(&format!(".members .member[title=\"@{ben}:localhost\"]"), DEFAULT_WAIT).await.expect("ben listed");
    app.click(&format!(".members .member[title=\"@{ben}:localhost\"]")).await.expect("his row");
    app.wait_for(".profile-card .card-button.more", DEFAULT_WAIT).await.expect("card");
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    app.click(".profile-card .card-button.more").await.expect("more");
    app.click_text(".context-item", "View full profile").await.expect("full");
    app.wait_for(".profile-modal .profile-id", DEFAULT_WAIT).await.expect("full profile");
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    let window = app.bounds(".shell").await.expect("b").expect("shell");
    let m = app.bounds(".profile-modal").await.expect("b").expect("modal");
    eprintln!("short window: modal {m:?} in {window:?}");
    assert!(m.bottom() <= window.bottom() - 16.0, "runs to the bottom of a short window: {m:?} in {window:?}");
    assert!(m.h > 100.0, "squeezed away: {m:?}");
}

/// Ann's window with a message from Ben in a room (task 22, item D).
async fn a_message_from_ben(hs: &str, name: &str) -> (App, String) {
    a_message_from_ben_saying(hs, name, "hello over there").await
}

async fn a_message_from_ben_saying(hs: &str, name: &str, body: &str) -> (App, String) {
    let (ann, ben) = (
        new_account(hs, "hovann").expect("account"),
        new_account(hs, "hovben").expect("account"),
    );
    let room = room_of(hs, &ann, &ben, name);
    morpheus_e2e::send_as(hs, &ben, &room, body).expect("send");
    let mut app = App::launch_desktop(hs).await.expect("launch app");
    common::login(&mut app, &ann, &format!("{ann}-password")).await;
    app.open_room(name).await.expect("open room");
    app.wait_for_text(".message .body", body.lines().next().unwrap_or(body), DEFAULT_WAIT)
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

/// The longest run of pixels along a row of `b` that differ between two
/// captures (the text itself is the same in both: what differs is the line).
fn longest_changed_run(a: &(Vec<u8>, usize, usize), c: &(Vec<u8>, usize, usize), b: &Bounds) -> usize {
    let mut best = 0;
    for y in (b.y.floor() as usize)..(b.bottom().ceil() as usize) {
        let mut run = 0;
        for x in (b.x.floor() as usize)..(b.right().ceil() as usize) {
            let (p, q) = (common::pixel_at(a, x as f64, y as f64), common::pixel_at(c, x as f64, y as f64));
            if common::colour_distance(p, q) > 60 {
                run += 1;
                best = best.max(run);
            } else {
                run = 0;
            }
        }
    }
    best
}

/// Link-like labels underline under the pointer (task 23): the id on the
/// profile card, its "mutual servers" label, the id on the full profile. A
/// hover underline set as a gpui style does nothing; these draw a line.
#[tokio::test]
async fn hovering_ids_and_link_labels_underlines_them() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = alice_in_general(&hs).await;
    if !app.is_gpui() {
        eprintln!("skipped: hover cannot be driven on the web build");
        return;
    }
    open_bobs_card(&mut app).await;
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    for (selector, away) in [
        (".profile-card .profile-id", ".profile-card .profile-name"),
        (".profile-card .mutual-servers", ".profile-card .profile-name"),
    ] {
        let b = app.bounds(selector).await.expect("b").unwrap_or_else(|| panic!("no {selector}"));
        app.hover(away).await.expect("away");
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let Some(rest) = common::capture(&app) else {
            eprintln!("no grim capture here; skipping the pixel check");
            return;
        };
        app.hover(selector).await.expect("over it");
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let over = common::capture(&app).expect("capture");
        let run = longest_changed_run(&rest, &over, &b);
        eprintln!("{selector}: {run} px of line when hovered (label is {} wide)", b.w);
        assert!(run as f64 >= b.w * 0.8, "{selector}: no underline when hovered: {run} of {}", b.w);
        app.hover(away).await.expect("away again");
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let gone = common::capture(&app).expect("capture");
        assert!(longest_changed_run(&rest, &gone, &b) < (b.w * 0.5) as usize, "{selector}: the line stayed");
    }
    // The full profile's id.
    app.click(".profile-card .card-button.more").await.expect("more");
    app.click_text(".context-item", "View full profile").await.expect("full");
    app.wait_for(".profile-modal .profile-id", DEFAULT_WAIT).await.expect("full profile");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let selector = ".profile-modal .profile-id";
    let b = app.bounds(selector).await.expect("b").expect("id");
    app.hover(".profile-modal .profile-name").await.expect("away");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let rest = common::capture(&app).expect("capture");
    app.hover(selector).await.expect("over it");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let over = common::capture(&app).expect("capture");
    let run = longest_changed_run(&rest, &over, &b);
    eprintln!("{selector}: {run} px of line when hovered (label is {} wide)", b.w);
    assert!(run as f64 >= b.w * 0.8, "{selector}: no underline when hovered: {run} of {}", b.w);
}

/// How much darker than `reference` (r+g+b) the pixel is.
fn darkness(shot: &(Vec<u8>, usize, usize), x: f64, y: f64, reference: u32) -> i32 {
    let p = common::pixel_at(shot, x, y);
    reference as i32 - (u32::from(p.0) + u32::from(p.1) + u32::from(p.2)) as i32
}

/// A timeline avatar's shadow sits evenly around the circle, not hanging
/// below it (a button as tall as the message made it a tall pill).
#[tokio::test]
async fn an_avatars_shadow_sits_evenly_around_the_circle() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    // A message of several lines: the row is much taller than the picture.
    let (mut app, _) = a_message_from_ben_saying(
        &hs,
        "shadowed",
        "first line of three\nsecond line of three\nthird line of three",
    )
    .await;
    if !app.is_gpui() {
        eprintln!("skipped: the avatar shadow is GPUI's");
        return;
    }
    let b = app.bounds(".message .avatar-button").await.expect("b").expect("avatar");
    eprintln!("avatar button: {b:?}");
    assert!(b.h <= b.w + 1.0, "the avatar button is {} tall for a picture {} wide", b.h, b.w);
    let r = b.w / 2.0;
    let (cx, cy) = (b.x + r, b.y + r);
    for hovered in [false, true] {
        app.hover(if hovered { ".message .avatar-button" } else { ".composer-input" })
            .await
            .expect("hover");
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let Some(shot) = common::capture(&app) else {
            eprintln!("no grim capture here; skipping the shadow pixels");
            return;
        };
        let reference = {
            let p = common::pixel_at(&shot, b.x - 14.0, cy);
            u32::from(p.0) + u32::from(p.1) + u32::from(p.2)
        };
        let away = r + 2.5;
        let sides = [
            ("above", cx, cy - away),
            ("below", cx, cy + away),
            ("left", cx - away, cy),
            ("right", cx + away, cy),
        ]
        .map(|(name, x, y)| (name, darkness(&shot, x, y, reference)));
        eprintln!("hovered {hovered}: darkness {sides:?} (reference sum {reference})");
        let most = sides.iter().map(|s| s.1).max().expect("sides");
        let least = sides.iter().map(|s| s.1).min().expect("sides");
        assert!(most >= 4, "hovered {hovered}: no shadow at all: {sides:?}");
        // Even: the darkest side is within a factor of two (and a few
        // levels) of the lightest.
        assert!(
            most <= least * 2 + 8,
            "hovered {hovered}: the shadow is uneven around the circle: {sides:?}"
        );
    }
}

/// Whether the pixel differs from `bg` enough to be a divider line.
fn is_line(p: (u8, u8, u8), bg: (u8, u8, u8)) -> bool {
    common::colour_distance(p, bg) >= 6
}

/// The date divider, as Discord's: the date centred, a line each side
/// reaching out to the timeline's edges.
#[tokio::test]
async fn the_date_divider_has_a_line_either_side_of_a_centred_date() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (mut app, _) = a_message_from_ben(&hs, "dated").await;
    app.wait_for(".date-divider", DEFAULT_WAIT).await.expect("a date divider");
    let d = app.bounds(".date-divider").await.expect("b").expect("divider");
    let label = app.bounds(".date-divider span").await.expect("b").expect("label");
    let list = app.bounds(".message").await.expect("b").expect("a message row");
    eprintln!("divider {d:?} label {label:?} message row {list:?}");
    // It spans the timeline (as wide as a message row, give or take margins).
    assert!(d.w >= list.w - 40.0, "the divider is {} wide, a message row {}", d.w, list.w);
    let mid = |b: &Bounds| b.x + b.w / 2.0;
    assert!(
        (mid(&label) - mid(&d)).abs() <= 4.0,
        "the date is not centred: its middle {} vs the divider's {}",
        mid(&label),
        mid(&d)
    );
    let Some(shot) = common::capture(&app) else {
        eprintln!("no grim capture here; skipping the line pixels");
        return;
    };
    let y = d.y + d.h / 2.0;
    let bg = common::pixel_at(&shot, d.x + 2.0, y - 6.0);
    // A line from just inside the left edge to the date, and from the date
    // to the right edge.
    for (name, from, to) in [
        ("left", d.x + 24.0, label.x - 8.0),
        ("right", label.right() + 8.0, d.right() - 24.0),
    ] {
        assert!(to - from > 50.0, "{name}: no room for a line ({from}..{to})");
        let mut lines = 0;
        let mut total = 0;
        let mut x = from;
        while x < to {
            total += 1;
            // The 1 px line sits on one of the rows around the middle.
            if (-2..=1).any(|dy| is_line(common::pixel_at(&shot, x, y + f64::from(dy)), bg)) {
                lines += 1;
            }
            x += 1.0;
        }
        eprintln!("{name}: {lines} of {total} pixels are line");
        assert!(lines * 10 >= total * 9, "{name}: the line is missing or broken: {lines} of {total}");
    }
}

/// Emoji in messages, one per message: simple, with variation selector,
/// skin tone, ZWJ sequences, flags, keycaps and newer Unicode (14-15).
const EMOJIS: &[(&str, &str)] = &[
    ("grinning", "\u{1F600}"),
    ("red heart", "\u{2764}\u{FE0F}"),
    ("thumbs up", "\u{1F44D}"),
    ("smiling face vs16", "\u{263A}\u{FE0F}"),
    ("check mark vs16", "\u{2714}\u{FE0F}"),
    ("thumbs up medium skin", "\u{1F44D}\u{1F3FD}"),
    ("family zwj", "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}"),
    ("rainbow flag zwj", "\u{1F3F3}\u{FE0F}\u{200D}\u{1F308}"),
    ("technologist zwj", "\u{1F9D1}\u{200D}\u{1F4BB}"),
    ("flag us", "\u{1F1FA}\u{1F1F8}"),
    ("flag jp", "\u{1F1EF}\u{1F1F5}"),
    ("keycap 1", "1\u{FE0F}\u{20E3}"),
    ("melting face", "\u{1FAE0}"),
    ("heart hands", "\u{1FAF6}"),
    ("pink heart", "\u{1FA77}"),
    ("goose", "\u{1FABF}"),
];

/// Each emoji alone in a message draws as one colour glyph about as wide as
/// a plain emoji: saturated pixels inside its area, no tofu box, a sequence
/// (ZWJ, flag, skin tone, keycap) one glyph wide rather than several.
#[tokio::test]
async fn emojis_draw_as_one_colour_glyph_each() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (ann, ben) = (
        new_account(&hs, "emoann").expect("account"),
        new_account(&hs, "emoben").expect("account"),
    );
    let room = room_of(&hs, &ann, &ben, "emojis");
    for (_, emoji) in EMOJIS {
        morpheus_e2e::send_as(&hs, &ben, &room, emoji).expect("send");
    }
    let mut app = App::launch_sized(&hs, 1200, 1000).await.expect("launch app");
    common::login(&mut app, &ann, &format!("{ann}-password")).await;
    app.open_room("emojis").await.expect("open room");
    app.wait_for_text(".message .body", EMOJIS[EMOJIS.len() - 1].1, DEFAULT_WAIT)
        .await
        .expect("the last emoji");
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    if !app.is_gpui() {
        eprintln!("skipped: the web view has its own emoji");
        return;
    }
    let Some(shot) = common::capture(&app) else {
        eprintln!("no grim capture here; skipping the emoji pixels");
        return;
    };
    let rows = app.all_bounds(".message .body").await.expect("bodies");
    if let (Ok(path), Some(pid)) = (std::env::var("EMOJI_SHOT"), app.pid()) {
        // Debug aid: the emoji column of the test window only.
        let clients = std::process::Command::new("hyprctl").args(["clients", "-j"]).output().expect("hyprctl").stdout;
        let clients: serde_json::Value = serde_json::from_slice(&clients).expect("json");
        let at = clients.as_array().expect("clients").iter().find(|c| c["pid"].as_u64() == Some(u64::from(pid))).expect("window")["at"].clone();
        let (x, y) = (at[0].as_f64().unwrap_or(0.0) + rows[0].x - 4.0, at[1].as_f64().unwrap_or(0.0) + rows[0].y - 4.0);
        let geometry = format!("{},{} 60x{}", x as i64, y as i64, (rows[rows.len() - 1].bottom() - rows[0].y) as i64 + 8);
        let _ = std::process::Command::new("grim").args(["-s", "8", "-g", &geometry, &path]).status();
    }
    assert_eq!(rows.len(), EMOJIS.len(), "{} bodies for {} emoji", rows.len(), EMOJIS.len());
    let mut failed = Vec::new();
    let mut widths: Vec<f64> = Vec::new();
    for ((name, _), b) in EMOJIS.iter().zip(&rows) {
        let bg = common::pixel_at(&shot, b.x + 300.0, b.y + 2.0);
        let (mut ink, mut colour) = (0usize, 0usize);
        let (mut left, mut right, mut top, mut bottom) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for y in (b.y.floor() as usize)..(b.bottom().ceil() as usize) {
            for x in (b.x.floor() as usize - 2)..(b.x.floor() as usize + 90) {
                let p = common::pixel_at(&shot, x as f64, y as f64);
                if common::colour_distance(p, bg) > 60 {
                    ink += 1;
                    left = left.min(x as f64);
                    right = right.max(x as f64);
                    top = top.min(y as f64);
                    bottom = bottom.max(y as f64);
                    let hi = p.0.max(p.1).max(p.2);
                    let lo = p.0.min(p.1).min(p.2);
                    if hi - lo > 50 {
                        colour += 1;
                    }
                }
            }
        }
        let width = if ink == 0 { 0.0 } else { right - left + 1.0 };
        eprintln!("{name:24} ink {ink:4} colour {colour:4} width {width:5.1} (row {}x{})", b.w, b.h);
        widths.push(width);
        // Noto draws these in greys and white: no saturated colour to count.
        let grey = ["check mark vs16", "family zwj", "goose"].contains(name);
        if !grey && colour < 40 {
            failed.push(format!("{name}: only {colour} colour pixels (ink {ink})"));
        }
        // A glyph of its own size (not a box outline or a sliver).
        let height = bottom - top + 1.0;
        if ink < 80 || width < 12.0 || height < 10.0 {
            failed.push(format!("{name}: a {width} x {height} glyph with {ink} ink pixels"));
        }
    }
    // One glyph: a sequence is as wide as one emoji (about 16-18 px at this size), not several.
    for ((name, _), w) in EMOJIS.iter().zip(&widths) {
        if *w > 22.0 {
            failed.push(format!("{name}: {w} px wide, more than one emoji"));
        }
    }
    assert!(failed.is_empty(), "emoji not drawn as one colour glyph:\n{}", failed.join("\n"));
}

/// A direct message between `owner` and `guest`, marked as one in the
/// owner's account data (as a client does).
fn dm_of(hs: &str, owner: &str, guest: &str) -> String {
    let made = api_as(
        hs,
        owner,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "invite": [format!("@{guest}:localhost")], "is_direct": true, "preset": "trusted_private_chat" })),
    )
    .expect("a DM");
    let room = made["room_id"].as_str().expect("room id").to_owned();
    api_as(
        hs,
        guest,
        "POST",
        &format!("join/{}", room.replace('!', "%21").replace(':', "%3A")),
        Some(serde_json::json!({})),
    )
    .expect("join");
    api_as(
        hs,
        owner,
        "PUT",
        &format!("user/%40{owner}%3Alocalhost/account_data/m.direct"),
        Some(serde_json::json!({ format!("@{guest}:localhost"): [room.clone()] })),
    )
    .expect("m.direct");
    room
}

/// The command palette finds a person by the nickname you gave them: their
/// DM comes up (listed under the nickname), and so does nothing for a name
/// nobody has.
#[tokio::test]
async fn the_palette_finds_a_person_by_nickname() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (ann, ben) = (
        new_account(&hs, "palnann").expect("account"),
        new_account(&hs, "palnben").expect("account"),
    );
    room_of(&hs, &ann, &ben, "lobby");
    let dm = dm_of(&hs, &ann, &ben);
    let nick = "Zorblax the Wise";
    api_as(
        &hs,
        &ann,
        "PUT",
        &format!("user/%40{ann}%3Alocalhost/account_data/dev.morpheus.nicknames"),
        Some(serde_json::json!({ format!("@{ben}:localhost"): nick })),
    )
    .expect("a nickname");
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    common::login(&mut app, &ann, &format!("{ann}-password")).await;
    app.shortcut(".shell", "k", true).await.expect("ctrl+k");
    app.wait_for(".switcher input", DEFAULT_WAIT).await.expect("palette");
    // Wait for the nickname to arrive: the DM is listed under it.
    let row = format!(".switcher-item[data-key=\"{dm}\"]");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    for part in ["zorb", "WISE", "the wi"] {
        app.type_into(".switcher input", part).await.expect("type");
        loop {
            if app.count(&row).await.unwrap_or(0) == 1 {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "typing {part:?} did not find the DM");
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            app.type_into(".switcher input", part).await.expect("type");
        }
        app.wait_for_text(&row, nick, DEFAULT_WAIT).await.expect("listed under the nickname");
    }
    // A name nobody has finds nothing.
    app.type_into(".switcher input", "qqqxyzzy").await.expect("type");
    app.wait_for(".switcher-item.muted", DEFAULT_WAIT).await.expect("nothing matches");
    assert_eq!(app.count(&row).await.expect("count"), 0);
    // Enter on the nickname opens the DM.
    app.type_into(".switcher input", "zorblax").await.expect("type");
    app.wait_for(&row, DEFAULT_WAIT).await.expect("row");
    app.press(".switcher input", "Enter").await.expect("enter");
    app.wait_for_text(".chat-header h2", nick, DEFAULT_WAIT).await.expect("the DM opens");
}

/// The palette's pictures are cut like every other avatar: a wide picture
/// fills a true circle with its middle (red), the blue sides cropped away.
#[tokio::test]
async fn a_wide_picture_fills_the_palettes_round_avatar() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (ann, ben) = (
        new_account(&hs, "palrann").expect("account"),
        new_account(&hs, "palrben").expect("account"),
    );
    room_of(&hs, &ann, &ben, "lobby");
    let dm = dm_of(&hs, &ann, &ben);
    morpheus_e2e::set_avatar_as(&hs, &ben, FRAMED_PNG).expect("an avatar");
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    common::login(&mut app, &ann, &format!("{ann}-password")).await;
    app.shortcut(".shell", "k", true).await.expect("ctrl+k");
    let sel = format!(".switcher-item[data-key=\"{dm}\"] img.avatar");
    app.wait_for(&sel, DEFAULT_WAIT).await.expect("his picture in the palette");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let b = app.bounds(&sel).await.expect("b").expect("avatar");
    assert!((b.w - b.h).abs() <= 1.0, "the avatar is not square: {b:?}");
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
    let (cx, cy) = (b.x + b.w / 2.0, b.y + b.h / 2.0);
    let inset = 2.5;
    for (x, y) in [(cx, cy), (b.x + inset, cy), (b.right() - inset, cy), (cx, b.y + inset), (cx, b.y + b.h - inset)] {
        let p = pixel(x, y);
        assert!(red(p), "inside the circle at ({x}, {y}) is {p:?}, not the picture's red middle ({b:?})");
    }
    for (x, y) in [(b.x + 1.5, b.y + 1.5), (b.right() - 1.5, b.y + 1.5), (b.x + 1.5, b.y + b.h - 1.5), (b.right() - 1.5, b.y + b.h - 1.5)] {
        let p = pixel(x, y);
        assert!(!red(p) && !blue(p), "the corner at ({x}, {y}) is {p:?}: not cut to a circle");
    }
}

/// A channel's row in the palette shows its server's picture, cut to fill
/// its (rounded) box like every avatar: the red middle of a wide picture,
/// its blue sides gone, the box's corners empty.
#[tokio::test]
async fn a_servers_wide_icon_fills_the_palettes_avatar() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let ann = new_account(&hs, "palsann").expect("account");
    let made = |body: serde_json::Value| {
        api_as(&hs, &ann, "POST", "createRoom", Some(body)).expect("a room")["room_id"]
            .as_str()
            .expect("room id")
            .to_owned()
    };
    let space = made(serde_json::json!({ "name": "Wide Server", "creation_content": { "type": "m.space" } }));
    made(serde_json::json!({ "name": "lobby" }));
    let chan = made(serde_json::json!({ "name": "wide-chan" }));
    api_as(
        &hs,
        &ann,
        "PUT",
        &format!("rooms/{space}/state/m.space.child/{chan}"),
        Some(serde_json::json!({ "via": ["localhost"] })),
    )
    .expect("child");
    morpheus_e2e::set_room_avatar_as(&hs, &ann, &space, FRAMED_PNG).expect("an icon");
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    common::login(&mut app, &ann, &format!("{ann}-password")).await;
    app.shortcut(".shell", "k", true).await.expect("ctrl+k");
    app.wait_for(".switcher input", DEFAULT_WAIT).await.expect("palette");
    app.type_into(".switcher input", "wide-chan").await.expect("type");
    let sel = format!(".switcher-item[data-key=\"{chan}\"] img.avatar");
    app.wait_for(&sel, DEFAULT_WAIT).await.expect("the server's picture in the palette");
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
    let (cx, cy) = (b.x + b.w / 2.0, b.y + b.h / 2.0);
    for (x, y) in [(cx, cy), (b.x + 2.5, cy), (b.right() - 2.5, cy), (cx, b.y + 2.5), (cx, b.y + b.h - 2.5)] {
        let p = pixel(x, y);
        assert!(red(p), "inside the icon at ({x}, {y}) is {p:?}, not the picture's red middle ({b:?})");
    }
    for (x, y) in [(b.x + 1.0, b.y + 1.0), (b.right() - 1.0, b.y + 1.0), (b.x + 1.0, b.y + b.h - 1.0), (b.right() - 1.0, b.y + b.h - 1.0)] {
        let p = pixel(x, y);
        assert!(!red(p) && !blue(p), "the corner at ({x}, {y}) is {p:?}: not cut to a rounded shape");
    }
}

/// A link in a message underlines under the pointer, that link only, and the
/// line goes when the pointer leaves (task 31). Links inside a text run need
/// their own hover tracking: gpui's hover underline does nothing there.
#[tokio::test]
async fn hovering_a_link_in_a_message_underlines_that_link_only() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (mut app, _) = a_message_from_ben_saying(
        &hs,
        "linkhover",
        "see https://example.org/first and also https://example.org/second here",
    )
    .await;
    if !app.is_gpui() {
        eprintln!("skipped: hover cannot be driven on the web build");
        return;
    }
    let first = ".message .body a[href=\"https://example.org/first\"]";
    let second = ".message .body a[href=\"https://example.org/second\"]";
    app.wait_for(first, DEFAULT_WAIT).await.expect("first link");
    let b1 = app.bounds(first).await.expect("b").expect("first bounds");
    let b2 = app.bounds(second).await.expect("b").expect("second bounds");
    app.hover(".composer .composer-input").await.expect("away");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(app.attr(first, "data-hovered").await.expect("attr"), None);
    let rest = common::capture(&app);

    app.hover(first).await.expect("over the first");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(app.attr(first, "data-hovered").await.expect("attr").as_deref(), Some("true"));
    assert_eq!(app.attr(second, "data-hovered").await.expect("attr"), None);
    if let Some(rest) = &rest {
        let over = common::capture(&app).expect("capture");
        let (run1, run2) = (longest_changed_run(rest, &over, &b1), longest_changed_run(rest, &over, &b2));
        eprintln!("hovering the first: {run1} px of line under it (it is {} wide), {run2} under the second", b1.w);
        assert!(run1 as f64 >= b1.w * 0.8, "no underline under the hovered link: {run1} of {}", b1.w);
        assert!(run2 < (b2.w * 0.3) as usize, "the other link is underlined too: {run2} of {}", b2.w);
    }

    app.hover(".composer .composer-input").await.expect("away again");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(app.attr(first, "data-hovered").await.expect("attr"), None, "still hovered after leaving");
    if let Some(rest) = &rest {
        let gone = common::capture(&app).expect("capture");
        assert!(longest_changed_run(rest, &gone, &b1) < (b1.w * 0.3) as usize, "the line stayed");
    }
}
