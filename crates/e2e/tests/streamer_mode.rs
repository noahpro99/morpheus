//! Streamer mode (GPUI only): while the screen is shared, or when turned on by
//! hand in Settings > Streamer Mode, names are abbreviated (first letter and
//! three dots), IDs are hidden, notifications say nothing, and switching
//! channels while sharing asks first.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT, api_as, new_account};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

/// How a name is abbreviated: its first letter and three dots.
fn masked(name: &str) -> String {
    format!("{}\u{2022}\u{2022}\u{2022}", name.chars().next().expect("a name"))
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

/// Settings > Streamer Mode, open.
async fn open_streamer_settings(app: &mut App) {
    app.click(".settings-button").await.expect("gear");
    app.wait_for_text(".settings-nav button", "Streamer Mode", DEFAULT_WAIT)
        .await
        .expect("a Streamer Mode page");
    app.click_text(".settings-nav button", "Streamer Mode").await.expect("page");
    app.wait_for(".settings-page input[name=streamer-now]", DEFAULT_WAIT)
        .await
        .expect("the switches");
}

/// Turn a Streamer Mode switch the other way.
async fn flip(app: &mut App, name: &str) {
    let sel = format!(".settings-page input[name={name}]");
    let was = app.checked(&sel).await.expect("checked")[0];
    app.click(&sel).await.expect("flip");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while app.checked(&sel).await.expect("checked")[0] == was {
        assert!(std::time::Instant::now() < deadline, "{name} did not flip");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

async fn flip_now(app: &mut App) {
    open_streamer_settings(app).await;
    flip(app, "streamer-now").await;
    app.press("body", "Escape").await.expect("close settings");
}

/// A fresh pair in a room of their own: `reader` is signed in on a desktop
/// app, and `writer` posts through the API. Their names start with r and w.
async fn pair(hs: &str, name: &str) -> (App, String, String, String) {
    let reader = new_account(hs, &format!("r{name}")).expect("reader");
    let writer = new_account(hs, &format!("w{name}")).expect("writer");
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
    api_as(hs, &reader, "POST", &format!("rooms/{id}/join"), Some(serde_json::json!({})))
        .expect("reader joins");
    let mut app = App::launch_desktop(hs).await.expect("launch");
    login(&mut app, &reader).await;
    (app, reader, writer, room)
}

/// What the desktop app would have shown (the test log of notifications).
fn shown(app: &App) -> Vec<serde_json::Value> {
    std::fs::read_to_string(app.data_dir().join("shown-notifications.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// `writer` posts `text` in `room` (no waiting).
fn post(hs: &str, writer: &str, room: &str, text: &str) {
    let room = room.replace('!', "%21").replace(':', "%3A");
    api_as(
        hs,
        writer,
        "PUT",
        &format!("rooms/{room}/send/m.room.message/{}", text.len()),
        Some(serde_json::json!({ "msgtype": "m.text", "body": text })),
    )
    .expect("send");
}

/// `writer` says `text` in `room`; the first notification the app shows after
/// those already shown.
async fn shown_for(app: &App, hs: &str, writer: &str, room: &str, text: &str) -> serde_json::Value {
    let before = shown(app).len();
    post(hs, writer, room, text);
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

/// Turned on by hand, names are abbreviated in the timeline, the member list,
/// the profile card and Settings' account page, IDs are hidden, a badge sits
/// in the title bar; turned off, all is as before.
#[tokio::test]
async fn streamer_mode_by_hand_abbreviates_names_and_hides_ids() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (mut app, reader, writer, room) = pair(&hs, "strm").await;
    if !app.is_gpui() {
        eprintln!("streamer mode is in the GPUI app only; skipping");
        return;
    }
    post(&hs, &writer, &room, "hello streamer");
    app.open_room("strm chat").await.expect("open");
    app.wait_for_text(".timeline .sender", &writer, DEFAULT_WAIT).await.expect("her message");
    app.wait_for_text(".members .member-name", &reader, DEFAULT_WAIT).await.expect("members");
    assert_eq!(app.count(".titlebar .streamer-badge").await.expect("count"), 0);

    flip_now(&mut app).await;
    app.wait_for(".titlebar .streamer-badge", DEFAULT_WAIT).await.expect("the badge");
    let senders = app.texts(".timeline .sender").await.expect("senders");
    assert!(!senders.is_empty() && senders.iter().all(|s| *s == masked(&writer)), "senders: {senders:?}");
    let members = app.texts(".members .member-name").await.expect("members");
    assert!(members.contains(&masked(&writer)) && members.contains(&masked(&reader)), "members: {members:?}");
    assert!(
        members.iter().all(|m| !m.contains(&writer) && !m.contains(&reader)),
        "members: {members:?}"
    );

    // The profile card: abbreviated name, no ID.
    app.click_text(".members .member", &masked(&writer)).await.expect("open the card");
    app.wait_for(".profile-card", DEFAULT_WAIT).await.expect("card");
    let card = app.text(".profile-card").await.expect("text").unwrap_or_default();
    assert!(!card.contains(&writer) && !card.contains("localhost"), "the card gives away: {card}");
    assert_eq!(app.text(".profile-card .profile-name").await.expect("name"), Some(masked(&writer)));
    app.press("body", "Escape").await.expect("close card");
    app.wait_for_count(".profile-card", 0, DEFAULT_WAIT).await.expect("closed");

    // Settings' account page does not show your ID.
    app.click(".settings-button").await.expect("gear");
    app.wait_for(".settings-user", DEFAULT_WAIT).await.expect("account page");
    let own = app.text(".settings-user").await.expect("text").unwrap_or_default();
    assert!(!own.contains(&reader) && !own.contains("localhost"), "your ID: {own}");
    app.press("body", "Escape").await.expect("close settings");

    // And off again: everything as it was.
    flip_now(&mut app).await;
    app.wait_for_count(".titlebar .streamer-badge", 0, DEFAULT_WAIT).await.expect("badge gone");
    app.wait_for_text(".timeline .sender", &writer, DEFAULT_WAIT).await.expect("name back");
    app.wait_for_text(".members .member-name", &reader, DEFAULT_WAIT).await.expect("members back");
}

/// In streamer mode a notification says nothing about who or what.
#[tokio::test]
async fn streamer_mode_hides_notification_contents() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (mut app, _reader, writer, room) = pair(&hs, "strn").await;
    if !app.is_gpui() {
        eprintln!("streamer mode is in the GPUI app only; skipping");
        return;
    }
    let pid = std::process::id();
    let before = format!("plain words {pid}");
    let notice = shown_for(&app, &hs, &writer, &room, &before).await;
    assert_eq!(notice["body"].as_str(), Some(before.as_str()), "{notice}");

    flip_now(&mut app).await;
    let secret = format!("secret words {pid}");
    let notice = shown_for(&app, &hs, &writer, &room, &secret).await;
    assert_eq!(notice["title"].as_str(), Some("Morpheus"), "{notice}");
    assert_eq!(notice["body"].as_str(), Some("Someone sent a message"), "{notice}");

    // Not hiding them is a choice of its own.
    open_streamer_settings(&mut app).await;
    flip(&mut app, "streamer-notifications").await;
    app.press("body", "Escape").await.expect("close settings");
    let open = format!("open words {pid}");
    let notice = shown_for(&app, &hs, &writer, &room, &open).await;
    assert_eq!(notice["body"].as_str(), Some(open.as_str()), "{notice}");
}

/// The page's defaults, and that the choices are kept over a restart.
#[tokio::test]
async fn streamer_mode_settings_are_kept() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch");
    if !app.is_gpui() {
        eprintln!("streamer mode is in the GPUI app only; skipping");
        return;
    }
    login(&mut app, "alice").await;
    open_streamer_settings(&mut app).await;
    let state = async |app: &mut App| {
        let mut out = Vec::new();
        for n in ["auto", "now", "names", "notifications", "ids", "ask"] {
            out.push(
                app.checked(&format!(".settings-page input[name=streamer-{n}]"))
                    .await
                    .expect("checked")[0],
            );
        }
        out
    };
    assert_eq!(state(&mut app).await, [true, false, true, true, true, true], "defaults");
    flip(&mut app, "streamer-auto").await;
    flip(&mut app, "streamer-ids").await;
    flip(&mut app, "streamer-ask").await;
    flip(&mut app, "streamer-now").await;
    assert_eq!(state(&mut app).await, [false, true, true, true, false, false]);

    let mut app = app.relaunch().await.expect("relaunch");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("signed in again");
    app.wait_for(".titlebar .streamer-badge", DEFAULT_WAIT).await.expect("still on");
    open_streamer_settings(&mut app).await;
    assert_eq!(state(&mut app).await, [false, true, true, true, false, false], "kept");
}

/// Sharing the screen turns it on by itself, asks before another channel is
/// opened (Cancel stays, Open goes, a tick stops the asking while this share
/// lasts) and the end of the share turns it off.
#[tokio::test]
async fn sharing_the_screen_turns_streamer_mode_on_and_asks_before_switching() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch alice");
    if !app.is_gpui() {
        eprintln!("streamer mode is in the GPUI app only; skipping");
        return;
    }
    login(&mut app, "alice").await;
    app.open_room("general").await.expect("general");
    app.wait_for_text(".members .member-name", "bob", DEFAULT_WAIT).await.expect("bob listed");
    app.click_text(".room", "hangout").await.expect("join the call");
    app.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT).await.expect("connected");
    assert_eq!(app.count(".titlebar .streamer-badge").await.expect("count"), 0);

    app.click(".voice-panel .screen-button").await.expect("share");
    app.wait_for(".screen-quality .screen-go", DEFAULT_WAIT).await.expect("popover");
    app.click(".screen-quality .screen-go").await.expect("go live");
    app.wait_for(".titlebar .streamer-badge", DEFAULT_WAIT).await.expect("streamer mode on");

    // Another channel: asked first. Cancel stays.
    app.click_text(".room", "general").await.expect("click general");
    app.wait_for_text(".confirm-dialog", "You're sharing your screen. Open #general?", DEFAULT_WAIT)
        .await
        .expect("asked");
    app.click(".confirm-dialog .confirm-cancel").await.expect("cancel");
    app.wait_for_count(".confirm-dialog", 0, DEFAULT_WAIT).await.expect("closed");
    assert_eq!(app.count(".chat-header h2").await.expect("count"), 0, "still on the call's page");

    // Open goes, with the names abbreviated there.
    app.click_text(".room", "general").await.expect("click general");
    app.wait_for(".confirm-dialog input[name=dont-ask]", DEFAULT_WAIT).await.expect("the tick");
    app.click(".confirm-dialog .confirm-go").await.expect("open");
    app.wait_for_text(".chat-header h2", "general", DEFAULT_WAIT).await.expect("general open");
    app.wait_for_text(".members .member-name", "b\u{2022}\u{2022}\u{2022}", DEFAULT_WAIT)
        .await
        .expect("abbreviated");
    let members = app.texts(".members .member-name").await.expect("members");
    assert!(members.iter().all(|m| m != "bob" && m != "alice"), "members: {members:?}");

    // "Don't ask again during this share": the next one goes straight.
    app.click_text(".room", "hangout").await.expect("hangout");
    app.wait_for(".confirm-dialog", DEFAULT_WAIT).await.expect("asked");
    app.click(".confirm-dialog input[name=dont-ask]").await.expect("tick");
    app.click(".confirm-dialog .confirm-go").await.expect("open");
    app.wait_for_count(".confirm-dialog", 0, DEFAULT_WAIT).await.expect("closed");
    app.click_text(".room", "general").await.expect("general again");
    app.wait_for_text(".chat-header h2", "general", DEFAULT_WAIT).await.expect("straight there");
    assert_eq!(app.count(".confirm-dialog").await.expect("count"), 0);

    // The share ends: back to normal.
    app.click(".voice-panel .screen-button").await.expect("stop");
    app.wait_for_count(".titlebar .streamer-badge", 0, DEFAULT_WAIT).await.expect("off again");
    app.wait_for_text(".members .member-name", "bob", DEFAULT_WAIT).await.expect("names back");
}

// ---- the sweep: every view, with the names hidden ----

/// `user`'s ID path-escaped for a URL.
fn esc(id: &str) -> String {
    id.replace('@', "%40").replace(':', "%3A").replace('!', "%21").replace('$', "%24")
}

/// What on screen (the text of every tagged element, and the titles) gives a
/// secret away: `stage`, and the snippet around each hit.
async fn leaks(app: &mut App, stage: &str, secrets: &[String], found: &mut std::collections::BTreeSet<String>) {
    let mut seen = app.texts("*").await.expect("texts");
    seen.extend(app.attrs("*", "title").await.expect("titles").into_iter().flatten());
    for text in seen {
        for secret in secrets.iter().map(String::as_str).chain([":localhost"]) {
            if let Some(at) = text.find(secret) {
                let from = text[..at].char_indices().rev().nth(20).map_or(0, |(i, _)| i);
                let to = text[at..].char_indices().nth(secret.len() + 20).map_or(text.len(), |(i, _)| at + i);
                found.insert(format!("{stage}: ...{}...", &text[from..to]));
            }
        }
    }
}

/// With streamer mode on, no view of the app gives a name or an ID away: the
/// timeline (senders, state lines, reactions, typing, mention pills, a
/// thread chip), the reactions viewer, the thread panel, search results, the
/// inbox (mentions and invites), an invite card, the invite dialog's friend
/// list, the Friends page, a DM and the member list. Every leak of every
/// stage is listed at once.
#[tokio::test]
async fn streamer_mode_hides_every_name_in_every_view() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let reader = new_account(&hs, "rsweep").expect("reader");
    let writer = new_account(&hs, "wsweep").expect("writer");
    let carol = new_account(&hs, "csweep").expect("carol");
    let dan = new_account(&hs, "dsweep").expect("dan, a friend in no room");
    let user = |name: &str| format!("@{name}:localhost");
    let secrets = [reader.clone(), writer.clone(), carol.clone(), dan.clone()];
    let api = |who: &str, method: &str, path: &str, body: serde_json::Value| {
        api_as(&hs, who, method, path, Some(body)).expect("api")
    };

    // The room: all three in it, with a topic, a message with a reaction, a
    // thread, a pill to the reader, a plain @name for carol, and the writer typing.
    let room = api(&writer, "POST", "createRoom", serde_json::json!({ "name": "sweep chat" }));
    let room = room["room_id"].as_str().expect("room").to_owned();
    let id = esc(&room);
    for who in [&reader, &carol] {
        api(&writer, "POST", &format!("rooms/{id}/invite"), serde_json::json!({ "user_id": user(who) }));
        api(who, "POST", &format!("rooms/{id}/join"), serde_json::json!({}));
    }
    api(&writer, "PUT", &format!("rooms/{id}/state/m.room.topic"), serde_json::json!({ "topic": "sweeping" }));
    let send = |who: &str, txn: &str, content: serde_json::Value| -> String {
        let sent = api(who, "PUT", &format!("rooms/{id}/send/m.room.message/{txn}"), content);
        sent["event_id"].as_str().expect("event id").to_owned()
    };
    let hello = send(&writer, "1", serde_json::json!({ "msgtype": "m.text", "body": "hello sweep" }));
    api(
        &carol,
        "PUT",
        &format!("rooms/{id}/send/m.reaction/2"),
        serde_json::json!({ "m.relates_to": { "rel_type": "m.annotation", "event_id": hello, "key": "\u{1f389}" } }),
    );
    let root = send(&writer, "3", serde_json::json!({ "msgtype": "m.text", "body": "thread root sweep" }));
    send(
        &carol,
        "4",
        serde_json::json!({
            "msgtype": "m.text", "body": "thread reply sweep",
            "m.relates_to": { "rel_type": "m.thread", "event_id": root, "is_falling_back": true,
                "m.in_reply_to": { "event_id": root } },
        }),
    );
    send(
        &writer,
        "5",
        serde_json::json!({
            "msgtype": "m.text", "body": "look over here pangolin", "format": "org.matrix.custom.html",
            "formatted_body": format!("<a href=\"https://matrix.to/#/{}\">{reader}</a> look over here pangolin", user(&reader)),
            "m.mentions": { "user_ids": [user(&reader)] },
        }),
    );
    send(&writer, "6", serde_json::json!({ "msgtype": "m.text", "body": format!("and you @{carol} too") }));

    // Another room, not opened.
    let other = api(&writer, "POST", "createRoom", serde_json::json!({ "name": "sweep other" }));
    let other = other["room_id"].as_str().expect("room").to_owned();
    api(&writer, "POST", &format!("rooms/{}/invite", esc(&other)), serde_json::json!({ "user_id": user(&reader) }));
    api(&reader, "POST", &format!("rooms/{}/join", esc(&other)), serde_json::json!({}));
    // The reader's friends: the writer, carol and dan (who is in no room).
    api(
        &reader,
        "PUT",
        &format!("user/{}/account_data/dev.morpheus.friends", esc(&user(&reader))),
        serde_json::json!({ "users": [user(&writer), user(&carol), user(&dan)] }),
    );

    let mut app = App::launch_desktop(&hs).await.expect("launch");
    if !app.is_gpui() {
        eprintln!("streamer mode is in the GPUI app only; skipping");
        return;
    }
    login(&mut app, &reader).await;
    app.open_room("sweep chat").await.expect("open");
    app.wait_for_text(".timeline .sender", &writer, DEFAULT_WAIT).await.expect("messages");
    app.wait_for_text(".timeline", "pangolin", DEFAULT_WAIT).await.expect("the pill message");
    app.wait_for(".thread-chip", DEFAULT_WAIT).await.expect("the thread chip");
    app.wait_for(".members .member", DEFAULT_WAIT).await.expect("members");
    api(
        &writer,
        "PUT",
        &format!("rooms/{id}/typing/{}", esc(&user(&writer))),
        serde_json::json!({ "typing": true, "timeout": 120000 }),
    );
    app.wait_for_text(".typing", "typing", DEFAULT_WAIT).await.expect("typing");
    app.wait_for_text(".timeline .system-text", "changed the topic", DEFAULT_WAIT).await.expect("topic line");
    app.wait_for(".members .member", DEFAULT_WAIT).await.expect("members");
    flip_now(&mut app).await;
    app.wait_for(".titlebar .streamer-badge", DEFAULT_WAIT).await.expect("the badge");
    // The sweep sees something: names are there, abbreviated.
    app.wait_for_text(".typing", &masked(&writer), DEFAULT_WAIT).await.expect("typing, abbreviated");

    let mut found = std::collections::BTreeSet::new();
    // The sweep does see the abbreviated forms, so a clean result means something.
    let (w, c, r) = (masked(&writer), masked(&carol), masked(&reader));
    for (selector, want) in [
        (".timeline .system-text", format!("{w} changed the topic")),
        (".timeline", format!("")),
        (".thread-chip", format!("last from {c}")),
        (".user-bar", r.clone()),
    ] {
        app.wait_for_text(selector, &want, DEFAULT_WAIT).await.unwrap_or_else(|e| panic!("{selector} lacks {want:?}: {e}"));
    }
    leaks(&mut app, "the timeline", &secrets, &mut found).await;

    // The reactions viewer.
    app.right_click_text(".message", "hello sweep").await.expect("menu");
    app.wait_for(".context-menu", DEFAULT_WAIT).await.expect("menu");
    app.click_text(".context-item", "View reactions").await.expect("view");
    app.wait_for(".reactions-dialog .reaction-senders", DEFAULT_WAIT).await.expect("viewer");
    leaks(&mut app, "the reactions viewer", &secrets, &mut found).await;
    app.press(".reactions-dialog", "Escape").await.expect("close");
    app.wait_for_count(".reactions-dialog", 0, DEFAULT_WAIT).await.expect("closed");

    // The thread panel.
    app.click_text(".thread-chip", "repl").await.expect("thread");
    app.wait_for(".thread-panel .message", DEFAULT_WAIT).await.expect("thread panel");
    app.wait_for_text(".thread-panel .timeline", "thread reply sweep", DEFAULT_WAIT).await.expect("reply");
    leaks(&mut app, "the thread panel", &secrets, &mut found).await;
    app.press("body", "Escape").await.expect("close thread");
    app.wait_for_count(".thread-panel", 0, DEFAULT_WAIT).await.expect("thread closed");

    // Search results.
    app.click(".chat-header .search-bar input[name=search]").await.expect("search box");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        app.type_into(".chat-header input[name=search]", "pangolin").await.expect("query");
        app.press(".chat-header input[name=search]", "Enter").await.expect("search");
        if app.wait_for(".search-panel .search-hit", std::time::Duration::from_secs(2)).await.is_ok() {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "no search results");
    }
    leaks(&mut app, "search results", &secrets, &mut found).await;
    app.press(".chat-header input[name=search]", "Escape").await.ok();

    // The invite dialog's friend list.
    app.right_click_text(".room:not(.invite-row)", "sweep chat").await.expect("room menu");
    app.wait_for(".context-menu", DEFAULT_WAIT).await.expect("room menu");
    app.click_text(".context-item", "Invite people").await.expect("invite");
    app.wait_for(".invite-dialog .friend-pick", DEFAULT_WAIT).await.expect("the friends");
    leaks(&mut app, "the invite dialog", &secrets, &mut found).await;
    app.press(".invite-dialog", "Escape").await.expect("close");
    app.wait_for_count(".invite-dialog", 0, DEFAULT_WAIT).await.expect("closed");

    // While it runs: a mention in the other room, and a DM invite from the
    // writer (not answered yet), for the inbox.
    api(
        &writer,
        "PUT",
        &format!("rooms/{}/send/m.room.message/7", esc(&other)),
        serde_json::json!({
            "msgtype": "m.text", "body": "ping in the other room",
            "m.mentions": { "user_ids": [user(&reader)] },
        }),
    );
    let dm = api(
        &writer,
        "POST",
        "createRoom",
        serde_json::json!({ "is_direct": true, "preset": "trusted_private_chat", "invite": [user(&reader)] }),
    );
    let dm = dm["room_id"].as_str().expect("dm").to_owned();
    // The inbox: a mention and an invite.
    app.click(".titlebar .inbox-button").await.expect("inbox");
    app.wait_for(".inbox-panel .inbox-mention", DEFAULT_WAIT).await.expect("a mention");
    app.wait_for(".inbox-panel .inbox-invite", DEFAULT_WAIT).await.expect("an invite");
    leaks(&mut app, "the inbox", &secrets, &mut found).await;
    // The invite's card.
    app.click(".inbox-panel .inbox-invite .inbox-open").await.expect("open the invite");
    app.wait_for(".invite-card", DEFAULT_WAIT).await.expect("the card");
    leaks(&mut app, "an invite card", &secrets, &mut found).await;

    // Joined, the DM is in the list and opens.
    api(&reader, "POST", &format!("rooms/{}/join", esc(&dm)), serde_json::json!({}));
    api(
        &reader,
        "PUT",
        &format!("user/{}/account_data/m.direct", esc(&user(&reader))),
        serde_json::json!({ user(&writer): [dm] }),
    );
    app.click(".rail-dms").await.expect("DMs");
    app.wait_for(".sidebar .room.dm:not(.invite-row)", DEFAULT_WAIT).await.expect("the DM");
    app.click(".sidebar .room.dm:not(.invite-row)").await.expect("open the DM");
    app.wait_for(".chat-header h2", DEFAULT_WAIT).await.expect("DM header");
    leaks(&mut app, "a DM", &secrets, &mut found).await;

    // The Friends page.
    app.click(".sidebar .friends-row").await.expect("Friends");
    app.wait_for_count(".friends-page .friend", 3, DEFAULT_WAIT).await.expect("friends");
    leaks(&mut app, "the Friends page", &secrets, &mut found).await;

    assert!(found.is_empty(), "streamer mode leaks:\n{}", found.into_iter().collect::<Vec<_>>().join("\n"));
}

