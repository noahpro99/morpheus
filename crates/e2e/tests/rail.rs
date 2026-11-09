//! Rail layout: drag a server onto another to make a folder, open it, move a
//! server out through the menu, rename and ungroup the folder, reorder by
//! dropping between badges. The layout is stored in account data.

#![allow(clippy::expect_used)]

use std::time::Duration;

use morpheus_e2e::{App, DEFAULT_WAIT, api_as, new_account};

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

/// A pointer drag from `from` onto the middle of `to` (or, with `before`,
/// just inside its top edge), let go there unless `hold`.
fn drag_js(from: &str, to: &str, before: bool, hold: bool) -> String {
    format!(
        "const from = document.querySelector({from}); const to = document.querySelector({to}); \
         if (!from || !to) throw new Error('drag endpoints missing'); \
         const f = from.getBoundingClientRect(), t = to.getBoundingClientRect(); \
         const fire = (type, x, y, target) => target.dispatchEvent(new MouseEvent(type, {{ bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons: 1 }})); \
         const x0 = f.left + f.width / 2, y0 = f.top + f.height / 2; \
         const x1 = t.left + t.width / 2, y1 = {before} ? t.top + 3 : t.top + t.height / 2; \
         fire('mousedown', x0, y0, from); \
         for (let i = 1; i <= 10; i++) fire('mousemove', x0 + (x1 - x0) * i / 10, y0 + (y1 - y0) * i / 10, document); \
         if (!{hold}) fire('mouseup', x1, y1, document); return true;",
        from = serde_json::to_string(from).expect("json"),
        to = serde_json::to_string(to).expect("json"),
    )
}

async fn drag(app: &mut App, from: &str, to: &str) {
    app.eval(&drag_js(from, to, false, false))
        .await
        .expect("drag");
}

async fn drag_before(app: &mut App, from: &str, to: &str) {
    app.eval(&drag_js(from, to, true, false))
        .await
        .expect("drag");
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
    // Both left the top level (into the new folder).
    app.wait_for_js(
        &format!(
            "return ![...document.querySelectorAll('.rail > .rail-item')].some(b => b.title === {} || b.title === {});",
            serde_json::to_string(&a).expect("json"),
            serde_json::to_string(&b).expect("json")
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("a folder of the two");
    // The new layout is saved to the account and comes back: let it settle.
    tokio::time::sleep(Duration::from_secs(1)).await;
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

    // Reorder: let go of b just above the first server, so it comes first.
    drag_before(
        &mut app,
        &format!(".rail-item[title=\"{b}\"]"),
        ".rail > [data-index=\"0\"]",
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

/// Servers (and the rail's other badges) are rounded squares, not circles.
#[tokio::test]
async fn rail_badges_are_rounded_squares() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login(&mut app, "alice").await;
    app.wait_for(".rail-item.space-badge", DEFAULT_WAIT)
        .await
        .expect("a server");
    // Each badge, and the picture inside it: [title, corner radius, width].
    let round = app
        .eval(
            "return [...document.querySelectorAll('.rail > .rail-item, .rail-folder .rail-item, .rail-item .avatar')] \
               .filter(el => !el.matches('.rail-add')) \
               .map(el => [el.title || el.className, parseFloat(getComputedStyle(el).borderTopLeftRadius), el.getBoundingClientRect().width]) \
               .filter(([, r, w]) => r >= w * 0.45 || r < w * 0.2);",
        )
        .await
        .expect("badges");
    assert_eq!(
        round,
        serde_json::json!([]),
        "badges that are circles (or square)"
    );
}

/// Dragging a server is plain: the pointer turns into a grabbing hand (no
/// picture of the badge follows it), a white line shows where it will land,
/// nothing else moves or spaces out, and letting go puts it there.
#[tokio::test]
async fn dragging_a_server_shows_a_line_where_it_lands() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let user = new_account(&hs, "drag").expect("account");
    let mut app = App::launch(&hs).await.expect("launch app");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", &user)
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
        .expect("signed in");
    for name in ["one", "two", "three"] {
        create_server(&mut app, name).await;
    }
    let order = "return [...document.querySelectorAll('.rail > .rail-item.space-badge')].map(b => b.title);";
    assert_eq!(
        app.eval(order).await.expect("order"),
        serde_json::json!(["three", "two", "one"])
    );
    assert_eq!(
        app.count(".rail [draggable=true]").await.expect("count"),
        0,
        "badges use the browser's drag (which drags a picture of them)"
    );
    let tops = "return [...document.querySelectorAll('.rail > .rail-item')].map(b => Math.round(b.getBoundingClientRect().top));";
    let before = app.eval(tops).await.expect("tops");

    // New servers go on top: hold "three" just above "one".
    app.eval(&drag_js(
        ".rail-item[title=\"three\"]",
        ".rail-item[title=\"one\"]",
        true,
        true,
    ))
    .await
    .expect("drag");
    let during = app
        .eval(
            "const line = document.querySelector('.rail-drop-line'); \
             const r = line && line.getBoundingClientRect(); \
             const two = document.querySelector('.rail-item[title=\"two\"]').getBoundingClientRect(); \
             const one = document.querySelector('.rail-item[title=\"one\"]').getBoundingClientRect(); \
             return { cursor: getComputedStyle(document.body).cursor, \
               line: !!r && r.height > 0 && r.height <= 4 && r.width >= 24, \
               colour: line ? getComputedStyle(line).backgroundColor : null, \
               between: !!r && r.top >= two.bottom - 2 && r.bottom <= one.top + 2 };",
        )
        .await
        .expect("during");
    assert_eq!(during["cursor"], "grabbing", "{during}");
    assert_eq!(during["line"], true, "a thin line: {during}");
    assert_eq!(during["colour"], "rgb(255, 255, 255)", "{during}");
    assert_eq!(during["between"], true, "between two and one: {during}");
    assert_eq!(
        app.eval(tops).await.expect("tops"),
        before,
        "the badges moved while dragging"
    );

    // Let go: "three" lands between two and one.
    app.eval(
        "const t = document.querySelector('.rail-item[title=\"one\"]').getBoundingClientRect(); \
         document.dispatchEvent(new MouseEvent('mouseup', { bubbles: true, clientX: t.left + t.width / 2, clientY: t.top + 3 })); return true;",
    )
    .await
    .expect("let go");
    app.wait_for_js(
        &format!(
            "return JSON.stringify((() => {{ {order} }})()) === '[\"two\",\"three\",\"one\"]';"
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("moved");
    assert_eq!(
        app.count(".rail-drop-line").await.expect("count"),
        0,
        "the line goes"
    );
    assert_eq!(
        app.eval("return getComputedStyle(document.body).cursor;")
            .await
            .expect("cursor"),
        serde_json::json!("auto"),
        "the pointer is back"
    );
}
