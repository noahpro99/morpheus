//! What a room's timeline shows, with every event made through the client
//! API (no message box needed): who is named once per run of messages, day
//! dividers, state lines, formatted bodies and their links, edits, reactions,
//! replies, pictures. Runs on the Dioxus and the GPUI app alike.

#![allow(clippy::expect_used)]

mod common;

use morpheus_e2e::{App, DEFAULT_WAIT, api_as, new_account, send_image_as};
use serde_json::{Value, json};

const DUSK_PNG: &[u8] = include_bytes!("../fixtures/dusk.png");

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

/// `!room:server` as it goes in a request path.
fn path_id(id: &str) -> String {
    id.replace('!', "%21").replace(':', "%3A").replace('$', "%24")
}

/// Send `content` into `room` as `user`; the new event's id.
fn send(hs: &str, user: &str, room: &str, kind: &str, content: Value) -> String {
    static TXN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let txn = TXN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let sent = api_as(
        hs,
        user,
        "PUT",
        &format!(
            "rooms/{}/send/{kind}/t{}x{txn}",
            path_id(room),
            std::process::id()
        ),
        Some(content),
    )
    .expect("send");
    sent["event_id"]
        .as_str()
        .unwrap_or_else(|| panic!("no event id in {sent}"))
        .to_owned()
}

fn text(body: &str) -> Value {
    json!({ "msgtype": "m.text", "body": body })
}

/// A public room made by `owner`, `guest` joined.
fn room_for(hs: &str, owner: &str, guest: &str) -> String {
    let made = api_as(
        hs,
        owner,
        "POST",
        "createRoom",
        Some(json!({ "name": "timeline room", "preset": "public_chat" })),
    )
    .expect("a room");
    let room = made["room_id"].as_str().expect("room id").to_owned();
    api_as(
        hs,
        guest,
        "POST",
        &format!("join/{}", path_id(&room)),
        Some(json!({})),
    )
    .expect("join");
    room
}

async fn open(hs: &str, user: &str) -> App {
    let mut app = App::launch(hs).await.expect("launch");
    common::login(&mut app, user, &format!("{user}-password")).await;
    app.open_room("timeline room").await.expect("open the room");
    app
}

#[tokio::test]
async fn names_once_per_run_dividers_state_lines_edits_reactions_and_replies() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (ann, ben) = (
        new_account(&hs, "tlann").expect("account"),
        new_account(&hs, "tlben").expect("account"),
    );
    let room = room_for(&hs, &ann, &ben);
    let one = send(&hs, &ann, &room, "m.room.message", text("first one"));
    send(&hs, &ann, &room, "m.room.message", text("second one"));
    let three = send(&hs, &ben, &room, "m.room.message", text("third one"));
    let reply = send(
        &hs,
        &ben,
        &room,
        "m.room.message",
        json!({ "msgtype": "m.text", "body": "a reply", "m.relates_to": { "m.in_reply_to": { "event_id": one } } }),
    );
    send(
        &hs,
        &ann,
        &room,
        "m.room.message",
        json!({ "msgtype": "m.text", "body": "last one", "m.relates_to": {} }),
    );
    // An edit of the first, and a reaction to the third.
    send(
        &hs,
        &ann,
        &room,
        "m.room.message",
        json!({
            "msgtype": "m.text",
            "body": "* first one, fixed",
            "m.new_content": { "msgtype": "m.text", "body": "first one, fixed" },
            "m.relates_to": { "rel_type": "m.replace", "event_id": one },
        }),
    );
    send(
        &hs,
        &ann,
        &room,
        "m.reaction",
        json!({ "m.relates_to": { "rel_type": "m.annotation", "event_id": three, "key": "👍" } }),
    );
    let _ = reply;

    let mut app = open(&hs, &ann).await;
    app.wait_for_text(".message .body", "last one", DEFAULT_WAIT)
        .await
        .expect("the newest message");

    // Named once per run: ann, ben (two messages), ann.
    app.wait_for_text(".message .body", "first one, fixed (edited)", DEFAULT_WAIT)
        .await
        .expect("the edit, with its marker");
    let senders = app
        .texts(".message.with-header .sender")
        .await
        .expect("senders");
    assert_eq!(senders, [ann.clone(), ben.clone(), ann.clone()]);
    let all = app.count(".message").await.expect("count");
    let headed = app.count(".message.with-header").await.expect("count");
    let system = app.count(".message.system").await.expect("count");
    assert_eq!(
        all - headed - system,
        2,
        "second one and the reply share their sender's header"
    );
    app.wait_for(".message.with-header .time", DEFAULT_WAIT)
        .await
        .expect("a time on each header");

    // Day divider, and the joining as a line.
    assert!(app.count(".date-divider").await.expect("count") >= 1);
    app.wait_for_text(".message.system .system-text", "joined the room", DEFAULT_WAIT)
        .await
        .expect("a state line");

    // Reaction and reply.
    app.wait_for_text(".message .reaction .reaction-key", "👍", DEFAULT_WAIT)
        .await
        .expect("a reaction");
    assert_eq!(
        app.texts(".message .reaction .reaction-count")
            .await
            .expect("counts"),
        ["1"]
    );
    app.wait_for_text(".message .reply-quote", "first one", DEFAULT_WAIT)
        .await
        .expect("the reply quotes its original");
    assert_eq!(
        app.texts(".message .reply-quote b").await.expect("who"),
        [ann.clone()]
    );
}

