//! Settings layout: categories down the side, about 70% of a desktop window
//! wide; full screen on a phone, list first, back from a page to the list.
//! Appearance switches the colour theme (kept in account data, so it
//! survives a restart); Notifications sets the account-wide defaults.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

const CATEGORIES: [&str; 5] = [
    "My Account",
    "Security & Privacy",
    "Notifications",
    "Appearance",
    "Voice & Video",
];

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

async fn open_settings(app: &mut App) {
    app.click(".settings-button").await.expect("gear button");
    app.wait_for(".settings", DEFAULT_WAIT)
        .await
        .expect("settings open");
}

async fn page(app: &mut App, category: &str) {
    app.click_text(".settings-nav button", category)
        .await
        .unwrap_or_else(|e| panic!("open {category}: {e}"));
    app.wait_for_text(".settings-page h2", category, DEFAULT_WAIT)
        .await
        .unwrap_or_else(|e| panic!("{category} page: {e}"));
}

fn visible(selector: &str) -> String {
    format!(
        "const el = document.querySelector({}); return !!el && el.offsetParent !== null && el.getBoundingClientRect().width > 0;",
        serde_json::to_string(selector).expect("json")
    )
}

async fn theme(app: &mut App) -> String {
    app.eval("return document.documentElement.dataset.theme || '';")
        .await
        .expect("theme")
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[tokio::test]
async fn desktop_settings_have_categories_themes_and_notification_defaults() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login(&mut app, "carol").await;
    open_settings(&mut app).await;

    // About 70% of the window, categories down the side, My Account first.
    let share = app
        .eval("return document.querySelector('.settings').getBoundingClientRect().width / innerWidth;")
        .await
        .expect("width")
        .as_f64()
        .expect("number");
    assert!(
        (0.66..=0.74).contains(&share),
        "settings is {share:.2} of the window"
    );
    for category in CATEGORIES {
        app.wait_for_text(".settings-nav button", category, DEFAULT_WAIT)
            .await
            .unwrap_or_else(|e| panic!("category {category}: {e}"));
    }
    app.wait_for_text(".settings-page h2", "My Account", DEFAULT_WAIT)
        .await
        .expect("opens on My Account");
    app.wait_for_text(".settings-user", "@carol:localhost", DEFAULT_WAIT)
        .await
        .expect("account shown");

    // Security & Privacy holds encryption, recovery and devices.
    page(&mut app, "Security & Privacy").await;
    app.wait_for(".settings-page .recovery-form", DEFAULT_WAIT)
        .await
        .expect("recovery form");
    app.wait_for_text(".settings-page .device", "this device", DEFAULT_WAIT)
        .await
        .expect("devices");

    // Appearance: picking a theme applies it at once.
    page(&mut app, "Appearance").await;
    let before = app
        .eval("return getComputedStyle(document.documentElement).getPropertyValue('--bg-chat').trim();")
        .await
        .expect("colour");
    app.click_text(".theme-option", "Light")
        .await
        .expect("pick Light");
    app.wait_for_js(
        "return document.documentElement.dataset.theme === 'light';",
        DEFAULT_WAIT,
    )
    .await
    .expect("light theme applied");
    let after = app
        .eval("return getComputedStyle(document.documentElement).getPropertyValue('--bg-chat').trim();")
        .await
        .expect("colour");
    assert_ne!(before, after, "the chat background changes with the theme");
    app.wait_for(".theme-option.selected[data-theme=light]", DEFAULT_WAIT)
        .await
        .expect("Light marked as chosen");

    // Notifications: the group-room default saves to the server.
    page(&mut app, "Notifications").await;
    app.wait_for(
        ".notify-default[data-kind=group] input:checked",
        DEFAULT_WAIT,
    )
    .await
    .expect("current group default shown");
    let was = app
        .eval("return document.querySelector('.notify-default[data-kind=group] input:checked').value;")
        .await
        .expect("current default");
    let other = if was == "all" { "mentions" } else { "all" };
    app.click(&format!(
        ".notify-default[data-kind=group] input[value={other}]"
    ))
    .await
    .expect("change default");
    app.wait_for_text(
        ".notify-default[data-kind=group] .ok",
        "Saved",
        DEFAULT_WAIT,
    )
    .await
    .expect("default saved");

    // A restart keeps the theme and shows the saved default.
    let mut app = app.relaunch().await.expect("relaunch");
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("rooms after restart");
    app.wait_for_js(
        "return document.documentElement.dataset.theme === 'light';",
        DEFAULT_WAIT,
    )
    .await
    .expect("light theme kept");
    open_settings(&mut app).await;
    page(&mut app, "Notifications").await;
    app.wait_for(
        &format!(".notify-default[data-kind=group] input[value={other}]:checked"),
        DEFAULT_WAIT,
    )
    .await
    .expect("saved default shown after restart");

    // Put both back for other tests.
    app.click(&format!(
        ".notify-default[data-kind=group] input[value={}]",
        was.as_str().unwrap_or("all")
    ))
    .await
    .expect("restore default");
    app.wait_for_text(
        ".notify-default[data-kind=group] .ok",
        "Saved",
        DEFAULT_WAIT,
    )
    .await
    .expect("restored");
    page(&mut app, "Appearance").await;
    app.click_text(".theme-option", "Dark")
        .await
        .expect("back to Dark");
    app.wait_for_js(
        "return document.documentElement.dataset.theme === 'dark';",
        DEFAULT_WAIT,
    )
    .await
    .expect("dark again");
    assert_eq!(theme(&mut app).await, "dark");
}

