//! The rest of the message menu: emoji picker, reaction viewer and removal,
//! pin/unpin with the header panel, mark unread, forward, report, copy ID.

#![allow(clippy::expect_used)]

use std::time::Duration;

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login_and_open(app: &mut App, user: &str, room: &str) {
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
    app.open_room(room).await.expect("open room");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
}

/// Send a message and return a JS expression selecting its row.
async fn send_fresh(app: &mut App, tag: &str) -> String {
    let text = format!("{tag} {}", std::process::id());
    app.type_into(".composer .composer-input", &text)
        .await
        .expect("type");
    app.press(".composer .composer-input", "Enter")
        .await
        .expect("send");
    app.wait_for_exact_text(".message.own:not(.pending) .body", &text, DEFAULT_WAIT)
        .await
        .expect("message sent");
    format!(
        "[...document.querySelectorAll('.message.own')].find(m => m.querySelector('.body')?.textContent === {})",
        serde_json::to_string(&text).expect("json")
    )
}

async fn open_menu_on(app: &mut App, row_js: &str) {
    // Retried: the row can be re-rendered (a timeline reset) right after sending.
    app.wait_for_js(
        &format!(
            "const row = {row_js}; if (!row) return false; \
             row.dispatchEvent(new MouseEvent('contextmenu', {{ bubbles: true, cancelable: true, clientX: 420, clientY: 320 }})); return true;"
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("right-click");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("menu");
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
    app.click_submenu(parent, child)
        .await
        .expect("submenu click");
}

#[tokio::test]
async fn reactions_picker_viewer_and_removal() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "alice", "general").await;
    let row = send_fresh(&mut app, "reaction picker").await;

    // Pick 🎉 from the grid.
    open_menu_on(&mut app, &row).await;
    click_submenu(&mut app, "Add reaction", "🎉").await;
    menu_closed(&mut app).await;
    app.wait_for_js(
        &format!("return [...{row}.querySelectorAll('.reaction.mine')].some(r => r.textContent.includes('🎉'));"),
        DEFAULT_WAIT,
    )
    .await
    .expect("🎉 reaction on the message");

    // View reactions lists the sender.
    open_menu_on(&mut app, &row).await;
    app.click_text(".context-item", "View reactions")
        .await
        .expect("view");
    app.wait_for_text(".reactions-dialog .reaction-senders", "alice", DEFAULT_WAIT)
        .await
        .expect("alice listed under 🎉");
    app.press(".reactions-dialog", "Escape")
        .await
        .expect("escape");
    app.wait_for_js(
        "return document.querySelector('.reactions-dialog') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("dialog closed");

    // Remove all my reactions.
    open_menu_on(&mut app, &row).await;
    app.click_text(".context-item", "Remove all my reactions")
        .await
        .expect("remove");
    menu_closed(&mut app).await;
    app.wait_for_js(
        &format!("return {row}.querySelector('.reaction.mine') === null;"),
        DEFAULT_WAIT,
    )
    .await
    .expect("reaction removed");

    // Copy message ID is offered; so is Report on someone else's message.
    open_menu_on(&mut app, &row).await;
    app.wait_for_text(".context-item", "Copy message ID", Duration::from_secs(2))
        .await
        .expect("copy id");
    app.press(".context-menu", "Escape").await.expect("escape");
    // Recent rows may all be ours; scroll up to someone else's message.
    app.reveal_element(".message:not(.own):not(.system)", DEFAULT_WAIT)
        .await
        .expect("another sender's message");
    app.right_click(".message:not(.own):not(.system)")
        .await
        .expect("other");
    app.wait_for_text(".context-item", "Report message", DEFAULT_WAIT)
        .await
        .expect("report offered on others' messages");
    app.click_text(".context-item", "Report message")
        .await
        .expect("report");
    app.wait_for(".report-dialog input[name=reason]", DEFAULT_WAIT)
        .await
        .expect("report dialog");
    app.type_into(".report-dialog input[name=reason]", "e2e test report")
        .await
        .expect("reason");
    app.submit(".report-dialog").await.expect("submit");
    app.wait_for_text(".report-dialog .ok", "Reported", DEFAULT_WAIT)
        .await
        .expect("report accepted by the server");
    app.press(".report-dialog", "Escape").await.expect("escape");
}

#[tokio::test]
async fn pin_unpin_and_the_pinned_panel() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "alice", "general").await;

    // Earlier failed runs may have left "pin me" messages pinned: sweep them.
    app.wait_for(".pin-toggle", DEFAULT_WAIT)
        .await
        .expect("pin toggle");
    app.click(".pin-toggle").await.expect("open pinned panel");
    app.wait_for(".pinned-dialog", DEFAULT_WAIT)
        .await
        .expect("panel");
    app.wait_for_js("return document.querySelector('.pinned-dialog .muted') !== null || document.querySelector('.pinned-item') !== null;", DEFAULT_WAIT)
        .await
        .expect("panel loaded");
    app.eval("[...document.querySelectorAll('.pinned-item')].filter(i => i.textContent.includes('pin me')).forEach(i => i.querySelector('button').click());")
        .await
        .expect("sweep");
    app.wait_for_js("return ![...document.querySelectorAll('.pinned-item')].some(i => i.textContent.includes('pin me'));", DEFAULT_WAIT)
        .await
        .expect("leftovers unpinned");
    app.press(".pinned-dialog", "Escape").await.expect("escape");
    app.wait_for_js(
        "return document.querySelector('.pinned-dialog') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("panel closed");

    let row = send_fresh(&mut app, "pin me").await;
    let text = app
        .eval(&format!("return {row}.querySelector('.body').textContent;"))
        .await
        .expect("text");
    let text = text.as_str().unwrap_or_default().to_owned();
    let before: usize = app
        .text(".pin-toggle")
        .await
        .expect("pin count")
        .unwrap_or_default()
        .trim_start_matches(|c: char| !c.is_ascii_digit())
        .trim()
        .parse()
        .unwrap_or(0);

    open_menu_on(&mut app, &row).await;
    app.click_text(".context-item", "Pin message")
        .await
        .expect("pin");
    menu_closed(&mut app).await;
    app.wait_for_text(".pin-toggle", &(before + 1).to_string(), DEFAULT_WAIT)
        .await
        .expect("header count went up");

    app.click(".pin-toggle").await.expect("open pinned panel");
    app.wait_for_text(".pinned-dialog .pinned-item .body", &text, DEFAULT_WAIT)
        .await
        .expect("pinned message listed");

    // Unpin from the panel.
    let needle = serde_json::to_string(&text).expect("json");
    app.eval(&format!(
        "[...document.querySelectorAll('.pinned-item')].find(i => i.textContent.includes({needle})).querySelector('button').click();"
    ))
    .await
    .expect("unpin");
    app.wait_for_js(
        &format!("return ![...document.querySelectorAll('.pinned-item')].some(i => i.textContent.includes({needle}));"),
        DEFAULT_WAIT,
    )
    .await
    .expect("gone from the panel");
    app.press(".pinned-dialog", "Escape").await.expect("escape");
    app.wait_for_text(".pin-toggle", &before.to_string(), DEFAULT_WAIT)
        .await
        .expect("header count back");
    open_menu_on(&mut app, &row).await;
    app.wait_for_text(".context-item", "Pin message", Duration::from_secs(2))
        .await
        .expect("menu offers pin again");
    app.press(".context-menu", "Escape").await.expect("escape");
}

