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
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");

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
    assert_eq!(
        style(&mut app, ".login h1", "text-wrap-style").await,
        "balance",
        "balanced heading"
    );

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
    app.wait_for(".message .time", DEFAULT_WAIT)
        .await
        .expect("a message's time");
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
    app.type_into(".composer .composer-input", &text)
        .await
        .expect("type");
    app.press(".composer .composer-input", "Enter")
        .await
        .expect("send");
    app.wait_for_exact_text(".message.own:not(.pending) .body", &text, DEFAULT_WAIT)
        .await
        .expect("sent");

    hover_action(&mut app, &text, "delete").await;
    app.wait_for_js(
        &shown(&text).replace("return ", "return !"),
        Duration::from_secs(2),
    )
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
    assert_eq!(
        app.eval(&shown(&text)).await.expect("look"),
        serde_json::json!(true),
        "still there"
    );

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
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("menu open");
    app.click_text(".context-menu .context-item", "Leave room")
        .await
        .expect("leave");
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
/// (On the phone when `MORPHEUS_E2E_ANDROID` is set: it has the button.)
#[tokio::test]
async fn a_double_send_sends_once() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch");
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
        app.type_into(".composer .composer-input", &tapped)
            .await
            .expect("type");
        app.eval(
            "const b = document.querySelector('.composer .send-button'); b.click(); b.click();",
        )
        .await
        .expect("double tap");
        app.wait_for_exact_text(".message.own:not(.pending) .body", &tapped, DEFAULT_WAIT)
            .await
            .expect("sent");
    }

    // Enter (a phone's Enter makes a new line instead).
    let entered = format!("entered twice {}", std::process::id());
    if !has_button {
        app.type_into(".composer .composer-input", &entered)
            .await
            .expect("type");
        app.eval("const i = document.querySelector('.composer .composer-input'); \
                  for (const _ of [1, 2]) i.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));")
            .await
            .expect("double enter");
        app.wait_for_exact_text(".message.own:not(.pending) .body", &entered, DEFAULT_WAIT)
            .await
            .expect("sent");
    }

    tokio::time::sleep(Duration::from_secs(3)).await;
    if has_button {
        assert_eq!(
            app.eval(&count(&tapped)).await.expect("count"),
            serde_json::json!(1),
            "one message from a double tap"
        );
    }
    if !has_button {
        assert_eq!(
            app.eval(&count(&entered)).await.expect("count"),
            serde_json::json!(1),
            "one message from a double Enter"
        );
    }
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
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("menu");

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
    assert_eq!(
        app.eval(point_in_parent).await.expect("hover"),
        serde_json::json!(true),
        "open on hover"
    );

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
    assert_eq!(
        app.eval("return window.__t.open();").await.expect("open"),
        serde_json::json!(true),
        "open inside it"
    );

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
    assert_eq!(
        in_list,
        serde_json::json!([]),
        "small targets on the room list"
    );

    // In a call: the call's controls in the profile card under the list.
    app.eval("[...document.querySelectorAll('.room')].find(r => r.querySelector('.room-name')?.textContent.trim() === 'hangout')?.click(); return true;")
        .await
        .expect("join #hangout");
    app.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("in the call");
    // Joining brings the chat forward: back to the list, where the card is.
    if app.count(".pane-back").await.expect("count") > 0 {
        app.click(".chat-header .pane-back").await.expect("back");
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    let in_call = app.eval(small).await.expect("call buttons");
    // The card grows to hold the call: your own row (picture, mute, deafen,
    // settings) stays whole inside it, under the call's controls.
    let cut = app
        .eval(
            "const pill = document.querySelector('.user-pill').getBoundingClientRect(); \
             return [...document.querySelectorAll('.user-pill .user-row > *')].filter(el => { \
               const r = el.getBoundingClientRect(); return r.height === 0 || r.top < pill.top - 1 || r.bottom > pill.bottom + 1; \
             }).map(el => el.className || el.tagName);",
        )
        .await
        .expect("card");
    let _ = app.click(".voice-panel .hang-up").await;
    assert_eq!(
        in_call,
        serde_json::json!([]),
        "small targets in the call card"
    );
    assert_eq!(
        cut,
        serde_json::json!([]),
        "your row is cut off by the call card"
    );
}

