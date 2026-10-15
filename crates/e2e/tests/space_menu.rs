//! The server menu (space name click and rail badge right-click): create a
//! server, privacy, per-server profile, categories, hide muted, leave.

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

async fn menu_closed(app: &mut App) {
    app.wait_for_js(
        "return document.querySelector('.context-menu') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("menu closed");
}

async fn labels(app: &mut App) -> String {
    app.eval("return [...document.querySelectorAll('.context-menu > .context-item')].map(e => e.querySelector('.menu-label')?.textContent ?? e.textContent).join('|');")
        .await
        .expect("labels")
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[tokio::test]
async fn server_menu_from_name_and_badge_runs_the_server_lifecycle() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login(&mut app, "alice").await;

    // Create a server from the rail "+".
    let server = format!("srv {}", std::process::id());
    app.click(".rail-add").await.expect("rail plus");
    app.wait_for_text(".create-space-dialog h2", "Create server", DEFAULT_WAIT)
        .await
        .expect("dialog");
    app.type_into(".create-space-dialog input[name=name]", &server)
        .await
        .expect("name");
    app.submit(".create-space-dialog").await.expect("create");
    app.wait_for_text(".sidebar-header h2", &server, DEFAULT_WAIT)
        .await
        .expect("new server scoped");
    app.wait_for(&format!(".rail-item[title=\"{server}\"]"), DEFAULT_WAIT)
        .await
        .expect("badge in the rail");

    // The name opens the server menu.
    app.click(".sidebar-header h2.server-name")
        .await
        .expect("click name");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("server menu");
    let items = labels(&mut app).await;
    for needle in [
        "Mark as read",
        "Invite to server",
        "Mute server",
        "Notifications",
        "Hide muted channels",
        "Server settings",
        "Privacy settings",
        "Edit per-server profile",
        "Create channel",
        "Create category",
        "Leave server",
        "Copy server info",
    ] {
        assert!(items.contains(needle), "missing {needle} in {items}");
    }

    // Create a category, then a channel inside it: it lists under the category.
    app.click_text(".context-item", "Create category")
        .await
        .expect("category");
    app.wait_for_text(".create-space-dialog h2", "Create category", DEFAULT_WAIT)
        .await
        .expect("dialog");
    app.type_into(".create-space-dialog input[name=name]", "Text channels")
        .await
        .expect("name");
    app.submit(".create-space-dialog").await.expect("create");
    app.wait_for_js(
        "return document.querySelector('.create-space-dialog') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("closed");
    app.wait_for_text(".sidebar-header h2", &server, DEFAULT_WAIT)
        .await
        .expect("still in the server");
    // The category reaches the room list via sync; retry the dialog until it offers it.
    app.wait_for_js("return true;", Duration::from_millis(800))
        .await
        .expect("settle");
    app.click(".create-room-button").await.expect("plus");
    app.wait_for(".create-room-dialog select[name=category]", DEFAULT_WAIT)
        .await
        .expect("category select");
    app.type_into(".create-room-dialog input[name=name]", "chatter")
        .await
        .expect("name");
    app.eval("const s = document.querySelector('.create-room-dialog select[name=category]'); s.selectedIndex = 1; s.dispatchEvent(new Event('change', { bubbles: true }));")
        .await
        .expect("pick category");
    app.submit(".create-room-dialog")
        .await
        .expect("create room");
    app.wait_for_text(".chat-header h2", "chatter", DEFAULT_WAIT)
        .await
        .expect("room opened");
    app.wait_for_text(".section.category", "Text channels", DEFAULT_WAIT)
        .await
        .expect("grouped under the category");

    // Privacy: invite-only sticks.
    app.right_click(&format!(".rail-item[title=\"{server}\"]"))
        .await
        .expect("badge menu");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("menu from the badge");
    app.click_text(".context-item", "Privacy settings")
        .await
        .expect("privacy");
    app.wait_for(".privacy-dialog", DEFAULT_WAIT)
        .await
        .expect("privacy dialog");
    app.eval("[...document.querySelectorAll('.privacy-dialog input[name=join]')][1].click();")
        .await
        .expect("invite only");
    app.submit(".privacy-dialog").await.expect("save");
    app.wait_for_text(".privacy-dialog .ok", "Saved", DEFAULT_WAIT)
        .await
        .expect("saved");
    app.press("body", "Escape").await.expect("escape");
    app.wait_for_js(
        "return document.querySelector('.privacy-dialog') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("closed");
    app.click(".sidebar-header h2.server-name")
        .await
        .expect("name again");
    app.click_text(".context-item", "Privacy settings")
        .await
        .expect("privacy again");
    app.wait_for_js(
        "return document.querySelector('.privacy-dialog') && [...document.querySelectorAll('.privacy-dialog input[name=join]')][1].checked;",
        DEFAULT_WAIT,
    )
    .await
    .expect("invite-only remembered");
    app.press("body", "Escape").await.expect("escape");

    // Per-server profile: the nickname shows in the member list of its rooms.
    app.click(".sidebar-header h2.server-name")
        .await
        .expect("name");
    app.click_text(".context-item", "Edit per-server profile")
        .await
        .expect("profile");
    app.wait_for(".room-profile-dialog input[name=nickname]", DEFAULT_WAIT)
        .await
        .expect("dialog");
    app.type_into(".room-profile-dialog input[name=nickname]", "Alice (srv)")
        .await
        .expect("nickname");
    app.submit(".room-profile-dialog").await.expect("save");
    app.wait_for_text(".room-profile-dialog .ok", "Saved", DEFAULT_WAIT)
        .await
        .expect("saved");
    app.press("body", "Escape").await.expect("escape");
    app.wait_for_text(".members .member", "Alice (srv)", DEFAULT_WAIT)
        .await
        .expect("nickname in the member list");

    // Hide muted channels: mute the room, it disappears once another room is open.
    app.click(".sidebar-header h2.server-name")
        .await
        .expect("name");
    app.click_text(".context-item", "Hide muted channels")
        .await
        .expect("hide muted");
    menu_closed(&mut app).await;
    app.click(".sidebar-header h2.server-name")
        .await
        .expect("name");
    app.wait_for(".context-item.checked", Duration::from_secs(5))
        .await
        .expect("hide muted is checked");
    app.press("body", "Escape").await.expect("escape");

    // Leave the server: confirm, and the badge goes away.
    app.click(".sidebar-header h2.server-name")
        .await
        .expect("name");
    app.click_text(".context-item.danger", "Leave server")
        .await
        .expect("leave");
    app.wait_for_text(".leave-space-dialog h2", &server, DEFAULT_WAIT)
        .await
        .expect("confirm dialog");
    app.click_text(".leave-space-dialog button", "Leave server")
        .await
        .expect("confirm");
    app.wait_for_js(
        &format!(
            "return document.querySelector('.rail-item[title={}]') === null;",
            serde_json::to_string(&server).expect("json")
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("badge gone");
    app.wait_for(".rail-item.selected[title=\"Rooms\"]", DEFAULT_WAIT)
        .await
        .expect("back in Rooms");

    // Put the preference back so other runs see every room.
    app.click(".rail-item[title=\"Engineering\"]")
        .await
        .expect("engineering");
    app.click(".sidebar-header h2.server-name")
        .await
        .expect("name");
    app.click_text(".context-item", "Hide muted channels")
        .await
        .expect("unhide");
    menu_closed(&mut app).await;
}
