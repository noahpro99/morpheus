//! A voice channel's chat: the Messages button in the call view opens the
//! channel's text chat as a rounded side panel (task 39). GPUI only.

#![allow(clippy::expect_used)]

mod common;

use morpheus_e2e::{App, Bounds, DEFAULT_WAIT, api_as};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

static NEXT_CHANNEL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// A fresh voice channel alice made and bob joined (so each test has its own).
fn fresh_voice_channel(hs: &str) -> String {
    static CLEANED: std::sync::Once = std::sync::Once::new();
    CLEANED.call_once(|| {
        for user in ["alice", "bob"] {
            morpheus_e2e::remove_rooms_named(hs, user, "vchat ");
        }
    });
    let name = format!(
        "vchat {} {}",
        std::process::id(),
        NEXT_CHANNEL.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    );
    let room = api_as(
        hs,
        "alice",
        "POST",
        "createRoom",
        Some(serde_json::json!({
            "name": name,
            "preset": "public_chat",
            "creation_content": { "type": "org.matrix.msc3417.call" },
            "power_level_content_override": { "events": { "org.matrix.msc3401.call.member": 0 } },
            "invite": ["@bob:localhost"],
        })),
    )
    .expect("a voice channel");
    let id = room["room_id"].as_str().expect("room id").replace('!', "%21").replace(':', "%3A");
    api_as(hs, "bob", "POST", &format!("rooms/{id}/join"), Some(serde_json::json!({}))).expect("bob joins");
    name
}

fn path_of(room_id: &str) -> String {
    room_id.replace('!', "%21").replace(':', "%3A")
}

async fn login(app: &mut App, user: &str) {
    common::login(app, user, &format!("{user}-password")).await;
}

/// Alice in `general`, then in the channel's call (joined in place) and on its page.
async fn join_and_open(alice: &mut App, channel: &str) -> String {
    alice.wait_for_text(".room .room-name", channel, DEFAULT_WAIT).await.expect("listed");
    let id = alice
        .attr_containing(".room.voice", channel, "data-room")
        .await
        .expect("room id")
        .expect("the channel's id");
    alice.open_room("general").await.expect("general");
    alice.click_text(".room.voice", channel).await.expect("join");
    alice.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT).await.expect("joined");
    alice.click_text(".room.voice", channel).await.expect("open the page");
    alice.wait_for(".call-stage.voice-page", DEFAULT_WAIT).await.expect("the call page");
    id
}

fn bob_says(hs: &str, room_id: &str, tag: &str, body: &str) {
    api_as(
        hs,
        "bob",
        "PUT",
        &format!("rooms/{}/send/m.room.message/{tag}-{}", path_of(room_id), std::process::id()),
        Some(serde_json::json!({ "msgtype": "m.text", "body": body })),
    )
    .expect("send");
}

async fn bounds(app: &mut App, sel: &str) -> Bounds {
    app.bounds(sel).await.expect("bounds").unwrap_or_else(|| panic!("no {sel}"))
}

fn overlaps(a: &Bounds, b: &Bounds) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

