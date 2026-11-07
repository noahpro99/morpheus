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
    let names = |selector: &str| {
        format!(
            "return [...document.querySelectorAll('.room-list {selector} .room-name')].map(e => e.textContent);"
        )
    };
    let joined = app
        .eval(&names(".room:not(.unjoined)"))
        .await
        .expect("joined");
    let unjoined = app.eval(&names(".room.unjoined")).await.expect("unjoined");
    (
        serde_json::from_value(joined).unwrap_or_default(),
        serde_json::from_value(unjoined).unwrap_or_default(),
    )
}

async fn open_projects(app: &mut App) {
    app.wait_for(".rail-item[title=\"Projects\"]", DEFAULT_WAIT)
        .await
        .expect("the Projects badge");
    app.click(".rail-item[title=\"Projects\"]")
        .await
        .expect("open Projects");
    app.wait_for_text(".sidebar-header h2", "Projects", DEFAULT_WAIT)
        .await
        .expect("Projects header");
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
    let (joined, unjoined) = channels(&mut app).await;
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
    app.click(".room.unjoined").await.expect("open it");
    app.wait_for_text(".join-card h2", "projects-archive", DEFAULT_WAIT)
        .await
        .expect("a join card");
    assert_eq!(app.count(".chat .composer").await.expect("count"), 0);
    app.wait_for_text(".join-card", "The projects-archive room", DEFAULT_WAIT)
        .await
        .expect("its topic");
    app.click(".join-card .join-button").await.expect("join");
    app.wait_for(".chat .composer", DEFAULT_WAIT)
        .await
        .expect("the room, joined");
    app.wait_for_js(
        "return document.querySelectorAll('.room.unjoined').length === 0 && \
         [...document.querySelectorAll('.room-list .room .room-name')].map(e => e.textContent).join() \
         === 'projects-news,projects-general,projects-archive';",
        DEFAULT_WAIT,
    )
    .await
    .expect("archive among the joined, in order");
    reset(&hs);
}

/// Synthetic HTML5 drag: dragstart on the source, drop on the target.
async fn drag(app: &mut App, from: &str, to: &str) {
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
    let order = "return [...document.querySelectorAll('.room-list .room:not(.unjoined) .room-name')]\
                 .map(e => e.textContent).join() === 'projects-general,projects-news,projects-archive';";
    carol
        .wait_for_js(order, DEFAULT_WAIT)
        .await
        .expect("carol's list in the new order");
    alice
        .wait_for_js(
            "return [...document.querySelectorAll('.room-list .room:not(.unjoined) .room-name')]\
             .map(e => e.textContent).join() === 'projects-general,projects-news';",
            DEFAULT_WAIT,
        )
        .await
        .expect("alice sees it too");
    reset(&hs);
}
