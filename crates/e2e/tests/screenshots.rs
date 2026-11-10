//! Screenshots of the app for the website, not checks. Needs the demo world
//! (`nix shell nixpkgs#imagemagick -c dev/homeserver/demo.sh`), then on demand:
//! `MORPHEUS_SHOTS_DIR=<dir> cargo test -p morpheus-e2e --test screenshots -- --ignored --test-threads=1`
//! (`desktop_tour` on Hyprland; with the emulator up, `phone_tour`).

#![allow(clippy::expect_used)]

use std::path::PathBuf;
use std::time::Duration;

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

fn shots_dir() -> PathBuf {
    let dir = std::env::var("MORPHEUS_SHOTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("target/shots"));
    std::fs::create_dir_all(&dir).expect("shots dir");
    dir
}

async fn shot(app: &mut App, name: &str) {
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let taken = app.screenshot(name).await.expect("screenshot");
    let Some(path) = taken else {
        panic!("no screenshot for {name} (needs Hyprland and grim, or adb)");
    };
    let to = shots_dir().join(format!("{name}.png"));
    std::fs::copy(&path, &to).expect("copy shot");
    eprintln!("[shot] {}", to.display());
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
    // The demo rooms are all in its space.
    app.wait_for(".rail-item[title=\"Night Owls\"]", DEFAULT_WAIT)
        .await
        .expect("the space");
    // Kept out of the pictures: the Secure Backup prompt a new account gets.
    if app
        .wait_for(".backup-prompt", Duration::from_secs(10))
        .await
        .is_ok()
    {
        let _ = app.click_text(".backup-prompt button", "Later").await;
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
}

async fn open(app: &mut App, room: &str) {
    app.open_room(room).await.expect("open room");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer");
    // Pictures and reactions settle.
    tokio::time::sleep(Duration::from_secs(2)).await;
}

/// Click a voice channel's row: the first click joins its call (staying
/// where you are), the next opens its call page.
async fn join_voice(app: &mut App, room: &str) {
    let row = format!(
        "[...document.querySelectorAll('.room')].find(r => r.querySelector('.room-name')?.textContent.trim() === {})",
        serde_json::to_string(room).expect("json")
    );
    app.wait_for_js(
        &format!("const r = {row}; if (!r) return false; r.click(); return true;"),
        DEFAULT_WAIT,
    )
    .await
    .expect("the voice channel");
}

#[tokio::test]
#[ignore = "takes screenshots for the website"]
async fn desktop_tour() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping");
        return;
    };
    let mut app = App::launch_sized(&hs, 1440, 900).await.expect("launch");
    shot(&mut app, "desktop-login").await;
    login(&mut app, "luna").await;
    open(&mut app, "lobby").await;
    shot(&mut app, "desktop-chat").await;

    // Someone's profile, from the member list.
    app.wait_for_text(".members .member", "Rosa", DEFAULT_WAIT)
        .await
        .expect("members");
    app.click_text(".members .member", "Rosa")
        .await
        .expect("Rosa");
    app.wait_for(".profile-card", DEFAULT_WAIT)
        .await
        .expect("card");
    shot(&mut app, "desktop-profile").await;
    app.press("body", "Escape").await.expect("close");
    app.wait_for_js(
        "return document.querySelector('.profile-card') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("card closed");

    // A picture and a thread in #design.
    open(&mut app, "design").await;
    shot(&mut app, "desktop-image").await;
    app.reveal_element(".thread-chip", DEFAULT_WAIT)
        .await
        .expect("thread chip");
    app.click(".thread-chip").await.expect("thread");
    app.wait_for(".thread-panel", DEFAULT_WAIT)
        .await
        .expect("thread panel");
    shot(&mut app, "desktop-thread").await;
    app.click(".thread-panel-header .ghost")
        .await
        .expect("close");

    // Ctrl+K.
    open(&mut app, "lobby").await;
    app.shortcut(".composer .composer-input", "k", true)
        .await
        .expect("ctrl+k");
    app.wait_for(".switcher input", DEFAULT_WAIT)
        .await
        .expect("switcher");
    shot(&mut app, "desktop-switcher").await;
    app.eval("document.querySelector('.overlay')?.click();")
        .await
        .expect("close");

    // Settings.
    app.click(".settings-button").await.expect("settings");
    shot(&mut app, "desktop-settings").await;
    app.click(".settings-header .ghost").await.expect("close");

    // A call in #hangout (a voice channel: opening it joins) with Milo,
    // both talking (test runs send a tone).
    join_voice(&mut app, "hangout").await;
    app.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("connected");
    let mut milo = App::launch(&hs).await.expect("milo");
    login(&mut milo, "milo").await;
    open(&mut milo, "lobby").await;
    join_voice(&mut milo, "hangout").await;
    app.wait_for_js(
        "return document.querySelectorAll('.call-participant').length >= 2;",
        DEFAULT_WAIT,
    )
    .await
    .expect("both in the call");
    // A second click on the channel opens its call page: a card each.
    join_voice(&mut app, "hangout").await;
    app.wait_for_js(
        "return document.querySelectorAll('.call-stage.voice-page .call-cards > *').length >= 2;",
        DEFAULT_WAIT,
    )
    .await
    .expect("both cards");
    let _ = app
        .wait_for_js(
            "return document.querySelector('.call-participant.speaking') !== null;",
            DEFAULT_WAIT,
        )
        .await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    shot(&mut app, "desktop-call").await;
    let _ = app
        .eval("document.querySelector('.voice-panel .hang-up')?.click();")
        .await;
    let _ = milo
        .eval("document.querySelector('.voice-panel .hang-up')?.click();")
        .await;
    tokio::time::sleep(Duration::from_secs(2)).await;
}

#[tokio::test]
#[ignore = "takes screenshots for the website"]
async fn phone_tour() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping");
        return;
    };
    let mut app = App::launch_sized(&hs, 400, 860).await.expect("launch");
    assert!(
        app.is_phone(),
        "needs the emulator (or a phone) for phone shots"
    );
    shot(&mut app, "phone-login").await;
    login(&mut app, "luna").await;
    open(&mut app, "lobby").await;
    shot(&mut app, "phone-chat").await;
    // Back to the room list.
    app.eval("history.back();").await.expect("back");
    let _ = app.press("body", "Escape").await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    shot(&mut app, "phone-rooms").await;
    open(&mut app, "design").await;
    shot(&mut app, "phone-image").await;
}

/// The web app (app.morpheuschat.org) at 1440×900: #lobby. Run with
/// MORPHEUS_E2E_WEB=1 in `nix develop .#web`, after `just e2e-web-build`.
#[tokio::test]
#[ignore = "takes screenshots for the website"]
async fn web_tour() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping");
        return;
    };
    let mut app = App::launch_sized(&hs, 1440, 900).await.expect("launch");
    login(&mut app, "luna").await;
    open(&mut app, "lobby").await;
    shot(&mut app, "web-chat").await;
}
