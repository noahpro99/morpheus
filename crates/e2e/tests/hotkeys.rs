//! Keyboard shortcuts (Discord's set): Ctrl+/ for the list, Alt+arrows for
//! rooms and scopes, Alt+Left/Right for history, Ctrl+I/U/P/, for panels,
//! Up in an empty composer to edit, Esc to jump to the newest message.

#![allow(clippy::expect_used)]

use std::time::Duration;

use morpheus_e2e::{App, DEFAULT_WAIT};

/// Holds once the timeline has more history than fits on screen.
const SCROLLABLE: &str = "const t = document.querySelector('.timeline'); return !!t && t.scrollHeight > t.clientHeight + 600;";

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login_and_open(app: &mut App, room: &str) {
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
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("room listed");
    app.open_room(room).await.expect("open room");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
}

async fn header(app: &mut App) -> String {
    app.text(".chat-header h2")
        .await
        .expect("header")
        .unwrap_or_default()
}

#[tokio::test]
async fn shortcuts_navigate_rooms_scopes_history_and_panels() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    // The rail's servers in a known order (Ctrl+Alt+Down follows it).
    set_rail(&hs, &["Engineering", "Projects"]);
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;

    // Ctrl+/ lists the shortcuts; Esc closes it.
    app.key_combo("body", "/", true, false, false)
        .await
        .expect("ctrl+/");
    app.wait_for_text(".help-dialog", "Keyboard shortcuts", DEFAULT_WAIT)
        .await
        .expect("shortcuts dialog");
    app.press("body", "Escape").await.expect("escape");
    app.wait_for_count(".help-dialog", 0, DEFAULT_WAIT)
        .await
        .expect("closed");

    // Alt+Down / Alt+Up step through the sidebar order.
    let before = header(&mut app).await;
    app.key_combo("body", "ArrowDown", false, true, false)
        .await
        .expect("alt+down");
    app.wait_for_text_gone(".chat-header h2", &before, DEFAULT_WAIT)
        .await
        .expect("moved to the next room");
    if app.is_gpui() {
        // The GPUI room opens its timeline a moment after its header; Alt+Up
        // before then would step from the room still opening. (On Dioxus the
        // next room can be one with no message box, so this is not asked there.)
        app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
            .await
            .expect("the next room is open");
    }
    app.key_combo("body", "ArrowUp", false, true, false)
        .await
        .expect("alt+up");
    app.wait_for_text(".chat-header h2", &before, DEFAULT_WAIT)
        .await
        .expect("back to the first room");

    // Ctrl+Alt+Down moves the rail scope; Alt+Left goes back in history.
    app.key_combo("body", "ArrowDown", true, true, false)
        .await
        .expect("ctrl+alt+down");
    app.wait_for(".rail-item.selected[title=\"Engineering\"]", DEFAULT_WAIT)
        .await
        .expect("next scope");
    // A room of the space opens. Which one is the app's choice (GPUI: the
    // first its sidebar lists; Dioxus: the most recent), and rooms other
    // tests made there come and go, so the header is checked against the list.
    let listed: Vec<String> = app
        .texts(".room-list .room .room-name")
        .await
        .expect("sidebar")
        .into_iter()
        .map(|n| n.trim().to_owned())
        .collect();
    assert!(!listed.is_empty(), "a room in the space");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let shown = header(&mut app).await;
        if listed.contains(&shown) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "a room of the space opens: the header says {shown:?}, the space lists {listed:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    app.key_combo("body", "ArrowLeft", false, true, false)
        .await
        .expect("alt+left");
    app.wait_for_text(".chat-header h2", &before, DEFAULT_WAIT)
        .await
        .expect("history back");
    app.wait_for(".rail-item.selected[title=\"Rooms\"]", DEFAULT_WAIT)
        .await
        .expect("scope followed");

    // Panels: Ctrl+I inbox, Ctrl+U members, Ctrl+P pins, Ctrl+, settings.
    app.key_combo("body", "i", true, false, false)
        .await
        .expect("ctrl+i");
    app.wait_for(".inbox-panel", DEFAULT_WAIT)
        .await
        .expect("inbox");
    app.press("body", "Escape").await.expect("escape");
    app.key_combo("body", "u", true, false, false)
        .await
        .expect("ctrl+u");
    app.wait_for_count(".members", 0, DEFAULT_WAIT)
        .await
        .expect("members hidden");
    app.key_combo("body", "u", true, false, false)
        .await
        .expect("ctrl+u again");
    app.wait_for(".members", DEFAULT_WAIT)
        .await
        .expect("members back");
    app.key_combo("body", "p", true, false, false)
        .await
        .expect("ctrl+p");
    app.wait_for(".pinned-dialog", DEFAULT_WAIT)
        .await
        .expect("pins");
    app.press("body", "Escape").await.expect("escape");
    app.key_combo("body", ",", true, false, false)
        .await
        .expect("ctrl+,");
    app.wait_for(".settings", DEFAULT_WAIT)
        .await
        .expect("settings");
    app.press("body", "Escape").await.expect("escape");
    app.wait_for_count(".settings", 0, DEFAULT_WAIT)
        .await
        .expect("settings closed");
}

