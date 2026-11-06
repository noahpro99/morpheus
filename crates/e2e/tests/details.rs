//! Small details that make an app feel finished: sign-in fields a password
//! manager fills, shortcuts named on the buttons they stand for, numbers
//! that do not jiggle as they change, headings without a lone last word,
//! undo instead of a point of no return, a send that cannot double, and
//! submenus that stay open while the mouse cuts across to them.

#![allow(clippy::expect_used)]

use std::time::Duration;

use morpheus_e2e::{App, DEFAULT_WAIT};

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
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    app.open_room(room).await.expect("open room");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer");
}

async fn style(app: &mut App, selector: &str, property: &str) -> String {
    app.eval(&format!(
        "const el = document.querySelector({}); return el ? getComputedStyle(el).getPropertyValue({}) : 'missing';",
        serde_json::to_string(selector).expect("json"),
        serde_json::to_string(property).expect("json"),
    ))
    .await
    .expect("style")
    .as_str()
    .unwrap_or_default()
    .to_owned()
}

#[tokio::test]
async fn autofill_shortcut_hints_steady_numbers_and_balanced_headings() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch");
    app.wait_for("form.login", DEFAULT_WAIT).await.expect("login form");

    // A password manager knows which field is which.
    let autocomplete = app
        .eval("return ['homeserver', 'username', 'password'].map(n => document.querySelector(`form.login input[name=${n}]`)?.getAttribute('autocomplete'));")
        .await
        .expect("autocomplete");
    assert_eq!(
        autocomplete,
        serde_json::json!(["url", "username", "current-password"]),
        "sign-in fields say what they hold"
    );
    // The sign-in heading wraps evenly.
    assert_eq!(style(&mut app, ".login h1", "text-wrap-style").await, "balance", "balanced heading");

    login_and_open(&mut app, "general").await;

    // Buttons name their shortcut.
    let titles = app
        .eval("return Object.fromEntries([['.pin-toggle'], ['.member-toggle'], ['.settings-button'], ['[title^=\"Inbox\"]'], ['[title^=\"Help\"]']].map(([s]) => [s, document.querySelector(s)?.getAttribute('title')]));")
        .await
        .expect("titles");
    assert_eq!(
        titles,
        serde_json::json!({
            ".pin-toggle": "Pinned messages (Ctrl+P)",
            ".member-toggle": "Member list (Ctrl+U)",
            ".settings-button": "Settings (Ctrl+,)",
            "[title^=\"Inbox\"]": "Inbox (Ctrl+I)",
            "[title^=\"Help\"]": "Help (Ctrl+/)",
        }),
        "shortcut hints on the buttons"
    );

    // Counters and times keep their width as digits change.
    app.wait_for(".message .time", DEFAULT_WAIT).await.expect("a message's time");
    for selector in [".message .time", ".pin-toggle", ".member-toggle"] {
        assert_eq!(
            style(&mut app, selector, "font-variant-numeric").await,
            "tabular-nums",
            "{selector} has steady digits"
        );
    }
    // And dialog headings wrap evenly too.
    app.click(".settings-button").await.expect("settings");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        style(&mut app, ".settings-page h2", "text-wrap-style").await,
        "balance",
        "balanced settings heading"
    );
}

/// Click the hover action `kind` on the message whose body is `text`.
async fn hover_action(app: &mut App, text: &str, kind: &str) {
    app.eval(&format!(
        "const msg = [...document.querySelectorAll('.message')].find(m => m.querySelector('.body')?.textContent.trim() === {}); \
         msg.querySelector('.action.{kind}').click();",
        serde_json::to_string(text).expect("json")
    ))
    .await
    .expect("hover action");
}

fn shown(text: &str) -> String {
    format!(
        "return [...document.querySelectorAll('.message .body')].some(b => b.textContent.trim() === {});",
        serde_json::to_string(text).expect("json")
    )
}

