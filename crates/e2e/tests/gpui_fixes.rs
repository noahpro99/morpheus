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
