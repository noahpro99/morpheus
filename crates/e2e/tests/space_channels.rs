//! A server's channels as other clients show them: the rooms a server lists
//! (its `m.space.child` events, even when the rooms don't name it back, as
//! GrapheneOS's don't) are under that server and not under Rooms, in the
//! order the server keeps (the children's `order`), and the ones you aren't
//! in are listed too, at the bottom, opening a join card. Only people who
//! may edit the server reorder its channels, and everyone sees the order.
//!
//! `dev/homeserver/seed.sh` makes Carol's "Projects": #projects-news (order
//! a), #projects-general (b, newest message), #projects-archive (c, Alice
//! not in it), listed by the server only.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT, api_as};
use serde_json::json;

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
        .expect("shell after login");
}

/// The names in the list, in order: (joined, not joined).
async fn channels(app: &mut App) -> (Vec<String>, Vec<String>) {
    // Every row, then the unjoined ones, which are listed last.
    let mut joined = app.texts(".room-list .room-name").await.expect("joined");
    let unjoined = app
        .texts(".room-list .room.unjoined .room-name")
        .await
        .expect("unjoined");
    let kept = joined.len().saturating_sub(unjoined.len());
    joined.truncate(kept);
    (joined, unjoined)
}

/// Click `click` until `wait` shows `text`: a click can land on a list that
/// is still settling and be lost, so it is made again if nothing happened.
async fn click_until_text(app: &mut App, click: &str, wait: &str, text: &str) {
    for _ in 0..5 {
        app.wait_for(click, DEFAULT_WAIT).await.expect("something to click");
        app.click(click).await.expect("click");
        if app
            .wait_for_text(wait, text, std::time::Duration::from_secs(6))
            .await
            .is_ok()
        {
            return;
        }
    }
    app.wait_for_text(wait, text, DEFAULT_WAIT)
        .await
        .unwrap_or_else(|err| panic!("{click} did not show {text:?} in {wait}: {err}"));
}

async fn open_projects(app: &mut App) {
    click_until_text(
        app,
        ".rail-item[title=\"Projects\"]",
        ".sidebar-header h2",
        "Projects",
    )
    .await;
}

fn room_id(hs: &str, alias: &str) -> String {
    api_as(
        hs,
        "carol",
        "GET",
        &format!("directory/room/%23{alias}%3Alocalhost"),
        None,
    )
    .expect("alias")["room_id"]
        .as_str()
        .expect("room id")
        .to_owned()
}

/// Put the seed's order back (news a, general b, archive c), and Alice out
/// of archive, whatever a test changed.
fn reset(hs: &str) {
    let space = room_id(hs, "projects");
    for (alias, order) in [
        ("proj-news", "a"),
        ("proj-general", "b"),
        ("proj-archive", "c"),
    ] {
        let child = room_id(hs, alias);
        api_as(
            hs,
            "carol",
            "PUT",
            &format!("rooms/{space}/state/m.space.child/{child}"),
            Some(json!({ "via": ["localhost"], "order": order })),
        )
        .expect("order");
    }
    let archive = room_id(hs, "proj-archive");
    let _ = api_as(
        hs,
        "alice",
        "POST",
        &format!("rooms/{archive}/leave"),
        Some(json!({})),
    );
    let _ = api_as(
        hs,
        "alice",
        "POST",
        &format!("rooms/{archive}/forget"),
        Some(json!({})),
    );
}

/// Rooms a server lists are under it (not under Rooms), in the server's
/// order rather than by recency, and the ones you aren't in come last.
#[tokio::test]
async fn a_servers_rooms_are_under_it_in_its_order() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    reset(&hs);
    let mut app = App::launch(&hs).await.expect("launch");
    login(&mut app, "alice").await;

    app.click(".rail-rooms").await.expect("Rooms");
    app.wait_for_text(".room", "general", DEFAULT_WAIT)
        .await
        .expect("rooms listed");
    let (rooms, _) = channels(&mut app).await;
    assert!(
        !rooms.iter().any(|name| name.starts_with("projects-")),
        "a server's rooms are not under Rooms: {rooms:?}"
    );

    open_projects(&mut app).await;
    app.wait_for(".room.unjoined", DEFAULT_WAIT)
        .await
        .expect("the room not joined, listed");
    // The list fills in as the sync delivers rooms: wait for it to settle.
    let started = std::time::Instant::now();
    let (joined, unjoined) = loop {
        let seen = channels(&mut app).await;
        if seen.0 == ["projects-news", "projects-general"] || started.elapsed() > DEFAULT_WAIT {
            break seen;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    };
    assert_eq!(
        joined,
        ["projects-news", "projects-general"],
        "in the server's order"
    );
    assert_eq!(unjoined, ["projects-archive"], "the rest, at the bottom");
    assert_eq!(
        app.text(".room-list h4.more-channels")
            .await
            .expect("heading"),
        Some("More channels".into())
    );
}