/// Deleting a message asks nothing: it goes at once, with a moment to undo
/// it from a toast; left alone, it is deleted for everyone.
#[tokio::test]
async fn deleting_a_message_can_be_undone() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch");
    login_and_open(&mut app, "random").await;
    let text = format!("undo me {}", std::process::id());
    app.type_into(".composer .composer-input", &text).await.expect("type");
    app.press(".composer .composer-input", "Enter").await.expect("send");
    app.wait_for_exact_text(".message.own:not(.pending) .body", &text, DEFAULT_WAIT)
        .await
        .expect("sent");

    hover_action(&mut app, &text, "delete").await;
    app.wait_for_js(&shown(&text).replace("return ", "return !"), Duration::from_secs(2))
        .await
        .expect("gone at once");
    app.wait_for_text(".toast", "Message deleted", Duration::from_secs(2))
        .await
        .expect("a toast saying so");
    app.click(".toast .toast-undo").await.expect("undo");
    app.wait_for_js(&shown(&text), Duration::from_secs(2))
        .await
        .expect("back after undo");
    // Not deleted behind the scenes either.
    tokio::time::sleep(Duration::from_secs(8)).await;
    assert_eq!(app.eval(&shown(&text)).await.expect("look"), serde_json::json!(true), "still there");

    // Left alone, it goes for good.
    hover_action(&mut app, &text, "delete").await;
    app.wait_for_text(".message .body", "Message deleted", DEFAULT_WAIT)
        .await
        .expect("deleted for everyone once the toast is gone");
}

/// Leaving a room is undoable the same way.
#[tokio::test]
async fn leaving_a_room_can_be_undone() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch");
    login_and_open(&mut app, "random").await;
    let row = "[...document.querySelectorAll('.room')].find(r => r.querySelector('.room-name')?.textContent.trim() === 'random')";
    app.eval(&format!(
        "const el = {row}; const r = el.getBoundingClientRect(); \
         el.dispatchEvent(new MouseEvent('contextmenu', {{ bubbles: true, cancelable: true, clientX: r.left + 20, clientY: r.top + 10 }}));"
    ))
    .await
    .expect("room menu");
    app.wait_for(".context-menu", DEFAULT_WAIT).await.expect("menu open");
    app.click_text(".context-menu .context-item", "Leave room").await.expect("leave");
    app.wait_for_js(&format!("return !{row};"), Duration::from_secs(2))
        .await
        .expect("gone from the list at once");
    app.wait_for_text(".toast", "Left #random", Duration::from_secs(2))
        .await
        .expect("a toast saying so");
    app.click(".toast .toast-undo").await.expect("undo");
    app.wait_for_js(&format!("return !!{row};"), Duration::from_secs(2))
        .await
        .expect("back after undo");
    tokio::time::sleep(Duration::from_secs(8)).await;
    assert_eq!(
        app.eval(&format!("return !!{row};")).await.expect("look"),
        serde_json::json!(true),
        "still a member"
    );
}

/// A double tap on send (or Enter pressed twice at once) sends one message.
#[tokio::test]
async fn a_double_send_sends_once() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch");
    login_and_open(&mut app, "random").await;
    let count = |text: &str| {
        format!(
            "return [...document.querySelectorAll('.message .body')].filter(b => b.textContent.trim() === {}).length;",
            serde_json::to_string(text).expect("json")
        )
    };

    // The send button (phones have one).
    let tapped = format!("tapped twice {}", std::process::id());
    let has_button = app.count(".composer .send-button").await.expect("count") > 0;
    if has_button {
        app.type_into(".composer .composer-input", &tapped).await.expect("type");
        app.eval("const b = document.querySelector('.composer .send-button'); b.click(); b.click();")
            .await
            .expect("double tap");
        app.wait_for_exact_text(".message.own:not(.pending) .body", &tapped, DEFAULT_WAIT)
            .await
            .expect("sent");
    }

    let entered = format!("entered twice {}", std::process::id());
    app.type_into(".composer .composer-input", &entered).await.expect("type");
    app.eval("const i = document.querySelector('.composer .composer-input'); \
              for (const _ of [1, 2]) i.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));")
        .await
        .expect("double enter");
    app.wait_for_exact_text(".message.own:not(.pending) .body", &entered, DEFAULT_WAIT)
        .await
        .expect("sent");

    tokio::time::sleep(Duration::from_secs(3)).await;
    if has_button {
        assert_eq!(app.eval(&count(&tapped)).await.expect("count"), serde_json::json!(1), "one message from a double tap");
    }
    assert_eq!(app.eval(&count(&entered)).await.expect("count"), serde_json::json!(1), "one message from a double Enter");
}