/// The title bar's streamer mode button exists only while the mode is on
/// (the pill); clicking it turns the mode off and it goes away.
#[tokio::test]
async fn title_bar_streamer_button_toggles_streamer_mode() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (mut app, _reader, _writer, _room) = pair(&hs, "strt").await;
    if !app.is_gpui() {
        eprintln!("streamer mode is in the GPUI app only; skipping");
        return;
    }
    assert_eq!(app.count(".titlebar .streamer-button").await.expect("count"), 0, "no button when off");
    assert_eq!(app.count(".titlebar .streamer-badge").await.expect("count"), 0);
    flip_now(&mut app).await;
    app.wait_for(".titlebar .streamer-badge", DEFAULT_WAIT).await.expect("the pill");
    assert_eq!(app.count(".titlebar .streamer-button").await.expect("count"), 1);
    app.click(".titlebar .streamer-button").await.expect("click the pill");
    app.wait_for_count(".titlebar .streamer-badge", 0, DEFAULT_WAIT).await.expect("off");
    app.wait_for_count(".titlebar .streamer-button", 0, DEFAULT_WAIT).await.expect("button gone");
}

/// Cancel the open-channel question, which must be showing.
async fn refuse(app: &mut App, what: &str) {
    app.wait_for(".confirm-dialog .confirm-cancel", DEFAULT_WAIT)
        .await
        .unwrap_or_else(|e| panic!("{what}: no question before switching: {e}"));
    app.click(".confirm-dialog .confirm-cancel").await.expect("cancel");
    app.wait_for_count(".confirm-dialog", 0, DEFAULT_WAIT).await.expect("closed");
}