/// The button opens and closes the channel's chat as a rounded panel that
/// shows this channel's messages, takes new ones, and does not cover the call.
#[tokio::test]
async fn the_messages_button_opens_a_rounded_chat_that_works() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    if !alice.is_gpui() {
        return;
    }
    login(&mut alice, "alice").await;
    let channel = fresh_voice_channel(&hs);
    let room_id = {
        alice.wait_for_text(".room .room-name", &channel, DEFAULT_WAIT).await.expect("listed");
        alice.attr_containing(".room.voice", &channel, "data-room").await.expect("id").expect("id")
    };
    let general = alice.attr_containing(".room", "general", "data-room").await.expect("id").expect("general");
    join_and_open(&mut alice, &channel).await;

    // Closed to begin with, button not lit.
    assert_eq!(alice.count(".voice-chat").await.expect("count"), 0, "closed at first");
    assert_eq!(alice.count(".voice-chat-toggle.active").await.expect("count"), 0, "not lit");
    assert_eq!(
        alice.attr(".voice-chat-toggle", "title").await.expect("title").as_deref(),
        Some("Show the chat")
    );

    // A message while it is closed: a dot on the button (as Discord marks
    // unread), and it is not read behind the page's back.
    bob_says(&hs, &room_id, "unread", "unread while closed");
    bob_says(&hs, &general, "elsewhere", "this is general's");
    alice
        .wait_for(".voice-chat-toggle .unread-dot", DEFAULT_WAIT)
        .await
        .expect("a dot on the button for the unread message");

    // Opening: lit, titled to hide, the dot goes, the channel's messages show.
    alice.click(".voice-chat-toggle").await.expect("open");
    alice.wait_for(".voice-chat .timeline", DEFAULT_WAIT).await.expect("panel");
    alice.wait_for(".voice-chat-toggle.active", DEFAULT_WAIT).await.expect("lit");
    assert_eq!(
        alice.attr(".voice-chat-toggle", "title").await.expect("title").as_deref(),
        Some("Hide the chat")
    );
    alice
        .wait_for_text(".voice-chat .message .body", "unread while closed", DEFAULT_WAIT)
        .await
        .expect("this channel's message");
    alice
        .wait_for_count(".voice-chat-toggle .unread-dot", 0, DEFAULT_WAIT)
        .await
        .expect("the dot goes once the chat is open");
    let shown = alice.texts(".voice-chat .message .body").await.expect("texts");
    assert!(!shown.iter().any(|t| t.contains("general's")), "another room's message: {shown:?}");

    // Geometry: inside the page, clear of the call and of the button, not
    // over the cards' own buttons.
    let page = bounds(&mut alice, ".voice-channel").await;
    let panel = bounds(&mut alice, ".voice-chat").await;
    let button = bounds(&mut alice, ".voice-chat-toggle").await;
    assert!(
        panel.x >= page.x && panel.right() <= page.right() + 0.5 && panel.y >= page.y && panel.bottom() <= page.bottom() + 0.5,
        "panel {panel:?} outside the page {page:?}"
    );
    assert!((280.0..=420.0).contains(&panel.w), "panel width {}", panel.w);
    assert!(!overlaps(&button, &panel), "the button {button:?} is over the panel {panel:?}");
    let stage = bounds(&mut alice, ".call-stage").await;
    assert!(stage.right() <= panel.x + 0.5, "the call {stage:?} runs under the panel {panel:?}");
    for card in alice.all_bounds(".call-cards .call-participant, .call-cards .call-tile").await.expect("cards") {
        assert!(card.right() <= panel.x + 0.5, "a card {card:?} is under the panel {panel:?}");
    }

    // Real rounded corners (a pixel check; the panel floats, so all four).
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    if let Some(square) = common::square_corners_at(&alice, &panel, [true; 4], 1.0) {
        assert!(square.is_empty(), "the panel's corners are square: {square:?}");
    }

    // Typing and sending: Enter sends, it appears, the box empties.
    alice.click(".voice-chat .composer-input").await.expect("focus the box");
    alice.type_into(".voice-chat .composer-input", "hello from the call").await.expect("type");
    alice.press(".voice-chat .composer-input", "Enter").await.expect("send");
    alice
        .wait_for_text(".voice-chat .message.own .body", "hello from the call", DEFAULT_WAIT)
        .await
        .expect("it appears");
    assert_eq!(
        alice.value(".voice-chat .composer-input").await.expect("value").unwrap_or_default(),
        "",
        "the box empties after sending"
    );

    // Escape while typing leaves the box (the chat stays, and the draft); the
    // next Escape closes the chat.
    alice.type_into(".voice-chat .composer-input", "a draft").await.expect("type");
    alice.press(".voice-chat .composer-input", "Escape").await.expect("esc");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(alice.count(".voice-chat").await.expect("count"), 1, "Escape closed the chat");
    assert_eq!(
        alice.value(".voice-chat .composer-input").await.expect("value").unwrap_or_default(),
        "a draft",
        "Escape threw the draft away"
    );
    alice.type_into(".voice-chat .composer-input", "").await.expect("clear");
    alice.unfocus().await.expect("blur");
    alice.key_combo("body", "Escape", false, false, false).await.expect("esc");
    alice.wait_for_count(".voice-chat", 0, DEFAULT_WAIT).await.expect("the second Escape closes the chat");

    // Closing by the button: open it again, then shut it.
    alice.click(".voice-chat-toggle").await.expect("reopen");
    alice.wait_for(".voice-chat .timeline", DEFAULT_WAIT).await.expect("reopened");
    alice.click(".voice-chat-toggle").await.expect("close");
    alice.wait_for_count(".voice-chat", 0, DEFAULT_WAIT).await.expect("closed");
    assert_eq!(alice.count(".voice-chat-toggle.active").await.expect("count"), 0, "still lit");

    // With the chat closed, typing on the page lands nowhere: not in a box
    // nobody can see (it would be waiting there when the chat opens).
    alice.unfocus().await.expect("blur");
    alice.key_combo("body", "q", false, false, false).await.expect("type q");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    alice.click(".voice-chat-toggle").await.expect("open again");
    alice.wait_for(".voice-chat .composer-input", DEFAULT_WAIT).await.expect("box");
    assert_eq!(
        alice.value(".voice-chat .composer-input").await.expect("value").unwrap_or_default(),
        "",
        "a key typed while the chat was closed is waiting in its box"
    );
    alice.click(".voice-panel .hang-up").await.expect("leave");
}