#[tokio::test]
async fn up_edits_last_message_and_escape_jumps_to_bottom() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;

    let text = format!("edit me {}", std::process::id());
    app.type_into(".composer .composer-input", &text)
        .await
        .expect("type");
    app.press(".composer .composer-input", "Enter")
        .await
        .expect("send");
    app.wait_for_exact_text(".message.own:not(.pending) .body", &text, DEFAULT_WAIT)
        .await
        .expect("sent");

    // Up in the empty box edits it: the banner shows and the text is back in the box.
    app.key_combo(".composer .composer-input", "ArrowUp", false, false, false)
        .await
        .expect("up");
    app.wait_for_text(".composer-banner", "Editing message", DEFAULT_WAIT)
        .await
        .expect("edit mode");
    app.wait_for_value(".composer .composer-input", &text, Duration::from_secs(5))
        .await
        .expect("draft holds the message");
    app.press(".composer .composer-input", "Escape")
        .await
        .expect("cancel edit");

    // Scroll up, then Esc with nothing open jumps back to the newest message.
    // A tall window can show the whole first page; load enough history to scroll.
    if app.is_gpui() {
        // Page up to the start until the chat is longer than the window.
        let deadline = std::time::Instant::now() + DEFAULT_WAIT;
        loop {
            app.scroll_to(".timeline", morpheus_e2e::ScrollTo::Top)
                .await
                .expect("to the top");
            let long = app
                .scroll_state(".timeline")
                .await
                .expect("state")
                .is_some_and(|s| s.range > 2600.0);
            if long {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "history");
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    } else {
        app.reveal("a scrollable timeline", SCROLLABLE, DEFAULT_WAIT)
            .await
            .expect("history");
    }
    app.scroll_to(".timeline", morpheus_e2e::ScrollTo::FromBottom(2000.0))
        .await
        .expect("scroll up");
    app.wait_for(".jump-to-bottom", DEFAULT_WAIT)
        .await
        .expect("scrolled away");
    app.press("body", "Escape").await.expect("escape");
    // On a phone Escape is the back key: it returns to the room list instead.
    if app.is_gpui() || app.count(".shell.narrow").await.expect("layout") == 0 {
        app.wait_for_count(".jump-to-bottom", 0, DEFAULT_WAIT)
            .await
            .expect("back at the bottom");
    } else {
        app.wait_for_js(
            "return document.querySelector('.shell').dataset.pane === 'rooms';",
            DEFAULT_WAIT,
        )
        .await
        .expect("back at the room list (phone)");
    }
}

/// Ctrl+R reloads the app, as in Discord: the interface is built afresh
/// (same account, no sign-in) and works.
#[tokio::test]
async fn ctrl_r_reloads_the_app() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "random").await;
    // A mark on the current interface, gone once it is rebuilt: a page flag
    // on Dioxus, the DMs being the open scope on GPUI (a new shell starts
    // in Rooms).
    if app.is_gpui() {
        app.click(".rail-dms").await.expect("the DMs");
        app.wait_for(".rail-item.selected[title=\"Direct messages\"]", DEFAULT_WAIT)
            .await
            .expect("in the DMs");
    } else {
        app.eval("document.querySelector('.shell').dataset.beforeReload = 'yes'; return true;")
            .await
            .expect("mark");
    }
    app.key_combo("body", "r", true, false, false)
        .await
        .expect("ctrl+r");
    if app.is_gpui() {
        app.wait_for(".rail-item.selected[title=\"Rooms\"]", DEFAULT_WAIT)
            .await
            .expect("rebuilt");
    } else {
        app.wait_for(".shell:not([data-before-reload])", DEFAULT_WAIT)
            .await
            .expect("rebuilt");
    }
    assert_eq!(
        app.count("form.login").await.expect("count"),
        0,
        "still signed in"
    );
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("rooms again");
    app.open_room("general")
        .await
        .expect("works after reloading");
}

