#![allow(clippy::expect_used)]
//! The FFI surface against the local homeserver, driven the way Kotlin does:
//! through listeners. Self-skips unless `MORPHEUS_TEST_HOMESERVER` is set
//! (`just hs-up hs-seed`, then `just test-ffi`).

use std::{sync::Arc, time::Duration};

use morpheus_ffi::{
    FileKind, MessageBody, MessageItem, Morpheus, ScopeKind, SuggestionKind, emoji_sections, with_emoji, Sidebar, SidebarListener, SyncListener, TimelineListener,
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
                MessageBody::Text { text } => Some(text.clone()),
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
        TimelineRow::Message { message } if matches!(&message.body, MessageBody::Text { text } if *text == stamp) => Some(message),
        _ => None,
    });
    let mine = mine.expect("the sent message");
    assert!(mine.is_own);

    // The event id arrives once the server has it.
    let with_id = until(&mut rows_rx, "my message's event id", |rows: &Vec<TimelineRow>| {
        rows.iter().any(|row| matches!(row, TimelineRow::Message { message } if message.event_id.is_some() && matches!(&message.body, MessageBody::Text { text } if *text == stamp)))
    })
    .await;
    let event_id = with_id
        .iter()
        .find_map(|row| match row {
            TimelineRow::Message { message } if matches!(&message.body, MessageBody::Text { text } if *text == stamp) => message.event_id.clone(),
            _ => None,
        })
        .expect("event id");

    let reply = format!("{stamp} (reply)");
    room.send_reply(reply.clone(), event_id.clone()).await.expect("reply");
    let replied = until(&mut rows_rx, "my reply", |rows: &Vec<TimelineRow>| {
        rows.iter().any(|row| matches!(row, TimelineRow::Message { message }
            if matches!(&message.body, MessageBody::Text { text } if text.contains("(reply)") && text.contains(&stamp))
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
            TimelineRow::Message { message } if matches!(&message.body, MessageBody::Text { text: t } if t == text) && message.event_id.is_some() => Some(message),
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
            if matches!(&message.body, MessageBody::Text { text: t } if t.contains(&shortcode)) && message.event_id.is_some()))
    })
    .await;
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