/// The panel shows its messages as the main timeline does: each a readable
/// row (its text laid out, inked and inside the panel), with the sender's
/// name and picture, one under another, the newest at the bottom.
#[tokio::test]
async fn the_chats_messages_show_as_readable_rows() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    // Bob first: the later window is on top, and the pixels are alice's.
    let mut bob = App::launch_desktop(&hs).await.expect("launch bob");
    let mut alice = App::launch(&hs).await.expect("launch alice");
    if !alice.is_gpui() {
        return;
    }
    let channel = fresh_voice_channel(&hs);
    login(&mut bob, "bob").await;
    bob.wait_for_text(".room .room-name", &channel, DEFAULT_WAIT).await.expect("bob's list");
    bob.click_text(".room.voice", &channel).await.expect("bob joins");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT).await.expect("bob in the call");
    login(&mut alice, "alice").await;
    alice.wait_for_text(".room .room-name", &channel, DEFAULT_WAIT).await.expect("listed");
    let room_id = alice.attr_containing(".room.voice", &channel, "data-room").await.expect("id").expect("id");
    let said: Vec<String> = (0..6)
        .map(|n| match n {
            3 => "readable 3: a longer message that has to wrap onto more than one line in a panel this narrow".to_owned(),
            n => format!("readable {n}"),
        })
        .collect();
    // As in a call: some come before alice is there, some while she is in
    // the call with the chat closed, the rest with it open.
    let say = |n: usize| bob_says(&hs, &room_id, &format!("readable{n}"), &said[n]);
    // A voice channel's history is mostly calls coming and going (events
    // the timeline hides).
    for n in 0..30 {
        api_as(
            &hs,
            "bob",
            "PUT",
            &format!("rooms/{}/state/org.matrix.msc3401.call.member/_@bob:localhost_OLD{n}", path_of(&room_id)),
            Some(serde_json::json!({})),
        )
        .expect("a call come and gone");
    }
    say(0);
    join_and_open(&mut alice, &channel).await;
    for n in 1..4 {
        say(n);
    }
    alice.wait_for(".voice-chat-toggle .unread-dot", DEFAULT_WAIT).await.expect("unread");
    alice.click(".voice-chat-toggle").await.expect("open");
    alice.wait_for(".voice-chat .timeline", DEFAULT_WAIT).await.expect("panel");
    // The rest typed by bob in his own panel.
    bob.click_text(".room.voice", &channel).await.expect("bob's page");
    bob.click(".voice-chat-toggle").await.expect("bob's chat");
    for body in &said[4..] {
        bob.type_into(".voice-chat .composer-input", body).await.expect("type");
        bob.press(".voice-chat .composer-input", "Enter").await.expect("send");
        bob.wait_for_text(".voice-chat .message.own .body", body, DEFAULT_WAIT).await.expect("sent");
    }
    alice
        .wait_for_text(".voice-chat .message .body", "readable 5", DEFAULT_WAIT)
        .await
        .expect("the newest message");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let panel = bounds(&mut alice, ".voice-chat").await;
    let timeline = bounds(&mut alice, ".voice-chat .timeline").await;
    let inside = |b: &Bounds, of: &Bounds| {
        b.x >= of.x - 0.5 && b.right() <= of.right() + 0.5 && b.y >= of.y - 0.5 && b.bottom() <= of.bottom() + 0.5
    };
    assert!(inside(&timeline, &panel), "the timeline {timeline:?} is not in the panel {panel:?}");
    assert!(timeline.h >= 200.0 && timeline.w >= 200.0, "the timeline is {timeline:?}");
    let texts = alice.texts(".voice-chat .message .body").await.expect("texts");
    // Each once, in the order sent.
    let ours: Vec<&str> = texts.iter().map(|t| t.trim()).filter(|t| t.starts_with("readable")).collect();
    assert_eq!(ours, said.iter().map(String::as_str).collect::<Vec<_>>(), "the messages as shown");
    let boxes = alice.all_bounds(".voice-chat .message .body").await.expect("bounds");
    assert_eq!(texts.len(), boxes.len());
    let mut rows = Vec::new();
    for body in &said {
        let at = texts.iter().position(|t| t.trim() == body).unwrap_or_else(|| panic!("{body:?} not shown: {texts:?}"));
        let b = boxes[at];
        assert!(b.h >= 12.0 && b.w >= 40.0, "{body:?} is laid out {b:?}");
        assert!(inside(&b, &timeline), "{body:?} at {b:?} is outside the timeline {timeline:?}");
        rows.push((body.clone(), b));
    }
    for pair in rows.windows(2) {
        let ((a, ab), (b, bb)) = (&pair[0], &pair[1]);
        assert!(ab.bottom() <= bb.y + 0.5, "{a:?} {ab:?} is not above {b:?} {bb:?}");
    }
    let (_, newest) = rows.last().expect("rows");
    assert!(timeline.bottom() - newest.bottom() <= 120.0, "the newest {newest:?} is not at the bottom of {timeline:?}");
    let long = rows[3].1;
    assert!(long.h >= 2.0 * rows[2].1.h - 1.0, "the long message did not wrap: {long:?}");
    // Every row has its main column laid out across the panel.
    let mains = alice.all_bounds(".voice-chat .message-main").await.expect("bounds");
    for b in &mains {
        assert!(b.w >= timeline.w / 2.0 && b.h > 0.0 && b.x >= timeline.x && b.right() <= timeline.right() + 0.5, "a row's main column at {b:?}");
    }
    for part in [".voice-chat .message .sender", ".voice-chat .message .avatar-button"] {
        let all = alice.all_bounds(part).await.expect("bounds");
        assert!(!all.is_empty(), "no {part}");
        for b in all {
            assert!(b.h > 0.0 && b.w > 0.0 && inside(&b, &timeline), "{part} at {b:?}");
        }
    }

    // In pixels: each message's text is drawn (ink over the panel).
    if let Some(shot) = common::capture(&alice) {
        let bg = common::pixel_at(&shot, panel.x + panel.w / 2.0, panel.y + 4.0);
        for (body, b) in &rows {
            let mut inked = 0;
            let (mut y, mut x);
            y = b.y.ceil();
            while y < b.bottom() {
                x = b.x.ceil();
                while x < b.right().min(shot.1 as f64 - 1.0) {
                    if common::colour_distance(common::pixel_at(&shot, x, y), bg) > 150 {
                        inked += 1;
                    }
                    x += 1.0;
                }
                y += 1.0;
            }
            assert!(inked >= 40, "{body:?}: only {inked} inked pixels at {b:?}");
        }
    }
    alice.click(".voice-panel .hang-up").await.expect("leave");
}

