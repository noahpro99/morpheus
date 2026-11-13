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
        .attr_containing(".notification-log li", &plain, "data-mention")
        .await
        .expect("flag");
    assert_eq!(
        flagged.as_deref(),
        Some("false"),
        "a plain message is not a mention"
    );
    bob.wait_for_text(".notification-log li", "alice: ", DEFAULT_WAIT)
        .await
        .expect("sender named");

    let mention = format!("@bob look here {pid}");
    send(&mut alice, &mention).await;
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let flag = bob
            .attr_containing(".notification-log li", &mention, "data-mention")
            .await
            .expect("flag");
        if flag.as_deref() == Some("true") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "mention flagged (saw {flag:?})"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

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
    alice
        .attach_file(true, &photo, "image/png", TINY_PNG)
        .await
        .expect("drop a photo");
    // It waits on the message being written, and goes with it.
    alice
        .wait_for(".composer .composer-attachment", DEFAULT_WAIT)
        .await
        .expect("attached");
    alice
        .press(".composer .composer-input", "Enter")
        .await
        .expect("send");
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

// ---- message sounds ---------------------------------------------------------

/// `writer` says `text` in `room` through the API.
fn say(hs: &str, writer: &str, room: &str, text: &str) {
    let room = room.replace('!', "%21").replace(':', "%3A");
    api_as(
        hs,
        writer,
        "PUT",
        &format!(
            "rooms/{room}/send/m.room.message/{}-{}",
            std::process::id(),
            text.replace(' ', "_")
        ),
        Some(serde_json::json!({ "msgtype": "m.text", "body": text })),
    )
    .expect("send");
}

/// A second room of `writer`'s with `reader` in it.
fn second_room(hs: &str, writer: &str, reader: &str, name: &str) -> String {
    let room = api_as(
        hs,
        writer,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": name })),
    )
    .expect("a room");
    let room = room["room_id"].as_str().expect("room id").to_owned();
    let id = room.replace('!', "%21").replace(':', "%3A");
    api_as(
        hs,
        writer,
        "POST",
        &format!("rooms/{id}/invite"),
        Some(serde_json::json!({ "user_id": format!("@{reader}:localhost") })),
    )
    .expect("invite");
    api_as(hs, reader, "POST", &format!("rooms/{id}/join"), Some(serde_json::json!({})))
        .expect("reader joins");
    room
}

/// How many message sounds `app` has played.
async fn message_sounds(app: &mut App) -> usize {
    app.texts(".sound-log li")
        .await
        .expect("sounds")
        .iter()
        .filter(|s| *s == "message")
        .count()
}

async fn wait_message_sounds(app: &mut App, want: usize, what: &str) {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let have = message_sounds(app).await;
        if have == want {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{what}: {have} message sounds, wanted {want}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

/// A message that notifies you makes one short sound, even for a burst; the
/// sound is told apart from the call chimes.
#[tokio::test]
async fn a_new_message_plays_one_sound_for_a_burst() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (mut app, _reader, writer, room) = pair(&hs, "chime").await;
    assert_eq!(message_sounds(&mut app).await, 0);
    for n in 0..3 {
        say(&hs, &writer, &room, &format!("burst {n}"));
    }
    wait_message_sounds(&mut app, 1, "a burst").await;
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    assert_eq!(message_sounds(&mut app).await, 1, "one for the burst");
    // Later, another.
    say(&hs, &writer, &room, "and later");
    wait_message_sounds(&mut app, 2, "a later message").await;
}

/// No sound for your own messages, nor for a muted room.
#[tokio::test]
async fn own_messages_and_muted_rooms_make_no_sound() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (mut app, reader, writer, room) = pair(&hs, "hushed").await;
    let muted = second_room(&hs, &writer, &reader, "muted room");
    let rule = format!("pushrules/global/override/{}", muted.replace('!', "%21").replace(':', "%3A"));
    api_as(
        &hs,
        &reader,
        "PUT",
        &rule,
        Some(serde_json::json!({
            "conditions": [{ "kind": "event_match", "key": "room_id", "pattern": muted }],
            "actions": [],
        })),
    )
    .expect("mute the room");
    say(&hs, &writer, &muted, "into the muted room");
    // The app's own message.
    app.open_room("hushed chat").await.ok();
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
    send(&mut app, "my own words").await;
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    assert_eq!(message_sounds(&mut app).await, 0, "no sound yet");
    // A message that does notify, as proof the others were seen and skipped.
    say(&hs, &writer, &room, "this one counts");
    wait_message_sounds(&mut app, 1, "the one that notifies").await;
}

