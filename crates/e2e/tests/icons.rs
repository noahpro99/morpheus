//! Buttons carry icons from the app's icon set, as the rail, title bar and
//! menus already do: the message hover bar (not emoji), and every action
//! button across Settings' pages.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT, api_as, new_account};

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
    app.wait_for(".shell", DEFAULT_WAIT).await.expect("shell");
}

/// Labels of the buttons under `scope` that have no icon.
async fn without_icons(app: &mut App, scope: &str) -> Vec<String> {
    let js = format!(
        "return [...document.querySelectorAll({scope:?})]
            .filter(b => b.offsetParent !== null && !b.querySelector('svg.icon'))
            .map(b => (b.textContent.trim() || b.title || b.className));"
    );
    app.eval(&js)
        .await
        .expect("buttons")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect()
}

#[tokio::test]
async fn the_message_hover_bar_uses_icons_not_emoji() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let user = new_account(&hs, "icons").expect("account");
    api_as(
        &hs,
        &user,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": "lounge" })),
    )
    .expect("a room");
    let mut app = App::launch(&hs).await.expect("launch");
    login(&mut app, &user).await;
    app.open_room("lounge").await.expect("open the room");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer");
    app.type_into(".composer .composer-input", "with icons")
        .await
        .expect("type");
    app.press(".composer .composer-input", "Enter")
        .await
        .expect("send");
    for action in ["reply", "thread", "edit", "delete"] {
        let selector = format!(".message .actions .action.{action} svg.icon");
        app.wait_for(&selector, DEFAULT_WAIT)
            .await
            .unwrap_or_else(|err| panic!("{action} is an icon: {err}"));
    }
    let text = app
        .eval(
            "return [...document.querySelectorAll('.message .actions .action.reply, .message .actions .action.thread, .message .actions .action.edit, .message .actions .action.delete')].map(b => b.textContent.trim()).join('');",
        )
        .await
        .expect("labels");
    assert_eq!(text.as_str(), Some(""), "no emoji beside the icons");
}

#[tokio::test]
async fn every_settings_button_has_an_icon() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let user = new_account(&hs, "iconset").expect("account");
    let mut app = App::launch(&hs).await.expect("launch");
    login(&mut app, &user).await;
    app.click(".settings-button").await.expect("settings");
    app.wait_for(".settings-nav", DEFAULT_WAIT)
        .await
        .expect("settings open");
    let pages = app
        .eval("return [...document.querySelectorAll('.settings-nav button.settings-nav-item')].map(b => b.textContent.trim());")
        .await
        .expect("pages");
    let pages: Vec<String> = pages
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    assert!(pages.len() >= 7, "settings pages: {pages:?}");
    let mut missing = Vec::new();
    for page in &pages {
        app.click_text(".settings-nav button.settings-nav-item", page)
            .await
            .expect("open the page");
        app.wait_for_text(".settings-page h2", page, DEFAULT_WAIT)
            .await
            .expect("page shown");
        if page == "Security & Privacy" {
            app.wait_for(".settings-page .status-list", DEFAULT_WAIT)
                .await
                .expect("encryption status");
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        for label in without_icons(
            &mut app,
            ".settings-page button.primary, .settings-page button.secondary, .settings-page button.danger, .settings-page a.button",
        )
        .await
        {
            missing.push(format!("{page}: {label}"));
        }
    }
    assert!(missing.is_empty(), "buttons without an icon: {missing:#?}");
}
