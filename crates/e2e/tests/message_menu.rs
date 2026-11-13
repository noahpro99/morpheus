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

/// Send a message and return its text (rows are found by it).
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
    text
}

async fn open_menu_on(app: &mut App, text: &str) {
    // Retried: the row can be re-rendered (a timeline reset) right after sending.
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        if app.right_click_text(".message", text).await.is_ok()
            && app
                .wait_for(".context-menu", Duration::from_secs(2))
                .await
                .is_ok()
        {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "no menu on {text:?}");
    }
}

async fn menu_closed(app: &mut App) {
    app.wait_for_count(".context-menu", 0, DEFAULT_WAIT)
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
    app.wait_for_within(".message", &row, ".reaction.mine", Some("🎉"), DEFAULT_WAIT)
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
    app.wait_for_count(".reactions-dialog", 0, DEFAULT_WAIT)
        .await
        .expect("dialog closed");

    // Remove all my reactions.
    open_menu_on(&mut app, &row).await;
    app.click_text(".context-item", "Remove all my reactions")
        .await
        .expect("remove");
    menu_closed(&mut app).await;
    app.wait_for_within(".message", &row, ".reaction.mine", None, DEFAULT_WAIT)
        .await
        .expect("reaction removed");

    // Copy message ID is offered; so is Report on someone else's message.
    open_menu_on(&mut app, &row).await;
    app.wait_for_text(".context-item", "Copy message ID", Duration::from_secs(2))
        .await
        .expect("copy id");
    app.press(".context-menu", "Escape").await.expect("escape");
    // Recent rows may all be ours; scroll up to someone else's message.
    app.reveal_element(".message:not(.own):not(.system):not(.skeleton-row)", DEFAULT_WAIT)
        .await
        .expect("another sender's message");
    app.right_click(".message:not(.own):not(.system):not(.skeleton-row)")
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
    app.wait_for(".pinned-dialog .muted, .pinned-item", DEFAULT_WAIT)
        .await
        .expect("panel loaded");
    for _ in 0..30 {
        let left = app
            .texts_within(".pinned-item", "pin me", "button")
            .await
            .expect("pinned items");
        if left.is_empty() {
            break;
        }
        app.click_within(".pinned-item", "pin me", "button")
            .await
            .expect("sweep");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    app.wait_for_within(".pinned-item", "pin me", "button", None, DEFAULT_WAIT)
        .await
        .expect("leftovers unpinned");
    app.press(".pinned-dialog", "Escape").await.expect("escape");
    app.wait_for_count(".pinned-dialog", 0, DEFAULT_WAIT)
        .await
        .expect("panel closed");

    let text = send_fresh(&mut app, "pin me").await;
    let before: usize = app
        .text(".pin-toggle")
        .await
        .expect("pin count")
        .unwrap_or_default()
        .trim_start_matches(|c: char| !c.is_ascii_digit())
        .trim()
        .parse()
        .unwrap_or(0);

    open_menu_on(&mut app, &text).await;
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
    app.click_within(".pinned-item", &text, "button")
        .await
        .expect("unpin");
    app.wait_for_within(".pinned-item", &text, "button", None, DEFAULT_WAIT)
        .await
        .expect("gone from the panel");
    app.press(".pinned-dialog", "Escape").await.expect("escape");
    app.wait_for_text(".pin-toggle", &before.to_string(), DEFAULT_WAIT)
        .await
        .expect("header count back");
    open_menu_on(&mut app, &text).await;
    app.wait_for_text(".context-item", "Pin message", Duration::from_secs(2))
        .await
        .expect("menu offers pin again");
    app.press(".context-menu", "Escape").await.expect("escape");
}

/// Wait until the room list shows `room` as unread (or not).
async fn wait_unread(app: &mut App, room: &str, unread: bool) {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let rows = app.texts(".room.unread").await.expect("rows");
        if rows.iter().any(|r| r.contains(room)) == unread {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{room} unread should be {unread}: {rows:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
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
    wait_unread(&mut app, "general", true).await;
    app.open_room("general").await.expect("return");
    wait_unread(&mut app, "general", false).await;

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
    // The row carries `data-menu-open` while its menu is open (the style
    // that highlights it hangs on it).
    app.wait_for_count(".message[data-menu-open]", 0, DEFAULT_WAIT)
        .await
        .expect("not highlighted before");
    open_menu_on(&mut app, &row).await;
    app.wait_for_text(".message[data-menu-open] .body", &row, DEFAULT_WAIT)
        .await
        .expect("highlighted while its menu is open");
    assert_eq!(
        app.count(".message[data-menu-open]").await.expect("count"),
        1,
        "only the right-clicked message"
    );
    app.press("body", "Escape").await.expect("close");
    menu_closed(&mut app).await;
    app.wait_for_count(".message[data-menu-open]", 0, DEFAULT_WAIT)
        .await
        .expect("the highlight goes with the menu");
}

/// The hover bar's "more" button opens the full emoji picker: every emoji in
/// categories with a tab for each, a search, skin tones, and recently used
/// first. Picking one reacts with it and closes the picker.
#[tokio::test]
async fn the_hover_bar_opens_a_full_emoji_picker_to_react() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "alice", "random").await;
    let row = send_fresh(&mut app, "pick an emoji").await;

    // Three quick reactions, then "more".
    let quick = app
        .texts_within(".message", &row, ".actions .action.quick-reaction")
        .await
        .expect("quick reactions");
    let more = app
        .texts_within(".message", &row, ".actions .action.more-reactions")
        .await
        .expect("more");
    assert_eq!((quick.len(), more.len()), (3, 1), "three quick reactions and more");

    app.click_in_message(&row, ".actions .more-reactions")
        .await
        .expect("more");
    app.wait_for(".emoji-picker", DEFAULT_WAIT)
        .await
        .expect("the picker");
    assert_eq!(
        app.count(".emoji-picker input.emoji-search")
            .await
            .expect("count"),
        1,
        "a search"
    );
    assert!(
        app.count(".emoji-picker .emoji-tab").await.expect("count") >= 8,
        "a tab per category"
    );
    assert!(
        app.count(".emoji-picker .emoji-cell").await.expect("count") > 1000,
        "every emoji"
    );

    // A tab brings its category into view.
    app.click(".emoji-tab[title=\"Travel & Places\"]")
        .await
        .expect("tab");
    let section = ".emoji-section[data-group=\"Travel & Places\"]";
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let top = app.bounds(section).await.expect("bounds");
        let grid = app.bounds(".emoji-grid-scroll").await.expect("bounds");
        if let (Some(top), Some(grid)) = (top, grid)
            && (top.y - grid.y).abs() < 8.0
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Travel & Places at the top"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Search, and pick: a reaction, and the picker closes.
    app.type_into(".emoji-picker input.emoji-search", "rocket")
        .await
        .expect("search");
    wait_cells(&mut app, |cells| {
        !cells.is_empty() && cells.len() < 10 && cells[0].trim() == "🚀"
    })
    .await;
    app.click(".emoji-picker .emoji-cell").await.expect("pick");
    app.wait_for_count(".emoji-picker", 0, DEFAULT_WAIT)
        .await
        .expect("picker closed");
    app.wait_for_within(".message", &row, ".reactions", Some("🚀"), DEFAULT_WAIT)
        .await
        .expect("reacted with 🚀");

    // Nothing found says so.
    app.click_in_message(&row, ".actions .more-reactions")
        .await
        .expect("more");
    app.wait_for(".emoji-picker", DEFAULT_WAIT)
        .await
        .expect("picker");
    app.type_into(".emoji-picker input.emoji-search", "zzqqxx")
        .await
        .expect("search");
    app.wait_for_text(".emoji-picker", "No emoji found", DEFAULT_WAIT)
        .await
        .expect("none found");

    // A skin tone: emoji that have tones show in it.
    app.type_into(".emoji-picker input.emoji-search", "")
        .await
        .expect("clear");
    app.click(".emoji-picker .skin-tone-button")
        .await
        .expect("tones");
    app.wait_for(".emoji-picker .skin-tone-option", DEFAULT_WAIT)
        .await
        .expect("tone options");
    app.click(".emoji-picker .skin-tone-option[data-tone=\"Medium\"]")
        .await
        .expect("medium");
    app.type_into(".emoji-picker input.emoji-search", "thumbsup")
        .await
        .expect("search");
    wait_cells(&mut app, |cells| cells.first().is_some_and(|c| c.trim() == "👍🏽")).await;
    app.click(".emoji-picker .emoji-cell").await.expect("pick");
    app.wait_for_within(".message", &row, ".reactions", Some("👍🏽"), DEFAULT_WAIT)
        .await
        .expect("reacted with 👍🏽");

    // Recently used come first, newest first; Escape closes.
    app.click_in_message(&row, ".actions .more-reactions")
        .await
        .expect("more");
    app.wait_for(".emoji-picker", DEFAULT_WAIT)
        .await
        .expect("picker");
    let recent = app
        .texts_within(".emoji-section[data-group=\"Recent\"]", "", ".emoji-cell")
        .await
        .expect("recent");
    let recent: Vec<_> = recent.iter().take(2).map(|c| c.trim()).collect();
    assert_eq!(recent, ["👍🏽", "🚀"], "recently used first");
    app.press(".emoji-picker input.emoji-search", "Escape")
        .await
        .expect("escape");
    app.wait_for_count(".emoji-picker", 0, DEFAULT_WAIT)
        .await
        .expect("closed");
}

/// Wait until the picker's cells (in order) satisfy `ok`.
async fn wait_cells(app: &mut App, ok: impl Fn(&[String]) -> bool) {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let cells = app
            .texts(".emoji-picker .emoji-cell")
            .await
            .expect("cells");
        if ok(&cells) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the cells never matched: {:?}",
            cells.iter().take(12).collect::<Vec<_>>()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Shift held while picking "Delete message" in the right-click menu skips
/// the question, as Shift on the hover bar's bin does (Discord).
#[tokio::test]
async fn shift_picking_delete_in_the_menu_deletes_without_asking() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "alice", "general").await;
    let text = send_fresh(&mut app, "menu shift delete").await;
    open_menu_on(&mut app, &text).await;
    app.shift_click_text(".context-menu .context-item", "Delete message")
        .await
        .expect("shift-pick delete");
    app.wait_for_text_gone(".message .body", &text, DEFAULT_WAIT)
        .await
        .expect("deleted at once");
    assert_eq!(app.count(".confirm-dialog").await.expect("count"), 0, "asked anyway");
}