/// Ctrl+Shift+M mutes and unmutes; Ctrl+Shift+D deafens (which mutes too)
/// and undeafens, leaving you muted if you were before.
#[tokio::test]
async fn ctrl_shift_m_mutes_and_ctrl_shift_d_deafens() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch");
    login_and_open(&mut app, "general").await;
    // [muted, deafened], as the buttons in the footer show it.
    async fn state(app: &mut App) -> serde_json::Value {
        if app.is_gpui() {
            let on = |n: usize| n > 0;
            let muted = on(app.count(".user-bar .mute-button.active").await.expect("mute"));
            let deafened = on(app.count(".user-bar .deafen-button.active").await.expect("deafen"));
            return serde_json::json!([muted, deafened]);
        }
        app.eval("return [document.querySelector('.user-bar .mute-button').classList.contains('active'), \
                         document.querySelector('.user-bar .deafen-button').classList.contains('active')];")
            .await
            .expect("state")
    }
    async fn press(app: &mut App, key: &str) {
        // Shift makes the letter a capital, as a real keyboard does.
        app.key_combo("body", key, true, false, true)
            .await
            .expect("shortcut");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let expect = async |app: &mut App, muted: bool, deafened: bool, why: &str| {
        assert_eq!(
            state(app).await,
            serde_json::json!([muted, deafened]),
            "{why}"
        );
    };
    expect(&mut app, false, false, "starts heard and hearing").await;
    press(&mut app, "M").await;
    expect(&mut app, true, false, "Ctrl+Shift+M mutes").await;
    press(&mut app, "M").await;
    expect(&mut app, false, false, "and unmutes").await;

    press(&mut app, "D").await;
    expect(&mut app, true, true, "Ctrl+Shift+D deafens, muted too").await;
    press(&mut app, "D").await;
    expect(&mut app, false, false, "undeafened: unmuted again").await;

    press(&mut app, "M").await;
    press(&mut app, "D").await;
    expect(&mut app, true, true, "muted, then deafened").await;
    press(&mut app, "D").await;
    expect(&mut app, true, false, "undeafened: still muted, as before").await;
    press(&mut app, "M").await;
    expect(&mut app, false, false, "back as it was").await;
}

/// Put Alice's rail in this order of servers (by name), as dragging would.
fn set_rail(hs: &str, names: &[&str]) {
    let rail: Vec<serde_json::Value> = names
        .iter()
        .map(|name| {
            let alias = name.to_lowercase();
            let id = morpheus_e2e::api_as(
                hs,
                "alice",
                "GET",
                &format!("directory/room/%23{alias}%3Alocalhost"),
                None,
            )
            .expect("alias")["room_id"]
                .clone();
            serde_json::json!({ "space": id })
        })
        .collect();
    morpheus_e2e::api_as(
        hs,
        "alice",
        "PUT",
        "user/@alice:localhost/account_data/dev.morpheus.ui",
        Some(serde_json::json!({ "rail": rail })),
    )
    .expect("rail prefs");
}

/// Ctrl+Alt+Down goes server by server in the order the rail shows them
/// (the order you dragged them into), not the order rooms were active.
#[tokio::test]
async fn ctrl_alt_down_goes_server_by_server_in_rail_order() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    for order in [["Projects", "Engineering"], ["Engineering", "Projects"]] {
        set_rail(&hs, &order);
        let mut app = App::launch_desktop(&hs).await.expect("launch app");
        login_and_open(&mut app, "general").await;
        // Servers arrive with the first sync: stepping needs the whole rail.
        for name in order {
            app.wait_for(&format!(".rail-item[title=\"{name}\"]"), DEFAULT_WAIT)
                .await
                .unwrap_or_else(|e| panic!("{name} in the rail: {e}"));
        }
        app.click(".rail-rooms").await.expect("Rooms");
        for name in order {
            app.key_combo("body", "ArrowDown", true, true, false)
                .await
                .expect("ctrl+alt+down");
            app.wait_for(
                &format!(".rail-item.selected[title=\"{name}\"]"),
                DEFAULT_WAIT,
            )
            .await
            .unwrap_or_else(|e| panic!("{name} next, as the rail has {order:?}: {e}"));
        }
        app.key_combo("body", "ArrowUp", true, true, false)
            .await
            .expect("ctrl+alt+up");
        app.wait_for(
            &format!(".rail-item.selected[title=\"{}\"]", order[0]),
            DEFAULT_WAIT,
        )
        .await
        .expect("and back up");
    }
    set_rail(&hs, &[]);
}