#[tokio::test]
async fn formatted_bodies_and_their_links() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (ann, ben) = (
        new_account(&hs, "tlfann").expect("account"),
        new_account(&hs, "tlfben").expect("account"),
    );
    let room = room_for(&hs, &ann, &ben);
    send(
        &hs,
        &ben,
        &room,
        "m.room.message",
        json!({
            "msgtype": "m.text",
            "body": "bold and code, see here",
            "format": "org.matrix.custom.html",
            "formatted_body": "<p><strong>bold</strong> and <code>code</code>, see <a href=\"https://example.org/formatted\">here</a></p><blockquote>quoted</blockquote><ul><li>one</li><li>two</li></ul>",
        }),
    );
    send(
        &hs,
        &ben,
        &room,
        "m.room.message",
        text("plain https://example.org/plain."),
    );

    let mut app = open(&hs, &ann).await;
    app.wait_for_text(".message .body.rich", "bold and code, see here", DEFAULT_WAIT)
        .await
        .expect("the formatted body, as text");
    let rich = app.texts(".message .body.rich").await.expect("rich");
    assert!(rich[0].contains("quoted") && rich[0].contains("one"), "{rich:?}");
    app.wait_for_exact_text(
        ".message .body.rich a[href=\"https://example.org/formatted\"]",
        "here",
        DEFAULT_WAIT,
    )
    .await
    .expect("a link in the formatted body");
    let plain = ".message .body a[href=\"https://example.org/plain\"]";
    app.wait_for_exact_text(plain, "https://example.org/plain", DEFAULT_WAIT)
        .await
        .expect("a bare URL is a link, without its full stop");

    app.click(plain).await.expect("click the link");
    app.wait_for_text(".link-log li", "https://example.org/plain", DEFAULT_WAIT)
        .await
        .expect("handed to the browser");
}

#[tokio::test]
async fn pictures_show_in_the_timeline() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (ann, ben) = (
        new_account(&hs, "tlpann").expect("account"),
        new_account(&hs, "tlpben").expect("account"),
    );
    let room = room_for(&hs, &ann, &ben);
    send_image_as(&hs, &ben, &room, DUSK_PNG, "dusk.png", None).expect("a picture");
    let mut app = open(&hs, &ann).await;
    app.wait_for(".message img.image[alt=\"dusk.png\"]", DEFAULT_WAIT)
        .await
        .expect("the picture, once fetched");
}

/// A message row's highlight on hover spans the timeline out to its right
/// edge, whatever the row holds, so the hover bar (reply, edit, reactions),
/// which sits at the row's top right, is at the timeline's right too.
#[tokio::test]
async fn every_message_row_spans_the_timeline_width() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (ann, ben, cat) = (
        new_account(&hs, "tlwann").expect("account"),
        new_account(&hs, "tlwben").expect("account"),
        new_account(&hs, "tlwcat").expect("account"),
    );
    let room = room_for(&hs, &ann, &ben);
    api_as(
        &hs,
        &cat,
        "POST",
        &format!("join/{}", path_id(&room)),
        Some(json!({})),
    )
    .expect("join");
    let one = send(&hs, &ann, &room, "m.room.message", text("short"));
    send(&hs, &ann, &room, "m.room.message", text("continuation"));
    send(
        &hs,
        &ben,
        &room,
        "m.room.message",
        text(&format!("longword {}", "and a long message that wraps ".repeat(30))),
    );
    send(
        &hs,
        &ann,
        &room,
        "m.room.message",
        json!({ "msgtype": "m.text", "body": "a reply", "m.relates_to": { "m.in_reply_to": { "event_id": one } } }),
    );
    send_image_as(&hs, &cat, &room, DUSK_PNG, "dusk.png", None).expect("a picture");
    let mut app = open(&hs, &ann).await;
    app.wait_for(".message img.image[alt=\"dusk.png\"]", DEFAULT_WAIT)
        .await
        .expect("the picture");

    let timeline = app
        .bounds(".timeline")
        .await
        .expect("bounds")
        .expect("the timeline");
    // Room for a scrollbar.
    let slack = 24.0;
    let rows = [
        (".message", "continuation"),
        (".message", "longword"),
        (".message", "a reply"),
        (".message", cat.as_str()),
        (".message.system", "joined"),
    ];
    for (selector, containing) in rows {
        let row = app
            .bounds_containing(selector, containing)
            .await
            .expect("bounds")
            .unwrap_or_else(|| panic!("a row with {containing:?}"));
        assert!(
            (timeline.right() - row.right()).abs() <= slack,
            "the {containing:?} row ends at {} but the timeline at {}",
            row.right(),
            timeline.right()
        );
        assert!(
            row.w >= timeline.w - slack,
            "the {containing:?} row is {} wide of {}",
            row.w,
            timeline.w
        );
        // The hover bar is at the row's top right (it is only in the page
        // while shown on GPUI; Dioxus shows it by `:hover`).
        if let Some(bar) = app
            .bounds_within(".message", containing, ".actions")
            .await
            .expect("bounds")
        {
            assert!(
                (row.right() - bar.right()).abs() <= 40.0,
                "the hover bar of the {containing:?} row ends at {} but the row at {}",
                bar.right(),
                row.right()
            );
        }
    }
}

