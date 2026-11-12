//! What the items of a DM's menu (right-click a name under Direct
//! messages) do: unread, pin, favorites, close, ignore, mute for a while.

#![allow(clippy::expect_used)]

use std::time::Duration;

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

fn js(value: &str) -> String {
    serde_json::to_string(value).expect("json")
}

/// The DM row named `name` (exactly), as a JS expression.
fn row(name: &str) -> String {
    format!(
        "[...document.querySelectorAll('.room.dm')].find(r => r.querySelector('.room-name')?.textContent.trim() === {})",
        js(name)
    )
}

/// Wait until the DM list shows `name` (or not).
async fn wait_listed(app: &mut App, name: &str, listed: bool) {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let names = app.texts(".room.dm .room-name").await.expect("names");
        if names.iter().any(|n| n.trim() == name) == listed {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the DM with {name} should be listed: {listed}: {names:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Alice signed in, on the DM list, nothing open (the Friends page).
async fn alice_on_dms(hs: &str) -> App {
    let mut app = App::launch_desktop(hs).await.expect("launch");
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
    app.wait_for(".rail-dms", DEFAULT_WAIT).await.expect("rail");
    app.click(".rail-dms").await.expect("DMs");
    wait_listed(&mut app, "carol", true).await;
    // The Friends page (nothing open) is not in the GPUI app yet.
    if app.count(".friends-row").await.expect("count") > 0 {
        app.click(".friends-row").await.expect("friends page");
    }
    app
}

async fn dm_menu(app: &mut App, name: &str) {
    app.right_click_exact(".room.dm .room-name", name)
        .await
        .expect("right-click");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("menu");
}

async fn choose(app: &mut App, label: &str) {
    app.click_text(".context-menu .context-item", label)
        .await
        .expect("menu item");
    app.wait_for_count(".context-menu", 0, DEFAULT_WAIT)
        .await
        .expect("menu closed");
}

/// The menu's top-level labels.
async fn labels(app: &mut App) -> Vec<String> {
    app.texts(".context-menu:not(.context-submenu) > .context-item > .menu-label")
        .await
        .expect("labels")
        .into_iter()
        .map(|l| l.trim().to_owned())
        .collect()
}

async fn has_class(app: &mut App, name: &str, class: &str) -> bool {
    app.texts(&format!(".room.dm.{class} .room-name"))
        .await
        .expect("class")
        .iter()
        .any(|n| n.trim() == name)
}

async fn wait_class(app: &mut App, name: &str, class: &str, on: bool) {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while has_class(app, name, class).await != on {
        assert!(
            std::time::Instant::now() < deadline,
            "{name} {} {class}",
            if on { "gets" } else { "loses" }
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Wait until the first DM listed is `name`.
async fn wait_first(app: &mut App, name: &str) {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let names = app.texts(".room.dm .room-name").await.expect("names");
        if names.first().is_some_and(|n| n.trim() == name) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{name} should be first: {names:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn mark_a_dm_unread_and_read_again() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = alice_on_dms(&hs).await;
    if has_class(&mut app, "carol", "unread").await {
        dm_menu(&mut app, "carol").await;
        choose(&mut app, "Mark as read").await;
        wait_class(&mut app, "carol", "unread", false).await;
    }
    dm_menu(&mut app, "carol").await;
    choose(&mut app, "Mark as unread").await;
    wait_class(&mut app, "carol", "unread", true).await;
    dm_menu(&mut app, "carol").await;
    choose(&mut app, "Mark as read").await;
    wait_class(&mut app, "carol", "unread", false).await;
}

#[tokio::test]
async fn a_pinned_dm_stays_at_the_top() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = alice_on_dms(&hs).await;
    let order: Vec<String> = app
        .texts(".room.dm .room-name")
        .await
        .expect("names")
        .into_iter()
        .map(|n| n.trim().to_owned())
        .collect();
    assert!(order.len() >= 2, "two DMs to order: {order:?}");
    let last = order.last().expect("a DM").clone();

    dm_menu(&mut app, &last).await;
    choose(&mut app, "Pin").await;
    wait_first(&mut app, &last).await;
    wait_class(&mut app, &last, "pinned", true).await;

    // Kept after a restart (it is in the account, not just this window).
    let mut app = app.relaunch().await.expect("relaunch");
    app.wait_for(".rail-dms", DEFAULT_WAIT).await.expect("rail");
    app.click(".rail-dms").await.expect("DMs");
    wait_first(&mut app, &last).await;

    dm_menu(&mut app, &last).await;
    assert!(labels(&mut app).await.contains(&"Unpin".to_owned()));
    choose(&mut app, "Unpin").await;
    wait_class(&mut app, &last, "pinned", false).await;
    // The change is saved in the account in the background: let it land.
    tokio::time::sleep(Duration::from_secs(1)).await;
}

#[tokio::test]
async fn favorites_get_their_own_place_in_the_rail() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = alice_on_dms(&hs).await;
    assert_eq!(
        app.count(".rail-favorites").await.expect("count"),
        0,
        "no favorites yet"
    );
    dm_menu(&mut app, "carol").await;
    choose(&mut app, "Add to favorites").await;
    app.wait_for(".rail-favorites", DEFAULT_WAIT)
        .await
        .expect("a Favorites place in the rail");
    app.click(".rail-favorites").await.expect("open favorites");
    app.wait_for_text(".sidebar-header h2", "Favorites", DEFAULT_WAIT)
        .await
        .expect("the Favorites list");
    wait_listed(&mut app, "carol", true).await;

    dm_menu(&mut app, "carol").await;
    choose(&mut app, "Remove from favorites").await;
    app.wait_for_count(".rail-favorites", 0, DEFAULT_WAIT)
        .await
        .expect("no Favorites place once empty");
}

/// The room ID of the DM with `name`.
async fn dm_room_id(app: &mut App, name: &str) -> String {
    app.attr_containing(".room.dm", name, "data-room")
        .await
        .expect("room id")
        .expect("a DM row with an ID")
}

#[tokio::test]
async fn a_closed_dm_comes_back_with_a_new_message() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = alice_on_dms(&hs).await;
    let room_id = dm_room_id(&mut app, "carol").await;
    dm_menu(&mut app, "carol").await;
    choose(&mut app, "Close DM").await;
    let by_id = format!(".room.dm[data-room=\"{room_id}\"]");
    app.wait_for_count(&by_id, 0, DEFAULT_WAIT)
        .await
        .expect("closed: gone from the list");
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(app.count(&by_id).await.expect("look"), 0, "stays closed");

    morpheus_e2e::send_as(
        &hs,
        "carol",
        &room_id,
        &format!("still there? {}", std::process::id()),
    )
    .expect("carol writes");
    app.wait_for(&format!("{by_id}.unread"), DEFAULT_WAIT)
        .await
        .expect("back with her message, unread");
}

/// The event IDs of the folds shown (their messages' IDs).
async fn fold_ids(app: &mut App) -> Vec<String> {
    app.attrs(".message.ignored", "data-event-id")
        .await
        .expect("fold ids")
        .into_iter()
        .flatten()
        .collect()
}

#[tokio::test]
async fn an_ignored_persons_messages_fold_away() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = alice_on_dms(&hs).await;
    let room_id = dm_room_id(&mut app, "carol").await;
    // A run cut short may have left her ignored (and silent).
    dm_menu(&mut app, "carol").await;
    if labels(&mut app).await.contains(&"Unignore".to_owned()) {
        choose(&mut app, "Unignore").await;
    } else {
        app.press("body", "Escape").await.expect("close");
        app.wait_for_count(".context-menu", 0, DEFAULT_WAIT)
            .await
            .expect("closed");
    }
    let text = format!("ignore me {}", std::process::id());
    morpheus_e2e::send_as(&hs, "carol", &room_id, &text).expect("carol writes");
    app.wait_for_text(".notification-log li", &text, DEFAULT_WAIT)
        .await
        .expect("a notification for her message");
    app.click_text(".room.dm", "carol").await.expect("open");
    app.wait_for_exact_text(".message .body", &text, DEFAULT_WAIT)
        .await
        .expect("her message");

    dm_menu(&mut app, "carol").await;
    choose(&mut app, "Ignore").await;
    app.wait_for(".message.ignored", DEFAULT_WAIT)
        .await
        .expect("a fold in its place");
    assert!(
        !app.texts(".message .body")
            .await
            .expect("bodies")
            .iter()
            .any(|b| b.trim() == text),
        "her message folded away"
    );

    // And she makes no sound: nothing new notifies.
    let folds = fold_ids(&mut app).await;
    app.click(".friends-row").await.expect("away from the DM");
    let quiet = format!("still ignored {}", std::process::id());
    morpheus_e2e::send_as(&hs, "carol", &room_id, &quiet).expect("carol writes again");
    app.click_text(".room.dm", "carol")
        .await
        .expect("open again");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while fold_ids(&mut app).await.iter().all(|id| folds.contains(id)) {
        assert!(
            std::time::Instant::now() < deadline,
            "her new message arrived, folded"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        !app.texts(".notification-log li")
            .await
            .expect("log")
            .iter()
            .any(|l| l.contains(&quiet)),
        "no notification from someone ignored"
    );

    dm_menu(&mut app, "carol").await;
    choose(&mut app, "Unignore").await;
    app.wait_for_exact_text(".message .body", &text, DEFAULT_WAIT)
        .await
        .expect("her message back");
}

#[tokio::test]
async fn mute_a_dm_for_a_while_then_unmute() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = alice_on_dms(&hs).await;
    let room_id = dm_room_id(&mut app, "carol").await;
    // A run cut short may have left her muted.
    if has_class(&mut app, "carol", "muted").await {
        dm_menu(&mut app, "carol").await;
        choose(&mut app, "Unmute @carol").await;
        wait_class(&mut app, "carol", "muted", false).await;
        wait_notifies(&hs, &room_id, true).await;
    }
    dm_menu(&mut app, "carol").await;
    app.click_submenu("Mute @carol", "For 15 minutes")
        .await
        .expect("mute for 15 minutes");
    wait_class(&mut app, "carol", "muted", true).await;
    wait_notifies(&hs, &room_id, false).await;
    dm_menu(&mut app, "carol").await;
    assert!(
        labels(&mut app).await.contains(&"Unmute @carol".to_owned()),
        "muted: offers to unmute"
    );
    choose(&mut app, "Unmute @carol").await;
    wait_class(&mut app, "carol", "muted", false).await;
    // The change is sent to the server in the background; ending the app
    // before it lands left her DM muted for the next test (notifications
    // from her never came).
    wait_notifies(&hs, &room_id, true).await;
}

/// The labels of the open menu's top-level items that have no icon (or
/// picture) in front of them.
async fn items_without_icons(app: &mut App) -> Vec<String> {
    if app.is_gpui() {
        // The app marks an item without a glyph or picture `no-icon`.
        return app
            .texts(".context-menu .context-item.no-icon .menu-label")
            .await
            .expect("labels")
            .into_iter()
            .map(|l| l.trim().to_owned())
            .collect();
    }
    let missing = "return [...document.querySelectorAll('.context-menu:not(.context-submenu) > .context-item')] \
        .filter(i => !i.querySelector(':scope > .menu-icon, :scope > .menu-avatar')) \
        .map(i => i.querySelector('.menu-label').textContent.trim());";
    serde_json::from_value(app.eval(missing).await.expect("labels")).expect("a list")
}

/// Every item of a DM's menu, and of a room's, has an icon.
#[tokio::test]
async fn every_menu_item_has_an_icon() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = alice_on_dms(&hs).await;
    dm_menu(&mut app, "carol").await;
    let in_dm = items_without_icons(&mut app).await;
    app.press("body", "Escape").await.expect("close");
    app.wait_for_count(".context-menu", 0, DEFAULT_WAIT)
        .await
        .expect("closed");

    app.open_room("general").await.expect("a room");
    app.right_click_exact(".room .room-name", "general")
        .await
        .expect("room menu");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("menu");
    let in_room = items_without_icons(&mut app).await;
    app.press("body", "Escape").await.expect("close");

    // A message's, a member's and a server's menus too.
    let mut others = Vec::new();
    for (what, target) in [
        ("message", ".message .body"),
        ("member", ".members .member"),
        ("server", ".rail-item[title=\"Engineering\"]"),
    ] {
        app.wait_for_count(".context-menu", 0, DEFAULT_WAIT)
            .await
            .expect("closed");
        app.wait_for(target, DEFAULT_WAIT)
            .await
            .unwrap_or_else(|err| panic!("a {what} to right-click: {err}"));
        app.right_click(target).await.expect("right-click");
        app.wait_for(".context-menu", DEFAULT_WAIT)
            .await
            .unwrap_or_else(|err| panic!("{what} menu: {err}"));
        others.push((what, items_without_icons(&mut app).await));
        app.press("body", "Escape").await.expect("close");
    }
    assert!(in_dm.is_empty(), "DM menu items without an icon: {in_dm:?}");
    assert!(
        in_room.is_empty(),
        "room menu items without an icon: {in_room:?}"
    );
    for (what, missing) in others {
        assert!(
            missing.is_empty(),
            "{what} menu items without an icon: {missing:?}"
        );
    }
}

/// Wait until the server's push rules agree that the room does (or does not)
/// notify alice.
async fn wait_notifies(hs: &str, room_id: &str, notifies: bool) {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while morpheus_e2e::room_notifies(hs, "alice", room_id).expect("push rules") != notifies {
        assert!(
            std::time::Instant::now() < deadline,
            "the server never {} {room_id}",
            if notifies { "unmuted" } else { "muted" }
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