/// Ctrl+Alt+Right (as in Discord) toggles between the DM you were last in and
/// the server channel you were last in: the very channels, not just the scope.
#[tokio::test]
async fn ctrl_alt_right_toggles_the_last_dm_and_server_channel() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login_and_open(&mut app, "random").await;
    app.wait_for_text(".chat-header h2", "random", DEFAULT_WAIT)
        .await
        .expect("in #random");
    // A DM that is not the first in the list.
    app.click(".rail-item[title=\"Direct messages\"]")
        .await
        .expect("dms");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    let dm = loop {
        let names = app.texts(".room.dm .room-name").await.expect("DMs");
        if names.len() >= 2 {
            break names[1].trim().to_owned();
        }
        assert!(std::time::Instant::now() < deadline, "two DMs: {names:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    // The second row itself (two DMs can carry the same name; the GPUI
    // selectors have no `+`, and there the first row of the name is taken).
    if app.is_gpui() {
        app.click_text(".room.dm", &dm).await
    } else {
        app.click(".room:not(.friends-row) + .room").await
    }
    .expect("open the second DM");
    app.wait_for_text(".chat-header h2", &dm, DEFAULT_WAIT)
        .await
        .expect("in that DM");
    for (scope, header) in [
        ("Rooms", "random"),
        ("Direct messages", dm.as_str()),
        ("Rooms", "random"),
    ] {
        app.key_combo("body", "ArrowRight", true, true, false)
            .await
            .expect("ctrl+alt+right");
        app.wait_for(
            &format!(".rail-item.selected[title=\"{scope}\"]"),
            DEFAULT_WAIT,
        )
        .await
        .unwrap_or_else(|e| panic!("to {scope}: {e}"));
        app.wait_for_text(".chat-header h2", header, DEFAULT_WAIT)
            .await
            .unwrap_or_else(|e| panic!("back in {header}: {e}"));
    }
}

/// Ctrl+Shift+P opens the command palette too (as in VS Code), and the
/// palette finds single settings: picking one opens Settings on its page,
/// scrolled to it and highlighted.
#[tokio::test]
async fn the_palette_opens_on_ctrl_shift_p_and_finds_settings() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;
    app.key_combo("body", "P", true, false, true)
        .await
        .expect("ctrl+shift+p");
    app.wait_for(".switcher input", DEFAULT_WAIT)
        .await
        .expect("the palette opens");
    assert_eq!(
        app.count(".pinned-dialog, .message-dialog")
            .await
            .expect("count"),
        0,
        "Ctrl+Shift+P is not Ctrl+P (pinned messages)"
    );
    app.type_into(".switcher input", "notifications show")
        .await
        .expect("filter");
    app.wait_for_text(
        ".switcher-item.active",
        "What notifications show",
        DEFAULT_WAIT,
    )
    .await
    .expect("the setting is offered");
    app.press(".switcher input", "Enter").await.expect("pick");
    app.wait_for_text(".settings-page h2", "Notifications", DEFAULT_WAIT)
        .await
        .expect("its page opens");
    app.wait_for(
        ".settings-page [data-setting=notification-content].setting-highlight",
        DEFAULT_WAIT,
    )
    .await
    .expect("the setting is highlighted");
    app.press("body", "Escape").await.expect("close");

    // Another page's setting, by a word in it.
    app.key_combo("body", "k", true, false, false)
        .await
        .expect("ctrl+k");
    app.wait_for(".switcher input", DEFAULT_WAIT)
        .await
        .expect("palette");
    app.type_into(".switcher input", "theme")
        .await
        .expect("filter");
    app.wait_for_text(".switcher-item.active", "Theme", DEFAULT_WAIT)
        .await
        .expect("offered");
    app.press(".switcher input", "Enter").await.expect("pick");
    app.wait_for_text(".settings-page h2", "Appearance", DEFAULT_WAIT)
        .await
        .expect("appearance");
    app.wait_for(
        ".settings-page [data-setting=theme].setting-highlight",
        DEFAULT_WAIT,
    )
    .await
    .expect("theme highlighted");
}