/// State lines lead with an icon by their kind, in the avatar's column, and
/// their text starts where a message's does (task 22, item E).
#[tokio::test]
async fn state_lines_have_icons_and_line_up_with_messages() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let ann = new_account(&hs, "stlann").expect("account");
    let (ben, cat, dan) = (
        new_account(&hs, "stlben").expect("account"),
        new_account(&hs, "stlcat").expect("account"),
        new_account(&hs, "stldan").expect("account"),
    );
    let made = api_as(
        &hs,
        &ann,
        "POST",
        "createRoom",
        Some(json!({ "name": "state lines", "preset": "public_chat" })),
    )
    .expect("room");
    let room = made["room_id"].as_str().expect("room id").to_owned();
    let join = |user: &str| {
        api_as(&hs, user, "POST", &format!("join/{}", path_id(&room)), Some(json!({}))).expect("join");
    };
    join(&ben);
    join(&cat);
    join(&dan);
    api_as(
        &hs,
        &dan,
        "POST",
        &format!("rooms/{}/leave", path_id(&room)),
        Some(json!({})),
    )
    .expect("leave");
    api_as(
        &hs,
        &ann,
        "POST",
        &format!("rooms/{}/kick", path_id(&room)),
        Some(json!({ "user_id": format!("@{cat}:localhost") })),
    )
    .expect("kick");
    api_as(
        &hs,
        &ann,
        "PUT",
        &format!("rooms/{}/state/m.room.topic", path_id(&room)),
        Some(json!({ "topic": "icons" })),
    )
    .expect("topic");
    send(&hs, &ben, &room, "m.room.message", json!({ "msgtype": "m.text", "body": "a plain message" }));

    let mut app = App::launch_desktop(&hs).await.expect("launch");
    common::login(&mut app, &ann, &format!("{ann}-password")).await;
    app.open_room("state lines").await.expect("open");
    app.wait_for_text(".message .body", "a plain message", DEFAULT_WAIT)
        .await
        .expect("the message");
    for (class, glyph) in [
        ("joined", "log-in"),
        ("left", "log-out"),
        ("removed", "log-out"),
        ("other", "info"),
    ] {
        let sel = format!(".message.system .state-icon.{class}");
        app.wait_for(&sel, DEFAULT_WAIT)
            .await
            .unwrap_or_else(|e| panic!("no {class} icon: {e}"));
        assert_eq!(
            app.attr(&sel, "data-icon").await.expect("attr").as_deref(),
            Some(glyph),
            "the {class} line's icon"
        );
    }
    // The tones: green in, red out, green phone, the rest grey.
    for (class, tone) in [("joined", "good"), ("left", "bad"), ("removed", "bad"), ("other", "quiet")] {
        let sel = format!(".message.system .state-icon.{class}");
        assert_eq!(app.attr(&sel, "data-tone").await.expect("attr").as_deref(), Some(tone), "{class}'s colour");
    }
    // Text where a message's text is, the icon in the avatar's column, the time after the text.
    let body = app.bounds(".message.with-header .body").await.expect("b").expect("a body");
    let avatar = app.bounds(".message.with-header .avatar-button, .message.with-header .avatar").await.expect("b").expect("avatar");
    for text in ["joined the room", "left the room", "was kicked", "changed the topic"] {
        let line = app.bounds_containing(".message.system .system-text", text).await.expect("b").unwrap_or_else(|| panic!("the {text:?} line"));
        assert!((line.x - body.x).abs() <= 1.5, "{text:?} starts at {}, a message's text at {}", line.x, body.x);
    }
    let icon = app.bounds(".message.system .state-icon.joined").await.expect("b").expect("icon");
    assert!(icon.x >= avatar.x - 1.0 && icon.right() <= avatar.right() + 1.0, "the icon {icon:?} is not in the avatar's column {avatar:?}");
    let line = app.bounds_containing(".message.system .system-text", "joined the room").await.expect("b").expect("line");
    let time = app.bounds(".message.system .time").await.expect("b").expect("a time");
    assert!(time.x >= line.right() - 1.0, "the time {time:?} is not after the text {line:?}");
}