/// Scrolling the panel with the wheel, and older messages loading.
#[tokio::test]
async fn the_chat_scrolls_and_loads_older_messages() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    if !alice.is_gpui() {
        return;
    }
    login(&mut alice, "alice").await;
    let channel = fresh_voice_channel(&hs);
    alice.wait_for_text(".room .room-name", &channel, DEFAULT_WAIT).await.expect("listed");
    let room_id = alice.attr_containing(".room.voice", &channel, "data-room").await.expect("id").expect("id");
    let bodies: Vec<String> = (0..150).map(|n| format!("history {n}")).collect();
    morpheus_e2e::send_many_as(&hs, "bob", &room_id, &bodies).expect("fill");
    join_and_open(&mut alice, &channel).await;
    alice.click(".voice-chat-toggle").await.expect("open");
    alice
        .wait_for_text(".voice-chat .message .body", "history 149", DEFAULT_WAIT)
        .await
        .expect("the newest");
    // The wheel moves the panel's own timeline (and nothing else).
    let before = alice.scroll_state(".voice-chat .timeline").await.expect("state").expect("scrolls");
    for _ in 0..5 {
        alice.wheel(".voice-chat .timeline", -6.0).await.expect("wheel");
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    let after = alice.scroll_state(".voice-chat .timeline").await.expect("state").expect("scrolls");
    assert!(after.from_bottom > before.from_bottom + 100.0, "the wheel did not scroll: {before:?} {after:?}");
    alice.wait_for(".voice-chat .jump-to-bottom", DEFAULT_WAIT).await.expect("jump to present");
    // Older messages load when going back far enough.
    alice.reveal_message("history 0", DEFAULT_WAIT * 2).await.expect("the oldest loads");
    let panel = bounds(&mut alice, ".voice-chat").await;
    for row in alice.all_bounds(".voice-chat .message").await.expect("rows") {
        assert!(
            row.x >= panel.x - 0.5 && row.right() <= panel.right() + 0.5 && row.y >= panel.y - 0.5 && row.bottom() <= panel.bottom() + 0.5,
            "a row {row:?} sticks out of the panel {panel:?}"
        );
    }
    alice.click(".voice-panel .hang-up").await.expect("leave");
}