/// In the DMs, Friends is part of the list Alt+Up and Alt+Down walk: up from
/// the top DM lands on it, and down from it goes back to that DM.
#[tokio::test]
async fn alt_up_from_the_top_dm_reaches_friends() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;
    app.click(".rail-item[title=\"Direct messages\"]")
        .await
        .expect("dms");
    app.wait_for(".room:not(.friends-row)", DEFAULT_WAIT)
        .await
        .expect("dms listed");
    let top = app
        .texts(".room:not(.friends-row) .room-name")
        .await
        .expect("top dm")
        .first()
        .map(|n| n.trim().to_owned())
        .expect("a DM");
    app.click_text(".room:not(.friends-row)", &top)
        .await
        .expect("open the top dm");
    app.wait_for_text(".chat-header h2", &top, DEFAULT_WAIT)
        .await
        .expect("in the top dm");
    app.key_combo("body", "ArrowUp", false, true, false)
        .await
        .expect("alt+up");
    app.wait_for(".friends-page", DEFAULT_WAIT)
        .await
        .expect("on Friends");
    app.wait_for(".friends-row.selected", DEFAULT_WAIT)
        .await
        .expect("Friends selected");
    app.key_combo("body", "ArrowDown", false, true, false)
        .await
        .expect("alt+down");
    app.wait_for_text(".chat-header h2", &top, DEFAULT_WAIT)
        .await
        .expect("back in the top dm");
}

/// The shortcuts dialog (Ctrl+/) is wide enough for its rows: each shortcut
/// sits on one line, its keys drawn as key caps, and the dialog fits the
/// window (scrolling inside when it must).
#[tokio::test]
async fn the_shortcuts_dialog_is_wide_with_one_line_rows() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;
    app.key_combo("body", "/", true, false, false)
        .await
        .expect("ctrl+/");
    app.wait_for(".help-dialog", DEFAULT_WAIT)
        .await
        .expect("shortcuts dialog");
    if app.is_gpui() {
        // Measured through the tree: row count, width, fit, one-line rows and
        // key caps (the page-only checks of text overflow and rounded clips
        // have no counterpart here).
        let window = app.bounds("body").await.expect("body").expect("a window");
        let dialog = app
            .bounds(".help-dialog")
            .await
            .expect("dialog")
            .expect("a dialog");
        let rows = app.all_bounds(".help-dialog .help-row").await.expect("rows");
        let caps = app.count(".help-dialog .help-row kbd").await.expect("caps");
        assert!(rows.len() >= 10, "shortcut rows: {}", rows.len());
        assert!(dialog.w >= 640.0, "the dialog is too narrow: {dialog:?}");
        assert!(
            dialog.y >= 0.0 && dialog.bottom() <= window.h,
            "the dialog does not fit the window: {dialog:?} in {window:?}"
        );
        // A row of one line is about 20 px; a wrapped one is twice that.
        let tall: Vec<_> = rows.iter().filter(|r| r.h > 38.0).collect();
        assert!(tall.is_empty(), "rows that wrap: {tall:?}");
        assert!(caps >= rows.len(), "rows without key caps");
        return;
    }
    let report = app
        .eval(
            "const d = document.querySelector('.help-dialog'); const r = d.getBoundingClientRect(); \
             const rows = [...d.querySelectorAll('.help-row')]; \
             const tall = rows.filter(row => { const lh = parseFloat(getComputedStyle(row).lineHeight) || 20; \
               return [...row.children].some(c => c.getBoundingClientRect().height > lh * 1.9); }) \
               .map(row => row.textContent.trim()); \
             const bare = rows.filter(row => !row.querySelector('kbd')).map(row => row.textContent.trim()); \
             const cut = [...d.querySelectorAll('.help-what')].filter(w => w.scrollWidth > w.clientWidth + 1).map(w => w.textContent); \
             return { width: r.width, page: innerWidth, top: r.top, bottom: r.bottom, height: innerHeight, rows: rows.length, tall, bare, cut };",
        )
        .await
        .expect("report");
    assert!(
        report["rows"].as_u64().unwrap_or(0) >= 10,
        "shortcut rows: {report}"
    );
    assert!(
        report["width"].as_f64().unwrap_or(0.0) >= 640.0,
        "the dialog is too narrow: {report}"
    );
    assert!(
        report["top"].as_f64().unwrap_or(-1.0) >= 0.0
            && report["bottom"].as_f64().unwrap_or(f64::MAX)
                <= report["height"].as_f64().unwrap_or(0.0),
        "the dialog does not fit the window: {report}"
    );
    assert_eq!(
        report["tall"],
        serde_json::json!([]),
        "rows that wrap: {report}"
    );
    assert_eq!(
        report["bare"],
        serde_json::json!([]),
        "rows without key caps: {report}"
    );
    assert_eq!(
        report["cut"],
        serde_json::json!([]),
        "descriptions cut off: {report}"
    );
    // Its list scrolls without a rounded clip around it (which can keep
    // WebKit from scrolling it smoothly with a wheel).
    let clipped = app
        .eval(
            "const b = document.querySelector('.help-body'); const out = []; \
             for (let e = b.parentElement; e && e !== document.body; e = e.parentElement) { \
               const s = getComputedStyle(e); \
               if (s.overflow !== 'visible' && parseFloat(s.borderTopLeftRadius) > 0) out.push(e.className); } \
             return out;",
        )
        .await
        .expect("ancestors");
    assert_eq!(
        clipped,
        serde_json::json!([]),
        "a rounded clip around the scrolling list"
    );
}

