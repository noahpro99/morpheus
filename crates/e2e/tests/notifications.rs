//! Desktop notifications: a message from someone else is announced (the app
//! mirrors what it announced into a hidden log), mentions are flagged, and
//! the inbox lists recent mentions.

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
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("room listed");
}

async fn send(app: &mut App, text: &str) {
    app.type_into(".composer .composer-input", text)
        .await
        .expect("type");
    app.press(".composer .composer-input", "Enter")
        .await
        .expect("send");
    app.wait_for_exact_text(".message.own:not(.pending) .body", text, DEFAULT_WAIT)
        .await
        .expect("message sent");
}

#[tokio::test]
async fn messages_from_others_are_announced_and_mentions_flagged() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut bob = App::launch_desktop(&hs).await.expect("launch bob");
    login(&mut bob, "bob").await;
    bob.open_room("random").await.expect("bob reads #random");

    let mut alice = App::launch(&hs).await.expect("launch alice");
    login(&mut alice, "alice").await;
    alice.open_room("general").await.expect("alice in #general");
    alice
        .wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");

    let pid = std::process::id();
    let plain = format!("notify ping {pid}");
    send(&mut alice, &plain).await;
    bob.wait_for_text(".notification-log li", &plain, DEFAULT_WAIT)
        .await
        .expect("announced");
    let flagged = bob
        .eval(&format!(
            "return [...document.querySelectorAll('.notification-log li')].find(li => li.textContent.includes({})).dataset.mention",
            serde_json::to_string(&plain).expect("json")
        ))
        .await
        .expect("flag");
    assert_eq!(
        flagged.as_str(),
        Some("false"),
        "a plain message is not a mention"
    );
    bob.wait_for_text(".notification-log li", "alice: ", DEFAULT_WAIT)
        .await
        .expect("sender named");

    let mention = format!("@bob look here {pid}");
    send(&mut alice, &mention).await;
    bob.wait_for_js(
        &format!(
            "return [...document.querySelectorAll('.notification-log li')].some(li => li.textContent.includes({}) && li.dataset.mention === 'true')",
            serde_json::to_string(&mention).expect("json")
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("mention flagged");

    // The inbox lists the mention and opens the room.
    bob.click(".titlebar .inbox-button")
        .await
        .expect("open inbox");
    bob.wait_for_text(".inbox-mention", &mention, DEFAULT_WAIT)
        .await
        .expect("mention in inbox");
    bob.click_text(".inbox-mention", &mention)
        .await
        .expect("open mention");
    bob.wait_for_text(".chat-header h2", "general", DEFAULT_WAIT)
        .await
        .expect("jumped to #general");
    assert_eq!(
        bob.count(".inbox-panel").await.expect("count"),
        0,
        "inbox closed"
    );
}

/// Notifications can be turned off on one device (Settings → Notifications):
/// the setting is kept on the device, and the system notifier stays quiet
/// while it is off.
#[tokio::test]
async fn notifications_can_be_turned_off_on_this_device() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut bob = App::launch_desktop(&hs).await.expect("launch bob");
    login(&mut bob, "bob").await;
    bob.open_room("random").await.expect("bob reads #random");
    let off = bob.data_dir().join("notifications_off");

    bob.click(".settings-button").await.expect("gear");
    bob.wait_for_text(".settings-nav button", "Notifications", DEFAULT_WAIT)
        .await
        .expect("settings");
    bob.click_text(".settings-nav button", "Notifications")
        .await
        .expect("notifications page");
    bob.wait_for(
        ".settings-page input[name=device-notifications]:checked",
        DEFAULT_WAIT,
    )
    .await
    .expect("on by default");
    bob.click(".settings-page input[name=device-notifications]")
        .await
        .expect("turn off");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while !off.exists() {
        assert!(std::time::Instant::now() < deadline, "setting not kept");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    bob.press("body", "Escape").await.expect("close settings");

    // A message for bob is not shown by the system notifier now.
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login(&mut alice, "alice").await;
    alice.open_room("general").await.expect("alice in #general");
    alice
        .wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
    let text = format!("quiet please {}", std::process::id());
    send(&mut alice, &text).await;
    let log = bob.data_dir().join("app.log");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        if text.contains("notifications are off on this device") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "notifier not held back"
        );
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }

    // And back on.
    bob.click(".settings-button").await.expect("gear");
    bob.wait_for_text(".settings-nav button", "Notifications", DEFAULT_WAIT)
        .await
        .expect("settings");
    bob.click_text(".settings-nav button", "Notifications")
        .await
        .expect("notifications page");
    bob.click(".settings-page input[name=device-notifications]")
        .await
        .expect("turn on");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while off.exists() {
        assert!(std::time::Instant::now() < deadline, "setting not cleared");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

/// What the desktop app would have shown: under test it writes each
/// notification to `shown-notifications.jsonl` instead of raising it.
fn shown(app: &App) -> Vec<serde_json::Value> {
    std::fs::read_to_string(app.data_dir().join("shown-notifications.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn is_image(path: &serde_json::Value) -> bool {
    path.as_str()
        .and_then(|path| std::fs::read(path).ok())
        .is_some_and(|bytes| {
            bytes.starts_with(&[0x89, b'P', b'N', b'G']) || bytes.starts_with(&[0xFF, 0xD8, 0xFF])
        })
}

const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
    0x44, 0xAE, 0x42, 0x60, 0x82,
];

/// A notification carries the sender's picture, as Discord's do, and a
/// photo sent shows in it too.
#[tokio::test]
async fn notifications_show_the_senders_picture_and_the_photo() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut bob = App::launch_desktop(&hs).await.expect("launch bob");
    login(&mut bob, "bob").await;
    bob.open_room("random").await.expect("bob reads #random");

    let mut alice = App::launch(&hs).await.expect("launch alice");
    login(&mut alice, "alice").await;
    alice.open_room("general").await.expect("alice in #general");
    alice
        .wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");

    let pid = std::process::id();
    let text = format!("picture ping {pid}");
    send(&mut alice, &text).await;
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    let notice = loop {
        if let Some(notice) = shown(&bob)
            .into_iter()
            .find(|n| n["body"].as_str().is_some_and(|b| b.contains(&text)))
        {
            break notice;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "not shown: {:?}",
            shown(&bob)
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    };
    assert!(
        is_image(&notice["icon"]),
        "alice's picture as the icon: {notice}"
    );

    // A photo: the picture itself in the notification.
    let photo = format!("photo-{pid}.png");
    let bytes: Vec<String> = TINY_PNG.iter().map(|b| b.to_string()).collect();
    alice
        .eval(&format!(
            "const dt = new DataTransfer(); dt.items.add(new File([new Uint8Array([{}])], {}, {{ type: 'image/png' }})); \
             const target = document.querySelector('.chat'); \
             target.dispatchEvent(new DragEvent('dragover', {{ bubbles: true, cancelable: true, dataTransfer: dt }})); \
             target.dispatchEvent(new DragEvent('drop', {{ bubbles: true, cancelable: true, dataTransfer: dt }}));",
            bytes.join(","),
            serde_json::to_string(&photo).expect("json")
        ))
        .await
        .expect("drop a photo");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    let notice = loop {
        if let Some(notice) = shown(&bob)
            .into_iter()
            .find(|n| n["body"].as_str().is_some_and(|b| b.contains(&photo)))
        {
            break notice;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "photo not shown: {:?}",
            shown(&bob)
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    };
    assert!(is_image(&notice["icon"]), "alice's picture: {notice}");
    assert!(is_image(&notice["image"]), "the photo: {notice}");
}

/// A fresh pair in a room of their own (just the two of them): `reader` is
/// signed in on a desktop app, and `writer` posts through the API.
async fn pair(hs: &str, name: &str) -> (App, String, String, String) {
    let reader = new_account(hs, &format!("{name}r")).expect("reader");
    let writer = new_account(hs, &format!("{name}w")).expect("writer");
    let room = api_as(
        hs,
        &writer,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": format!("{name} chat") })),
    )
    .expect("a room");
    let room = room["room_id"].as_str().expect("room id").to_owned();
    let id = room.replace('!', "%21").replace(':', "%3A");
    api_as(
        hs,
        &writer,
        "POST",
        &format!("rooms/{id}/invite"),
        Some(serde_json::json!({ "user_id": format!("@{reader}:localhost") })),
    )
    .expect("invite");
    api_as(
        hs,
        &reader,
        "POST",
        &format!("rooms/{id}/join"),
        Some(serde_json::json!({})),
    )
    .expect("reader joins");
    let mut app = App::launch_desktop(hs).await.expect("launch");
    login(&mut app, &reader).await;
    (app, reader, writer, room)
}

/// `writer` says `text` in `room`; what the reader's app showed for it (the
/// first notification after the `before` already shown).
async fn shown_for(app: &App, hs: &str, writer: &str, room: &str, text: &str) -> serde_json::Value {
    let before = shown(app).len();
    let room = room.replace('!', "%21").replace(':', "%3A");
    api_as(
        hs,
        writer,
        "PUT",
        &format!("rooms/{room}/send/m.room.message/{}", text.len()),
        Some(serde_json::json!({ "msgtype": "m.text", "body": text })),
    )
    .expect("send");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        if let Some(notice) = shown(app).into_iter().nth(before) {
            return notice;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "nothing shown for {text:?}: {:?}",
            shown(app)
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

/// A room of just two people reads like a DM: the notification is titled
/// with the other person's name alone, not "name in room".
#[tokio::test]
async fn a_two_person_room_notifies_like_a_dm() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (app, _reader, writer, room) = pair(&hs, "duo").await;
    let text = format!("just us {}", std::process::id());
    let notice = shown_for(&app, &hs, &writer, &room, &text).await;
    assert_eq!(notice["title"].as_str(), Some(writer.as_str()), "{notice}");
    assert_eq!(notice["body"].as_str(), Some(text.as_str()), "{notice}");
}

/// Settings → Notifications chooses what a notification gives away: the
/// message (the default), only who sent it, or nothing ("Someone sent a
/// message"), kept on this device.
#[tokio::test]
async fn notifications_can_hide_the_message_or_the_sender() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (mut app, _reader, writer, room) = pair(&hs, "hush").await;
    let choose = async |app: &mut App, value: &str| {
        app.click(".settings-button").await.expect("gear");
        app.wait_for_text(".settings-nav button", "Notifications", DEFAULT_WAIT)
            .await
            .expect("settings");
        app.click_text(".settings-nav button", "Notifications")
            .await
            .expect("notifications page");
        let option = format!(".settings-page input[name=notification-content][value={value}]");
        app.wait_for(&option, DEFAULT_WAIT)
            .await
            .expect("the choice");
        app.click(&option).await.expect("choose");
        app.wait_for(&format!("{option}:checked"), DEFAULT_WAIT)
            .await
            .expect("chosen");
        app.press("body", "Escape").await.expect("close settings");
    };
    let pid = std::process::id();

    choose(&mut app, "sender").await;
    let text = format!("secret plans {pid}");
    let notice = shown_for(&app, &hs, &writer, &room, &text).await;
    assert_eq!(notice["title"].as_str(), Some(writer.as_str()), "{notice}");
    assert_eq!(notice["body"].as_str(), Some("Sent a message"), "{notice}");

    choose(&mut app, "hidden").await;
    let text = format!("more secret plans {pid}");
    let notice = shown_for(&app, &hs, &writer, &room, &text).await;
    assert_eq!(notice["title"].as_str(), Some("Morpheus"), "{notice}");
    assert_eq!(
        notice["body"].as_str(),
        Some("Someone sent a message"),
        "{notice}"
    );
    assert!(
        notice["icon"].is_null() && notice["image"].is_null(),
        "no pictures when hidden: {notice}"
    );

    choose(&mut app, "full").await;
    let text = format!("open plans {pid}");
    let notice = shown_for(&app, &hs, &writer, &room, &text).await;
    assert_eq!(notice["body"].as_str(), Some(text.as_str()), "{notice}");
}
