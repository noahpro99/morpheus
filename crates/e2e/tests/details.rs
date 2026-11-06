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
