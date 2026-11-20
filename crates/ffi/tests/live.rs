#![allow(clippy::expect_used)]
#![recursion_limit = "512"]
//! The FFI surface against the local homeserver, driven the way Kotlin does:
//! through listeners. Self-skips unless `MORPHEUS_TEST_HOMESERVER` is set
//! (`just hs-up hs-seed`, then `just test-ffi`).

use std::{sync::Arc, time::Duration};

use morpheus_ffi::{
    FileKind, MessageBody, MessageItem, Morpheus, ScopeKind, SuggestionKind, emoji_sections, with_emoji, MemberRow, TypingListener, Sidebar, SidebarListener, SyncListener, TimelineListener,
    TimelineRow,
};
use tokio::{sync::mpsc, time::timeout};

const WAIT: Duration = Duration::from_secs(45);

struct Feed<T>(std::sync::Mutex<mpsc::UnboundedSender<T>>);

impl<T> Feed<T> {
    fn new() -> (Arc<Self>, mpsc::UnboundedReceiver<T>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Arc::new(Self(std::sync::Mutex::new(tx))), rx)
    }
    fn push(&self, value: T) {
        let _ = self.0.lock().expect("lock").send(value);
    }
}

impl SidebarListener for Feed<Sidebar> {
    fn on_sidebar(&self, sidebar: Sidebar) {
        self.push(sidebar);
    }
}
impl TimelineListener for Feed<Vec<TimelineRow>> {
    fn on_rows(&self, rows: Vec<TimelineRow>) {
        self.push(rows);
    }
}
impl SyncListener for Feed<String> {
    fn on_sync_state(&self, state: String) {
        self.push(state);
    }
}