#[tokio::test]
async fn mark_unread_and_forward() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "alice", "general").await;
    let row = send_fresh(&mut app, "forward me").await;

    // Mark unread: the row stays bold after leaving, and clears on return.
    open_menu_on(&mut app, &row).await;
    app.click_text(".context-item", "Mark unread")
        .await
        .expect("mark unread");
    menu_closed(&mut app).await;
    app.wait_for(".room.selected.unread", DEFAULT_WAIT)
        .await
        .expect("general flagged unread");
    // Something new arrives while still in the room: it stays marked unread
    // (a read receipt for it would clear the flag).
    let news = format!("news {}", std::process::id());
    post_as_bob(&homeserver().expect("homeserver"), "general", &news);
    app.wait_for_text(".message .body", &news, DEFAULT_WAIT)
        .await
        .expect("bob's message arrives");
    app.wait_for(".room.selected.unread", DEFAULT_WAIT)
        .await
        .expect("still flagged");
    app.open_room("random").await.expect("leave");
    app.wait_for_js(
        "return [...document.querySelectorAll('.room.unread')].some(r => r.textContent.includes('general'));",
        DEFAULT_WAIT,
    )
    .await
    .expect("still unread from elsewhere");
    app.open_room("general").await.expect("return");
    app.wait_for_js(
        "return ![...document.querySelectorAll('.room.unread')].some(r => r.textContent.includes('general'));",
        DEFAULT_WAIT,
    )
    .await
    .expect("opening clears the flag");

    // Forward to #random: the same text shows up there.
    open_menu_on(&mut app, &row).await;
    click_submenu(&mut app, "Forward to", "#random").await;
    menu_closed(&mut app).await;
    app.open_room("random").await.expect("open random");
    app.wait_for_text(".message.own .body", "forward me", DEFAULT_WAIT)
        .await
        .expect("forwarded copy in random");
}