/// A channel you aren't in opens a card to join it, not its messages, and
/// joining puts it among the others.
#[tokio::test]
async fn an_unjoined_channel_opens_a_join_card() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    reset(&hs);
    let mut app = App::launch(&hs).await.expect("launch");
    login(&mut app, "alice").await;
    open_projects(&mut app).await;
    app.wait_for(".room.unjoined", DEFAULT_WAIT)
        .await
        .expect("unjoined row");
    click_until_text(&mut app, ".room.unjoined", ".join-card h2", "projects-archive").await;
    assert_eq!(app.count(".chat .composer").await.expect("count"), 0);
    app.wait_for_text(".join-card", "The projects-archive room", DEFAULT_WAIT)
        .await
        .expect("its topic");
    app.click(".join-card .join-button").await.expect("join");
    app.wait_for(".chat .composer", DEFAULT_WAIT)
        .await
        .expect("the room, joined");
    app.wait_for_count(".room.unjoined", 0, DEFAULT_WAIT)
        .await
        .expect("no longer unjoined");
    let want = ["projects-news", "projects-general", "projects-archive"];
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let names = app
            .texts(".room-list .room .room-name")
            .await
            .expect("room names");
        if names == want {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "archive among the joined, in order: {names:?}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    reset(&hs);
}

/// Drag the row `from` onto the row `to`: a synthetic HTML5 drag on the
/// Dioxus page, a real pointer drag (press, move, release) in the GPUI app.
async fn drag(app: &mut App, from: &str, to: &str) {
    if app.is_gpui() {
        let a = app.bounds(from).await.expect("from").expect("a source row");
        let b = app.bounds(to).await.expect("to").expect("a target row");
        // Onto the target's upper half, as the pointer arrives from below.
        let (dx, dy) = (
            (b.centre_x() - a.centre_x()).round() as i32,
            (b.y + b.h / 4.0 - (a.y + a.h / 2.0)).round() as i32,
        );
        app.drag(from, dx, dy).await.expect("drag");
        return;
    }
    let js = format!(
        "const from = document.querySelector({}); const to = document.querySelector({}); \
         if (!from || !to) throw new Error('drag endpoints missing'); \
         from.dispatchEvent(new DragEvent('dragstart', {{ bubbles: true }})); \
         to.dispatchEvent(new DragEvent('dragover', {{ bubbles: true, cancelable: true }})); \
         to.dispatchEvent(new DragEvent('drop', {{ bubbles: true, cancelable: true }})); \
         from.dispatchEvent(new DragEvent('dragend', {{ bubbles: true }}));",
        serde_json::to_string(from).expect("json"),
        serde_json::to_string(to).expect("json")
    );
    app.eval(&js).await.expect("drag");
}

/// Carol (who runs Projects) drags a channel to a new place, the server
/// keeps it, and Alice (who can't reorder) sees the new order.
#[tokio::test]
async fn a_server_admin_reorders_channels_for_everyone() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    reset(&hs);
    let general = room_id(&hs, "proj-general");
    let news = room_id(&hs, "proj-news");

    let mut alice = App::launch(&hs).await.expect("launch alice");
    login(&mut alice, "alice").await;
    open_projects(&mut alice).await;
    alice
        .wait_for_text(".room-list .room", "projects-news", DEFAULT_WAIT)
        .await
        .expect("alice's list");
    assert_eq!(
        alice
            .count(".room-list .room[draggable=true]")
            .await
            .expect("count"),
        0,
        "no reordering for someone who can't edit the server"
    );

    let mut carol = App::launch(&hs).await.expect("launch carol");
    login(&mut carol, "carol").await;
    open_projects(&mut carol).await;
    carol
        .wait_for(
            &format!(".room-list .room[draggable=true][data-room=\"{general}\"]"),
            DEFAULT_WAIT,
        )
        .await
        .expect("carol may drag");
    // general onto news: general goes before it.
    drag(
        &mut carol,
        &format!(".room[data-room=\"{general}\"]"),
        &format!(".room[data-room=\"{news}\"]"),
    )
    .await;
    wait_joined_order(
        &mut carol,
        &["projects-general", "projects-news", "projects-archive"],
    )
    .await
    .expect("carol's list in the new order");
    wait_joined_order(&mut alice, &["projects-general", "projects-news"])
        .await
        .expect("alice sees it too");
    reset(&hs);
}

/// Wait until the joined channels of the open server are listed in `want`'s order.
async fn wait_joined_order(app: &mut App, want: &[&str]) -> anyhow::Result<()> {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let names: Vec<String> = app
            .texts(".room-list .room:not(.unjoined) .room-name")
            .await?
            .into_iter()
            .map(|n| n.trim().to_owned())
            .collect();
        if names == want {
            return Ok(());
        }
        anyhow::ensure!(
            std::time::Instant::now() < deadline,
            "joined channels are {names:?}, wanted {want:?}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