/// Wait for a value from `rx` that satisfies `want`.
async fn until<T>(rx: &mut mpsc::UnboundedReceiver<T>, what: &str, want: impl Fn(&T) -> bool) -> T {
    timeout(WAIT, async {
        loop {
            let value = rx.recv().await.expect("the feed ended");
            if want(&value) {
                return value;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

fn texts(rows: &[TimelineRow]) -> Vec<String> {
    rows.iter()
        .filter_map(|row| match row {
            TimelineRow::Message { message } => match &message.body {
                MessageBody::Text { text, .. } => Some(text.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn sign_in_lists_servers_and_rooms_opens_a_timeline_sends_replies_and_restores() {
    let Ok(homeserver) = std::env::var("MORPHEUS_TEST_HOMESERVER") else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping");
        return;
    };
    let dir = tempfile::tempdir().expect("dir");
    let app = Morpheus::new(dir.path().display().to_string()).expect("app");
    assert!(app.restore_session().await.expect("restore").is_none(), "a fresh folder has no session");
    assert!(app.account().is_none());

    let account = app
        .login(homeserver.clone(), "alice".into(), "alice-password".into())
        .await
        .expect("login");
    assert!(account.user_id.starts_with("@alice:"), "{account:?}");

    let (sync, mut sync_rx) = Feed::new();
    let _sync = app.watch_sync(sync).expect("watch sync");
    until(&mut sync_rx, "sync running", |s| s == "running").await;

    // Servers and rooms: general outside any server, dev under Engineering.
    let (sidebar, mut sidebar_rx) = Feed::new();
    let _sidebar = app.watch_sidebar(sidebar).await.expect("watch sidebar");
    let list = until(&mut sidebar_rx, "the channel list", |sidebar: &Sidebar| {
        let has = |key: &str, room: &str| {
            sidebar.scopes.iter().any(|scope| {
                scope.title == key && scope.sections.iter().any(|s| s.rooms.iter().any(|r| r.name == room))
            })
        };
        has("Rooms", "general") && has("Engineering", "dev")
    })
    .await;
    let kinds: Vec<_> = list.scopes.iter().map(|s| s.kind).collect();
    assert_eq!(&kinds[..2], [ScopeKind::Dms, ScopeKind::Rooms]);
    assert!(kinds.contains(&ScopeKind::Server));
    let general = list
        .scopes
        .iter()
        .flat_map(|s| &s.sections)
        .flat_map(|s| &s.rooms)
        .find(|r| r.name == "general")
        .expect("general");

    // Its timeline: text, a picture (bytes come back), a sender's header.
    let (rows, mut rows_rx) = Feed::new();
    let room = app.open_room(general.id.clone(), rows).await.expect("open");
    let mut seen = until(&mut rows_rx, "messages", |rows: &Vec<TimelineRow>| !texts(rows).is_empty()).await;
    // Test runs keep posting: page back until the seeded picture shows.
    for _ in 0..12 {
        if seen.iter().any(|row| matches!(row, TimelineRow::Message { message } if matches!(message.body, MessageBody::Image { .. }))) {
            break;
        }
        if room.load_older().await.expect("older") {
            break;
        }
        while let Ok(Some(rows)) = timeout(Duration::from_millis(800), rows_rx.recv()).await {
            seen = rows;
        }
    }
    let image = seen.iter().find_map(|row| match row {
        TimelineRow::Message { message } => match &message.body {
            MessageBody::Image { media, .. } => Some(media.clone()),
            _ => None,
        },
        _ => None,
    });
    let image = image.expect("the seeded picture in #general");
        let bytes = image.thumbnail(64, 64).await.expect("the picture's bytes");
        assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']) || bytes.starts_with(&[0xFF, 0xD8]), "a PNG or JPEG");

    // Sending shows up as a message of mine, then a reply points at it.
    let stamp = format!("ffi hello {}", std::process::id());
    room.send_text(stamp.clone()).await.expect("send");
    let sent = until(&mut rows_rx, "my message", |rows: &Vec<TimelineRow>| {
        texts(rows).contains(&stamp)
    })
    .await;
    let mine = sent.iter().find_map(|row| match row {
        TimelineRow::Message { message } if matches!(&message.body, MessageBody::Text { text, .. } if *text == stamp) => Some(message),
        _ => None,
    });
    let mine = mine.expect("the sent message");
    assert!(mine.is_own);

    // The event id arrives once the server has it.
    let with_id = until(&mut rows_rx, "my message's event id", |rows: &Vec<TimelineRow>| {
        rows.iter().any(|row| matches!(row, TimelineRow::Message { message } if message.event_id.is_some() && matches!(&message.body, MessageBody::Text { text, .. } if *text == stamp)))
    })
    .await;
    let event_id = with_id
        .iter()
        .find_map(|row| match row {
            TimelineRow::Message { message } if matches!(&message.body, MessageBody::Text { text, .. } if *text == stamp) => message.event_id.clone(),
            _ => None,
        })
        .expect("event id");

    let reply = format!("{stamp} (reply)");
    room.send_reply(reply.clone(), event_id.clone()).await.expect("reply");
    let replied = until(&mut rows_rx, "my reply", |rows: &Vec<TimelineRow>| {
        rows.iter().any(|row| matches!(row, TimelineRow::Message { message }
            if matches!(&message.body, MessageBody::Text { text, .. } if text.contains("(reply)") && text.contains(&stamp))
                && message.reply_to.is_some()))
    })
    .await;
    assert!(!replied.is_empty());

    room.toggle_reaction(event_id.clone(), "👍".into()).await.expect("react");
    until(&mut rows_rx, "my reaction", |rows: &Vec<TimelineRow>| {
        rows.iter().any(|row| matches!(row, TimelineRow::Message { message }
            if message.event_id.as_deref() == Some(event_id.as_str()) && message.reactions.iter().any(|r| r.key == "👍" && r.mine)))
    })
    .await;
    room.toggle_reaction(event_id, "👍".into()).await.expect("unreact");

    // Older messages load.
    room.load_older().await.expect("older");

    // A new launch finds the session again; signing out forgets it.
    drop(room);
    app.suspend().await.expect("suspend");
    drop(app);
    let again = Morpheus::new(dir.path().display().to_string()).expect("app again");
    let restored = again.restore_session().await.expect("restore").expect("a session");
    assert_eq!(restored.user_id, account.user_id);
    again.logout().await.expect("logout");
    let third = Morpheus::new(dir.path().display().to_string()).expect("app third");
    assert!(third.restore_session().await.expect("restore").is_none());
}

#[tokio::test]
async fn own_messages_can_be_edited_and_deleted() {
    let Ok(homeserver) = std::env::var("MORPHEUS_TEST_HOMESERVER") else {
        return;
    };
    let dir = tempfile::tempdir().expect("dir");
    let app = Morpheus::new(dir.path().display().to_string()).expect("app");
    app.login(homeserver, "alice".into(), "alice-password".into()).await.expect("login");
    let (sidebar, mut sidebar_rx) = Feed::new();
    let _sidebar = app.watch_sidebar(sidebar).await.expect("watch sidebar");
    let list = until(&mut sidebar_rx, "general", |s: &Sidebar| {
        s.scopes.iter().flat_map(|s| &s.sections).flat_map(|s| &s.rooms).any(|r| r.name == "general")
    })
    .await;
    let general = list.scopes.iter().flat_map(|s| &s.sections).flat_map(|s| &s.rooms).find(|r| r.name == "general").expect("general");
    let (rows, mut rows_rx) = Feed::new();
    let room = app.open_room(general.id.clone(), rows).await.expect("open");

    let stamp = format!("ffi edit {}", std::process::id());
    room.send_text(stamp.clone()).await.expect("send");
    fn mine<'a>(rows: &'a [TimelineRow], text: &str) -> Option<&'a MessageItem> {
        rows.iter().find_map(|row| match row {
            TimelineRow::Message { message } if matches!(&message.body, MessageBody::Text { text: t, .. } if t == text) && message.event_id.is_some() => Some(message),
            _ => None,
        })
    }
    let rows = until(&mut rows_rx, "my message with an id", |rows: &Vec<TimelineRow>| mine(rows, &stamp).is_some()).await;
    let sent = mine(&rows, &stamp).expect("sent");
    assert!(sent.can_edit && sent.can_delete, "own text can be edited and deleted");
    let id = sent.event_id.clone().expect("id");

    let new = format!("{stamp} fixed");
    room.edit_message(id.clone(), new.clone()).await.expect("edit");
    let rows = until(&mut rows_rx, "the edit", |rows: &Vec<TimelineRow>| mine(rows, &new).is_some_and(|m| m.edited)).await;
    assert!(mine(&rows, &stamp).is_none(), "the old text is gone, and the new text has no mark in it");

    room.delete_message(id).await.expect("delete");
    until(&mut rows_rx, "the deletion", |rows: &Vec<TimelineRow>| mine(rows, &new).is_none()).await;
}

#[tokio::test]
async fn a_thread_opens_on_a_message_takes_replies_and_shows_on_its_root() {
    let Ok(homeserver) = std::env::var("MORPHEUS_TEST_HOMESERVER") else {
        return;
    };
    let dir = tempfile::tempdir().expect("dir");
    let app = Morpheus::new(dir.path().display().to_string()).expect("app");
    app.login(homeserver, "alice".into(), "alice-password".into()).await.expect("login");
    let (sidebar, mut sidebar_rx) = Feed::new();
    let _sidebar = app.watch_sidebar(sidebar).await.expect("watch sidebar");
    let list = until(&mut sidebar_rx, "general", |s: &Sidebar| {
        s.scopes.iter().flat_map(|s| &s.sections).flat_map(|s| &s.rooms).any(|r| r.name == "general")
    })
    .await;
    let general = list.scopes.iter().flat_map(|s| &s.sections).flat_map(|s| &s.rooms).find(|r| r.name == "general").expect("general");
    let (rows, mut rows_rx) = Feed::new();
    let room = app.open_room(general.id.clone(), rows).await.expect("open");

    let root_text = format!("ffi thread root {}", std::process::id());
    room.send_text(root_text.clone()).await.expect("send");
    fn find<'a>(rows: &'a [TimelineRow], text: &str) -> Option<&'a MessageItem> {
        rows.iter().find_map(|row| match row {
            TimelineRow::Message { message } if matches!(&message.body, MessageBody::Text { text: t, .. } if t == text) && message.event_id.is_some() => Some(message),
            _ => None,
        })
    }
    let rows = until(&mut rows_rx, "the root with an id", |rows: &Vec<TimelineRow>| find(rows, &root_text).is_some()).await;
    let root = find(&rows, &root_text).expect("root");
    assert!(root.thread.is_none(), "no replies yet, no chip");
    let root_id = root.event_id.clone().expect("id");

    // The thread's own timeline: its root, then what is sent through it.
    let (thread_rows, mut thread_rx) = Feed::new();
    let thread = app.open_thread(general.id.clone(), root_id, thread_rows).await.expect("open thread");
    until(&mut thread_rx, "the root in its thread", |rows: &Vec<TimelineRow>| find(rows, &root_text).is_some()).await;
    let reply = format!("{root_text} reply");
    thread.send_text(reply.clone()).await.expect("send in thread");
    until(&mut thread_rx, "the reply in the thread", |rows: &Vec<TimelineRow>| find(rows, &reply).is_some()).await;

    // The room shows the root with a chip (as on the desktop, the reply is listed there too).
    let rows = until(&mut rows_rx, "the chip on the root", |rows: &Vec<TimelineRow>| {
        find(rows, &root_text).is_some_and(|m| m.thread.is_some())
    })
    .await;
    let chip = find(&rows, &root_text).and_then(|m| m.thread.as_ref()).expect("chip");
    assert_eq!(chip.count, "1 reply");
}

#[tokio::test]
async fn files_are_sent_listed_and_fetched_whole() {
    let Ok(homeserver) = std::env::var("MORPHEUS_TEST_HOMESERVER") else {
        return;
    };
    let dir = tempfile::tempdir().expect("dir");
    let app = Morpheus::new(dir.path().display().to_string()).expect("app");
    app.login(homeserver, "alice".into(), "alice-password".into()).await.expect("login");
    let (sidebar, mut sidebar_rx) = Feed::new();
    let _sidebar = app.watch_sidebar(sidebar).await.expect("watch sidebar");
    let list = until(&mut sidebar_rx, "general", |s: &Sidebar| {
        s.scopes.iter().flat_map(|s| &s.sections).flat_map(|s| &s.rooms).any(|r| r.name == "general")
    })
    .await;
    let general = list.scopes.iter().flat_map(|s| &s.sections).flat_map(|s| &s.rooms).find(|r| r.name == "general").expect("general");
    let (rows, mut rows_rx) = Feed::new();
    let room = app.open_room(general.id.clone(), rows).await.expect("open");

    let name = format!("notes-{}.txt", std::process::id());
    room.send_file(name.clone(), "text/plain".into(), b"hello from the phone".to_vec()).await.expect("send the file");
    let rows = until(&mut rows_rx, "the file row", |rows: &Vec<TimelineRow>| {
        rows.iter().any(|row| matches!(row, TimelineRow::Message { message }
            if matches!(&message.body, MessageBody::File { name: n, .. } if *n == name) && message.event_id.is_some()))
    })
    .await;
    let (kind, size, media) = rows
        .iter()
        .find_map(|row| match row {
            TimelineRow::Message { message } => match &message.body {
                MessageBody::File { name: n, kind, size, media, .. } if *n == name => Some((*kind, *size, media.clone())),
                _ => None,
            },
            _ => None,
        })
        .expect("the file");
    assert_eq!(kind, FileKind::File);
    assert_eq!(size, Some(20));
    assert_eq!(media.file().await.expect("the whole file"), b"hello from the phone");

    let movie = format!("clip-{}.mp4", std::process::id());
    room.send_file(movie.clone(), "video/mp4".into(), vec![0; 64]).await.expect("send a video");
    until(&mut rows_rx, "the video row", |rows: &Vec<TimelineRow>| {
        rows.iter().any(|row| matches!(row, TimelineRow::Message { message }
            if matches!(&message.body, MessageBody::File { name: n, kind: FileKind::Video, .. } if *n == movie)))
    })
    .await;
}

#[tokio::test]
async fn the_composer_suggests_members_emoji_and_custom_emoji_and_sends_the_picture() {
    use morpheus_core::{Core, LoginRequest, SessionStoreKind};
    let Ok(homeserver) = std::env::var("MORPHEUS_TEST_HOMESERVER") else {
        return;
    };
    // A custom emoji in alice's own pack, put there by another device of hers.
    let shortcode = format!("pk{}", std::process::id());
    let other = Core::with_session_store(tempfile::tempdir().expect("dir").keep(), SessionStoreKind::File)
        .expect("core")
        .login(LoginRequest { homeserver: homeserver.clone(), username: "alice".into(), password: "alice-password".into() })
        .await
        .expect("login");
    let mut png = Vec::new();
    {
        let img = image_png();
        png.extend_from_slice(&img);
    }
    other.add_emote(None, String::new(), shortcode.clone(), "image/png".into(), png).await.expect("add emote");

    let dir = tempfile::tempdir().expect("dir");
    let app = Morpheus::new(dir.path().display().to_string()).expect("app");
    app.login(homeserver, "alice".into(), "alice-password".into()).await.expect("login");
    let (sidebar, mut sidebar_rx) = Feed::new();
    let _sidebar = app.watch_sidebar(sidebar).await.expect("watch sidebar");
    let list = until(&mut sidebar_rx, "general", |s: &Sidebar| {
        s.scopes.iter().flat_map(|s| &s.sections).flat_map(|s| &s.rooms).any(|r| r.name == "general")
    })
    .await;
    let general = list.scopes.iter().flat_map(|s| &s.sections).flat_map(|s| &s.rooms).find(|r| r.name == "general").expect("general");
    let (rows, mut rows_rx) = Feed::new();
    let room = app.open_room(general.id.clone(), rows).await.expect("open");

    // @ finds members (by display name), and accepting writes the name in.
    let members = room.suggest("hello @bo".into()).await;
    let bob = members.iter().find(|s| s.kind == SuggestionKind::Member).expect("bob is suggested");
    assert!(bob.label.to_lowercase().starts_with("bob"), "{}", bob.label);
    assert_eq!(bob.draft_after, format!("hello @{} ", bob.label));
    // : finds unicode emoji.
    let emoji = room.suggest(":smil".into()).await;
    assert!(emoji.iter().any(|s| s.kind == SuggestionKind::Emoji), "no emoji for :smil");
    // The panel: sections with cells; a search narrows.
    assert!(emoji_sections(String::new()).len() > 3);
    let hearts = emoji_sections("heart".into());
    assert!(hearts.iter().all(|s| !s.cells.is_empty()) && !hearts.is_empty());
    assert_eq!(with_emoji("hi".into(), "😀".into()), "hi 😀 ");

    // The custom emoji is found by code, listed in the panel with a picture, and sent as one.
    let mut custom = Vec::new();
    for _ in 0..40 {
        custom = room.suggest(format!(":{}", &shortcode[..shortcode.len() - 1])).await;
        if custom.iter().any(|s| s.kind == SuggestionKind::Custom) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let found = custom.iter().find(|s| s.kind == SuggestionKind::Custom).expect("the custom emoji is suggested");
    assert_eq!(found.label, shortcode);
    assert!(found.media.is_some());
    let panel = room.custom_emoji(String::new()).await;
    assert!(panel.iter().flat_map(|p| &p.emotes).any(|e| e.shortcode == shortcode));
    let text = format!("custom :{shortcode}: {}", std::process::id());
    room.send_text(text.clone()).await.expect("send");
    until(&mut rows_rx, "the sent message", |rows: &Vec<TimelineRow>| {
        rows.iter().any(|row| matches!(row, TimelineRow::Message { message }
            if matches!(&message.body, MessageBody::Text { text: t, .. } if t.contains(&shortcode)) && message.event_id.is_some()))
    })
    .await;
    // Leave alice's pack as it was: a leftover emoji comes first in every later test's panel.
    other.remove_emote(None, String::new(), shortcode.clone()).await.expect("remove emote");
}

/// A tiny valid PNG.
fn image_png() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01,
        0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41,
        0x54, 0x78, 0x9C, 0x63, 0xF8, 0xFF, 0xFF, 0x3F, 0x00, 0x05, 0xFE, 0x02, 0xFE, 0xA7, 0x35, 0x81, 0x84, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}

/// Bob, as a plain Matrix client (no Morpheus around him).
async fn bob_client(homeserver: &str) -> matrix_sdk::Client {
    let client = matrix_sdk::Client::builder()
        .homeserver_url(homeserver)
        .build()
        .await
        .expect("client");
    client
        .matrix_auth()
        .login_username("bob", "bob-password")
        .send()
        .await
        .expect("bob signs in");
    client
}

#[tokio::test]
async fn direct_messages_invites_join_create_friends_and_the_directory() {
    use matrix_sdk::ruma::{
        api::client::room::{create_room, Visibility},
        OwnedUserId,
    };
    let Ok(homeserver) = std::env::var("MORPHEUS_TEST_HOMESERVER") else {
        return;
    };
    let dir = tempfile::tempdir().expect("dir");
    let app = Morpheus::new(dir.path().display().to_string()).expect("app");
    let account = app.login(homeserver.clone(), "alice".into(), "alice-password".into()).await.expect("login");
    let (sidebar, mut sidebar_rx) = Feed::new();
    let _sidebar = app.watch_sidebar(sidebar).await.expect("watch sidebar");
    let stamp = std::process::id();
    let bob = bob_client(&homeserver).await;
    let bob_id: OwnedUserId = bob.user_id().expect("bob").to_owned();
    let alice_id: OwnedUserId = account.user_id.as_str().try_into().expect("alice id");
    fn has_room_named(sidebar: &Sidebar, name: &str) -> bool {
        sidebar.scopes.iter().flat_map(|s| &s.sections).flat_map(|s| &s.rooms).any(|r| r.name == name)
    }
    fn has_room(sidebar: &Sidebar, id: &str) -> bool {
        sidebar.scopes.iter().flat_map(|s| &s.sections).flat_map(|s| &s.rooms).any(|r| r.id == id)
    }

    // The account has synced (a DM is looked for in what it knows).
    until(&mut sidebar_rx, "the rooms", |s: &Sidebar| has_room_named(s, "general")).await;

    // A direct message: by a bare username, again the same room, never with myself.
    let dm = app.start_dm("bob".into()).await.expect("dm");
    assert!(!dm.is_empty());
    app.start_dm(bob_id.to_string()).await.expect("a full Matrix ID works too");
    let own = app.start_dm("alice".into()).await.expect_err("not myself");
    assert!(own.to_string().contains("That is you"), "{own}");
    let nobody = app.start_dm(format!("nobody-{stamp}")).await.expect_err("no such person");
    assert!(nobody.to_string().contains("No one is called"), "{nobody}");

    // A room of my own.
    let made = app.create_room(format!("ffi made {stamp}"), String::new()).await.expect("create");
    until(&mut sidebar_rx, "the new room", |s: &Sidebar| has_room(s, &made)).await;

    // Invites: bob asks twice; I accept one and decline the other.
    let mut invited = Vec::new();
    for n in 1..=2 {
        let mut request = create_room::v3::Request::new();
        request.name = Some(format!("ffi invite {n} {stamp}"));
        request.invite = vec![alice_id.clone()];
        invited.push(bob.create_room(request).await.expect("bob's room").room_id().to_string());
    }
    let list = until(&mut sidebar_rx, "both invites", |s: &Sidebar| invited.iter().all(|id| s.invites.iter().any(|i| i.id == *id))).await;
    let first = list.invites.iter().find(|i| i.id == invited[0]).expect("first");
    assert_eq!(first.name, format!("ffi invite 1 {stamp}"));
    assert!(first.line.contains("invited you to a room"), "{}", first.line);
    assert!(!has_room(&list, &invited[0]), "an invite is not yet one of my rooms");
    app.accept_invite(invited[0].clone()).await.expect("accept");
    app.decline_invite(invited[1].clone()).await.expect("decline");
    until(&mut sidebar_rx, "accepted and declined", |s: &Sidebar| {
        has_room(s, &invited[0]) && s.invites.iter().all(|i| !invited.contains(&i.id))
    })
    .await;

    // Joining by address, and finding a room in the directory.
    let mut request = create_room::v3::Request::new();
    request.name = Some(format!("ffi public {stamp}"));
    request.visibility = Visibility::Public;
    request.preset = Some(create_room::v3::RoomPreset::PublicChat);
    let public = bob.create_room(request).await.expect("public room").room_id().to_string();
    let page = timeout(WAIT, async {
        loop {
            let page = app.public_rooms(None, format!("ffi public {stamp}"), None).await.expect("directory");
            if page.rooms.iter().any(|r| r.id == public) {
                return page;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .expect("the room is listed");
    let listed = page.rooms.iter().find(|r| r.id == public).expect("listed");
    assert_eq!(listed.name, format!("ffi public {stamp}"));
    assert!(listed.members.contains("member"), "{}", listed.members);
    let words = app.join_address("not an address".into()).await.expect_err("not an address");
    assert!(words.to_string().contains("not a room address"), "{words}");
    assert_eq!(app.join_address(public.clone()).await.expect("join by id"), public);
    until(&mut sidebar_rx, "the joined room", |s: &Sidebar| has_room(s, &public)).await;

    // Friends come and go.
    app.set_friend(bob_id.to_string(), true).await.expect("add");
    let friends = app.friends().await.expect("friends");
    assert!(friends.iter().any(|f| f.user_id == bob_id.as_str()), "bob is a friend now");
    app.set_friend(bob_id.to_string(), false).await.expect("remove");
    assert!(app.friends().await.expect("friends").iter().all(|f| f.user_id != bob_id.as_str()));
}

impl TypingListener for Feed<Option<String>> {
    fn on_typing(&self, line: Option<String>) {
        self.push(line);
    }
}

#[tokio::test]
async fn typing_members_profiles_and_a_rooms_own_settings() {
    use matrix_sdk::ruma::api::client::room::create_room;
    let Ok(homeserver) = std::env::var("MORPHEUS_TEST_HOMESERVER") else {
        return;
    };
    let dir = tempfile::tempdir().expect("dir");
    let app = Morpheus::new(dir.path().display().to_string()).expect("app");
    app.login(homeserver.clone(), "alice".into(), "alice-password".into()).await.expect("login");
    let (sidebar, mut sidebar_rx) = Feed::new();
    let _sidebar = app.watch_sidebar(sidebar).await.expect("watch sidebar");
    let stamp = std::process::id();

    // A room of alice's that bob is in.
    let bob = bob_client(&homeserver).await;
    let id = app.create_room(format!("ffi people {stamp}"), "a topic".into()).await.expect("create");
    until(&mut sidebar_rx, "the room", |s: &Sidebar| s.scopes.iter().flat_map(|s| &s.sections).flat_map(|s| &s.rooms).any(|r| r.id == id)).await;
    let room_id: matrix_sdk::ruma::OwnedRoomId = id.as_str().try_into().expect("id");
    bob.join_room_by_id(&room_id).await.expect("bob joins");
    let syncing = bob.clone();
    tokio::spawn(async move { let _ = syncing.sync(matrix_sdk::config::SyncSettings::default()).await; });
    let (rows, mut rows_rx) = Feed::new();
    let room = app.open_room(id.clone(), rows).await.expect("open");
    until(&mut rows_rx, "the joined line", |rows: &Vec<TimelineRow>| rows.iter().any(|r| matches!(r, TimelineRow::Message { message } if matches!(&message.body, MessageBody::Other { summary } if summary.contains("bob"))))).await;

    // Typing: bob's shows as a line, and goes; mine reaches bob.
    let (feed, mut typing_rx) = Feed::new();
    let _typing = room.watch_typing(feed).await.expect("watch typing");
    let bobs_room = bob.get_room(&room_id).expect("bob's room");
    bobs_room.typing_notice(true).await.expect("bob types");
    until(&mut typing_rx, "bob typing", |line: &Option<String>| line.as_deref() == Some("bob is typing…")).await;
    bobs_room.typing_notice(false).await.expect("bob stops");
    until(&mut typing_rx, "bob stopped", |line: &Option<String>| line.is_none()).await;
    let (mut seen, _guard) = {
        let (guard, rx) = bobs_room.subscribe_to_typing_notifications();
        (rx, guard)
    };
    room.renew_typing().await.expect("alice types");
    timeout(WAIT, async {
        loop {
            let users = seen.recv().await.expect("typing feed");
            if users.iter().any(|u| u.as_str().starts_with("@alice:")) {
                return;
            }
        }
    })
    .await
    .expect("bob sees alice typing");
    room.stop_typing().await.expect("alice stops");

    // Members under headings, with roles; a person's card.
    let list = room.members().await.expect("members");
    let headings: Vec<_> = list.iter().filter_map(|r| match r { MemberRow::Heading { title } => Some(title.clone()), _ => None }).collect();
    assert_eq!(headings, ["Admins — 1", "Members — 1"], "{headings:?}");
    let people: Vec<_> = list.iter().filter_map(|r| match r { MemberRow::Member { member } => Some((member.name.clone(), member.role.clone())), _ => None }).collect();
    assert_eq!(people, [("alice".to_string(), "Admin".to_string()), ("bob".to_string(), "Member".to_string())]);
    let card = room.profile(bob.user_id().expect("bob").to_string()).await.expect("card");
    assert_eq!(card.name, "bob");
    assert!(card.role.is_none() && !card.is_me);
    assert!(card.mutual_rooms.contains(&format!("ffi people {stamp}")), "{:?}", card.mutual_rooms);
    let me = room.profile(app.account().expect("account").user_id).await.expect("me");
    assert!(me.is_me && me.role.as_deref() == Some("Admin"));

    // The room's own notification setting, and leaving.
    room.set_notify_mode("mute".into()).await.expect("mute");
    // The new rule comes back through sync: it takes a moment to read as set.
    async fn becomes(room: &morpheus_ffi::RoomTimeline, want: &str) {
        timeout(WAIT, async {
            while room.notify_mode().await.expect("mode") != want {
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("the mode never became {want}"));
    }
    becomes(&room, "mute").await;
    room.set_notify_mode("all".into()).await.expect("all");
    becomes(&room, "all").await;
    let wrong = room.set_notify_mode("loud".into()).await.expect_err("not a choice");
    assert!(wrong.to_string().contains("not a notification choice"), "{wrong}");
    drop(room);
    app.leave_room(id.clone()).await.expect("leave");
    until(&mut sidebar_rx, "the room gone", |s: &Sidebar| !s.scopes.iter().flat_map(|s| &s.sections).flat_map(|s| &s.rooms).any(|r| r.id == id)).await;
}

#[tokio::test]
async fn search_finds_messages_the_new_divider_marks_the_first_unread_and_rooms_are_marked_read() {
    let Ok(homeserver) = std::env::var("MORPHEUS_TEST_HOMESERVER") else {
        return;
    };
    use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;
    let dir = tempfile::tempdir().expect("dir");
    let app = Morpheus::new(dir.path().display().to_string()).expect("app");
    app.login(homeserver.clone(), "alice".into(), "alice-password".into()).await.expect("login");
    let (sidebar, mut sidebar_rx) = Feed::new();
    let _sidebar = app.watch_sidebar(sidebar).await.expect("watch sidebar");
    let stamp = std::process::id();
    let id = app.create_room(format!("ffi unread {stamp}"), String::new()).await.expect("create");
    let unread_of = |s: &Sidebar| s.scopes.iter().flat_map(|s| &s.sections).flat_map(|s| &s.rooms).find(|r| r.id == id).map(|r| r.unread);
    until(&mut sidebar_rx, "the room", |s: &Sidebar| unread_of(s).is_some()).await;

    // Search: a word nobody else has used finds the message that has it.
    let (rows, mut rows_rx) = Feed::new();
    let room = app.open_room(id.clone(), rows).await.expect("open");
    let word = format!("okapi{stamp}");
    room.send_text(format!("an {word} in the forest")).await.expect("send");
    room.send_text("something else".into()).await.expect("send");
    until(&mut rows_rx, "both messages", |rows: &Vec<TimelineRow>| texts(rows).iter().any(|t| t == "something else") && texts(rows).iter().any(|t| t.contains(&word))).await;
    let hits = timeout(WAIT, async {
        loop {
            let hits = room.search(word.clone()).await.expect("search");
            if !hits.is_empty() {
                return hits;
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
    })
    .await
    .expect("a hit");
    assert_eq!(hits.len(), 1);
    assert!(hits[0].body.contains(&word) && hits[0].sender == "alice" && !hits[0].event_id.is_empty());
    assert!(room.search("   ".into()).await.expect("blank").is_empty());
    room.mark_read().await.expect("read");
    drop(room);

    // Bob writes two messages while alice is away: she sees them as unread...
    let bob = bob_client(&homeserver).await;
    let room_id: matrix_sdk::ruma::OwnedRoomId = id.as_str().try_into().expect("id");
    bob.join_room_by_id(&room_id).await.expect("bob joins");
    let syncing = bob.clone();
    tokio::spawn(async move { let _ = syncing.sync(matrix_sdk::config::SyncSettings::default()).await; });
    let theirs = bob.get_room(&room_id).expect("room");
    theirs.send(RoomMessageEventContent::text_plain("first unread")).await.expect("send");
    theirs.send(RoomMessageEventContent::text_plain("second unread")).await.expect("send");
    until(&mut sidebar_rx, "two unread", |s: &Sidebar| unread_of(s).unwrap_or(0) >= 2).await;

    // ...and opening the room puts the NEW divider above the first of them.
    let (rows, mut rows_rx) = Feed::new();
    let room = app.open_room(id.clone(), rows).await.expect("open");
    let rows = until(&mut rows_rx, "the divider", |rows: &Vec<TimelineRow>| rows.iter().any(|r| matches!(r, TimelineRow::New))).await;
    let at = rows.iter().position(|r| matches!(r, TimelineRow::New)).expect("divider");
    let words = |part: &[TimelineRow]| texts(part);
    assert!(words(&rows[..at]).contains(&"something else".to_string()), "what was read is above the divider");
    let below = words(&rows[at + 1..]);
    assert!(below.contains(&"first unread".to_string()) && below.contains(&"second unread".to_string()), "the unread messages are below it: {below:?}");
    assert!(!below.contains(&"something else".to_string()));
    drop(room);

    // Marking the room read takes the count away.
    app.mark_rooms_read(vec![id.clone()]).await.expect("mark read");
    until(&mut sidebar_rx, "read", |s: &Sidebar| unread_of(s) == Some(0)).await;
}

#[tokio::test]
async fn links_in_a_message_are_found_get_a_preview_address_and_previews_can_be_turned_off() {
    let Ok(homeserver) = std::env::var("MORPHEUS_TEST_HOMESERVER") else {
        return;
    };
    let dir = tempfile::tempdir().expect("dir");
    let app = Morpheus::new(dir.path().display().to_string()).expect("app");
    app.login(homeserver, "alice".into(), "alice-password".into()).await.expect("login");
    let (sidebar, mut sidebar_rx) = Feed::new();
    let _sidebar = app.watch_sidebar(sidebar).await.expect("watch sidebar");
    let id = app.create_room(format!("ffi links {}", std::process::id()), String::new()).await.expect("create");
    until(&mut sidebar_rx, "the room", |s: &Sidebar| s.scopes.iter().flat_map(|s| &s.sections).flat_map(|s| &s.rooms).any(|r| r.id == id)).await;
    let (rows, mut rows_rx) = Feed::new();
    let room = app.open_room(id, rows).await.expect("open");

    room.send_text("see https://example.org/page, ok".into()).await.expect("send");
    room.send_text("no link here".into()).await.expect("send");
    let rows = until(&mut rows_rx, "both", |rows: &Vec<TimelineRow>| texts(rows).iter().any(|t| t == "no link here")).await;
    let message = |part: &str| rows.iter().find_map(|r| match r {
        TimelineRow::Message { message } if matches!(&message.body, MessageBody::Text { text, .. } if text.contains(part)) => Some(message),
        _ => None,
    });
    let with = message("see ").expect("the message with a link");
    let MessageBody::Text { segments, .. } = &with.body else { panic!("text") };
    let shown: Vec<_> = segments.iter().map(|s| (s.text.as_str(), s.url.as_deref())).collect();
    assert_eq!(shown, [("see ", None), ("https://example.org/page", Some("https://example.org/page")), (", ok", None)]);
    assert_eq!(with.preview_url.as_deref(), Some("https://example.org/page"));
    assert!(message("no link").expect("plain").preview_url.is_none());
    // A homeserver that does no previews says nothing rather than failing.
    let _ = room.link_preview("https://example.org/page".into()).await.expect("asking is fine");

    // The settings are this device's and are kept.
    let before = app.preview_settings();
    assert!(before.enabled && !before.in_encrypted);
    app.set_preview_settings(false, true).expect("set");
    app.dismiss_preview("$one".into()).expect("dismiss");
    let again = Morpheus::new(dir.path().display().to_string()).expect("app again").preview_settings();
    assert!(!again.enabled && again.in_encrypted && again.dismissed == ["$one"]);
}

#[tokio::test]
async fn pinned_and_closed_dms_and_the_servers_order_come_from_the_accounts_layout() {
    let Ok(homeserver) = std::env::var("MORPHEUS_TEST_HOMESERVER") else {
        return;
    };
    let dir = tempfile::tempdir().expect("dir");
    let app = Morpheus::new(dir.path().display().to_string()).expect("app");
    app.login(homeserver, "alice".into(), "alice-password".into()).await.expect("login");
    let (sidebar, mut rx) = Feed::new();
    let _s = app.watch_sidebar(sidebar).await.expect("watch");
    let list = until(&mut rx, "the servers", |s: &Sidebar| s.scopes.iter().filter(|c| c.kind == ScopeKind::Server).count() >= 2).await;
    let dms = |s: &Sidebar| s.scopes.iter().find(|c| c.kind == ScopeKind::Dms).map(|c| c.sections.iter().flat_map(|s| s.rooms.iter().map(|r| (r.id.clone(), r.pinned)).collect::<Vec<_>>()).collect::<Vec<_>>()).unwrap_or_default();
    let servers = |s: &Sidebar| s.scopes.iter().filter(|c| c.kind == ScopeKind::Server).map(|c| c.key.trim_start_matches("space:").to_owned()).collect::<Vec<_>>();

    // A DM, pinned: it is first and says so; closed: it is gone until a message; both on any device.
    let dm = app.start_dm("bob".into()).await.expect("dm");
    until(&mut rx, "the DM", |s: &Sidebar| dms(s).iter().any(|(id, _)| *id == dm)).await;
    app.set_dm_mark(dm.clone(), "pinned".into(), true).await.expect("pin");
    until(&mut rx, "pinned first", |s: &Sidebar| dms(s).first().is_some_and(|(id, pinned)| *id == dm && *pinned)).await;
    app.set_dm_mark(dm.clone(), "pinned".into(), false).await.expect("unpin");
    app.set_dm_mark(dm.clone(), "closed".into(), true).await.expect("close");
    until(&mut rx, "closed", |s: &Sidebar| !dms(s).iter().any(|(id, _)| *id == dm)).await;
    app.set_dm_mark(dm.clone(), "closed".into(), false).await.expect("reopen");
    until(&mut rx, "back", |s: &Sidebar| dms(s).iter().any(|(id, pinned)| *id == dm && !*pinned)).await;
    let wrong = app.set_dm_mark(dm, "loud".into(), true).await.expect_err("not a mark");
    assert!(wrong.to_string().contains("not a mark"), "{wrong}");

    // The servers' order is the account's: reversed here, reversed in the list.
    let now = servers(&list);
    let mut reversed = now.clone();
    reversed.reverse();
    app.set_rail_order(reversed.clone()).await.expect("order");
    until(&mut rx, "the new order", |s: &Sidebar| servers(s) == reversed).await;
    app.set_rail_order(now.clone()).await.expect("put back");
    until(&mut rx, "the old order", |s: &Sidebar| servers(s) == now).await;
}

struct CallFeed(std::sync::Mutex<mpsc::UnboundedSender<morpheus_ffi::CallSnapshot>>);
impl morpheus_ffi::CallListener for CallFeed {
    fn on_call(&self, snapshot: morpheus_ffi::CallSnapshot) {
        let _ = self.0.lock().expect("lock").send(snapshot);
    }
}

async fn signed_in(homeserver: &str, user: &str) -> (tempfile::TempDir, Arc<Morpheus>) {
    let dir = tempfile::tempdir().expect("dir");
    let app = Morpheus::new(dir.path().display().to_string()).expect("app");
    app.login(homeserver.to_owned(), user.into(), format!("{user}-password")).await.expect("login");
    (dir, app)
}

/// The voice channel #hangout's id, as the channel list shows it.
async fn hangout(app: &Morpheus) -> String {
    let (feed, mut rx) = Feed::new();
    let _watch = app.watch_sidebar(feed).await.expect("sidebar");
    let sidebar = until(&mut rx, "#hangout listed", |s| {
        s.scopes.iter().any(|sc| sc.sections.iter().any(|sec| sec.rooms.iter().any(|r| r.name == "hangout")))
    })
    .await;
    let room = sidebar
        .scopes
        .iter()
        .flat_map(|s| s.sections.iter().flat_map(|sec| sec.rooms.iter()))
        .find(|r| r.name == "hangout")
        .expect("hangout");
    assert!(room.is_voice, "hangout is a voice channel");
    room.id.clone()
}

#[tokio::test]
async fn a_call_is_joined_heard_muted_watched_and_left_through_the_listener() {
    use morpheus_ffi::{CallPhase, CardKind};
    let Ok(homeserver) = std::env::var("MORPHEUS_TEST_HOMESERVER") else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping");
        return;
    };
    // Tones for microphones, test patterns for cameras and screens.
    morpheus_ffi::call_use_test_tone();
    let ((_d1, alice), (_d2, bob)) = (signed_in(&homeserver, "alice").await, signed_in(&homeserver, "bob").await);
    let room = hangout(&alice).await;
    let (alice_feed, mut alice_rx) = mpsc::unbounded_channel();
    let (bob_feed, mut bob_rx) = mpsc::unbounded_channel();

    // Alice joins: connected, listed, not muted.
    let alice_call = alice
        .join_call(room.clone(), Arc::new(CallFeed(std::sync::Mutex::new(alice_feed))))
        .await
        .expect("alice joins");
    let first = until(&mut alice_rx, "alice connected", |s| s.phase == CallPhase::Connected).await;
    assert_eq!(first.room_name, "hangout");
    assert!(first.people.iter().any(|p| p.me && p.user_id.starts_with("@alice:")), "alice is listed");
    assert!(!first.muted && !first.deafened);

    // Bob joins: each sees the other, and hears them (the tone).
    let bob_call = bob
        .join_call(room.clone(), Arc::new(CallFeed(std::sync::Mutex::new(bob_feed))))
        .await
        .expect("bob joins");
    let with_bob = until(&mut alice_rx, "alice sees bob", |s| {
        s.people.iter().any(|p| p.user_id.starts_with("@bob:") && !p.connecting)
    })
    .await;
    assert!(with_bob.people.iter().all(|p| !p.name.is_empty()));
    until(&mut bob_rx, "bob sees alice", |s| s.people.iter().any(|p| p.user_id.starts_with("@alice:"))).await;
    until(&mut alice_rx, "alice sees bob speak", |s| {
        s.people.iter().any(|p| p.user_id.starts_with("@bob:") && p.speaking)
    })
    .await;

    // Muting shows to everyone, deafening mutes, unmuting undeafens.
    bob_call.toggle_mute();
    until(&mut bob_rx, "bob muted", |s| s.muted && !s.deafened).await;
    until(&mut alice_rx, "alice sees bob muted", |s| {
        s.people.iter().any(|p| p.user_id.starts_with("@bob:") && p.muted)
    })
    .await;
    bob_call.toggle_deafen();
    until(&mut alice_rx, "alice sees bob deafened", |s| {
        s.people.iter().any(|p| p.user_id.starts_with("@bob:") && p.deafened)
    })
    .await;
    bob_call.toggle_mute();
    until(&mut bob_rx, "bob back", |s| !s.muted && !s.deafened).await;

    // Bob's camera: a tile with pictures under it.
    bob_call.set_camera(true);
    let with_camera = until(&mut alice_rx, "alice sees bob's camera", |s| {
        s.cards.iter().any(|c| c.kind == CardKind::Camera && c.user_id.starts_with("@bob:"))
    })
    .await;
    let feed = with_camera.cards.iter().find(|c| c.kind == CardKind::Camera).and_then(|c| c.feed.clone()).expect("a feed");
    let picture = timeout(WAIT, async {
        loop {
            if let Some(frame) = alice_call.frame(feed.clone()) {
                return frame;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("a picture arrives");
    assert_eq!(&picture.jpeg[..2], &[0xFF, 0xD8], "a JPEG");
    bob_call.set_camera(false);
    until(&mut alice_rx, "the camera goes", |s| !s.cards.iter().any(|c| c.kind == CardKind::Camera)).await;

    // A shared screen is a card to press Watch on: nothing arrives, and the sharer sees no watchers, until asked.
    bob_call.set_screen_share(true);
    let share = until(&mut alice_rx, "alice sees bob's share", |s| {
        s.cards.iter().any(|c| c.kind == CardKind::UnwatchedScreen && c.user_id.starts_with("@bob:"))
    })
    .await;
    let bob_id = share.cards.iter().find(|c| c.kind == CardKind::UnwatchedScreen).expect("card").user_id.clone();
    assert!(share.cards.iter().all(|c| c.kind != CardKind::Screen), "nothing watched yet");
    alice_call.watch(bob_id.clone(), true);
    let watched = until(&mut alice_rx, "the share arrives", |s| s.cards.iter().any(|c| c.kind == CardKind::Screen)).await;
    assert!(watched.cards.iter().all(|c| c.kind != CardKind::UnwatchedScreen));
    until(&mut bob_rx, "bob sees a watcher", |s| s.watchers == 1).await;
    alice_call.watch(bob_id, false);
    until(&mut bob_rx, "the watcher goes", |s| s.watchers == 0).await;
    until(&mut alice_rx, "back to a Watch card", |s| s.cards.iter().any(|c| c.kind == CardKind::UnwatchedScreen)).await;
    bob_call.set_screen_share(false);

    // Leaving: the others no longer list the person, and the call says it ended.
    bob_call.leave().await;
    until(&mut bob_rx, "bob's call ends", |s| s.phase == CallPhase::Ended).await;
    until(&mut alice_rx, "alice no longer lists bob", |s| !s.people.iter().any(|p| p.user_id.starts_with("@bob:"))).await;
    alice_call.leave().await;
    until(&mut alice_rx, "alice's call ends", |s| s.phase == CallPhase::Ended).await;
}

#[tokio::test]
async fn a_room_is_muted_for_a_while_unmuted_and_the_mute_ends_by_itself() {
    let Ok(homeserver) = std::env::var("MORPHEUS_TEST_HOMESERVER") else {
        return;
    };
    let (_dir, app) = signed_in(&homeserver, "alice").await;
    let (feed, mut rx) = Feed::new();
    let _watch = app.watch_sidebar(feed).await.expect("watch");
    let room = app.create_room(format!("mute {}", std::process::id()), String::new()).await.expect("a room");
    let state = |s: &Sidebar| s.scopes.iter().flat_map(|c| c.sections.iter().flat_map(|sec| sec.rooms.iter())).find(|r| r.id == room).map(|r| r.muted);
    until(&mut rx, "the room", |s: &Sidebar| state(s) == Some(false)).await;

    // The choices the menu offers, shortest first, the last for good.
    let choices = morpheus_ffi::mute_choices();
    assert_eq!(choices.len(), 6);
    assert_eq!(choices.last().map(|c| c.ms), Some(0), "the last is until turned back on");
    assert!(choices.iter().all(|c| !c.label.is_empty()));

    // Muted, then unmuted, on every device.
    app.mute_room(room.clone(), choices[0].ms).await.expect("mute");
    until(&mut rx, "muted", |s: &Sidebar| state(s) == Some(true)).await;
    app.unmute_room(room.clone()).await.expect("unmute");
    until(&mut rx, "unmuted", |s: &Sidebar| state(s) == Some(false)).await;

    // A mute that runs out ends: here a few seconds.
    app.mute_room(room.clone(), 3_000).await.expect("mute briefly");
    until(&mut rx, "muted again", |s: &Sidebar| state(s) == Some(true)).await;
    until(&mut rx, "the mute ended", |s: &Sidebar| state(s) == Some(false)).await;
}

#[tokio::test]
async fn ignored_people_are_listed_kept_and_given_back() {
    let Ok(homeserver) = std::env::var("MORPHEUS_TEST_HOMESERVER") else {
        return;
    };
    let (_dir, app) = signed_in(&homeserver, "alice").await;
    let bob = "@bob:localhost".to_owned();
    assert!(!app.ignored_people().await.expect("list").contains(&bob), "nobody is ignored to start with");
    app.set_ignored(bob.clone(), true).await.expect("ignore");
    assert!(app.ignored_people().await.expect("list").contains(&bob));
    // It is the account's: a second sign-in of the same person has it.
    let (_dir2, other) = signed_in(&homeserver, "alice").await;
    assert!(other.ignored_people().await.expect("list").contains(&bob), "another device sees it");
    app.set_ignored(bob.clone(), false).await.expect("unignore");
    assert!(!app.ignored_people().await.expect("list").contains(&bob));
}

#[tokio::test]
async fn a_message_lists_who_has_read_it() {
    let Ok(homeserver) = std::env::var("MORPHEUS_TEST_HOMESERVER") else {
        return;
    };
    let (_d1, alice) = signed_in(&homeserver, "alice").await;
    let (_d2, bob) = signed_in(&homeserver, "bob").await;
    let general = |app: &Arc<Morpheus>| {
        let app = app.clone();
        async move {
            let (feed, mut rx) = Feed::new();
            let _watch = app.watch_sidebar(feed).await.expect("sidebar");
            let list = until(&mut rx, "general", |s: &Sidebar| {
                s.scopes.iter().flat_map(|s| &s.sections).flat_map(|s| &s.rooms).any(|r| r.name == "general")
            })
            .await;
            list.scopes.iter().flat_map(|s| &s.sections).flat_map(|s| &s.rooms).find(|r| r.name == "general").expect("general").id.clone()
        }
    };
    let (alice_room, bob_room) = (general(&alice).await, general(&bob).await);
    let (rows, mut alice_rows) = Feed::new();
    let alices = alice.open_room(alice_room, rows).await.expect("alice opens");
    let (rows, mut bob_rows) = Feed::new();
    let bobs = bob.open_room(bob_room, rows).await.expect("bob opens");

    let stamp = format!("read by {}", std::process::id());
    alices.send_text(stamp.clone()).await.expect("send");
    fn find<'a>(rows: &'a [TimelineRow], text: &str) -> Option<&'a MessageItem> {
        rows.iter().find_map(|row| match row {
            TimelineRow::Message { message } if matches!(&message.body, MessageBody::Text { text: t, .. } if t == text) && message.event_id.is_some() => Some(message),
            _ => None,
        })
    }
    // Nobody has read it yet; then Bob reads it (his receipt), and the message says so, not naming Alice.
    let rows = until(&mut alice_rows, "the message", |rows: &Vec<TimelineRow>| find(rows, &stamp).is_some()).await;
    assert!(find(&rows, &stamp).expect("message").read_by.is_empty(), "nobody has read it");
    until(&mut bob_rows, "bob has it", |rows: &Vec<TimelineRow>| find(rows, &stamp).is_some()).await;
    bobs.mark_read().await.expect("bob reads");
    let rows = until(&mut alice_rows, "bob's receipt", |rows: &Vec<TimelineRow>| find(rows, &stamp).is_some_and(|m| !m.read_by.is_empty())).await;
    let read_by = &find(&rows, &stamp).expect("message").read_by;
    assert_eq!(read_by.len(), 1, "{read_by:?}");
    assert!(read_by[0].starts_with("@bob:"), "{read_by:?}");
}