/// Switching rooms with the chat open and coming back, and leaving the call.
#[tokio::test]
async fn the_chat_survives_switching_rooms_and_leaving_the_call() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    if !alice.is_gpui() {
        return;
    }
    login(&mut alice, "alice").await;
    let channel = fresh_voice_channel(&hs);
    let room_id = {
        alice.wait_for_text(".room .room-name", &channel, DEFAULT_WAIT).await.expect("listed");
        alice.attr_containing(".room.voice", &channel, "data-room").await.expect("id").expect("id")
    };
    bob_says(&hs, &room_id, "first", "voice one");
    join_and_open(&mut alice, &channel).await;
    alice.click(".voice-chat-toggle").await.expect("open");
    alice.wait_for_text(".voice-chat .message .body", "voice one", DEFAULT_WAIT).await.expect("its message");
    alice.type_into(".voice-chat .composer-input", "half written").await.expect("draft");

    // To #general while the call goes on: its own page, no stale panel.
    alice.open_room("general").await.expect("general");
    alice.wait_for_text(".chat-header h2", "general", DEFAULT_WAIT).await.expect("general open");
    assert_eq!(alice.count(".voice-chat, .voice-chat-toggle").await.expect("count"), 0, "a stale panel over #general");
    let note = format!("in general {}", std::process::id());
    alice.type_into(".composer .composer-input", &note).await.expect("type");
    alice.press(".composer .composer-input", "Enter").await.expect("send");
    alice.wait_for_text(".message.own .body", &note, DEFAULT_WAIT).await.expect("sent in general");
    assert_eq!(alice.count(".voice-chat-toggle").await.expect("count"), 0);

    // Back to the call's page: the chat is open still, and is the channel's.
    alice.click_text(".room.voice", &channel).await.expect("back");
    alice.wait_for(".call-stage.voice-page", DEFAULT_WAIT).await.expect("page");
    alice.wait_for(".voice-chat .composer-input", DEFAULT_WAIT).await.expect("the chat stays open");
    alice.wait_for(".voice-chat-toggle.active", DEFAULT_WAIT).await.expect("lit");
    alice.wait_for_text(".voice-chat .message .body", "voice one", DEFAULT_WAIT).await.expect("its message");
    let shown = alice.texts(".voice-chat .message .body").await.expect("texts");
    assert!(!shown.iter().any(|t| t.contains(&note)), "general's message in the voice chat: {shown:?}");
    assert_eq!(alice.count(".voice-chat .composer-input").await.expect("count"), 1, "one box");

    // Leaving the call with the chat open: the page offers Join, the chat works.
    alice.click(".voice-panel .hang-up").await.expect("leave");
    alice.wait_for(".voice-page-join", DEFAULT_WAIT).await.expect("join page");
    alice.wait_for(".voice-chat .composer-input", DEFAULT_WAIT).await.expect("the chat is still there");
    alice.wait_for(".voice-chat-toggle.active", DEFAULT_WAIT).await.expect("still lit");
    alice.type_into(".voice-chat .composer-input", "after the call").await.expect("type");
    alice.press(".voice-chat .composer-input", "Enter").await.expect("send");
    alice
        .wait_for_text(".voice-chat .message.own .body", "after the call", DEFAULT_WAIT)
        .await
        .expect("it sends without a call");
    let page = bounds(&mut alice, ".voice-channel").await;
    let panel = bounds(&mut alice, ".voice-chat").await;
    let join = bounds(&mut alice, ".voice-page-join").await;
    assert!(join.right() <= panel.x + 0.5 && panel.right() <= page.right() + 0.5, "the join page runs under the chat");
    alice.click(".voice-chat-toggle").await.expect("close");
    alice.wait_for_count(".voice-chat", 0, DEFAULT_WAIT).await.expect("closed");
}