/// Nothing animates forever once a room has loaded: placeholder rows that
/// wait above the oldest message stay still (a shimmer under the dimmed
/// shortcuts dialog redrew the whole window every frame, and its scrolling
/// stuttered).
#[tokio::test]
async fn nothing_animates_forever_once_loaded() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    if std::env::var("MORPHEUS_E2E_UI").as_deref() == Ok("gpui") {
        eprintln!("skipped on GPUI: CSS animations (a page-only concept; the GPUI app draws only when something changes)");
        return;
    }
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    app.key_combo("body", "/", true, false, false)
        .await
        .expect("ctrl+/");
    app.wait_for(".help-dialog", DEFAULT_WAIT)
        .await
        .expect("shortcuts");
    let endless = app
        .eval(
            "return document.getAnimations() \
               .filter(a => a.playState === 'running' && a.effect && a.effect.getComputedTiming().iterations === Infinity) \
               .map(a => (a.animationName || 'animation') + ' on ' + (a.effect.target && a.effect.target.className));",
        )
        .await
        .expect("animations");
    assert_eq!(endless, serde_json::json!([]), "endless animations");
}

/// The palette names each room's server on the right ("Engineering" for
/// #dev); your own picture shows in the profile card at the bottom left.
#[tokio::test]
async fn the_palette_names_servers_and_the_card_has_your_picture() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;
    if app.is_gpui() {
        app.wait_for(".user-bar .profile-chip img.avatar", DEFAULT_WAIT)
            .await
            .expect("alice's picture in the profile card");
    } else {
        app.wait_for_js(
            "const a = document.querySelector('.user-pill .profile-chip .avatar'); return !!a && a.tagName === 'IMG' && a.src.startsWith('data:');",
            DEFAULT_WAIT,
        )
        .await
        .expect("alice's picture in the profile card");
    }
    app.key_combo("body", "k", true, false, false)
        .await
        .expect("ctrl+k");
    app.wait_for(".switcher input", DEFAULT_WAIT)
        .await
        .expect("palette");
    app.type_into(".switcher input", "dev")
        .await
        .expect("filter");
    app.wait_for_within(
        ".switcher-item:not(.action)",
        "dev",
        ".kind",
        Some("Engineering"),
        DEFAULT_WAIT,
    )
    .await
    .expect("#dev named with its server");
}

