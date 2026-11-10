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

/// matrix.to links open in Morpheus, not the browser: a person's opens their
/// profile, a room you are in opens it, a message's jumps to the message, and
/// a room you are not in shows what it is, with Join.
#[tokio::test]
async fn matrix_to_links_open_in_the_app() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let id = std::process::id();
    // A room bob made that alice is not in.
    let alias = format!("linked{id}");
    let made = morpheus_e2e::api_as(
        &hs,
        "bob",
        "POST",
        "createRoom",
        Some(serde_json::json!({
            "name": format!("Linked {id}"),
            "topic": "come on in",
            "preset": "public_chat",
            "room_alias_name": alias,
        })),
    )
    .expect("a public room");
    assert!(made["room_id"].is_string(), "room made: {made}");

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
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    app.open_room("random").await.expect("open random");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");

    // A message in #random to link to.
    let target = format!("link target {id}");
    send(&mut app, &target).await;
    let find = format!(
        "const m = [...document.querySelectorAll('.message[data-event-id^=\"$\"]')].find(m => m.querySelector('.body')?.textContent.trim() === {}); return m ? m.dataset.eventId : null;",
        serde_json::to_string(&target).expect("json")
    );
    app.wait_for_js(
        &find.replace("return m ? m.dataset.eventId : null;", "return !!m;"),
        DEFAULT_WAIT,
    )
    .await
    .expect("sent");
    let event = app.eval(&find).await.expect("its event id");
    let random_id: String = app
        .eval("return [...document.querySelectorAll('.room')].find(r => r.querySelector('.room-name')?.textContent.trim() === 'random').dataset.room;")
        .await
        .expect("room id")
        .as_str()
        .expect("string")
        .to_owned();

    app.open_room("general").await.expect("open general");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
    let links = [
        format!("https://matrix.to/#/@bob:localhost?n={id}"),
        format!("https://matrix.to/#/#random:localhost?n={id}"),
        format!(
            "https://matrix.to/#/{}/{}?via=localhost",
            random_id.replace('!', "%21"),
            event.as_str().expect("event id").replace('$', "%24")
        ),
        format!("https://matrix.to/#/%23{alias}:localhost"),
    ];
    for link in &links {
        send(&mut app, link).await;
    }
    // Click the last link to `link` in the room, once it is there.

    // A person: their profile.
    click_link(&mut app, &links[0]).await;
    app.wait_for_text(".profile-modal .profile-id", "@bob:localhost", DEFAULT_WAIT)
        .await
        .expect("bob's profile");
    app.press("body", "Escape").await.expect("close");

    // A room you are in, by its alias: it opens.
    click_link(&mut app, &links[1]).await;
    app.wait_for_text(".chat-header h2", "random", DEFAULT_WAIT)
        .await
        .expect("#random open");

    // A message: its room opens at it.
    app.open_room("general").await.expect("back to general");
    click_link(&mut app, &links[2]).await;
    app.wait_for_text(".chat-header h2", "random", DEFAULT_WAIT)
        .await
        .expect("#random open");
    app.wait_for(
        &format!(
            ".message.jump-target[data-event-id=\"{}\"]",
            event.as_str().expect("id")
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("at the message, highlighted");

    // A room you are not in: what it is, and Join.
    app.open_room("general").await.expect("back to general");
    click_link(&mut app, &links[3]).await;
    app.wait_for_text(".room-preview", &format!("Linked {id}"), DEFAULT_WAIT)
        .await
        .expect("the room's name");
    app.wait_for_text(".room-preview", "come on in", DEFAULT_WAIT)
        .await
        .expect("its topic");
    app.click_text(".room-preview button", "Join")
        .await
        .expect("join");
    app.wait_for_text(".chat-header h2", &format!("Linked {id}"), DEFAULT_WAIT)
        .await
        .expect("joined and open");

    // None of them went to the browser.
    let handed = app
        .eval("return [...document.querySelectorAll('.link-log li')].map(li => li.textContent).filter(t => t.includes('matrix.to')).length;")
        .await
        .expect("log");
    assert_eq!(handed, 0, "matrix.to links stay in the app");
}

/// Click the newest link to `link` in the room, waiting for it to arrive.
async fn click_link(app: &mut App, link: &str) {
    let find = format!(
        "const a = [...document.querySelectorAll('.message .body a')].reverse().find(a => a.href === {});",
        serde_json::to_string(link).expect("json")
    );
    app.wait_for_js(&format!("{find} return !!a;"), DEFAULT_WAIT)
        .await
        .unwrap_or_else(|err| panic!("{link} is in the room: {err}"));
    app.eval(&format!("{find} a.click(); return true;"))
        .await
        .expect("click");
}

/// Links from outside the app (a `matrix:` link the desktop hands over, or a
/// matrix.to link opened with Morpheus) open in the running app, which comes
/// forward: a room by its alias, a person's profile.
#[tokio::test]
async fn links_from_outside_open_in_the_running_app() {
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
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    app.open_room("general").await.expect("open general");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");

    app.open_link_from_outside("https://matrix.to/#/%23random:localhost")
        .expect("hand over a matrix.to link");
    app.wait_for_text(".chat-header h2", "random", DEFAULT_WAIT)
        .await
        .expect("#random open");

    app.open_link_from_outside("matrix:u/bob:localhost")
        .expect("hand over a matrix: link");
    app.wait_for_text(".profile-modal .profile-id", "@bob:localhost", DEFAULT_WAIT)
        .await
        .expect("bob's profile");
}
