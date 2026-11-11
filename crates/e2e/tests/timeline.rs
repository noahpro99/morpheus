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