#[tokio::test]
async fn phone_settings_are_full_screen_list_then_page() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_sized(&hs, 420, 800)
        .await
        .expect("launch phone-sized app");
    login(&mut app, "carol").await;
    open_settings(&mut app).await;

    // The whole screen, starting on the list of categories.
    let fits = app
        .eval(
            "const s = document.querySelector('.settings').getBoundingClientRect(), \
                   a = document.querySelector('.shell').getBoundingClientRect(); \
             return Math.abs(s.left - a.left) < 1 && Math.abs(s.right - a.right) < 1 \
                 && Math.abs(s.top - a.top) < 1 && Math.abs(s.bottom - a.bottom) < 1;",
        )
        .await
        .expect("geometry");
    assert_eq!(fits, serde_json::json!(true), "settings fill the screen");
    assert_eq!(
        app.eval(&visible(".settings-nav")).await.expect("eval"),
        serde_json::json!(true),
        "list shown"
    );
    assert_eq!(
        app.eval(&visible(".settings-page")).await.expect("eval"),
        serde_json::json!(false),
        "no page yet"
    );

    // A category replaces the list; back returns to it, then closes.
    page(&mut app, "Appearance").await;
    assert_eq!(
        app.eval(&visible(".settings-nav")).await.expect("eval"),
        serde_json::json!(false),
        "list hidden"
    );
    app.key_combo("body", "Escape", false, false, false)
        .await
        .expect("back");
    app.wait_for_js(&visible(".settings-nav"), DEFAULT_WAIT)
        .await
        .expect("back at the list");
    app.key_combo("body", "Escape", false, false, false)
        .await
        .expect("back again");
    app.wait_for_js("return !document.querySelector('.settings');", DEFAULT_WAIT)
        .await
        .expect("settings closed");
}

/// Settings opens with its search bar focused: typing finds single
/// settings, and picking one opens its page with it highlighted.
#[tokio::test]
async fn settings_open_with_the_search_bar_focused() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = morpheus_e2e::App::launch(&hs).await.expect("launch");
    login(&mut app, "alice").await;
    app.click(".settings-button").await.expect("gear");
    app.wait_for_js(
        "return document.activeElement === document.querySelector('.settings .settings-search');",
        morpheus_e2e::DEFAULT_WAIT,
    )
    .await
    .expect("the search bar focused");
    app.type_into(".settings .settings-search", "microphone")
        .await
        .expect("type");
    app.wait_for_text(
        ".settings .settings-result",
        "Microphone and speaker",
        morpheus_e2e::DEFAULT_WAIT,
    )
    .await
    .expect("found");
    app.press(".settings .settings-search", "Enter")
        .await
        .expect("pick");
    app.wait_for_text(
        ".settings-page h2",
        "Voice & Video",
        morpheus_e2e::DEFAULT_WAIT,
    )
    .await
    .expect("its page");
    app.wait_for(
        ".settings-page [data-setting=audio-devices].setting-highlight",
        morpheus_e2e::DEFAULT_WAIT,
    )
    .await
    .expect("highlighted");
}