/// In the palette a server's channel shows the server's picture on the left.
#[tokio::test]
async fn the_palette_shows_a_channels_server_picture() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;
    app.key_combo("body", "k", true, false, false)
        .await
        .expect("ctrl+k");
    app.wait_for(".switcher input", DEFAULT_WAIT)
        .await
        .expect("palette");
    app.type_into(".switcher input", "dev")
        .await
        .expect("filter");
    // Engineering's picture: its letter badge when it has none.
    // (`#dev` is the room; the picture is the server's: its letter badge when it has none.)
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let found = if app.is_gpui() {
            // The row has a picture or a letter, and the letter is the server's.
            let pics = app
                .texts_within(".switcher-item:not(.action)", "dev", ".avatar")
                .await
                .expect("avatars");
            let kinds = app
                .texts_within(".switcher-item:not(.action)", "dev", ".kind")
                .await
                .expect("kinds");
            kinds.iter().any(|k| k.trim() == "Engineering")
                && pics.iter().any(|p| p.trim() == "E" || p.trim().is_empty())
        } else {
            app.eval(
                "const row = [...document.querySelectorAll('.switcher-item:not(.action)')].find(r => r.querySelector('.name')?.textContent.trim() === 'dev'); \
                 const pic = row && row.querySelector('.avatar'); \
                 return !!pic && (pic.getAttribute('alt') === 'Engineering' || pic.textContent.trim() === 'E');",
            )
            .await
            .expect("row")
            .as_bool()
            == Some(true)
        };
        if found {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "#dev with Engineering's picture"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// What scrolls inside an overlay (the shortcuts, Settings) sits on a layer
/// of its own, so scrolling it does not repaint the whole app underneath.
#[tokio::test]
async fn overlays_scroll_on_their_own_layer() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    if std::env::var("MORPHEUS_E2E_UI").as_deref() == Ok("gpui") {
        eprintln!("skipped on GPUI: CSS will-change layers (a page-only concept)");
        return;
    }
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;
    let layered = "const out = []; \
        document.querySelectorAll('.overlay').forEach(o => { \
          if (getComputedStyle(o).willChange === 'auto') out.push('overlay ' + o.className); \
          o.querySelectorAll('*').forEach(e => { const s = getComputedStyle(e); \
            if ((s.overflowY === 'auto' || s.overflowY === 'scroll') && e.scrollHeight > e.clientHeight && s.willChange === 'auto') out.push(e.className); }); }); \
        return out;";
    app.key_combo("body", "/", true, false, false)
        .await
        .expect("ctrl+/");
    app.wait_for(".help-dialog", DEFAULT_WAIT)
        .await
        .expect("shortcuts");
    assert_eq!(
        app.eval(layered).await.expect("layers"),
        serde_json::json!([]),
        "shortcuts"
    );
    app.press("body", "Escape").await.expect("close");
    app.click(".settings-button").await.expect("settings");
    app.wait_for(".settings-page", DEFAULT_WAIT)
        .await
        .expect("settings");
    assert_eq!(
        app.eval(layered).await.expect("layers"),
        serde_json::json!([]),
        "settings"
    );
}

/// Discord's shortcuts for search (Ctrl+F), the emoji picker (Ctrl+E) and
/// creating or joining a server (Ctrl+Shift+N).
#[tokio::test]
async fn ctrl_f_ctrl_e_and_ctrl_shift_n() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;
    app.key_combo("body", "f", true, false, false)
        .await
        .expect("ctrl+f");
    app.wait_for(".search-bar input[name=search][data-focused]", DEFAULT_WAIT)
        .await
        .expect("the search box has the caret");
    app.press("body", "Escape").await.expect("close");
    app.key_combo("body", "e", true, false, false)
        .await
        .expect("ctrl+e");
    app.wait_for(".composer .emoji-panel", DEFAULT_WAIT)
        .await
        .expect("the emoji picker");
    app.press(".composer .composer-input", "Escape")
        .await
        .expect("close");
    app.key_combo("body", "N", true, false, true)
        .await
        .expect("ctrl+shift+n");
    app.wait_for(".create-space-dialog", DEFAULT_WAIT)
        .await
        .expect("create a server");
}

