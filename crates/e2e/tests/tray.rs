//! Closing the window keeps Morpheus running (it lives on in the tray):
//! the process stays, messages that arrive meanwhile are announced, and the
//! window comes back. A hidden webview suspends its JavaScript, so the
//! driver reopens the window through the bridge before looking at the DOM.

#![allow(clippy::expect_used)]

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
        .expect("rooms listed");
}

#[tokio::test]
async fn closing_the_window_keeps_syncing_and_announcing() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut bob = App::launch_desktop(&hs).await.expect("launch bob");
    login(&mut bob, "bob").await;
    bob.open_room("random").await.expect("bob in #random");

    // Close through the title bar: the window hides, the process stays.
    // Deferred: the webview stops answering the moment it hides, so the
    // eval must return before the click lands.
    bob.close_to_tray().await.expect("close");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    assert!(bob.is_running(), "closing the window must not quit the app");

    let mut alice = App::launch_desktop(&hs).await.expect("launch alice");
    login(&mut alice, "alice").await;
    alice.open_room("general").await.expect("alice in #general");
    alice
        .wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
    let text = format!("tray ping {}", std::process::id());
    alice
        .type_into(".composer .composer-input", &text)
        .await
        .expect("type");
    alice
        .press(".composer .composer-input", "Enter")
        .await
        .expect("send");
    alice
        .wait_for_exact_text(".message.own:not(.pending) .body", &text, DEFAULT_WAIT)
        .await
        .expect("sent");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Reopen (what the tray does) and find the message announced meanwhile.
    bob.show_window().await.expect("show window");
    bob.wait_for_text(".notification-log li", &text, DEFAULT_WAIT)
        .await
        .expect("announced while hidden");
    bob.wait_for(".composer .composer-input", DEFAULT_WAIT)
        .await
        .expect("still usable");
}

/// `busctl --user call … --json=short`, parsed; `None` when it fails.
fn bus_call(args: &[&str]) -> Option<serde_json::Value> {
    let output = std::process::Command::new("busctl")
        .args(["--user", "--json=short", "call", "--"])
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        eprintln!("busctl: {}", String::from_utf8_lossy(&output.stderr));
        return None;
    }
    Some(serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null))
}

/// Walk a dbusmenu layout for `(id, label)` pairs.
fn menu_items(node: &serde_json::Value, found: &mut Vec<(i64, String)>) {
    match node {
        serde_json::Value::Array(items) => {
            if let [id, props, ..] = items.as_slice()
                && let (Some(id), Some(label)) = (
                    id.as_i64(),
                    props.pointer("/label/data").and_then(|l| l.as_str()),
                )
            {
                found.push((id, label.to_owned()));
            }
            items.iter().for_each(|item| menu_items(item, found));
        }
        serde_json::Value::Object(map) => map.values().for_each(|item| menu_items(item, found)),
        _ => {}
    }
}

/// The real tray item, driven over the session bus the way a tray host
/// does: the menu has Discord's four entries, a left click (Activate) brings
/// a hidden window back, and a menu entry reaches the app.
#[tokio::test]
async fn tray_click_opens_and_the_menu_reaches_the_app() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let watcher = std::process::Command::new("busctl")
        .args(["--user", "status", "org.kde.StatusNotifierWatcher"])
        .output();
    if !watcher.is_ok_and(|out| out.status.success()) {
        eprintln!("no tray host (StatusNotifierWatcher) on the session bus; skipping");
        return;
    }
    let mut app = App::launch_with_env(&hs, &[("MORPHEUS_E2E_TRAY", "1")])
        .await
        .expect("launch app");
    login(&mut app, "carol").await;
    let name = format!(
        "org.kde.StatusNotifierItem-{}-1",
        app.pid().expect("app pid")
    );

    let layout = bus_call(&[
        &name,
        "/MenuBar",
        "com.canonical.dbusmenu",
        "GetLayout",
        "iias",
        "0",
        "-1",
        "0",
    ])
    .expect("tray menu layout");
    let mut items = Vec::new();
    menu_items(&layout, &mut items);
    let labels: Vec<&str> = items.iter().map(|(_, label)| label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Open Morpheus",
            "Check for Updates…",
            "Acknowledgements",
            "Quit Morpheus"
        ],
        "tray menu"
    );

    // Hide, then left-click the icon.
    app.close_to_tray().await.expect("close");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    bus_call(&[
        &name,
        "/StatusNotifierItem",
        "org.kde.StatusNotifierItem",
        "Activate",
        "ii",
        "0",
        "0",
    ])
    .expect("activate the tray item");
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("window back after a tray click");

    // Hide again; "Acknowledgements" from the menu opens the window on it.
    app.close_to_tray().await.expect("close");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let id = items
        .iter()
        .find(|(_, label)| label == "Acknowledgements")
        .map(|(id, _)| id.to_string())
        .expect("menu id");
    bus_call(&[
        &name,
        "/MenuBar",
        "com.canonical.dbusmenu",
        "Event",
        "isvu",
        &id,
        "clicked",
        "s",
        "",
        "0",
    ])
    .expect("click the menu entry");
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    app.wait_for(".about", DEFAULT_WAIT)
        .await
        .expect("acknowledgements opened from the tray");
}

/// Launching Morpheus while it already runs (here: hidden in the tray)
/// brings the running window back instead of starting a second copy on the
/// same data.
#[tokio::test]
async fn second_launch_shows_the_running_window() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login(&mut app, "carol").await;
    app.close_to_tray().await.expect("hide");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let second = app
        .launch_again(std::time::Duration::from_secs(10))
        .await
        .expect("second launch");
    assert!(
        second.is_some_and(|status| status.success()),
        "the second launch should hand over and exit: {second:?}"
    );
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("the running window is back");
}