fn hyprctl(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("hyprctl").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Float the app's window and size it.
fn resize_window(pid: u32, w: u32, h: u32) {
    let clients: serde_json::Value = serde_json::from_str(&hyprctl(&["clients", "-j"]).expect("hyprctl")).expect("json");
    let window = clients
        .as_array()
        .expect("clients")
        .iter()
        .find(|c| c["pid"].as_u64() == Some(u64::from(pid)))
        .expect("the window")
        .clone();
    let target = format!("address:{}", window["address"].as_str().expect("address"));
    if window["floating"].as_bool() != Some(true) {
        hyprctl(&["eval", &format!("hl.dispatch(hl.dsp.window.float({{ action = \"enable\", window = \"{target}\" }}))")]);
    }
    hyprctl(&["eval", &format!("hl.dispatch(hl.dsp.window.resize({{ x = {w}, y = {h}, window = \"{target}\" }}))")]);
}

/// A tile focused with the chat open stays in the call's part; the window
/// resized keeps the chat a sane width and the call reflows; Escape.
#[tokio::test]
async fn the_chat_beside_a_focused_tile_and_a_resized_window() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    if !morpheus_e2e::display_or_skip("resizes the window through Hyprland") {
        return;
    }
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
        eprintln!("not on Hyprland; skipping");
        return;
    }
    let mut alice = App::launch(&hs).await.expect("launch alice");
    if !alice.is_gpui() {
        return;
    }
    login(&mut alice, "alice").await;
    let channel = fresh_voice_channel(&hs);
    let mut bob = App::launch_desktop(&hs).await.expect("launch bob");
    login(&mut bob, "bob").await;
    join_and_open(&mut alice, &channel).await;
    bob.click_text(".room.voice", &channel).await.expect("bob joins");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT).await.expect("bob in");
    alice.wait_for_count(".call-cards .call-participant, .call-cards .call-tile", 2, DEFAULT_WAIT).await.expect("two cards");
    alice.click(".voice-chat-toggle").await.expect("open");
    alice.wait_for(".voice-chat .timeline", DEFAULT_WAIT).await.expect("panel");

    // The button is not over any card's own button.
    let button = bounds(&mut alice, ".voice-chat-toggle").await;
    for enlarge in alice.all_bounds(".card-enlarge").await.expect("enlarge buttons") {
        assert!(!overlaps(&button, &enlarge), "the chat button {button:?} covers a card's button {enlarge:?}");
    }

    // Focus a tile: still beside the chat.
    alice.click(".card-enlarge").await.expect("enlarge");
    alice.wait_for(".call-cards.focused", DEFAULT_WAIT).await.expect("focused");
    let panel = bounds(&mut alice, ".voice-chat").await;
    let main = bounds(&mut alice, ".call-focus-main").await;
    assert!(main.right() <= panel.x + 0.5, "the focused tile {main:?} runs under the chat {panel:?}");
    assert_eq!(alice.count(".voice-chat").await.expect("count"), 1, "the chat closed");
    // Escape (nothing typed in the chat): closes the chat first, keeping the
    // focused tile; the next Escape un-focuses the tile.
    alice.unfocus().await.expect("blur");
    alice.key_combo("body", "Escape", false, false, false).await.expect("esc");
    alice.wait_for_count(".voice-chat", 0, DEFAULT_WAIT).await.expect("the chat closes first");
    assert_eq!(alice.count(".call-cards.focused").await.expect("count"), 1, "the first Escape un-focused the tile");
    alice.key_combo("body", "Escape", false, false, false).await.expect("esc");
    alice.wait_for_count(".call-cards.focused", 0, DEFAULT_WAIT).await.expect("back to the grid");
    alice.click(".voice-chat-toggle").await.expect("reopen the chat");
    alice.wait_for(".voice-chat .timeline", DEFAULT_WAIT).await.expect("chat back");

    // Window sizes: big, then small. The chat's width follows, the call keeps room.
    let pid = alice.pid().expect("pid");
    let mut widths = Vec::new();
    for (w, h) in [(1500, 900), (900, 640), (1300, 800)] {
        resize_window(pid, w, h);
        tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
        let page = bounds(&mut alice, ".voice-channel").await;
        let panel = bounds(&mut alice, ".voice-chat").await;
        let stage = bounds(&mut alice, ".call-stage").await;
        assert!(panel.right() <= page.right() + 0.5 && panel.x >= page.x, "{w}x{h}: panel {panel:?} outside {page:?}");
        assert!((240.0..=420.0).contains(&panel.w), "{w}x{h}: panel width {}", panel.w);
        assert!(panel.w <= page.w * 0.5, "{w}x{h}: the panel {panel:?} takes over half of the page {page:?}");
        assert!(stage.w >= 300.0, "{w}x{h}: the call is squeezed to {stage:?}");
        assert!(stage.right() <= panel.x + 0.5, "{w}x{h}: call {stage:?} under panel {panel:?}");
        for card in alice.all_bounds(".call-cards .call-participant, .call-cards .call-tile").await.expect("cards") {
            assert!(
                card.x >= stage.x - 0.5 && card.right() <= stage.right() + 0.5,
                "{w}x{h}: a card {card:?} outside the call {stage:?}"
            );
        }
        widths.push(panel.w);
    }
    assert!(widths[0] > widths[1], "the chat does not narrow with the window: {widths:?}");
    alice.click(".voice-panel .hang-up").await.expect("leave");
}

