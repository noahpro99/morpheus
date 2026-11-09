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
    app.wait_for_js(
        "return document.querySelector('.help-dialog') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("closed");

    // Alt+Down / Alt+Up step through the sidebar order.
    let before = header(&mut app).await;
    app.key_combo("body", "ArrowDown", false, true, false)
        .await
        .expect("alt+down");
    app.wait_for_js(
        &format!(
            "return document.querySelector('.chat-header h2')?.textContent !== {};",
            serde_json::to_string(&before).expect("json")
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("moved to the next room");
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
    app.wait_for_text(".chat-header h2", "dev", DEFAULT_WAIT)
        .await
        .expect("first room of the space");
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
    app.wait_for_js(
        "return document.querySelector('.members') === null;",
        DEFAULT_WAIT,
    )
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
    app.wait_for_js(
        "return document.querySelector('.settings') === null;",
        DEFAULT_WAIT,
    )
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
    app.wait_for_js(
        &format!(
            "return document.querySelector('.composer .composer-input').value === {};",
            serde_json::to_string(&text).expect("json")
        ),
        Duration::from_secs(5),
    )
    .await
    .expect("draft holds the message");
    app.press(".composer .composer-input", "Escape")
        .await
        .expect("cancel edit");

    // Scroll up, then Esc with nothing open jumps back to the newest message.
    // A tall window can show the whole first page; load enough history to scroll.
    app.reveal("a scrollable timeline", SCROLLABLE, DEFAULT_WAIT)
        .await
        .expect("history");
    app.eval("document.querySelector('.timeline').scrollTop = -2000;")
        .await
        .expect("scroll up");
    app.wait_for(".jump-to-bottom", DEFAULT_WAIT)
        .await
        .expect("scrolled away");
    app.press("body", "Escape").await.expect("escape");
    // On a phone Escape is the back key: it returns to the room list instead.
    let phone = app
        .eval("return !!document.querySelector('.shell.narrow');")
        .await
        .expect("layout");
    let expected = if phone == true {
        "return document.querySelector('.shell').dataset.pane === 'rooms';"
    } else {
        "return document.querySelector('.jump-to-bottom') === null;"
    };
    app.wait_for_js(expected, DEFAULT_WAIT)
        .await
        .expect("back at the bottom (desktop) or the room list (phone)");
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
    // A mark on the current interface, gone once it is rebuilt.
    app.eval("document.querySelector('.shell').dataset.beforeReload = 'yes'; return true;")
        .await
        .expect("mark");
    app.key_combo("body", "r", true, false, false)
        .await
        .expect("ctrl+r");
    app.wait_for(".shell:not([data-before-reload])", DEFAULT_WAIT)
        .await
        .expect("rebuilt");
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
    let state = "return [document.querySelector('.user-bar .mute-button').classList.contains('active'), \
                         document.querySelector('.user-bar .deafen-button').classList.contains('active')];";
    async fn press(app: &mut App, key: &str) {
        // Shift makes the letter a capital, as a real keyboard does.
        app.key_combo("body", key, true, false, true)
            .await
            .expect("shortcut");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let expect = async |app: &mut App, muted: bool, deafened: bool, why: &str| {
        assert_eq!(
            app.eval(state).await.expect("state"),
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
            .unwrap_or_else(|_| panic!("{name} next, as the rail has {order:?}"));
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

/// Shift+Alt+Right goes to the DMs, and from the DMs back to the server you
/// were last in.
#[tokio::test]
async fn shift_alt_right_toggles_dms_and_the_last_server() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;
    app.click(".rail-item[title=\"Projects\"]")
        .await
        .expect("Projects");
    app.wait_for(".rail-item.selected[title=\"Projects\"]", DEFAULT_WAIT)
        .await
        .expect("in Projects");
    app.key_combo("body", "ArrowRight", false, true, true)
        .await
        .expect("shift+alt+right");
    app.wait_for(".rail-dms.selected", DEFAULT_WAIT)
        .await
        .expect("to the DMs");
    app.key_combo("body", "ArrowRight", false, true, true)
        .await
        .expect("shift+alt+right");
    app.wait_for(".rail-item.selected[title=\"Projects\"]", DEFAULT_WAIT)
        .await
        .expect("back to the last server");
    // Another server, then the DMs and back: that one.
    app.click(".rail-item[title=\"Engineering\"]")
        .await
        .expect("Engineering");
    app.key_combo("body", "ArrowRight", false, true, true)
        .await
        .expect("shift+alt+right");
    app.wait_for(".rail-dms.selected", DEFAULT_WAIT)
        .await
        .expect("to the DMs");
    app.key_combo("body", "ArrowRight", false, true, true)
        .await
        .expect("shift+alt+right");
    app.wait_for(".rail-item.selected[title=\"Engineering\"]", DEFAULT_WAIT)
        .await
        .expect("back to Engineering");
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
        .eval("return document.querySelector('.room:not(.friends-row) .room-name').textContent.trim();")
        .await
        .expect("top dm");
    let top = top.as_str().expect("name").to_owned();
    app.eval("document.querySelector('.room:not(.friends-row)').click(); return true;")
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
    app.wait_for_js(
        "const a = document.querySelector('.user-pill .profile-chip .avatar'); return !!a && a.tagName === 'IMG' && a.src.startsWith('data:');",
        DEFAULT_WAIT,
    )
    .await
    .expect("alice's picture in the profile card");
    app.key_combo("body", "k", true, false, false)
        .await
        .expect("ctrl+k");
    app.wait_for(".switcher input", DEFAULT_WAIT)
        .await
        .expect("palette");
    app.type_into(".switcher input", "dev")
        .await
        .expect("filter");
    app.wait_for_js(
        "return [...document.querySelectorAll('.switcher-item:not(.action)')].some(r => r.querySelector('.name')?.textContent.trim() === 'dev' && r.querySelector('.kind')?.textContent.trim() === 'Engineering');",
        DEFAULT_WAIT,
    )
    .await
    .expect("#dev named with its server");
}