/// Settings > Notifications > "Message sounds" turns them off, kept on the
/// device.
#[tokio::test]
async fn message_sounds_can_be_turned_off() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (mut app, _reader, writer, room) = pair(&hs, "toggle").await;
    let off = app.data_dir().join("message_sounds_off");
    app.click(".settings-button").await.expect("gear");
    app.wait_for_text(".settings-nav button", "Notifications", DEFAULT_WAIT)
        .await
        .expect("settings");
    app.click_text(".settings-nav button", "Notifications")
        .await
        .expect("notifications page");
    app.wait_for(".settings-page input[name=message-sounds]:checked", DEFAULT_WAIT)
        .await
        .expect("on by default");
    app.click(".settings-page input[name=message-sounds]")
        .await
        .expect("turn off");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while !off.exists() {
        assert!(std::time::Instant::now() < deadline, "setting not kept");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    app.press("body", "Escape").await.expect("close settings");
    say(&hs, &writer, &room, "silent one");
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    assert_eq!(message_sounds(&mut app).await, 0, "off means off");

    app.click(".settings-button").await.expect("gear");
    app.wait_for_text(".settings-nav button", "Notifications", DEFAULT_WAIT)
        .await
        .expect("settings");
    app.click_text(".settings-nav button", "Notifications")
        .await
        .expect("notifications page");
    app.click(".settings-page input[name=message-sounds]")
        .await
        .expect("turn on");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while off.exists() {
        assert!(std::time::Instant::now() < deadline, "setting not cleared");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    app.press("body", "Escape").await.expect("close settings");
    say(&hs, &writer, &room, "audible one");
    wait_message_sounds(&mut app, 1, "back on").await;
}

/// Seeing a room clears it from the inbox and its badges at once, not after
/// the server has answered the read receipt.
#[tokio::test]
async fn viewing_a_room_clears_its_inbox_entries_at_once() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (mut app, reader, writer, room) = pair(&hs, "seen").await;
    say(&hs, &writer, &room, &format!("@{reader} are you there"));
    app.wait_for(".inbox-badge", DEFAULT_WAIT)
        .await
        .expect("inbox badge");
    app.wait_for(".room .badge", DEFAULT_WAIT)
        .await
        .expect("the room's mention badge");
    app.click(".titlebar .inbox-button").await.expect("open inbox");
    app.wait_for(".inbox-mention", DEFAULT_WAIT)
        .await
        .expect("a mention listed");
    app.press("body", "Escape").await.expect("close inbox");
    app.wait_for_count(".inbox-panel", 0, DEFAULT_WAIT)
        .await
        .expect("inbox closed");

    app.open_room("seen chat").await.expect("open the room");
    let opened = std::time::Instant::now();
    let mut gone = None;
    while opened.elapsed() < std::time::Duration::from_millis(1500) {
        if app.count(".inbox-badge").await.expect("badge") == 0
            && app.count(".room .badge").await.expect("room badge") == 0
        {
            gone = Some(opened.elapsed());
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let gone = gone.expect("the badges never went");
    assert!(
        gone < std::time::Duration::from_millis(300),
        "the badges went after {gone:?}, not at once"
    );
    app.click(".titlebar .inbox-button").await.expect("open inbox");
    app.wait_for(".inbox-panel", DEFAULT_WAIT).await.expect("inbox");
    assert_eq!(
        app.count(".inbox-panel .inbox-item").await.expect("items"),
        0,
        "nothing left in the inbox"
    );
}