/// Turning the camera on while looking at another room brings you to the
/// call's view (the channel's page), where the picture shows.
#[tokio::test]
async fn turning_the_camera_on_opens_the_call() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    if !alice.is_gpui() {
        return;
    }
    login(&mut alice, "alice").await;
    let channel = fresh_voice_channel(&hs);
    alice.wait_for_text(".room .room-name", &channel, DEFAULT_WAIT).await.expect("listed");
    alice.open_room("general").await.expect("general");
    alice.click_text(".room.voice", &channel).await.expect("join in place");
    alice.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT).await.expect("joined");
    alice.wait_for_text(".chat-header h2", "general", DEFAULT_WAIT).await.expect("still in general");
    assert_eq!(alice.count(".call-stage.voice-page").await.expect("count"), 0, "no call view yet");
    alice.click(".voice-panel .camera-button").await.expect("camera on");
    alice
        .wait_for(".call-stage.voice-page", DEFAULT_WAIT)
        .await
        .expect("the camera brought the call's view up");
    alice.wait_for(".voice-chat-toggle", DEFAULT_WAIT).await.expect("the channel's page");
    // Off again, and back to #general by hand: turning it off does not move you.
    alice.click(".voice-panel .camera-button").await.expect("camera off");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    alice.open_room("general").await.expect("general");
    alice.click(".voice-panel .camera-button").await.expect("camera on");
    alice.wait_for(".call-stage.voice-page", DEFAULT_WAIT).await.expect("again");
    alice.click(".voice-panel .camera-button").await.expect("camera off");
    alice.open_room("general").await.expect("general");
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    assert_eq!(alice.count(".call-stage.voice-page").await.expect("count"), 0, "turning it off moved the view");
    alice.click(".voice-panel .hang-up").await.expect("leave");
}

/// Sharing the screen while looking at another room brings you to the call's
/// view (the channel's page), as the camera does; stopping does not move you.
#[tokio::test]
async fn sharing_the_screen_opens_the_call() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    if !alice.is_gpui() {
        return;
    }
    login(&mut alice, "alice").await;
    let channel = fresh_voice_channel(&hs);
    alice.wait_for_text(".room .room-name", &channel, DEFAULT_WAIT).await.expect("listed");
    alice.open_room("general").await.expect("general");
    alice.click_text(".room.voice", &channel).await.expect("join in place");
    alice.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT).await.expect("joined");
    alice.wait_for_text(".chat-header h2", "general", DEFAULT_WAIT).await.expect("still in general");
    assert_eq!(alice.count(".call-stage.voice-page").await.expect("count"), 0, "no call view yet");
    alice.click(".voice-panel .screen-button").await.expect("share");
    alice.wait_for(".screen-quality .screen-go", DEFAULT_WAIT).await.expect("the quality popover");
    alice.click(".screen-quality .screen-go").await.expect("go live");
    alice
        .wait_for(".call-stage.voice-page", DEFAULT_WAIT)
        .await
        .expect("sharing brought the call's view up");
    alice.wait_for(".voice-chat-toggle", DEFAULT_WAIT).await.expect("the channel's page");
    // Stopping from another room does not move you. (Streamer mode asks before
    // leaving for another channel while the screen is shared: Open.)
    alice.click_text(".room .room-name", "general").await.expect("general");
    alice.wait_for(".confirm-dialog .confirm-go", DEFAULT_WAIT).await.expect("streamer mode asks");
    alice.click(".confirm-dialog .confirm-go").await.expect("open it");
    alice.wait_for_text(".chat-header h2", "general", DEFAULT_WAIT).await.expect("in general");
    alice.wait_for(".voice-panel .screen-button.active", DEFAULT_WAIT).await.expect("sharing");
    alice.click(".voice-panel .screen-button").await.expect("stop");
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    assert_eq!(alice.count(".call-stage.voice-page").await.expect("count"), 0, "stopping moved the view");
    alice.click(".voice-panel .hang-up").await.expect("leave");
}

