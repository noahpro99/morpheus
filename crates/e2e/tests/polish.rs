//! Small things that are easy to regress: pictures in the command palette,
//! the member menu on a message's avatar and sender name, "Invite to
//! server" listing servers (and server-less rooms) with pictures, the gear
//! sitting close to the attach button, no hover shadow on your own avatar,
//! acknowledgements, and the update check.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
        .expect("rooms listed");
    app.open_room(room).await.expect("open room");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
}

async fn menu_labels(app: &mut App, selector: &str) -> String {
    app.texts(selector).await.expect("labels").join("|")
}

async fn palette(app: &mut App, query: &str) {
    app.key_combo("body", "k", true, false, false)
        .await
        .expect("ctrl+k");
    app.wait_for(".switcher input", DEFAULT_WAIT)
        .await
        .expect("palette");
    app.type_into(".switcher input", query)
        .await
        .expect("filter");
}

/// The rows of the open menu's flyouts (only "Invite to server" has one).
const INVITE_ROWS: &str = ".context-menu > .has-submenu .context-submenu .context-item";
const INVITE_PARENT: (&str, &str) = (".context-menu > .has-submenu", "Invite to server");

#[tokio::test]
async fn pictures_menus_and_spacing() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;

    // Rooms in the palette carry their picture.
    palette(&mut app, "").await;
    app.wait_for(".switcher-item .avatar", DEFAULT_WAIT)
        .await
        .expect("room pictures in the palette");
    app.key_combo("body", "Escape", false, false, false)
        .await
        .expect("close palette");
    app.wait_for_count(".switcher", 0, DEFAULT_WAIT)
        .await
        .expect("palette closed");

    // The gear and the attach button sit close together across the edge.
    let gear = app
        .bounds(".user-pill .settings-button")
        .await
        .expect("gear")
        .expect("the gear");
    let plus = app
        .bounds(".composer .attach-button")
        .await
        .expect("plus")
        .expect("the attach button");
    let gap = plus.x - gear.right();
    // And the pill spans the band under the rail and sidebar.
    let pill = app
        .bounds(".user-pill")
        .await
        .expect("pill")
        .expect("the pill");
    let side = app
        .bounds(".sidebar")
        .await
        .expect("sidebar")
        .expect("the sidebar");
    let spans = serde_json::json!([pill.x, side.right() - pill.right()]);
    assert_eq!(
        spans,
        serde_json::json!([8.0, 8.0]),
        "pill margins inside the band"
    );
    assert!(
        (0.0..=32.0).contains(&gap),
        "gear to attach button gap is {gap}px"
    );

    // The logo on the DMs button fills its circle rather than floating in it.
    let button = app
        .bounds(".rail-dms")
        .await
        .expect("dms")
        .expect("the DMs button");
    let logo = app
        .bounds(".rail-dms .logo")
        .await
        .expect("dms logo")
        .expect("its logo");
    let fill = logo.w / button.w;
    assert!(fill >= 0.8, "DMs logo fills {fill:.2} of its circle");

    // Your own avatar gets no shadow when the chip is hovered.
    if !app.is_gpui() {
        let shadowed = app
            .eval("return [...document.styleSheets].flatMap(s => [...s.cssRules]).some(r => \
                   (r.selectorText || '').includes('.profile-chip:hover .avatar') && r.style.boxShadow);")
            .await
            .expect("rules");
        assert_eq!(shadowed, false, "own avatar still has a hover shadow");
    }

    // Right-click on someone's avatar in a message: the member menu.
    app.reveal_element(".message:not(.own) .avatar-button", DEFAULT_WAIT)
        .await
        .expect("a message from someone else");
    app.right_click(".message:not(.own) .avatar-button")
        .await
        .expect("right-click avatar");
    app.wait_for_text(".context-menu > .context-item", "Mention", DEFAULT_WAIT)
        .await
        .expect("member menu from the avatar");
    let labels = menu_labels(&mut app, ".context-menu > .context-item").await;
    for needle in ["Profile", "Message", "Invite to server", "Block"] {
        assert!(labels.contains(needle), "missing {needle} in {labels}");
    }
    assert!(
        !labels.contains("Reply"),
        "the message menu opened instead: {labels}"
    );

    // Invite to server: servers and rooms outside any server, with pictures.
    if app.is_gpui() {
        // The flyout opens on a hover or a click; the Dioxus one is in the page.
        app.click_text(".context-menu > .has-submenu", "Invite to server")
            .await
            .expect("open the flyout");
    }
    app.wait_for(INVITE_ROWS, DEFAULT_WAIT).await.expect("the flyout");
    let targets = app
        .texts_within(INVITE_PARENT.0, INVITE_PARENT.1, ".context-submenu .context-item .menu-label")
        .await
        .expect("targets")
        .join("|");
    assert!(
        targets.contains("Engineering"),
        "no server in the invite list: {targets}"
    );
    assert!(
        targets.contains("random"),
        "no server-less room in the invite list: {targets}"
    );
    assert!(
        !targets.split('|').any(|t| t == "dev" || t == "#dev"),
        "a room inside a server is listed on its own: {targets}"
    );
    let rows = app
        .texts_within(INVITE_PARENT.0, INVITE_PARENT.1, ".context-submenu .context-item")
        .await
        .expect("rows")
        .len();
    let pictures = app
        .texts_within(INVITE_PARENT.0, INVITE_PARENT.1, ".context-submenu .context-item .menu-avatar .avatar")
        .await
        .expect("pictures")
        .len();
    assert!(rows > 0 && pictures >= rows, "invite targets lack pictures ({pictures} for {rows})");
    app.key_combo("body", "Escape", false, false, false)
        .await
        .expect("close menu");
    app.wait_for_count(".context-menu", 0, DEFAULT_WAIT)
        .await
        .expect("menu closed");

    // The sender's name does the same.
    app.right_click(".message:not(.own) button.sender")
        .await
        .expect("right-click name");
    app.wait_for_text(".context-menu > .context-item", "Mention", DEFAULT_WAIT)
        .await
        .expect("member menu from the name");
}

