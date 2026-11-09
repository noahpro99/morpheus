//! Rail layout: drag a server onto another to make a folder, open it, move a
//! server out through the menu, rename and ungroup the folder, reorder by
//! dropping between badges. The layout is stored in account data.

#![allow(clippy::expect_used)]

use std::time::Duration;

use morpheus_e2e::{App, DEFAULT_WAIT, api_as};

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

async fn leave_server(app: &mut App, name: &str) {
    app.right_click(&format!(".rail-item[title=\"{name}\"]"))
        .await
        .expect("badge menu");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("menu");
    app.click_text(".context-item.danger", "Leave server")
        .await
        .expect("leave");
    app.wait_for(".leave-space-dialog", DEFAULT_WAIT)
        .await
        .expect("confirm");
    app.click_text(".leave-space-dialog button", "Leave server")
        .await
        .expect("confirm");
    app.wait_for_js(
        &format!(
            "return document.querySelector('.rail-item[title={}]') === null;",
            serde_json::to_string(name).expect("json")
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("badge gone");
}

#[tokio::test]
async fn servers_drag_into_folders_and_reorder() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login(&mut app, "alice").await;
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
    // Earlier runs may have left folders behind: work with the newest one and
    // give it a unique name right away.
    app.wait_for(".rail-folder .folder-badge", DEFAULT_WAIT)
        .await
        .expect("folder created");
    let folder_name = format!("Work {pid}");
    app.eval("[...document.querySelectorAll('.rail-folder .folder-badge')].at(-1).dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 30, clientY: 300 }));").await.expect("folder menu");
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
    app.wait_for_js(
        &format!(
            "return document.querySelector('.rail > .rail-item[title={}]') !== null;",
            serde_json::to_string(&b).expect("json")
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("b is a top-level badge again");

    // Reorder: drop b on the first drop zone, so it comes before the folder.
    drag(
        &mut app,
        &format!(".rail-item[title=\"{b}\"]"),
        ".rail-drop",
    )
    .await;
    app.wait_for_js(
        &format!("const items = [...document.querySelectorAll('.rail > .rail-item.space-badge, .rail > .rail-folder')]; return items[0]?.getAttribute('title') === {};", serde_json::to_string(&b).expect("json")),
        DEFAULT_WAIT,
    )
    .await
    .expect("b first");

    // The layout survives a relaunch (it lives in account data).
    let mut app = app.relaunch().await.expect("relaunch");
    app.wait_for(".shell", DEFAULT_WAIT)
        .await
        .expect("restored");
    app.wait_for(&badge, DEFAULT_WAIT)
        .await
        .expect("folder remembered");
    app.wait_for_js(
        &format!("const items = [...document.querySelectorAll('.rail > .rail-item.space-badge, .rail > .rail-folder')]; return items[0]?.getAttribute('title') === {};", serde_json::to_string(&b).expect("json")),
        Duration::from_secs(10),
    )
    .await
    .expect("order remembered");

    // Ungroup, then clean up both servers. Wait for the room to settle first:
    // the shell closes menus whenever the open room changes, and right after
    // a relaunch the list is still re-picking.
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("room settled");
    app.right_click(&badge).await.expect("folder menu");
    app.click_text(".context-item", "Ungroup")
        .await
        .expect("ungroup");
    app.wait_for_js(
        &format!(
            "return document.querySelector({}) === null;",
            serde_json::to_string(&badge).expect("json")
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("folder gone");
    leave_server(&mut app, &a).await;
    leave_server(&mut app, &b).await;
}

/// The rail marks where you are and what is waiting, as Discord's does: a
/// white bar beside the scope you are in, and a white dot beside any other
/// with unread messages.
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

    // Each mark: [height, colour, right edge of the mark, left edge of its badge].
    let mark = |selector: &str| {
        format!(
            "const item = document.querySelector({}); const pill = item && item.querySelector('.rail-pill'); \
             if (!pill) return null; const p = pill.getBoundingClientRect(), i = item.getBoundingClientRect(); \
             return [p.height, getComputedStyle(pill).backgroundColor, p.right, i.left];",
            serde_json::to_string(selector).expect("json")
        )
    };
    app.wait_for_js(
        &format!(
            "const m = (() => {{ {} }})(); return !!m && m[0] >= 32;",
            mark(".rail-item.selected")
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("a tall bar beside the scope you are in");
    app.wait_for_js(
        &format!(
            "const m = (() => {{ {} }})(); return !!m && m[0] > 0 && m[0] <= 10;",
            mark(".rail-item[title=\"Rooms\"]")
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("a dot beside Rooms, which has an unread message");
    for selector in [".rail-item.selected", ".rail-item[title=\"Rooms\"]"] {
        let shown = app.eval(&mark(selector)).await.expect("mark");
        assert_eq!(
            shown[1].as_str(),
            Some("rgb(255, 255, 255)"),
            "{selector}: not white: {shown}"
        );
        assert!(
            shown[2].as_f64() <= shown[3].as_f64(),
            "{selector}: not to the left of its badge: {shown}"
        );
    }
}
