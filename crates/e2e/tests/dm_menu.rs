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
    app.wait_for_js(&format!("return !!{};", row("carol")), DEFAULT_WAIT)
        .await
        .expect("the DM with carol listed");
    app.click(".friends-row").await.expect("friends page");
    app
}

async fn dm_menu(app: &mut App, name: &str) {
    app.eval(&format!(
        "const el = {}; const r = el.getBoundingClientRect(); \
         el.dispatchEvent(new MouseEvent('contextmenu', {{ bubbles: true, cancelable: true, clientX: r.left + 20, clientY: r.top + 10 }}));",
        row(name)
    ))
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
    app.wait_for_js(
        "return document.querySelector('.context-menu') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("menu closed");
}

/// The menu's top-level labels.
async fn labels(app: &mut App) -> Vec<String> {
    serde_json::from_value(
        app.eval("return [...document.querySelectorAll('.context-menu:not(.context-submenu) > .context-item > .menu-label')].map(l => l.textContent.trim());")
            .await
            .expect("labels"),
    )
    .expect("strings")
}

async fn has_class(app: &mut App, name: &str, class: &str) -> bool {
    app.eval(&format!(
        "return !!{}?.classList.contains({});",
        row(name),
        js(class)
    ))
    .await
    .expect("class")
        == serde_json::json!(true)
}

async fn wait_class(app: &mut App, name: &str, class: &str, on: bool) {
    app.wait_for_js(
        &format!(
            "return !!{}?.classList.contains({}) === {on};",
            row(name),
            js(class)
        ),
        DEFAULT_WAIT,
    )
    .await
    .unwrap_or_else(|err| {
        panic!(
            "{name} {} {class}: {err}",
            if on { "gets" } else { "loses" }
        )
    });
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
    let names = "return [...document.querySelectorAll('.room.dm .room-name')].map(n => n.textContent.trim());";
    let order: Vec<String> =
        serde_json::from_value(app.eval(names).await.expect("names")).expect("strings");
    assert!(order.len() >= 2, "two DMs to order: {order:?}");
    let last = order.last().expect("a DM").clone();

    dm_menu(&mut app, &last).await;
    choose(&mut app, "Pin").await;
    app.wait_for_js(
        &format!(
            "return document.querySelector('.room.dm .room-name')?.textContent.trim() === {};",
            js(&last)
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("the pinned DM first");
    wait_class(&mut app, &last, "pinned", true).await;

    // Kept after a restart (it is in the account, not just this window).
    let mut app = app.relaunch().await.expect("relaunch");
    app.wait_for(".rail-dms", DEFAULT_WAIT).await.expect("rail");
    app.click(".rail-dms").await.expect("DMs");
    app.wait_for_js(
        &format!(
            "return document.querySelector('.room.dm .room-name')?.textContent.trim() === {};",
            js(&last)
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("still first after a restart");

    dm_menu(&mut app, &last).await;
    assert!(labels(&mut app).await.contains(&"Unpin".to_owned()));
    choose(&mut app, "Unpin").await;
    wait_class(&mut app, &last, "pinned", false).await;
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
    app.wait_for_js(&format!("return !!{};", row("carol")), DEFAULT_WAIT)
        .await
        .expect("carol's DM in it");

    dm_menu(&mut app, "carol").await;
    choose(&mut app, "Remove from favorites").await;
    app.wait_for_js(
        "return document.querySelector('.rail-favorites') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("no Favorites place once empty");
}

#[tokio::test]
async fn a_closed_dm_comes_back_with_a_new_message() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = alice_on_dms(&hs).await;
    let room_id: String = app
        .eval(&format!("return {}.dataset.room;", row("carol")))
        .await
        .expect("room id")
        .as_str()
        .expect("string")
        .to_owned();
    dm_menu(&mut app, "carol").await;
    choose(&mut app, "Close DM").await;
    let by_id = format!("document.querySelector('.room.dm[data-room=\"{room_id}\"]')");
    if let Err(err) = app
        .wait_for_js(&format!("return !{by_id};"), DEFAULT_WAIT)
        .await
    {
        let state = app
            .eval(&format!("const r = {by_id}; return [r?.className, document.querySelector('.chat-header h2')?.textContent, document.querySelector('.friends-page') !== null];"))
            .await;
        panic!("closed: gone from the list: {err}\nrow, header, friends page: {state:?}");
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(
        app.eval(&format!("return !!{by_id};")).await.expect("look"),
        serde_json::json!(false),
        "stays closed"
    );

    morpheus_e2e::send_as(
        &hs,
        "carol",
        &room_id,
        &format!("still there? {}", std::process::id()),
    )
    .expect("carol writes");
    app.wait_for_js(
        &format!("return !!{by_id}?.classList.contains('unread');"),
        DEFAULT_WAIT,
    )
    .await
    .expect("back with her message, unread");
}

#[tokio::test]
async fn an_ignored_persons_messages_fold_away() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = alice_on_dms(&hs).await;
    let room_id: String = app
        .eval(&format!("return {}.dataset.room;", row("carol")))
        .await
        .expect("room id")
        .as_str()
        .expect("string")
        .to_owned();
    let text = format!("ignore me {}", std::process::id());
    morpheus_e2e::send_as(&hs, "carol", &room_id, &text).expect("carol writes");
    app.wait_for_text(".notification-log li", &text, DEFAULT_WAIT)
        .await
        .expect("a notification for her message");
    app.eval(&format!("{}.click();", row("carol")))
        .await
        .expect("open");
    app.wait_for_exact_text(".message .body", &text, DEFAULT_WAIT)
        .await
        .expect("her message");

    dm_menu(&mut app, "carol").await;
    choose(&mut app, "Ignore").await;
    app.wait_for_js(
        &format!("return ![...document.querySelectorAll('.message .body')].some(b => b.textContent.trim() === {});", js(&text)),
        DEFAULT_WAIT,
    )
    .await
    .expect("her message folded away");
    app.wait_for(".message.ignored", DEFAULT_WAIT)
        .await
        .expect("a fold in its place");

    // And she makes no sound: nothing new notifies.
    let folds = app.count(".message.ignored").await.expect("count");
    app.click(".friends-row").await.expect("away from the DM");
    let quiet = format!("still ignored {}", std::process::id());
    morpheus_e2e::send_as(&hs, "carol", &room_id, &quiet).expect("carol writes again");
    app.eval(&format!("{}.click();", row("carol")))
        .await
        .expect("open again");
    app.wait_for_js(
        &format!("return document.querySelectorAll('.message.ignored').length > {folds};"),
        DEFAULT_WAIT,
    )
    .await
    .expect("her new message arrived, folded");
    assert_eq!(
        app.eval(&format!(
            "return [...document.querySelectorAll('.notification-log li')].some(l => l.textContent.includes({}));",
            js(&quiet)
        ))
        .await
        .expect("log"),
        serde_json::json!(false),
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
    dm_menu(&mut app, "carol").await;
    app.click_submenu("Mute @carol", "For 15 minutes")
        .await
        .expect("mute for 15 minutes");
    wait_class(&mut app, "carol", "muted", true).await;
    dm_menu(&mut app, "carol").await;
    assert!(
        labels(&mut app).await.contains(&"Unmute @carol".to_owned()),
        "muted: offers to unmute"
    );
    choose(&mut app, "Unmute @carol").await;
    wait_class(&mut app, "carol", "muted", false).await;
}