/// Every way to another room asks first while sharing: the rail's DMs and
/// server buttons, Ctrl+Alt+Down and Alt+Down; nothing changes until Open.
#[tokio::test]
async fn every_way_to_another_room_asks_while_sharing() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch alice");
    if !app.is_gpui() {
        eprintln!("streamer mode is in the GPUI app only; skipping");
        return;
    }
    login(&mut app, "alice").await;
    app.open_room("general").await.expect("general");
    app.click_text(".room", "hangout").await.expect("join the call");
    app.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT).await.expect("connected");
    app.click(".voice-panel .screen-button").await.expect("share");
    app.wait_for(".screen-quality .screen-go", DEFAULT_WAIT).await.expect("popover");
    app.click(".screen-quality .screen-go").await.expect("go live");
    app.wait_for(".titlebar .streamer-badge", DEFAULT_WAIT).await.expect("streamer mode on");

    // Get a DM and a server's room remembered, answering Open each time.
    app.click_text(".room", "general").await.expect("general");
    app.click(".confirm-dialog .confirm-go").await.expect("open general");
    app.wait_for_text(".chat-header h2", "general", DEFAULT_WAIT).await.expect("general");
    app.click(".rail-item[title=\"Direct messages\"]").await.expect("DMs");
    app.wait_for(".room.dm", DEFAULT_WAIT).await.expect("a DM listed");
    app.click(".room.dm").await.expect("a DM");
    app.wait_for(".confirm-dialog .confirm-go", DEFAULT_WAIT).await.expect("asked");
    app.click(".confirm-dialog .confirm-go").await.expect("open the DM");
    app.wait_for_count(".confirm-dialog", 0, DEFAULT_WAIT).await.expect("closed");
    app.wait_for(".chat-header h2", DEFAULT_WAIT).await.expect("DM open");
    let dm_header = app.text(".chat-header h2").await.expect("header");

    // Another server from the DMs: asked, and Cancel leaves everything.
    app.click(".rail > .rail-item.space-badge").await.expect("a server");
    refuse(&mut app, "a server button").await;
    assert_eq!(app.count(".rail-item.selected[title=\"Direct messages\"]").await.expect("count"), 1, "still in the DMs");
    assert_eq!(app.text(".chat-header h2").await.expect("header"), dm_header, "still the DM");

    // Open goes there.
    app.click(".rail > .rail-item.space-badge").await.expect("a server");
    app.wait_for(".confirm-dialog .confirm-go", DEFAULT_WAIT).await.expect("asked");
    app.click(".confirm-dialog .confirm-go").await.expect("open");
    app.wait_for_count(".rail-item.selected[title=\"Direct messages\"]", 0, DEFAULT_WAIT)
        .await
        .expect("in the server now");

    // The DMs button from inside the server: asked.
    let server_header = app.text(".chat-header h2").await.expect("header");
    app.click(".rail-item[title=\"Direct messages\"]").await.expect("DMs");
    refuse(&mut app, "the DMs button").await;
    assert_eq!(app.count(".rail-item.selected[title=\"Direct messages\"]").await.expect("count"), 0, "still in the server");
    assert_eq!(app.text(".chat-header h2").await.expect("header"), server_header);

    // Ctrl+Alt+Down (the next rail entry) and Alt+Down (the next room): asked.
    app.key_combo("body", "ArrowDown", true, true, false).await.expect("ctrl+alt+down");
    refuse(&mut app, "Ctrl+Alt+Down").await;
    assert_eq!(app.text(".chat-header h2").await.expect("header"), server_header);
    app.key_combo("body", "ArrowDown", false, true, false).await.expect("alt+down");
    refuse(&mut app, "Alt+Down").await;
    assert_eq!(app.text(".chat-header h2").await.expect("header"), server_header);
}
