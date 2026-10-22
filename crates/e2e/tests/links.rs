//! Links in messages are detected and open outside the app: bare URLs in
//! plain and formatted messages become links (trailing punctuation left
//! out), and clicking one hands it to the browser instead of navigating the
//! app's own page. Test runs log the hand-off rather than opening a browser.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn send(app: &mut App, text: &str) {
    app.type_into(".composer .composer-input", text)
        .await
        .expect("type");
    app.press(".composer .composer-input", "Enter")
        .await
        .expect("send");
}

#[tokio::test]
async fn links_are_detected_and_open_outside_the_app() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
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
    app.open_room("random").await.expect("open random");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");

    let id = std::process::id();
    let plain = format!("https://example.org/plain-{id}");
    send(&mut app, &format!("see {plain}.")).await;
    let plain_link = format!(".message .body a[href=\"{plain}\"]");
    app.wait_for(&plain_link, DEFAULT_WAIT)
        .await
        .expect("a bare URL in a plain message is a link");
    app.wait_for_exact_text(&plain_link, &plain, DEFAULT_WAIT)
        .await
        .expect("the full stop is not part of the link");

    let rich = format!("https://example.org/rich-{id}");
    send(&mut app, &format!("**bold** and {rich}")).await;
    let rich_link = format!(".message .body a[href=\"{rich}\"]");
    app.wait_for(&rich_link, DEFAULT_WAIT)
        .await
        .expect("a bare URL in a formatted message is a link");

    let page = app.eval("return location.href;").await.expect("page url");
    app.click(&plain_link).await.expect("click the link");
    app.wait_for_text(".link-log li", &plain, DEFAULT_WAIT)
        .await
        .expect("the link was handed to the browser");
    let after = app.eval("return location.href;").await.expect("page url");
    assert_eq!(page, after, "clicking a link must not navigate the app");
    app.wait_for(".composer .composer-input", DEFAULT_WAIT)
        .await
        .expect("the app is still there");
}
