//! Room menu and dialogs: mark as read, notification modes, invite, edit,
//! create (ungrouped and inside a space), copy, leave.

#![allow(clippy::expect_used)]

use std::time::Duration;

use morpheus_e2e::{App, DEFAULT_WAIT};

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
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("room listed");
}

/// Right-click the room row whose name contains `name`.
async fn room_menu(app: &mut App, name: &str) {
    let js = format!(
        "const el = [...document.querySelectorAll('.room')].find(el => el.textContent.includes({})); \
         if (!el) throw new Error('no room ' + {}); const r = el.getBoundingClientRect(); \
         el.dispatchEvent(new MouseEvent('contextmenu', {{ bubbles: true, cancelable: true, clientX: r.left + 20, clientY: r.top + 10 }}));",
        serde_json::to_string(name).expect("json"),
        serde_json::to_string(name).expect("json")
    );
    app.eval(&js).await.expect("right-click room");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("room menu");
}

async fn menu_closed(app: &mut App) {
    app.wait_for_js(
        "return document.querySelector('.context-menu') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("menu closed");
}

async fn click_submenu(app: &mut App, parent: &str, child: &str) {
    let js = format!(
        "const p = [...document.querySelectorAll('.context-item.has-submenu')].find(el => el.querySelector('.menu-label')?.textContent === {}); \
         if (!p) throw new Error('no submenu ' + {}); \
         const c = [...p.querySelectorAll('.context-submenu .context-item')].find(el => el.textContent.trim() === {}); \
         if (!c) throw new Error('no child ' + {}); c.click();",
        serde_json::to_string(parent).expect("json"),
        serde_json::to_string(parent).expect("json"),
        serde_json::to_string(child).expect("json"),
        serde_json::to_string(child).expect("json"),
    );
    app.eval(&js).await.expect("submenu click");
}

async fn checked_label(app: &mut App) -> String {
    app.eval("return [...document.querySelectorAll('.context-submenu .context-item.checked')].map(e => e.textContent.trim()).join('|');")
        .await
        .expect("checked")
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[tokio::test]
async fn room_menu_marks_read_and_sets_notification_mode() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login(&mut alice, "alice").await;
    alice.open_room("general").await.expect("open general");

    // Bob posts in #random: it turns unread for alice. (#random, not
    // #moderation: the moderation tests ban bob there.)
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login(&mut bob, "bob").await;
    bob.open_room("random").await.expect("bob opens random");
    bob.wait_for(".composer input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer");
    bob.type_into(
        ".composer input",
        &format!("unread ping {}", std::process::id()),
    )
    .await
    .expect("type");
    bob.press(".composer input", "Enter").await.expect("send");

    alice
        .wait_for_js(
            "return [...document.querySelectorAll('.room.unread')].some(r => r.textContent.includes('random'));",
            DEFAULT_WAIT,
        )
        .await
        .expect("random shows unread");
    room_menu(&mut alice, "random").await;
    for label in [
        "Mark as read",
        "Invite people",
        "Notifications",
        "Edit room",
        "Create room",
        "Copy link",
        "Copy room ID",
        "Leave room",
    ] {
        alice
            .wait_for_text(".context-item", label, Duration::from_secs(2))
            .await
            .unwrap_or_else(|e| panic!("menu item {label}: {e}"));
    }
    alice
        .click_text(".context-item", "Mark as read")
        .await
        .expect("mark read");
    menu_closed(&mut alice).await;
    alice
        .wait_for_js(
            "return ![...document.querySelectorAll('.room.unread')].some(r => r.textContent.includes('random'));",
            DEFAULT_WAIT,
        )
        .await
        .expect("random no longer unread");

    // Notification mode round-trips through the push rules.
    room_menu(&mut alice, "general").await;
    click_submenu(&mut alice, "Notifications", "Mute").await;
    menu_closed(&mut alice).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    room_menu(&mut alice, "general").await;
    assert_eq!(
        checked_label(&mut alice).await,
        "Mute",
        "mute is the checked mode"
    );
    click_submenu(&mut alice, "Notifications", "All messages").await;
    menu_closed(&mut alice).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    room_menu(&mut alice, "general").await;
    assert_eq!(
        checked_label(&mut alice).await,
        "All messages",
        "back to all messages"
    );
    alice
        .press(".context-menu", "Escape")
        .await
        .expect("escape");
}

#[tokio::test]
async fn create_edit_invite_and_leave_rooms() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login(&mut app, "alice").await;
    app.open_room("general").await.expect("open general");

    // Create an ungrouped room from the sidebar "+".
    let name = format!("e2e room {}", std::process::id());
    app.click(".create-room-button").await.expect("plus");
    app.wait_for(".create-room-dialog input[name=name]", DEFAULT_WAIT)
        .await
        .expect("dialog");
    app.wait_for_text(
        ".create-room-dialog",
        "Not in any space",
        Duration::from_secs(2),
    )
    .await
    .expect("scope shown");
    app.type_into(".create-room-dialog input[name=name]", &name)
        .await
        .expect("name");
    app.type_into(".create-room-dialog input[name=topic]", "made by e2e")
        .await
        .expect("topic");
    app.submit(".create-room-dialog").await.expect("create");
    app.wait_for_text(".chat-header h2", &name, DEFAULT_WAIT)
        .await
        .expect("new room opened");
    app.wait_for(".rail-item.selected[title=\"Rooms\"]", DEFAULT_WAIT)
        .await
        .expect("rooms scope");
    app.wait_for_text(".room.selected", &name, DEFAULT_WAIT)
        .await
        .expect("listed and selected");

    // Edit name and topic.
    room_menu(&mut app, &name).await;
    app.click_text(".context-item", "Edit room")
        .await
        .expect("edit");
    app.wait_for(".edit-room-dialog input[name=name]", DEFAULT_WAIT)
        .await
        .expect("edit dialog");
    let renamed = format!("{name} renamed");
    app.type_into(".edit-room-dialog input[name=name]", &renamed)
        .await
        .expect("rename");
    app.type_into(".edit-room-dialog input[name=topic]", "edited topic")
        .await
        .expect("retopic");
    app.submit(".edit-room-dialog").await.expect("save");
    app.wait_for_text(".edit-room-dialog .ok", "Saved", DEFAULT_WAIT)
        .await
        .expect("saved");
    app.press(".edit-room-dialog", "Escape")
        .await
        .expect("escape");
    app.wait_for_text(".chat-header h2", &renamed, DEFAULT_WAIT)
        .await
        .expect("header renamed");
    app.wait_for_text(".chat-header .topic", "edited topic", DEFAULT_WAIT)
        .await
        .expect("topic updated");

    // Invite bob.
    room_menu(&mut app, &renamed).await;
    app.click_text(".context-item", "Invite people")
        .await
        .expect("invite");
    app.wait_for(".invite-dialog input[name=user]", DEFAULT_WAIT)
        .await
        .expect("invite dialog");
    app.type_into(".invite-dialog input[name=user]", "@bob:localhost")
        .await
        .expect("user");
    app.submit(".invite-dialog").await.expect("send invite");
    app.wait_for_text(".invite-dialog .ok", "Invited @bob:localhost", DEFAULT_WAIT)
        .await
        .expect("invited");
    app.press(".invite-dialog", "Escape").await.expect("escape");

    // Create a room inside the Engineering space: it lands under that badge.
    app.click(".rail-item[title=\"Engineering\"]")
        .await
        .expect("space");
    app.wait_for_text(".sidebar-header h2", "Engineering", DEFAULT_WAIT)
        .await
        .expect("space scope");
    let space_room = format!("space room {}", std::process::id());
    app.click(".create-room-button")
        .await
        .expect("plus in space");
    app.wait_for_text(
        ".create-room-dialog",
        "In the Engineering space",
        DEFAULT_WAIT,
    )
    .await
    .expect("space named");
    app.type_into(".create-room-dialog input[name=name]", &space_room)
        .await
        .expect("name");
    app.submit(".create-room-dialog").await.expect("create");
    app.wait_for_text(".chat-header h2", &space_room, DEFAULT_WAIT)
        .await
        .expect("space room opened");
    app.wait_for(".rail-item.selected[title=\"Engineering\"]", DEFAULT_WAIT)
        .await
        .expect("still in the space");
    app.wait_for_text(".room.selected", &space_room, DEFAULT_WAIT)
        .await
        .expect("listed under the space");

    // Leave both: they drop out of the list.
    room_menu(&mut app, &space_room).await;
    app.click_text(".context-item.danger", "Leave room")
        .await
        .expect("leave");
    menu_closed(&mut app).await;
    app.wait_for_js(
        &format!("return ![...document.querySelectorAll('.room')].some(r => r.textContent.includes({}));", serde_json::to_string(&space_room).expect("json")),
        DEFAULT_WAIT,
    )
    .await
    .expect("space room gone");
    app.click(".rail-item[title=\"Rooms\"]")
        .await
        .expect("rooms");
    room_menu(&mut app, &renamed).await;
    app.click_text(".context-item.danger", "Leave room")
        .await
        .expect("leave");
    menu_closed(&mut app).await;
    app.wait_for_js(
        &format!("return ![...document.querySelectorAll('.room')].some(r => r.textContent.includes({}));", serde_json::to_string(&renamed).expect("json")),
        DEFAULT_WAIT,
    )
    .await
    .expect("room gone after leaving");
}