/// Waiting shows the shape of what is coming, never a "Loading…" line: the
/// member list, pinned messages, your devices and a picture opening full
/// size all show skeletons while they load.
#[tokio::test]
async fn loading_shows_skeletons_not_words() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch");
    login_and_open(&mut app, "general").await;
    // Note any "Loading…" that appears anywhere, however briefly.
    app.eval("window.__loadingSeen = []; new MutationObserver(changes => { for (const c of changes) for (const n of [c.target, ...c.addedNodes]) { const t = n.textContent || ''; if (/Loading…/.test(t) && n.nodeType !== 9) window.__loadingSeen.push((n.className || n.nodeName) + ': ' + t.trim().slice(0, 40)); } }).observe(document.body, { subtree: true, childList: true, characterData: true }); return true;")
        .await
        .expect("observer");

    // The member list, shut and opened again.
    app.click(".member-toggle").await.expect("hide members");
    app.click(".member-toggle").await.expect("show members");
    app.wait_for(".members .member", DEFAULT_WAIT)
        .await
        .expect("members");
    // Pinned messages.
    app.click(".pin-toggle").await.expect("pins");
    app.wait_for(".pinned-dialog", DEFAULT_WAIT)
        .await
        .expect("pinned dialog");
    tokio::time::sleep(Duration::from_secs(1)).await;
    app.press("body", "Escape").await.expect("close");
    // Your devices.
    app.click(".settings-button").await.expect("settings");
    app.click_text(".settings-nav button", "Security & Privacy")
        .await
        .expect("security");
    tokio::time::sleep(Duration::from_secs(2)).await;
    app.press("body", "Escape").await.expect("close settings");
    // A picture, full size.
    app.reveal_element(".message img.image", DEFAULT_WAIT)
        .await
        .expect("an image");
    app.click(".message img.image").await.expect("open it");
    app.wait_for(".lightbox-overlay", DEFAULT_WAIT)
        .await
        .expect("lightbox");
    tokio::time::sleep(Duration::from_secs(1)).await;

    let seen = app
        .eval("return window.__loadingSeen;")
        .await
        .expect("seen");
    assert_eq!(
        seen,
        serde_json::json!([]),
        "\"Loading…\" shown while waiting"
    );
}

const DUSK_PNG: &[u8] = include_bytes!("../fixtures/dusk.png");

/// Whether the image named `body` shows a blurred placeholder (a decoded
/// blurhash) behind it or in its place.
fn blurred(body: &str) -> String {
    format!(
        "const el = [...document.querySelectorAll('.message .image')].find(i => i.alt === {0} || i.title === {0} || i.textContent.includes({0})); \
         return !!el && getComputedStyle(el).backgroundImage.startsWith('url(\"data:image/png');",
        serde_json::to_string(body).expect("json")
    )
}

/// Pictures show a blurred preview of themselves while they load: from the
/// blurhash others' apps send with them, and one this app sends with its
/// own.
#[tokio::test]
async fn pictures_blur_in_from_a_placeholder() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch");
    login_and_open(&mut app, "random").await;
    let room_id: String = app
        .eval("return document.querySelector('.room.selected').dataset.room;")
        .await
        .expect("room id")
        .as_str()
        .expect("string")
        .to_owned();

    // From someone else's app, with a blurhash.
    let theirs = format!("their dusk {}.png", std::process::id());
    morpheus_e2e::send_image_as(
        &hs,
        "bob",
        &room_id,
        DUSK_PNG,
        &theirs,
        Some("LKO2?U%2Tw=w]~RBVZRi};RPxuwH"),
    )
    .expect("bob sends a picture");
    app.wait_for_js(&blurred(&theirs), DEFAULT_WAIT)
        .await
        .expect("a blurred placeholder from their blurhash");

    // This app's own: dropped in, sent with a blurhash of its own.
    let ours = format!("our dusk {}.png", std::process::id());
    let bytes: Vec<String> = DUSK_PNG.iter().map(u8::to_string).collect();
    app.eval(&format!(
        "const dt = new DataTransfer(); dt.items.add(new File([new Uint8Array([{}])], {}, {{ type: 'image/png' }})); \
         const target = document.querySelector('.chat'); \
         target.dispatchEvent(new DragEvent('dragover', {{ bubbles: true, cancelable: true, dataTransfer: dt }})); \
         target.dispatchEvent(new DragEvent('drop', {{ bubbles: true, cancelable: true, dataTransfer: dt }}));",
        bytes.join(","),
        serde_json::to_string(&ours).expect("json")
    ))
    .await
    .expect("drop a picture");
    app.wait_for_js(&blurred(&ours), DEFAULT_WAIT)
        .await
        .expect("sent with a blurhash of its own");
}