/// Post `body` to `room` as bob, through the API (`curl`).
fn post_as_bob(hs: &str, room: &str, body: &str) {
    let curl = |args: &[&str]| -> serde_json::Value {
        let out = std::process::Command::new("curl")
            .arg("-sf")
            .args(args)
            .output()
            .expect("curl");
        assert!(out.status.success(), "curl {args:?}");
        serde_json::from_slice(&out.stdout).unwrap_or_default()
    };
    let login = curl(&[
        "-X",
        "POST",
        &format!("{hs}/_matrix/client/v3/login"),
        "-d",
        r#"{"type":"m.login.password","identifier":{"type":"m.id.user","user":"bob"},"password":"bob-password"}"#,
    ]);
    let auth = format!(
        "Authorization: Bearer {}",
        login["access_token"].as_str().expect("token")
    );
    let room_id = curl(&[&format!(
        "{hs}/_matrix/client/v3/directory/room/%23{room}:localhost"
    )])["room_id"]
        .as_str()
        .expect("room id")
        .to_owned();
    curl(&[
        "-X",
        "PUT",
        "-H",
        &auth,
        &format!(
            "{hs}/_matrix/client/v3/rooms/{room_id}/send/m.room.message/n{}",
            std::process::id()
        ),
        "-d",
        &serde_json::json!({ "msgtype": "m.text", "body": body }).to_string(),
    ]);
    curl(&[
        "-X",
        "POST",
        "-H",
        &auth,
        &format!("{hs}/_matrix/client/v3/logout"),
    ]);
}

/// The message right-clicked stays highlighted while its menu is open (the
/// pointer is off it, on the menu), and not after.
#[tokio::test]
async fn the_right_clicked_message_stays_highlighted() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch");
    login_and_open(&mut app, "alice", "random").await;
    let row = send_fresh(&mut app, "highlight me").await;
    let background =
        format!("const row = {row}; return row ? getComputedStyle(row).backgroundColor : null;");
    let before = app.eval(&background).await.expect("before");
    open_menu_on(&mut app, &row).await;
    let during = app.eval(&background).await.expect("during");
    assert_ne!(during, before, "not highlighted while its menu is open");
    app.press("body", "Escape").await.expect("close");
    menu_closed(&mut app).await;
    app.wait_for_js(
        &format!(
            "return JSON.stringify((() => {{ {background} }})()) === {};",
            serde_json::to_string(&before.to_string()).expect("json")
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("the highlight goes with the menu");
}