/// Create Channel offers Text or Voice (each with its icon). A voice channel
/// is listed with the speaker icon, and clicking it joins its call.
#[tokio::test]
async fn a_voice_channel_is_created_from_the_create_dialog_and_joins_its_call() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let _clean = morpheus_e2e::reset_rooms_named(&hs, &["alice", "bob"], "e2e new voice");
    let mut alice = App::launch(&hs).await.expect("launch alice");
    if !alice.is_gpui() {
        return;
    }
    login(&mut alice, "alice").await;
    alice.click(".rail-item[title=\"Engineering\"]").await.expect("the server");
    alice.wait_for(".create-room-button", DEFAULT_WAIT).await.expect("plus");
    alice.click(".create-room-button").await.expect("plus");
    alice.wait_for(".create-room-dialog input[name=name]", DEFAULT_WAIT).await.expect("dialog");
    // Two options with icons; Text is chosen to begin with.
    let kinds = alice.attrs(".create-room-dialog .channel-type", "data-value").await.expect("types");
    assert_eq!(kinds, vec![Some("text".to_owned()), Some("voice".to_owned())], "the channel types");
    assert_eq!(
        alice.count(".create-room-dialog .channel-type svg.icon").await.expect("icons"),
        2,
        "each type has an icon"
    );
    alice.wait_for(".create-room-dialog .channel-type.selected[data-value=text]", DEFAULT_WAIT).await.expect("text first");
    alice.click(".create-room-dialog .channel-type[data-value=voice]").await.expect("voice");
    alice
        .wait_for(".create-room-dialog .channel-type.selected[data-value=voice]", DEFAULT_WAIT)
        .await
        .expect("voice chosen");
    let name = format!("e2e new voice {}", std::process::id());
    alice.type_into(".create-room-dialog input[name=name]", &name).await.expect("name");
    alice.submit(".create-room-dialog").await.expect("create");
    alice.wait_for_text(".room.voice", &name, DEFAULT_WAIT).await.expect("listed as a voice channel");
    let icons = alice.count(".room.voice .voice-icon").await.expect("icons");
    assert!(icons >= 1, "the speaker icon");
    // Anyone may join: clicking the row joins its call.
    alice.open_room("dev").await.expect("another channel of the server");
    alice.click_text(".room.voice", &name).await.expect("join");
    alice.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT).await.expect("joined its call");
    alice.click(".voice-panel .hang-up").await.expect("leave");
}

/// In a big window (the panel at its widest) the chat's list fills the
/// panel's timeline: its messages show at the bottom of the panel, not in a
/// strip at its top (#25: the list fell back to its rows' measured height,
/// a sliver, when the panel was at its widest).
#[tokio::test]
async fn the_chat_fills_its_panel_in_a_big_window() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch_sized(&hs, 1600, 1000).await.expect("launch alice");
    if !alice.is_gpui() {
        return;
    }
    login(&mut alice, "alice").await;
    let channel = fresh_voice_channel(&hs);
    alice.wait_for_text(".room .room-name", &channel, DEFAULT_WAIT).await.expect("listed");
    let room_id = alice.attr_containing(".room.voice", &channel, "data-room").await.expect("id").expect("id");
    let said: Vec<String> = (0..4).map(|n| format!("big window {n}")).collect();
    morpheus_e2e::send_many_as(&hs, "bob", &room_id, &said).expect("send");
    join_and_open(&mut alice, &channel).await;
    alice.click(".voice-chat-toggle").await.expect("open");
    alice
        .wait_for_text(".voice-chat .message .body", "big window 3", DEFAULT_WAIT)
        .await
        .expect("the newest message");
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    let panel = bounds(&mut alice, ".voice-chat").await;
    let timeline = bounds(&mut alice, ".voice-chat .timeline").await;
    assert!(panel.w >= 370.0, "the panel is not at its widest: {panel:?}");
    assert!(timeline.h >= panel.h - 120.0, "the timeline {timeline:?} is not most of the panel {panel:?}");
    // The list is as tall as the timeline it is in.
    let list = alice.scroll_state(".voice-chat .timeline").await.expect("state").expect("scrolls");
    assert!(
        list.viewport >= timeline.h - 1.0,
        "the list is {} px tall in a {} px timeline: a sliver",
        list.viewport,
        timeline.h
    );
    // Its messages are laid out in it, the newest at its bottom.
    let texts = alice.texts(".voice-chat .message .body").await.expect("texts");
    let boxes = alice.all_bounds(".voice-chat .message .body").await.expect("bounds");
    for body in &said {
        let at = texts.iter().position(|t| t.trim() == body).unwrap_or_else(|| panic!("{body:?} not shown: {texts:?}"));
        let b = boxes[at];
        assert!(b.h >= 12.0 && b.y >= timeline.y && b.bottom() <= timeline.bottom() + 0.5, "{body:?} at {b:?}, timeline {timeline:?}");
    }
    let _ = alice.screenshot("voice-chat-big-window").await;
    let newest = boxes[texts.iter().position(|t| t.trim() == said[3]).expect("newest")];
    assert!(timeline.bottom() - newest.bottom() <= 120.0, "the newest {newest:?} is not at the bottom of {timeline:?}");
    alice.click(".voice-panel .hang-up").await.expect("leave");
}