/// Heading for a submenu across the item below it (the mouse cuts the
/// corner) keeps the submenu open; moving down the menu away from it
/// closes it.
#[tokio::test]
async fn submenus_stay_open_while_the_mouse_heads_for_them() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch");
    login_and_open(&mut app, "random").await;
    app.eval(
        "const el = [...document.querySelectorAll('.room')].find(r => r.querySelector('.room-name')?.textContent.trim() === 'random'); \
         const r = el.getBoundingClientRect(); \
         el.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: r.left + 20, clientY: r.top + 10 }));",
    )
    .await
    .expect("room menu");
    app.wait_for(".context-menu", DEFAULT_WAIT).await.expect("menu");

    // Helpers in the page: the Notifications row, the row below it, a
    // pointer move to a point over an element, and whether the flyout shows.
    let setup = "window.__t = { \
        parent: () => [...document.querySelectorAll('.context-item.has-submenu')].find(r => r.querySelector('.menu-label').textContent.trim() === 'Notifications'), \
        below: () => window.__t.parent().nextElementSibling, \
        move: (el, x, y) => { for (const type of ['mousemove', 'mouseover']) el.dispatchEvent(new MouseEvent(type, { bubbles: true, clientX: x, clientY: y })); }, \
        open: () => getComputedStyle(window.__t.parent().querySelector(':scope > .context-submenu')).display !== 'none', \
    }; return true;";
    app.eval(setup).await.expect("helpers");

    let point_in_parent = "const p = window.__t.parent().getBoundingClientRect(); window.__t.move(window.__t.parent(), p.left + 30, p.top + p.height / 2); return window.__t.open();";
    assert_eq!(app.eval(point_in_parent).await.expect("hover"), serde_json::json!(true), "open on hover");

    // Toward the flyout, crossing the row below near its right end.
    app.eval("const b = window.__t.below().getBoundingClientRect(); window.__t.move(window.__t.below(), b.right - 12, b.top + 4); return true;")
        .await
        .expect("cut across");
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(
        app.eval("return window.__t.open();").await.expect("open"),
        serde_json::json!(true),
        "still open while heading for it"
    );
    // And on into it: it stays.
    app.eval("const s = window.__t.parent().querySelector('.context-submenu .context-item'); const r = s.getBoundingClientRect(); window.__t.move(s, r.left + 10, r.top + r.height / 2); return true;")
        .await
        .expect("into the flyout");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(app.eval("return window.__t.open();").await.expect("open"), serde_json::json!(true), "open inside it");

    // Back on the parent, then straight down the left edge, away from it: it closes.
    app.eval(point_in_parent).await.expect("hover again");
    app.eval("const b = window.__t.below().getBoundingClientRect(); window.__t.move(window.__t.below(), b.left + 10, b.top + b.height / 2); return true;")
        .await
        .expect("down the menu");
    app.wait_for_js("return !window.__t.open();", Duration::from_secs(2))
        .await
        .expect("closes when moving away");
}

/// On a phone every button is a comfortable target: at least 44 points
/// each way (Apple's guideline), however small its icon.
#[tokio::test]
async fn phone_tap_targets_are_at_least_44_points() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_sized(&hs, 400, 860).await.expect("launch");
    login_and_open(&mut app, "general").await;
    // Back on the room list, then in the chat: the buttons of each.
    let small = "return [...document.querySelectorAll('.shell button, .shell .room')] \
        .filter(b => b.offsetParent && !b.closest('.titlebar, .message, .timeline, .context-menu')) \
        .map(b => { const r = b.getBoundingClientRect(); return [Math.round(r.width), Math.round(r.height), (b.className || b.tagName) + ' ' + (b.title || b.textContent.trim().slice(0, 16))]; }) \
        .filter(([w, h]) => w < 44 || h < 44).map(([w, h, what]) => `${w}x${h} ${what}`);";
    tokio::time::sleep(Duration::from_secs(1)).await;
    let in_chat = app.eval(small).await.expect("chat buttons");
    app.click(".pane-back").await.expect("back to the list");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let in_list = app.eval(small).await.expect("list buttons");
    assert_eq!(in_chat, serde_json::json!([]), "small targets in the chat");
    assert_eq!(in_list, serde_json::json!([]), "small targets on the room list");
}