/// Answers every request with a release feed naming version 99.0.0.
async fn serve_feed() -> String {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind feed");
    let port = listener.local_addr().expect("addr").port();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut request = [0u8; 2048];
            let _ = socket.read(&mut request).await;
            let body = r#"{"version":"99.0.0","url":"https://example.org/morpheus"}"#;
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(reply.as_bytes()).await;
        }
    });
    format!("http://127.0.0.1:{port}/feed.json")
}

#[tokio::test]
async fn acknowledgements_and_update_check_from_the_palette() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let feed = serve_feed().await;
    let mut app = App::launch_with_env(&hs, &[("MORPHEUS_UPDATE_FEED", &feed)])
        .await
        .expect("launch app");
    login_and_open(&mut app, "general").await;

    palette(&mut app, "acknow").await;
    app.wait_for_text(".switcher-item.active", "Acknowledgements", DEFAULT_WAIT)
        .await
        .expect("acknowledgements offered");
    app.press(".switcher input", "Enter").await.expect("run");
    app.wait_for_text(".about .about-headliners", "Dioxus", DEFAULT_WAIT)
        .await
        .expect("acknowledgements shown");
    app.wait_for_text(".about .about-crates", "matrix-sdk", DEFAULT_WAIT)
        .await
        .expect("crate list shown");
    app.key_combo("body", "Escape", false, false, false)
        .await
        .expect("escape");
    app.wait_for_count(".about", 0, DEFAULT_WAIT)
        .await
        .expect("escape closes it");

    palette(&mut app, "updates").await;
    app.wait_for_text(".switcher-item.active", "Check for updates", DEFAULT_WAIT)
        .await
        .expect("update check offered");
    app.press(".switcher input", "Enter").await.expect("run");
    app.wait_for_text(".toast", "Morpheus 99.0.0 is available", DEFAULT_WAIT)
        .await
        .expect("the feed's version is announced");
}