/// Page Up / Page Down scroll the chat; Ctrl+Shift+L copies the room's link.
#[tokio::test]
async fn page_keys_scroll_and_ctrl_shift_l_copies_the_link() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    // A long chat of its own.
    let user = morpheus_e2e::new_account(&hs, "pages").expect("account");
    let room = morpheus_e2e::api_as(
        &hs,
        &user,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": "pages" })),
    )
    .expect("a room");
    let room = room["room_id"].as_str().expect("room id").to_owned();
    let bodies: Vec<String> = (1..=80).map(|n| format!("line {n}")).collect();
    morpheus_e2e::send_many_as(&hs, &user, &room, &bodies).expect("history");
    let mut app = App::launch_sized(&hs, 1100, 700).await.expect("launch app");
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
    app.open_room("pages").await.expect("open the room");
    // Pixels up from the bottom of the chat (Dioxus: scrollTop is negative up there).
    async fn up_by(app: &mut App) -> f64 {
        app.scroll_state(".timeline")
            .await
            .expect("scroll")
            .expect("a timeline")
            .from_bottom
    }
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let long = app
            .scroll_state(".timeline")
            .await
            .expect("scroll")
            .is_some_and(|s| s.range > 400.0);
        if long {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "a long chat");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let before = up_by(&mut app).await;
    app.press(".composer .composer-input", "PageUp")
        .await
        .expect("page up");
    tokio::time::sleep(Duration::from_millis(600)).await;
    let up = up_by(&mut app).await;
    assert!(up > before + 100.0, "page up scrolls up: {before} → {up}");
    app.press(".composer .composer-input", "PageDown")
        .await
        .expect("page down");
    tokio::time::sleep(Duration::from_millis(600)).await;
    let down = up_by(&mut app).await;
    assert!(down < up - 100.0, "page down scrolls down: {up} → {down}");

    app.key_combo("body", "L", true, false, true)
        .await
        .expect("ctrl+shift+l");
    app.wait_for_text(".toast", "Link copied", DEFAULT_WAIT)
        .await
        .expect("copied");
}

/// Ctrl+Shift+Alt+Down goes to the next room that mentions you, past rooms
/// merely unread.
#[tokio::test]
async fn ctrl_shift_alt_down_goes_to_rooms_that_mention_you() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    // Rooms an earlier (failed) run left in alice's list would be stepped to first.
    let _clean_ping = morpheus_e2e::reset_rooms_named(&hs, &["alice"], "ping ");
    let _clean_quiet = morpheus_e2e::reset_rooms_named(&hs, &["alice"], "quiet ");
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;
    // Nothing unread to start from, then two rooms from bob: one merely
    // unread, one mentioning alice.
    app.press("body", "Escape").await.expect("focus out");
    app.key_combo("body", "Escape", false, false, true)
        .await
        .expect("shift+esc marks all read");
    let stamp = std::process::id();
    let room = |name: &str| -> String {
        let made = morpheus_e2e::api_as(
            &hs,
            "bob",
            "POST",
            "createRoom",
            Some(serde_json::json!({ "name": name, "invite": ["@alice:localhost", "@carol:localhost"] })),
        )
        .expect("a room");
        let id = made["room_id"].as_str().expect("room id").to_owned();
        // Three people, so it is listed as a room (not titled like a DM).
        for who in ["alice", "carol"] {
            morpheus_e2e::api_as(
                &hs,
                who,
                "POST",
                &format!("rooms/{}/join", id.replace('!', "%21").replace(':', "%3A")),
                Some(serde_json::json!({})),
            )
            .expect("joins");
        }
        id
    };
    let (quiet, ping) = (format!("quiet {stamp}"), format!("ping {stamp}"));
    let quiet_id = room(&quiet);
    let ping_id = room(&ping);
    morpheus_e2e::send_as(&hs, "bob", &quiet_id, "just chatting").expect("unread");
    morpheus_e2e::api_as(
        &hs,
        "bob",
        "PUT",
        &format!(
            "rooms/{}/send/m.room.message/ping{stamp}",
            ping_id.replace('!', "%21").replace(':', "%3A")
        ),
        Some(serde_json::json!({
            "msgtype": "m.text",
            "body": "@alice:localhost look",
            "m.mentions": { "user_ids": ["@alice:localhost"] },
        })),
    )
    .expect("mention");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while app
        .texts_within(".room", &ping, ".badge")
        .await
        .expect("badges")
        .is_empty()
    {
        assert!(std::time::Instant::now() < deadline, "the mention shows");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    app.key_combo("body", "ArrowDown", true, true, true)
        .await
        .expect("ctrl+shift+alt+down");
    app.wait_for_text(".chat-header h2", &ping, DEFAULT_WAIT)
        .await
        .expect("to the room that mentions alice");
    // Out of alice's list again, for the tests walking through it.
    for id in [&quiet_id, &ping_id] {
        morpheus_e2e::api_as(
            &hs,
            "alice",
            "POST",
            &format!("rooms/{}/leave", id.replace('!', "%21").replace(':', "%3A")),
            Some(serde_json::json!({})),
        )
        .expect("alice leaves");
    }
}
