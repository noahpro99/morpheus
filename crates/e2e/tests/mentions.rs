//! Mentions in a text channel, as Discord shows them: a person's or room's
//! link is a tinted pill that opens the profile or the room, and a message
//! that mentions you lights its whole row with a gold bar at the left.

#![allow(clippy::expect_used)]

mod common;

use morpheus_e2e::{App, DEFAULT_WAIT, api_as, new_account};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login(app: &mut App, user: &str) {
    app.wait_for("form.login", DEFAULT_WAIT).await.expect("login form");
    app.type_into("form.login input[name=username]", user).await.expect("username");
    app.type_into("form.login input[name=password]", &format!("{user}-password"))
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("room listed");
}

fn enc(id: &str) -> String {
    id.replace('!', "%21").replace(':', "%3A")
}

/// A room "<name> room" of a writer with `readers` in it; returns the id.
fn room_of(hs: &str, writer: &str, readers: &[&str], name: &str) -> String {
    let room = api_as(
        hs,
        writer,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": format!("{name} room") })),
    )
    .expect("a room");
    let room = room["room_id"].as_str().expect("room id").to_owned();
    for reader in readers {
        api_as(
            hs,
            writer,
            "POST",
            &format!("rooms/{}/invite", enc(&room)),
            Some(serde_json::json!({ "user_id": format!("@{reader}:localhost") })),
        )
        .expect("invite");
        api_as(hs, reader, "POST", &format!("rooms/{}/join", enc(&room)), Some(serde_json::json!({})))
            .expect("join");
    }
    room
}

/// `writer` says `who` is mentioned (a pill, and `m.mentions`), and a
/// link to #random.
fn mention(hs: &str, writer: &str, room: &str, who: &str, word: &str) {
    let body = format!("hello {who} {word}");
    api_as(
        hs,
        writer,
        "PUT",
        &format!("rooms/{}/send/m.room.message/{}-{word}", enc(room), std::process::id()),
        Some(serde_json::json!({
            "msgtype": "m.text",
            "body": body,
            "format": "org.matrix.custom.html",
            "formatted_body": format!(
                "hello <a href=\"https://matrix.to/#/@{who}:localhost\">{who}</a> {word} \
                 <a href=\"https://matrix.to/#/#random:localhost\">#random</a>"
            ),
            "m.mentions": { "user_ids": [format!("@{who}:localhost")] },
        })),
    )
    .expect("send");
}

/// A mention is a pill: a tinted background that is not the text's colour,
/// stronger under the pointer, and a click opens the person's card.
#[tokio::test]
async fn a_mention_is_a_clickable_tinted_pill() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let reader = new_account(&hs, "pillr").expect("reader");
    let writer = new_account(&hs, "pillw").expect("writer");
    let room = room_of(&hs, &writer, &[&reader], "pill");
    let mut app = App::launch_desktop(&hs).await.expect("launch");
    login(&mut app, &reader).await;
    app.open_room("pill room").await.expect("open");
    mention(&hs, &writer, &room, &writer, "pillword");
    let pill = r##".message a[href^="https://matrix.to/#/@"]"##;
    app.wait_for(pill, DEFAULT_WAIT).await.expect("the pill");
    assert_eq!(app.attr(pill, "data-pill").await.expect("attr").as_deref(), Some("true"));
    let bg = app.attr(pill, "data-bg").await.expect("bg").expect("a background");
    let colour = app.attr(pill, "data-color").await.expect("colour").expect("a text colour");
    assert!(bg.len() == 8 && !bg.ends_with("ff"), "a translucent background {bg} behind text {colour}");
    // The tint is about 30% (alpha 0x4d), stronger when hovered.
    assert!(bg.ends_with("4d"), "tint {bg}");
    app.hover(pill).await.expect("hover");
    let hovered = app.attr(pill, "data-bg").await.expect("bg").expect("bg");
    assert_ne!(hovered, bg, "a stronger tint under the pointer");
    // Where the session can be captured: the tint is painted behind the
    // text, with a little padding at its left, and not beyond.
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    if let Some(shot) = common::capture(&app) {
        let b = app.bounds(pill).await.expect("b").expect("pill bounds");
        let y = b.y + 2.0;
        let inside = common::pixel_at(&shot, b.x - 1.0, y);
        let beside = common::pixel_at(&shot, b.x - 12.0, y);
        let within = common::pixel_at(&shot, b.x + b.w / 2.0, y);
        assert_ne!(inside, beside, "the tint is not painted behind the pill");
        assert!(inside.2 > beside.2 + 10, "tinted towards the accent: {inside:?} vs {beside:?}");
        assert_eq!(inside, within, "one even tint along the pill");
    }
    // A link to a room is one too.
    let room_pill = r##".message a[href^="https://matrix.to/#/#random"]"##;
    assert_eq!(app.attr(room_pill, "data-pill").await.expect("attr").as_deref(), Some("true"));
    app.click(pill).await.expect("click");
    app.wait_for_text(".profile-modal .profile-id", &format!("@{writer}:localhost"), DEFAULT_WAIT)
        .await
        .expect("the profile");
}

/// A message that mentions you lights its row; one that mentions someone
/// else does not.
#[tokio::test]
async fn a_message_that_mentions_you_is_lit() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let alice = new_account(&hs, "lita").expect("alice");
    let carol = new_account(&hs, "litc").expect("carol");
    let writer = new_account(&hs, "litw").expect("writer");
    let room = room_of(&hs, &writer, &[&alice, &carol], "lit");
    let mut a = App::launch_desktop(&hs).await.expect("launch alice");
    login(&mut a, &alice).await;
    let mut c = App::launch_desktop(&hs).await.expect("launch carol");
    login(&mut c, &carol).await;
    a.open_room("lit room").await.expect("open");
    c.open_room("lit room").await.expect("open");
    mention(&hs, &writer, &room, &alice, "litword");
    for app in [&mut a, &mut c] {
        app.wait_for_text(".message .body", "litword", DEFAULT_WAIT)
            .await
            .expect("the message");
    }
    a.wait_for(".message[data-mentioned=true]", DEFAULT_WAIT)
        .await
        .expect("alice's row is lit");
    let row = a.bounds(".message[data-mentioned=true]").await.expect("b").expect("row bounds");
    let bar = a
        .bounds(".message[data-mentioned=true] .mention-bar")
        .await
        .expect("b")
        .expect("the gold bar");
    assert!((bar.x - row.x).abs() < 1.0, "the bar at the row's left edge: {bar:?} {row:?}");
    assert!((bar.w - 2.0).abs() < 0.5, "2 px wide: {bar:?}");
    assert!((bar.h - row.h).abs() < 1.0, "the row's height: {bar:?} {row:?}");
    let lit = a.attr(".message[data-mentioned=true]", "data-bg").await.expect("bg").expect("bg");
    assert!(lit.starts_with("faa81a"), "a warm yellow tint: {lit}");
    // Carol is not mentioned.
    assert_eq!(c.count(".message[data-mentioned=true]").await.expect("count"), 0);
    assert_eq!(c.count(".mention-bar").await.expect("count"), 0);
}
