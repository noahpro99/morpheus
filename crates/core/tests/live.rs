#![allow(clippy::expect_used)]
//! End-to-end tests against a local homeserver.
//!
//! They self-skip unless `MORPHEUS_TEST_HOMESERVER` is set. Bring the
//! homeserver up with `just hs-up && just hs-seed`, then run `just test-live`.

use std::time::Duration;

use morpheus_core::{
    Core, LoginRequest, MessageId, MessageKind, Session, SessionStoreKind, TimelineEntry, Vector,
    VectorDiff, VerificationHandle, VerificationStep,
};
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(30);
const SEEDED_ROOM: &str = "general";
const SEEDED_MESSAGE: &str = "Welcome to Morpheus";
const SEEDED_THREAD_ROOT: &str = "Let's plan Q4 in this thread";
const SEEDED_THREAD_REPLY: &str = "I think threads should ship first";

struct TestEnv {
    homeserver: String,
    user: String,
    password: String,
}

fn env() -> Option<TestEnv> {
    let homeserver = std::env::var("MORPHEUS_TEST_HOMESERVER").ok()?;
    Some(TestEnv {
        homeserver,
        user: std::env::var("MORPHEUS_TEST_USER").unwrap_or_else(|_| "alice".into()),
        password: std::env::var("MORPHEUS_TEST_PASSWORD")
            .unwrap_or_else(|_| "alice-password".into()),
    })
}

fn fresh_data_dir() -> std::path::PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!("morpheus-live-test-{nonce}"))
}

/// Apply diff batches until `done` is satisfied or the deadline passes.
async fn drive<T: Clone>(
    items: &mut Vector<T>,
    mut next: impl AsyncFnMut() -> Option<Vec<VectorDiff<T>>>,
    mut done: impl FnMut(&Vector<T>) -> bool,
) {
    let result = timeout(WAIT, async {
        while !done(items) {
            let Some(diffs) = next().await else { break };
            for diff in diffs {
                diff.apply(items);
            }
        }
    })
    .await;
    assert!(
        result.is_ok(),
        "timed out waiting for condition; state had {} items",
        items.len()
    );
}

/// Like [`drive`] on a room timeline, paging back through history whenever
/// updates go quiet: test runs keep posting, so seeded messages fall out of
/// the first page.
async fn drive_back(
    items: &mut Vector<TimelineEntry>,
    opened: &mut morpheus_core::OpenTimeline,
    mut done: impl FnMut(&Vector<TimelineEntry>) -> bool,
) {
    let result = timeout(WAIT, async {
        while !done(items) {
            match timeout(Duration::from_millis(500), opened.updates.next()).await {
                Ok(Some(diffs)) => diffs.into_iter().for_each(|diff| diff.apply(items)),
                Ok(None) => break,
                Err(_) => {
                    if opened.handle.paginate_backwards(50).await.unwrap_or(true) {
                        // At the start of the room: one last look, then give up.
                        while let Ok(Some(diffs)) =
                            timeout(Duration::from_millis(500), opened.updates.next()).await
                        {
                            diffs.into_iter().for_each(|diff| diff.apply(items));
                        }
                        break;
                    }
                }
            }
        }
    })
    .await;
    assert!(
        result.is_ok() && done(items),
        "not found in the timeline; it had {} items",
        items.len()
    );
}

#[tokio::test]
async fn login_list_rooms_read_and_send() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter("morpheus=debug,warn")
        .try_init();

    let core = Core::with_session_store(fresh_data_dir(), SessionStoreKind::File).expect("core");
    let session = core
        .login(LoginRequest {
            homeserver: env.homeserver,
            username: env.user.clone(),
            password: env.password,
        })
        .await
        .expect("login");
    assert_eq!(session.user_id().expect("user id").localpart(), env.user);

    // Room list: wait for the seeded room to show up.
    let mut room_updates = session.room_list().await.expect("room list");
    let mut rooms = Vector::new();
    drive(
        &mut rooms,
        async || room_updates.next().await,
        |rooms| rooms.iter().any(|r| r.name == SEEDED_ROOM),
    )
    .await;
    let general = rooms
        .iter()
        .find(|r| r.name == SEEDED_ROOM)
        .expect("general room");

    // Timeline: wait for the seeded message to be back-filled.
    let mut opened = session
        .timeline(general.id.clone())
        .await
        .expect("open timeline");
    let mut items = Vector::new();
    let has_body = |items: &Vector<TimelineEntry>, body: &str| {
        items.iter().any(|e| matches!(e, TimelineEntry::Message(m) if matches!(&m.kind, MessageKind::Text { body: t, .. } if t == body)))
    };
    drive_back(&mut items, &mut opened, |items| {
        has_body(items, SEEDED_MESSAGE)
    })
    .await;

    // Send: our message must come back as a remote echo (not just the local one).
    let body = format!("live test {}", std::process::id());
    opened.handle.send_text(body.clone()).await.expect("send");
    drive(&mut items, async || opened.updates.next().await, |items| {
        items.iter().any(|e| {
            matches!(e, TimelineEntry::Message(m) if !m.is_local_echo && m.is_own && matches!(&m.kind, MessageKind::Text { body: t, .. } if *t == body))
        })
    })
    .await;
}

#[tokio::test]
async fn opening_a_thread_timeline_shows_replies_and_posts_into_it() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter("morpheus=debug,warn")
        .try_init();

    let core = Core::with_session_store(fresh_data_dir(), SessionStoreKind::File).expect("core");
    let session = core
        .login(LoginRequest {
            homeserver: env.homeserver,
            username: env.user.clone(),
            password: env.password,
        })
        .await
        .expect("login");

    let mut room_updates = session.room_list().await.expect("room list");
    let mut rooms = Vector::new();
    drive(
        &mut rooms,
        async || room_updates.next().await,
        |rooms| rooms.iter().any(|r| r.name == SEEDED_ROOM),
    )
    .await;
    let general = rooms
        .iter()
        .find(|r| r.name == SEEDED_ROOM)
        .expect("general room")
        .id
        .clone();

    // Find the seeded thread root in the live timeline to get its event id.
    let mut opened = session
        .timeline(general.clone())
        .await
        .expect("open timeline");
    let mut items = Vector::new();
    let is_root = |m: &morpheus_core::Message| matches!(&m.kind, MessageKind::Text { body, .. } if body == SEEDED_THREAD_ROOT);
    drive_back(&mut items, &mut opened, |items| {
        items
            .iter()
            .any(|e| matches!(e, TimelineEntry::Message(m) if is_root(m)))
    })
    .await;
    let root_event_id = items
        .iter()
        .find_map(|e| match e {
            TimelineEntry::Message(m) if is_root(m) => match &m.id {
                MessageId::Event(id) => Some(id.clone()),
                MessageId::Transaction(_) => None,
            },
            _ => None,
        })
        .expect("thread root event id");

    // Open its thread: the seeded reply should be there.
    // (Paging back: every run adds a reply, so the seeded one gets older.)
    let mut thread = session
        .thread(general.clone(), root_event_id.clone())
        .await
        .expect("open thread");
    let mut thread_items = Vector::new();
    drive_back(&mut thread_items, &mut thread, |items| {
        items.iter().any(|e| matches!(e, TimelineEntry::Message(m) if matches!(&m.kind, MessageKind::Text { body, .. } if body == SEEDED_THREAD_REPLY)))
    })
    .await;

    // Sending through the thread handle posts into the thread, not the room.
    let body = format!("live thread test {}", std::process::id());
    thread
        .handle
        .send_text(body.clone())
        .await
        .expect("send into thread");
    drive(&mut thread_items, async || thread.updates.next().await, |items| {
        items.iter().any(|e| {
            matches!(e, TimelineEntry::Message(m) if !m.is_local_echo && m.is_own && matches!(&m.kind, MessageKind::Text { body: t, .. } if *t == body))
        })
    })
    .await;

    // And the sent message is tagged with the thread it belongs to.
    let sent = thread_items
        .iter()
        .find_map(|e| match e {
            TimelineEntry::Message(m)
                if !m.is_local_echo
                    && matches!(&m.kind, MessageKind::Text { body: t, .. } if *t == body) =>
            {
                Some(m.clone())
            }
            _ => None,
        })
        .expect("sent message");
    assert_eq!(sent.thread_root, Some(root_event_id));
}

#[tokio::test]
async fn typing_notifications_propagate_between_users() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter("morpheus=debug,warn")
        .try_init();

    let login = |user: &str, password: &str| {
        let hs = env.homeserver.clone();
        let (user, password) = (user.to_owned(), password.to_owned());
        async move {
            let core =
                Core::with_session_store(fresh_data_dir(), SessionStoreKind::File).expect("core");
            core.login(LoginRequest {
                homeserver: hs,
                username: user,
                password,
            })
            .await
            .expect("login")
        }
    };
    let alice = login("alice", "alice-password").await;
    let bob = login("bob", "bob-password").await;

    // Both find #general through their room lists.
    let mut alice_rooms = alice.room_list().await.expect("room list");
    let mut rooms = Vector::new();
    drive(
        &mut rooms,
        async || alice_rooms.next().await,
        |rooms| rooms.iter().any(|r| r.name == SEEDED_ROOM),
    )
    .await;
    let general = rooms
        .iter()
        .find(|r| r.name == SEEDED_ROOM)
        .expect("general")
        .id
        .clone();

    let mut bob_rooms = bob.room_list().await.expect("bob room list");
    let mut rooms = Vector::new();
    drive(
        &mut rooms,
        async || bob_rooms.next().await,
        |rooms| rooms.iter().any(|r| r.id == general),
    )
    .await;

    let mut typing = alice
        .typing(general.clone())
        .await
        .expect("typing subscription");
    let bob_timeline = bob.timeline(general).await.expect("bob timeline");

    // Keep renewing while waiting, in case the first notice races the subscription.
    let result = timeout(WAIT, async {
        loop {
            bob_timeline
                .handle
                .renew_typing()
                .await
                .expect("renew typing");
            match timeout(Duration::from_secs(3), typing.next()).await {
                Ok(Some(names)) if names.iter().any(|n| n == "bob") => break names,
                Ok(Some(_)) | Err(_) => continue,
                Ok(None) => panic!("typing stream ended"),
            }
        }
    })
    .await;
    assert!(result.is_ok(), "alice never saw bob typing");
}

/// `dev/homeserver/seed.sh` puts #dev inside the "Engineering" space and
/// leaves #general and #random outside any space. `RoomSummary::parent_spaces`
/// should reflect that reciprocal `m.space.parent`/`m.space.child` relationship.
#[tokio::test]
async fn room_summaries_expose_space_membership() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter("morpheus=debug,warn")
        .try_init();

    let core = Core::with_session_store(fresh_data_dir(), SessionStoreKind::File).expect("core");
    let session = core
        .login(LoginRequest {
            homeserver: env.homeserver,
            username: env.user,
            password: env.password,
        })
        .await
        .expect("login");

    let mut room_updates = session.room_list().await.expect("room list");
    let mut rooms = Vector::new();
    drive(
        &mut rooms,
        async || room_updates.next().await,
        |rooms| {
            // Wait for all the rooms involved, with #dev's parent space
            // relationship resolved (it arrives asynchronously).
            let names: Vec<_> = rooms.iter().map(|r| r.name.as_str()).collect();
            names.contains(&SEEDED_ROOM)
                && names.contains(&"random")
                && names.contains(&"Engineering")
                && rooms
                    .iter()
                    .any(|r| r.name == "dev" && !r.parent_spaces.is_empty())
        },
    )
    .await;

    let engineering = rooms
        .iter()
        .find(|r| r.is_space && r.name == "Engineering")
        .expect("engineering space in room list");
    let dev = rooms.iter().find(|r| r.name == "dev").expect("dev room");
    let general = rooms
        .iter()
        .find(|r| r.name == SEEDED_ROOM)
        .expect("general room");
    let random = rooms
        .iter()
        .find(|r| r.name == "random")
        .expect("random room");

    assert_eq!(
        dev.parent_spaces,
        vec![engineering.id.clone()],
        "dev is in Engineering"
    );
    assert!(
        general.parent_spaces.is_empty(),
        "general has no parent space"
    );
    assert!(
        random.parent_spaces.is_empty(),
        "random has no parent space"
    );
}
#[tokio::test]
async fn fetch_full_image_media() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter("morpheus=debug,warn")
        .try_init();

    let core = Core::with_session_store(fresh_data_dir(), SessionStoreKind::File).expect("core");
    let session = core
        .login(LoginRequest {
            homeserver: env.homeserver,
            username: env.user,
            password: env.password,
        })
        .await
        .expect("login");

    let mut room_updates = session.room_list().await.expect("room list");
    let mut rooms = Vector::new();
    drive(
        &mut rooms,
        async || room_updates.next().await,
        |rooms| rooms.iter().any(|r| r.name == SEEDED_ROOM),
    )
    .await;
    let general = rooms
        .iter()
        .find(|r| r.name == SEEDED_ROOM)
        .expect("general room")
        .id
        .clone();

    let mut opened = session.timeline(general).await.expect("open timeline");
    let mut items = Vector::new();
    let find_image = |items: &Vector<TimelineEntry>| {
        items.iter().find_map(|e| match e {
            TimelineEntry::Message(m) => match &m.kind {
                MessageKind::Image { media, .. } => Some(media.clone()),
                _ => None,
            },
            _ => None,
        })
    };
    drive_back(&mut items, &mut opened, |items| find_image(items).is_some()).await;
    let media = find_image(&items).expect("seeded image message");

    let fetched = session
        .media_file(media)
        .await
        .expect("media_file call")
        .expect("full image bytes");
    assert!(!fetched.bytes.is_empty(), "full image is non-empty");
    assert!(
        fetched.mime().starts_with("image/"),
        "sniffed mime should be an image type, got {}",
        fetched.mime()
    );
}
#[tokio::test]
async fn fully_read_marker_sits_above_first_unread_and_clears_when_caught_up() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter("morpheus=debug,warn")
        .try_init();

    let login = |user: &str, password: &str| {
        let hs = env.homeserver.clone();
        let (user, password) = (user.to_owned(), password.to_owned());
        async move {
            let core =
                Core::with_session_store(fresh_data_dir(), SessionStoreKind::File).expect("core");
            core.login(LoginRequest {
                homeserver: hs,
                username: user,
                password,
            })
            .await
            .expect("login")
        }
    };
    let alice = login("alice", "alice-password").await;
    let bob = login("bob", "bob-password").await;

    // Both find #general through their room lists.
    let mut alice_rooms = alice.room_list().await.expect("alice room list");
    let mut rooms = Vector::new();
    drive(
        &mut rooms,
        async || alice_rooms.next().await,
        |rooms| rooms.iter().any(|r| r.name == SEEDED_ROOM),
    )
    .await;
    let general = rooms
        .iter()
        .find(|r| r.name == SEEDED_ROOM)
        .expect("general")
        .id
        .clone();

    let mut bob_rooms = bob.room_list().await.expect("bob room list");
    let mut rooms = Vector::new();
    drive(
        &mut rooms,
        async || bob_rooms.next().await,
        |rooms| rooms.iter().any(|r| r.id == general),
    )
    .await;

    // Alice opens the room first (as if she'd left it open), then bob posts
    // a brand new message she hasn't seen yet.
    let alice_opened = alice
        .timeline(general.clone())
        .await
        .expect("alice timeline");
    let mut alice_updates = alice_opened.updates;
    let mut alice_items = Vector::new();
    // Caught up to here first: earlier runs leave #general full of
    // messages she never read, and her marker far out of view.
    alice_opened
        .handle
        .mark_as_fully_read()
        .await
        .expect("catch up first");

    let bob_opened = bob.timeline(general).await.expect("bob timeline");
    let body = format!("marker test {}", std::process::id());
    bob_opened
        .handle
        .send_text(body.clone())
        .await
        .expect("bob send");

    let has_body = |items: &Vector<TimelineEntry>, body: &str| {
        items.iter().any(|e| matches!(e, TimelineEntry::Message(m) if matches!(&m.kind, MessageKind::Text { body: t, .. } if t == body)))
    };
    drive(
        &mut alice_items,
        async || alice_updates.next().await,
        |items| has_body(items, &body),
    )
    .await;

    // The read-marker virtual item should sit chronologically before (above)
    // bob's still-unread message.
    let marker_before_message = |items: &Vector<TimelineEntry>| {
        let marker_idx = items
            .iter()
            .position(|e| matches!(e, TimelineEntry::ReadMarker));
        let message_idx = items.iter().position(|e| {
            matches!(e, TimelineEntry::Message(m) if matches!(&m.kind, MessageKind::Text { body: t, .. } if *t == body))
        });
        matches!((marker_idx, message_idx), (Some(mi), Some(gi)) if mi < gi)
    };
    assert!(
        marker_before_message(&alice_items),
        "the NEW marker should sit above bob's unread message"
    );

    // Catching up (what happens when alice leaves the room, or sends a
    // message of her own) advances the marker past the message.
    alice_opened
        .handle
        .mark_as_fully_read()
        .await
        .expect("mark fully read");
    drive(
        &mut alice_items,
        async || alice_updates.next().await,
        |items| !marker_before_message(items),
    )
    .await;
}

#[tokio::test]
async fn homeserver_advertises_password_login() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let core = Core::with_session_store(fresh_data_dir(), SessionStoreKind::File).expect("core");
    let flows = core
        .login_flows(env.homeserver.clone())
        .await
        .expect("login flows");
    eprintln!("{} offers: {flows:?}", env.homeserver);
    assert!(flows.password, "password login should be offered");
}

/// Wait until `session` can see at least one other device of the same user,
/// which both [`Session::request_self_verification`] (to pick a target) and
/// the other device's acceptance need.
async fn wait_for_second_device(session: &Session) {
    let result = timeout(WAIT, async {
        loop {
            let devices = session.own_devices().await.expect("own devices");
            if devices.len() >= 2 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    })
    .await;
    assert!(
        result.is_ok(),
        "the other device never showed up in the device list"
    );
}

/// Drive a verification handle until it reports the emoji short auth string,
/// ignoring earlier steps (`Requested`, `Ready`) along the way.
async fn wait_for_emojis(handle: &mut VerificationHandle) -> Vec<(String, String)> {
    let result = timeout(WAIT, async {
        loop {
            match handle.next().await {
                Some(VerificationStep::Emojis(emojis)) => return emojis,
                Some(VerificationStep::Cancelled(reason)) => {
                    panic!("verification was cancelled: {reason}")
                }
                Some(_) => continue,
                None => panic!("verification stream ended before showing emoji"),
            }
        }
    })
    .await;
    result.expect("timed out waiting for emoji")
}

/// Drive a verification handle until it reports `Done`, ignoring
/// intermediate steps (`Confirmed`, repeated `Emojis`) along the way.
async fn wait_for_done(handle: &mut VerificationHandle) {
    let result = timeout(WAIT, async {
        loop {
            match handle.next().await {
                Some(VerificationStep::Done) => return,
                Some(VerificationStep::Cancelled(reason)) => {
                    panic!("verification was cancelled: {reason}")
                }
                Some(_) => continue,
                None => panic!("verification stream ended before completing"),
            }
        }
    })
    .await;
    assert!(
        result.is_ok(),
        "timed out waiting for verification to finish"
    );
}

#[tokio::test]
async fn self_verification_emoji_sas_between_two_devices() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter("morpheus=debug,warn")
        .try_init();

    let login = |hs: String, user: String, password: String| async move {
        let core =
            Core::with_session_store(fresh_data_dir(), SessionStoreKind::File).expect("core");
        core.login(LoginRequest {
            homeserver: hs,
            username: user,
            password,
        })
        .await
        .expect("login")
    };

    // Two `Core`s with separate data dirs, logged in as the same user, are
    // two devices of the same account.
    let device_a = login(
        env.homeserver.clone(),
        env.user.clone(),
        env.password.clone(),
    )
    .await;
    let device_b = login(env.homeserver, env.user, env.password).await;

    wait_for_second_device(&device_a).await;
    wait_for_second_device(&device_b).await;

    // B starts listening for incoming requests before A asks to verify.
    let mut incoming = device_b
        .verification_requests()
        .await
        .expect("verification requests");

    let mut outgoing = device_a
        .request_self_verification()
        .await
        .expect("request self verification");

    let mut incoming_handle = timeout(WAIT, incoming.next())
        .await
        .expect("timed out waiting for the incoming request")
        .expect("incoming verification stream ended");
    assert!(!incoming_handle.we_started());
    assert!(outgoing.we_started());

    incoming_handle.accept().await.expect("accept");

    let a_emojis = wait_for_emojis(&mut outgoing).await;
    let b_emojis = wait_for_emojis(&mut incoming_handle).await;
    assert_eq!(a_emojis, b_emojis, "both devices must show the same emoji");

    outgoing.confirm_match().await.expect("confirm match on A");
    incoming_handle
        .confirm_match()
        .await
        .expect("confirm match on B");

    wait_for_done(&mut outgoing).await;
    wait_for_done(&mut incoming_handle).await;

    // The trust change is persisted just after the `Done` signal, so poll briefly.
    let sees_other_verified = |session: Session| async move {
        timeout(WAIT, async {
            loop {
                let devices = session.own_devices().await.expect("devices");
                if devices.iter().any(|d| !d.is_current && d.verified) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        })
        .await
        .is_ok()
    };
    assert!(
        sees_other_verified(device_a).await,
        "A should see B as verified"
    );
    assert!(
        sees_other_verified(device_b).await,
        "B should see A as verified"
    );
}

/// Display name, avatar upload and presence status round-trip through the
/// profile API. Runs as carol so the alice display name other tests match
/// on stays untouched.
#[tokio::test]
async fn profile_name_avatar_and_status_round_trip() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let core = Core::with_session_store(fresh_data_dir(), SessionStoreKind::File).expect("core");
    let session = core
        .login(LoginRequest {
            homeserver: env.homeserver,
            username: "carol".into(),
            password: "carol-password".into(),
        })
        .await
        .expect("login");

    let before = session.own_profile().await.expect("profile");
    assert_eq!(before.user_id.as_str(), "@carol:localhost");

    let name = format!("Carol {}", std::process::id());
    session
        .set_display_name(name.clone())
        .await
        .expect("set name");
    session
        .set_status(Some("In a meeting".into()))
        .await
        .expect("set status");

    // A 1x1 PNG is enough for the upload path.
    let png = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==",
    )
    .expect("png");
    let path = fresh_data_dir().with_extension("png");
    std::fs::write(&path, png).expect("write png");
    session.set_avatar(path.clone()).await.expect("set avatar");
    let _ = std::fs::remove_file(path);

    let after = session.own_profile().await.expect("profile");
    assert_eq!(after.name, name);
    assert!(after.avatar.is_some(), "avatar uploaded");
    assert_eq!(after.status.as_deref(), Some("In a meeting"));

    // Clearing the status removes it again; restore the seeded name.
    session.set_status(None).await.expect("clear status");
    session
        .set_display_name("carol".into())
        .await
        .expect("restore name");
    let restored = session.own_profile().await.expect("profile");
    assert_eq!(restored.status, None);
    assert_eq!(restored.name, "carol");
}

/// Moderation through the core: permissions, kick, invite back, ban, unban,
/// and role changes, all on the seeded #moderation room (restored by seed).
#[tokio::test]
async fn admin_can_kick_invite_ban_unban_and_change_roles() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let core = Core::with_session_store(fresh_data_dir(), SessionStoreKind::File).expect("core");
    let session = core
        .login(LoginRequest {
            homeserver: env.homeserver,
            username: "alice".into(),
            password: "alice-password".into(),
        })
        .await
        .expect("login");
    let room_id = {
        let mut rooms = Vector::new();
        let mut updates = session.room_list().await.expect("room list");
        drive(
            &mut rooms,
            async || updates.next().await,
            |rooms| rooms.iter().any(|r| r.name == "moderation"),
        )
        .await;
        rooms
            .iter()
            .find(|r| r.name == "moderation")
            .expect("moderation room")
            .id
            .clone()
    };
    let carol: morpheus_core::OwnedUserId = "@carol:localhost".parse().expect("user id");

    let perms = session
        .my_permissions(room_id.clone())
        .await
        .expect("perms");
    assert!(perms.can_kick && perms.can_ban && perms.can_invite && perms.can_change_roles);

    let has_carol =
        |members: &Vec<morpheus_core::Member>| members.iter().any(|m| m.user_id == carol);
    assert!(has_carol(
        &session.members(room_id.clone()).await.expect("members")
    ));

    session
        .set_role(
            room_id.clone(),
            carol.clone(),
            morpheus_core::MemberRole::Moderator,
        )
        .await
        .expect("promote");
    let members = session.members(room_id.clone()).await.expect("members");
    let carol_member = members.iter().find(|m| m.user_id == carol).expect("carol");
    assert_eq!(carol_member.role, morpheus_core::MemberRole::Moderator);
    session
        .set_role(
            room_id.clone(),
            carol.clone(),
            morpheus_core::MemberRole::Member,
        )
        .await
        .expect("demote");

    session
        .kick(room_id.clone(), carol.clone(), Some("test".into()))
        .await
        .expect("kick");
    // Inviting back works (she is kicked, not banned)...
    session
        .invite(room_id.clone(), carol.clone())
        .await
        .expect("invite after kick");
    // ...and banning then unbanning both succeed.
    session
        .ban(room_id.clone(), carol.clone(), None)
        .await
        .expect("ban");
    session
        .unban(room_id.clone(), carol.clone())
        .await
        .expect("unban");
    session
        .invite(room_id.clone(), carol.clone())
        .await
        .expect("invite after unban");

    // Blocking is account-wide and reversible.
    session.block(carol.clone(), true).await.expect("block");
    session.block(carol.clone(), false).await.expect("unblock");

    // A DM lookup is stable: asking twice yields the same room.
    let bob: morpheus_core::OwnedUserId = "@bob:localhost".parse().expect("user id");
    let first = session.dm_with(bob.clone()).await.expect("dm");
    let second = session.dm_with(bob).await.expect("dm again");
    assert_eq!(first, second);
}

/// Space lifecycle through the core: create a server, a category in it,
/// a room in the category, privacy and per-room profile, then leave it all.
#[tokio::test]
async fn space_create_category_privacy_profile_and_leave() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "morpheus=debug,warn".into()))
        .try_init();
    let core = Core::with_session_store(fresh_data_dir(), SessionStoreKind::File).expect("core");
    let session = core
        .login(LoginRequest {
            homeserver: env.homeserver,
            username: "alice".into(),
            password: "alice-password".into(),
        })
        .await
        .expect("login");
    let t = std::time::Instant::now();
    let server = session
        .create_space(format!("live srv {}", std::process::id()), None)
        .await
        .expect("server");
    eprintln!(
        "server created in {:?} (before the room list sync)",
        t.elapsed()
    );
    // Keep the room list alive so sync runs like it does in the app.
    let mut updates = session.room_list().await.expect("room list");
    let mut rooms = Vector::new();
    drive(
        &mut rooms,
        async || updates.next().await,
        |rooms| !rooms.is_empty(),
    )
    .await;

    let t = std::time::Instant::now();
    let category = session
        .create_space("live category".into(), Some(server.clone()))
        .await
        .expect("category");
    eprintln!("category created in {:?}", t.elapsed());
    let t = std::time::Instant::now();
    let room = session
        .create_room("live chatter".into(), String::new(), Some(category.clone()))
        .await
        .expect("room");
    eprintln!("room created in {:?}", t.elapsed());

    session
        .set_privacy(
            server.clone(),
            morpheus_core::Privacy {
                public: false,
                history: morpheus_core::History::Joined,
            },
        )
        .await
        .expect("privacy");
    let privacy = session.privacy(server.clone()).await.expect("read privacy");
    assert!(!privacy.public);
    assert_eq!(privacy.history, morpheus_core::History::Joined);

    session
        .set_room_profile(
            vec![server.clone(), room.clone()],
            "Alice (live)".into(),
            None,
        )
        .await
        .expect("room profile");
    let profile = session
        .room_profile(room.clone())
        .await
        .expect("read profile");
    assert_eq!(profile.name, "Alice (live)");

    session
        .leave_space(server, vec![category, room])
        .await
        .expect("leave");
}

/// Rooms in common are computed from locally known membership, so after the
/// members of a room were loaded it must list that room.
#[tokio::test]
async fn mutual_rooms_lists_rooms_shared_with_a_user() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let core = Core::with_session_store(fresh_data_dir(), SessionStoreKind::File).expect("core");
    let session = core
        .login(LoginRequest {
            homeserver: env.homeserver,
            username: "alice".into(),
            password: "alice-password".into(),
        })
        .await
        .expect("login");
    let mut updates = session.room_list().await.expect("room list");
    let mut rooms = Vector::new();
    drive(
        &mut rooms,
        async || updates.next().await,
        |rooms| rooms.iter().any(|r| r.name == "general"),
    )
    .await;
    let general = rooms
        .iter()
        .find(|r| r.name == "general")
        .expect("general")
        .id
        .clone();
    let members = session.members(general.clone()).await.expect("members");
    assert!(
        members
            .iter()
            .any(|m| m.user_id.as_str() == "@bob:localhost"),
        "bob is in general"
    );

    let bob: morpheus_core::OwnedUserId = "@bob:localhost".parse().expect("user id");
    let mutual = session.mutual_rooms(bob).await.expect("mutual");
    eprintln!("mutual rooms with bob: {mutual:?}");
    assert!(
        mutual.iter().any(|(id, _)| *id == general),
        "general is shared with bob"
    );
}

#[tokio::test]
async fn invites_name_the_inviter_and_can_be_declined_then_accepted() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let login = |user: &str| {
        let core =
            Core::with_session_store(fresh_data_dir(), SessionStoreKind::File).expect("core");
        core.login(LoginRequest {
            homeserver: env.homeserver.clone(),
            username: user.into(),
            password: format!("{user}-password"),
        })
    };
    let alice = login("alice").await.expect("alice");
    let bob = login("bob").await.expect("bob");
    let name = format!("live invite {}", std::process::id());
    let room_id = alice
        .create_room(name.clone(), String::new(), None)
        .await
        .expect("create room");
    let bob_id: morpheus_core::OwnedUserId = "@bob:localhost".parse().expect("user id");
    alice
        .invite(room_id.clone(), bob_id.clone())
        .await
        .expect("invite");

    let mut rooms = Vector::new();
    let mut updates = bob.room_list().await.expect("room list");
    let pending = |rooms: &Vector<morpheus_core::RoomSummary>| {
        rooms
            .iter()
            .any(|r| r.id == room_id && r.invited && r.name == name)
    };
    drive(&mut rooms, async || updates.next().await, pending).await;
    let invite = rooms
        .iter()
        .find(|r| r.id == room_id)
        .expect("invite listed");
    assert_eq!(invite.name, name);
    assert_eq!(invite.inviter.as_deref(), Some("alice"), "inviter named");

    // Decline: the invite leaves the list.
    bob.leave_room(room_id.clone()).await.expect("decline");
    drive(
        &mut rooms,
        async || updates.next().await,
        |rooms| !rooms.iter().any(|r| r.id == room_id),
    )
    .await;

    // Invite again and accept: the entry flips to joined.
    alice
        .invite(room_id.clone(), bob_id)
        .await
        .expect("re-invite");
    drive(&mut rooms, async || updates.next().await, pending).await;
    bob.join_room(room_id.clone()).await.expect("accept");
    drive(
        &mut rooms,
        async || updates.next().await,
        |rooms| rooms.iter().any(|r| r.id == room_id && !r.invited),
    )
    .await;
}

/// Runs only where a Secret Service answers (`MORPHEUS_TEST_KEYRING=1`).
#[tokio::test]
async fn keyring_store_round_trips_a_session() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    if std::env::var_os("MORPHEUS_TEST_KEYRING").is_none() {
        eprintln!("MORPHEUS_TEST_KEYRING not set; skipping keyring test");
        return;
    }
    let dir = fresh_data_dir();
    let core = Core::with_session_store(&dir, SessionStoreKind::Keyring).expect("core");
    let session = core
        .login(LoginRequest {
            homeserver: env.homeserver.clone(),
            username: "alice".into(),
            password: "alice-password".into(),
        })
        .await
        .expect("login");
    assert!(
        !dir.join("session.enc").exists(),
        "keyring store writes no file"
    );
    drop(session);
    let core = Core::with_session_store(&dir, SessionStoreKind::Keyring).expect("core again");
    let restored = core
        .restore_session()
        .await
        .expect("restore")
        .expect("a stored session");
    assert_eq!(
        restored.user_id().map(|u| u.to_string()).as_deref(),
        Some("@alice:localhost")
    );
    restored.logout().await.expect("logout");
    let core =
        Core::with_session_store(&dir, SessionStoreKind::Keyring).expect("core after logout");
    assert!(
        core.restore_session()
            .await
            .expect("restore after logout")
            .is_none(),
        "entry cleared"
    );
}

/// Serves `{"unifiedpush":{"gateway":"matrix"}}` at the Matrix gateway path,
/// like a push server with a built-in gateway (ntfy). Returns its origin.
async fn gateway_push_server() -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut request = [0u8; 2048];
            let _ = socket.read(&mut request).await;
            let body = r#"{"unifiedpush":{"gateway":"matrix"}}"#;
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(reply.as_bytes()).await;
        }
    });
    format!("http://127.0.0.1:{port}")
}

#[tokio::test]
async fn push_endpoints_register_a_pusher_through_the_right_gateway() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let core = Core::with_session_store(fresh_data_dir(), SessionStoreKind::File).expect("core");
    let session = core
        .login(LoginRequest {
            homeserver: env.homeserver,
            username: "carol".into(),
            password: "carol-password".into(),
        })
        .await
        .expect("login");

    // A push server without its own gateway (Sunup's) goes through the
    // public UnifiedPush gateway.
    let nonce = std::process::id();
    let plain = format!("https://push.example.invalid/wpush/{nonce}");
    let gateway = session
        .register_push(plain.clone(), "Morpheus test".into())
        .await
        .expect("register");
    assert_eq!(gateway, morpheus_core::DEFAULT_PUSH_GATEWAY);

    // One with a built-in gateway (ntfy) is used directly.
    let origin = gateway_push_server().await;
    let own = format!("{origin}/up{nonce}");
    let gateway = session
        .register_push(own.clone(), "Morpheus test".into())
        .await
        .expect("register");
    assert_eq!(gateway, format!("{origin}/_matrix/push/v1/notify"));

    let pushers = session.push_registrations().await.expect("pushers");
    for (endpoint, gateway) in [
        (&plain, morpheus_core::DEFAULT_PUSH_GATEWAY.to_owned()),
        (&own, format!("{origin}/_matrix/push/v1/notify")),
    ] {
        let found = pushers
            .iter()
            .find(|p| &p.pushkey == endpoint)
            .unwrap_or_else(|| panic!("no pusher for {endpoint} in {pushers:?}"));
        assert_eq!(found.app_id, morpheus_core::PUSH_APP_ID);
        assert_eq!(found.gateway, gateway);
    }

    // Unregistering removes it again.
    for endpoint in [&plain, &own] {
        session
            .unregister_push(endpoint.clone())
            .await
            .expect("unregister");
    }
    let pushers = session.push_registrations().await.expect("pushers");
    assert!(
        !pushers
            .iter()
            .any(|p| p.pushkey == plain || p.pushkey == own),
        "pushers left behind: {pushers:?}"
    );
}

#[tokio::test]
async fn default_notification_modes_round_trip() {
    use morpheus_core::NotifyMode;
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let core = Core::with_session_store(fresh_data_dir(), SessionStoreKind::File).expect("core");
    let session = core
        .login(LoginRequest {
            homeserver: env.homeserver,
            username: "carol".into(),
            password: "carol-password".into(),
        })
        .await
        .expect("login");

    for dm in [false, true] {
        let before = session.default_notify_mode(dm).await.expect("read");
        let other = if before == NotifyMode::AllMessages {
            NotifyMode::MentionsOnly
        } else {
            NotifyMode::AllMessages
        };
        session
            .set_default_notify_mode(dm, other)
            .await
            .expect("set");
        assert_eq!(
            session.default_notify_mode(dm).await.expect("read"),
            other,
            "dm={dm}"
        );
        session
            .set_default_notify_mode(dm, before)
            .await
            .expect("restore");
        assert_eq!(
            session.default_notify_mode(dm).await.expect("read"),
            before,
            "dm={dm}"
        );
    }
}

#[tokio::test]
async fn two_accounts_side_by_side_switch_without_signing_in_again() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let dir = fresh_data_dir();
    let core = Core::with_session_store(&dir, SessionStoreKind::File).expect("core");
    let login = |user: &str| LoginRequest {
        homeserver: env.homeserver.clone(),
        username: user.into(),
        password: format!("{user}-password"),
    };
    let user = |s: &Session| s.user_id().expect("user").localpart().to_owned();

    let alice = core.login(login("alice")).await.expect("alice");
    assert_eq!(user(&alice), "alice");
    let accounts = core.accounts().await.expect("accounts");
    assert_eq!(accounts.len(), 1);
    assert!(accounts[0].active);

    // Adding a second account leaves the first stored.
    alice.suspend().await.expect("suspend alice");
    let bob = core.login(login("bob")).await.expect("bob");
    assert_eq!(user(&bob), "bob");
    let accounts = core.accounts().await.expect("accounts");
    let names: Vec<(String, bool)> = accounts
        .iter()
        .map(|a| (a.user_id.localpart().to_owned(), a.active))
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names.contains(&("alice".into(), false)), "{names:?}");
    assert!(names.contains(&("bob".into(), true)), "{names:?}");

    // Switching back needs no password.
    bob.suspend().await.expect("suspend bob");
    let alice_key = accounts
        .iter()
        .find(|a| a.user_id.localpart() == "alice")
        .expect("alice listed")
        .key
        .clone();
    let alice = core
        .switch_account(alice_key)
        .await
        .expect("switch")
        .expect("alice restored");
    assert_eq!(user(&alice), "alice");
    let mut room_updates = alice.room_list().await.expect("room list");
    let mut rooms = Vector::new();
    drive(
        &mut rooms,
        async || room_updates.next().await,
        |rooms| rooms.iter().any(|r| r.name == SEEDED_ROOM),
    )
    .await;

    // The active account is what a restart restores.
    alice.suspend().await.expect("suspend");
    let restarted = Core::with_session_store(&dir, SessionStoreKind::File).expect("core");
    let again = restarted
        .restore_session()
        .await
        .expect("restore")
        .expect("a session");
    assert_eq!(user(&again), "alice");

    // Signing out of one leaves the other, which becomes active.
    again.logout().await.expect("logout alice");
    let accounts = restarted.accounts().await.expect("accounts");
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].user_id.localpart(), "bob");
    assert!(accounts[0].active);
    let bob = restarted
        .restore_session()
        .await
        .expect("restore")
        .expect("bob");
    assert_eq!(user(&bob), "bob");
    bob.logout().await.expect("logout bob");
    assert!(restarted.accounts().await.expect("accounts").is_empty());
}

/// A fresh account for one test run (through the admin API, as alice).
async fn new_account(homeserver: &str, name: &str) -> String {
    let http = matrix_sdk::reqwest::Client::new();
    let body = serde_json::json!({
        "type": "m.login.password",
        "identifier": { "type": "m.id.user", "user": "alice" },
        "password": "alice-password",
    });
    let login = http
        .post(format!("{homeserver}/_matrix/client/v3/login"))
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .expect("admin login")
        .text()
        .await
        .expect("admin login body");
    let login: serde_json::Value = serde_json::from_str(&login).expect("json");
    let token = login["access_token"].as_str().expect("token").to_owned();
    let user = format!("{name}-{}", std::process::id());
    let status = http
        .put(format!(
            "{homeserver}/_synapse/admin/v2/users/@{user}:localhost"
        ))
        .bearer_auth(&token)
        .header("content-type", "application/json")
        .body(serde_json::json!({ "password": format!("{user}-password") }).to_string())
        .send()
        .await
        .expect("create user")
        .status();
    assert!(status.is_success(), "creating {user}: {status}");
    let _ = http
        .post(format!("{homeserver}/_matrix/client/v3/logout"))
        .bearer_auth(&token)
        .send()
        .await;
    user
}

/// Wait until a session's recovery state is `want` (it reads "unknown"
/// until the first sync has settled).
async fn wait_recovery(session: &Session, want: morpheus_core::RecoveryAvailability) {
    let deadline = std::time::Instant::now() + WAIT;
    loop {
        let status = session.encryption_status().await.expect("status");
        if status.recovery == want {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "wanted {want:?}, still {status:?}"
        );
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

#[tokio::test]
async fn setting_up_recovery_lets_a_new_device_unlock_history() {
    use morpheus_core::RecoveryAvailability;
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let user = new_account(&env.homeserver, "recovery").await;
    let login = |dir| {
        let core = Core::with_session_store(dir, SessionStoreKind::File).expect("core");
        let request = LoginRequest {
            homeserver: env.homeserver.clone(),
            username: user.clone(),
            password: format!("{user}-password"),
        };
        async move { core.login(request).await.expect("login") }
    };

    let first = login(fresh_data_dir()).await;
    wait_recovery(&first, RecoveryAvailability::Disabled).await;

    let key = first.enable_recovery().await.expect("enable recovery");
    assert!(
        key.split_whitespace().count() >= 8,
        "a recovery key: {key:?}"
    );
    wait_recovery(&first, RecoveryAvailability::Enabled).await;

    // A second device sees the backup and unlocks it with the key.
    let second = login(fresh_data_dir()).await;
    wait_recovery(&second, RecoveryAvailability::NeedsKey).await;
    second.recover(key).await.expect("recover with the key");
    wait_recovery(&second, RecoveryAvailability::Enabled).await;
}

#[tokio::test]
async fn messages_are_searchable_including_encrypted_ones() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let core = Core::with_session_store(fresh_data_dir(), SessionStoreKind::File).expect("core");
    let session = core
        .login(LoginRequest {
            homeserver: env.homeserver,
            username: env.user,
            password: env.password,
        })
        .await
        .expect("login");
    let mut room_updates = session.room_list().await.expect("room list");
    let mut rooms = Vector::new();
    drive(
        &mut rooms,
        async || room_updates.next().await,
        |rooms| rooms.iter().any(|r| r.name == SEEDED_ROOM),
    )
    .await;
    let general = rooms
        .iter()
        .find(|r| r.name == SEEDED_ROOM)
        .expect("general")
        .id
        .clone();

    let word = format!("zebrafish{}", std::process::id());
    let opened = session.timeline(general.clone()).await.expect("timeline");
    opened
        .handle
        .send_text(format!("the {word} swims home"))
        .await
        .expect("send");

    let deadline = std::time::Instant::now() + WAIT;
    loop {
        let hits = session
            .search_room(general.clone(), word.clone(), 10)
            .await
            .expect("search");
        if let Some(hit) = hits.first() {
            assert!(hit.body.contains(&word), "{hit:?}");
            assert_eq!(hit.sender_name, env_user_name(), "{hit:?}");
            assert!(hit.timestamp_ms > 0);
            break;
        }
        assert!(std::time::Instant::now() < deadline, "never found {word}");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let none = session
        .search_room(general, format!("nothing{word}"), 10)
        .await
        .expect("search");
    assert!(none.is_empty(), "{none:?}");

    // Encrypted rooms too: the index holds what this device decrypted.
    drive(
        &mut rooms,
        async || room_updates.next().await,
        |rooms| rooms.iter().any(|r| r.name == "encrypted"),
    )
    .await;
    let encrypted = rooms
        .iter()
        .find(|r| r.name == "encrypted")
        .expect("encrypted room")
        .id
        .clone();
    let secret = format!("narwhal{}", std::process::id());
    let opened = session.timeline(encrypted.clone()).await.expect("timeline");
    opened
        .handle
        .send_text(format!("a {secret} in the dark"))
        .await
        .expect("send");
    let deadline = std::time::Instant::now() + WAIT;
    loop {
        let hits = session
            .search_room(encrypted.clone(), secret.clone(), 10)
            .await
            .expect("search");
        if hits.iter().any(|hit| hit.body.contains(&secret)) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "never found {secret} in the encrypted room"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn env_user_name() -> String {
    std::env::var("MORPHEUS_TEST_USER").unwrap_or_else(|_| "alice".into())
}

#[tokio::test]
async fn messages_from_unverified_sessions_carry_a_shield() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let login = |user: &'static str| {
        let core =
            Core::with_session_store(fresh_data_dir(), SessionStoreKind::File).expect("core");
        let request = LoginRequest {
            homeserver: env.homeserver.clone(),
            username: user.into(),
            password: format!("{user}-password"),
        };
        async move { core.login(request).await.expect("login") }
    };
    let room_named = async |session: &Session, name: &str| {
        let mut updates = session.room_list().await.expect("room list");
        let mut rooms = Vector::new();
        drive(
            &mut rooms,
            async || updates.next().await,
            |rooms| rooms.iter().any(|r| r.name == name),
        )
        .await;
        rooms
            .iter()
            .find(|r| r.name == name)
            .expect("room")
            .id
            .clone()
    };

    // Bob is signed in first, so alice's message is encrypted for him too.
    let bob = login("bob").await;
    let bob_encrypted = room_named(&bob, "encrypted").await;
    let mut opened = bob.timeline(bob_encrypted).await.expect("timeline");

    // Alice has a cross-signing identity (from some session) and writes in
    // #encrypted from another session she never verified.
    login("alice")
        .await
        .ensure_cross_signing()
        .await
        .expect("alice's identity");
    let alice = login("alice").await;
    let encrypted = room_named(&alice, "encrypted").await;
    let body = format!("shielded {}", std::process::id());
    let alice_timeline = alice.timeline(encrypted).await.expect("timeline");
    alice_timeline
        .handle
        .send_text(body.clone())
        .await
        .expect("send");

    // Bob sees it with a shield explaining why.
    let mut items = Vector::new();
    let find = |items: &Vector<TimelineEntry>| {
        items.iter().find_map(|e| match e {
            TimelineEntry::Message(m) if matches!(&m.kind, MessageKind::Text { body: b, .. } if *b == body) => {
                Some((**m).clone())
            }
            _ => None,
        })
    };
    drive_back(&mut items, &mut opened, |items| {
        find(items).is_some_and(|m| m.shield.is_some())
    })
    .await;
    let shield = find(&items).and_then(|m| m.shield).expect("a shield");
    assert!(!shield.reason.is_empty(), "{shield:?}");

    // Unencrypted rooms carry none.
    let general = room_named(&bob, SEEDED_ROOM).await;
    let mut opened = bob.timeline(general).await.expect("timeline");
    let mut items = Vector::new();
    drive_back(&mut items, &mut opened, |items| {
        items.iter().any(|e| matches!(e, TimelineEntry::Message(_)))
    })
    .await;
    assert!(
        items
            .iter()
            .all(|e| !matches!(e, TimelineEntry::Message(m) if m.shield.is_some())),
        "no shields in an unencrypted room"
    );
}

#[tokio::test]
async fn a_closed_app_can_preview_a_pushed_message() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let room_named = async |session: &Session, name: &str| {
        let mut updates = session.room_list().await.expect("room list");
        let mut rooms = Vector::new();
        drive(
            &mut rooms,
            async || updates.next().await,
            |rooms| rooms.iter().any(|r| r.name == name),
        )
        .await;
        rooms
            .iter()
            .find(|r| r.name == name)
            .expect("room")
            .id
            .clone()
    };
    // Bob's device signs in (so messages are encrypted for it), then stops.
    let bob_dir = fresh_data_dir();
    let bob_core = Core::with_session_store(&bob_dir, SessionStoreKind::File).expect("core");
    let bob = bob_core
        .login(LoginRequest {
            homeserver: env.homeserver.clone(),
            username: "bob".into(),
            password: "bob-password".into(),
        })
        .await
        .expect("bob");
    let encrypted = room_named(&bob, "encrypted").await;
    bob.suspend().await.expect("bob stops");
    drop(bob);
    drop(bob_core);

    // Alice writes in #encrypted.
    let alice_core =
        Core::with_session_store(fresh_data_dir(), SessionStoreKind::File).expect("core");
    let alice = alice_core
        .login(LoginRequest {
            homeserver: env.homeserver,
            username: "alice".into(),
            password: "alice-password".into(),
        })
        .await
        .expect("alice");
    let body = format!("for your lock screen {}", std::process::id());
    let _ = room_named(&alice, "encrypted").await;
    let mut opened = alice.timeline(encrypted.clone()).await.expect("timeline");
    opened.handle.send_text(body.clone()).await.expect("send");
    let mut items = Vector::new();
    let sent = |items: &Vector<TimelineEntry>| {
        items.iter().find_map(|e| match e {
            TimelineEntry::Message(m)
                if !m.is_local_echo
                    && matches!(&m.kind, MessageKind::Text { body: b, .. } if *b == body) =>
            {
                match &m.id {
                    MessageId::Event(id) => Some(id.to_string()),
                    MessageId::Transaction(_) => None,
                }
            }
            _ => None,
        })
    };
    drive(
        &mut items,
        async || opened.updates.next().await,
        |items| sent(items).is_some(),
    )
    .await;
    let event_id = sent(&items).expect("event id");

    // A fresh core on bob's data, as a push would start it, previews it.
    let cold = Core::with_session_store(&bob_dir, SessionStoreKind::File).expect("cold core");
    let deadline = std::time::Instant::now() + WAIT;
    let preview = loop {
        match cold
            .preview_notification(encrypted.to_string(), event_id.clone())
            .await
        {
            Ok(Some(preview)) if preview.body.contains(&body) => break preview,
            other => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "no preview: {other:?}"
                );
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    };
    assert_eq!(preview.room, "encrypted", "{preview:?}");
    assert!(preview.sender.contains("alice"), "{preview:?}");
}

#[tokio::test]
async fn joining_a_call_gets_livekit_credentials_and_shows_in_the_room_list() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let login = async |user: &str| {
        let core =
            Core::with_session_store(fresh_data_dir(), SessionStoreKind::File).expect("core");
        core.login(LoginRequest {
            homeserver: env.homeserver.clone(),
            username: user.into(),
            password: format!("{user}-password"),
        })
        .await
        .expect("login")
    };
    let alice = login("alice").await;
    let bob = login("bob").await;

    let mut alice_rooms = alice.room_list().await.expect("room list");
    let mut rooms = Vector::new();
    drive(
        &mut rooms,
        async || alice_rooms.next().await,
        |rooms| rooms.iter().any(|r| r.name == SEEDED_ROOM),
    )
    .await;
    let general = rooms
        .iter()
        .find(|r| r.name == SEEDED_ROOM)
        .expect("general")
        .id
        .clone();
    let alice_id = alice.user_id().expect("user id");

    // The homeserver names a LiveKit service; it hands out a token for the room.
    let call = alice.join_call(general.clone()).await.expect("join call");
    assert!(
        call.livekit_url.starts_with("ws"),
        "livekit url: {}",
        call.livekit_url
    );
    assert!(!call.jwt.is_empty(), "a LiveKit token");

    // Others see who is in the call, in their room list.
    let mut bob_rooms = bob.room_list().await.expect("bob room list");
    let mut rooms = Vector::new();
    drive(
        &mut rooms,
        async || bob_rooms.next().await,
        |rooms| {
            rooms
                .iter()
                .any(|r| r.id == general && r.call_participants.contains(&alice_id))
        },
    )
    .await;

    // Hanging up takes them out again.
    alice.leave_call(general.clone()).await.expect("leave call");
    drive(
        &mut rooms,
        async || bob_rooms.next().await,
        |rooms| {
            rooms
                .iter()
                .any(|r| r.id == general && !r.call_participants.contains(&alice_id))
        },
    )
    .await;
}

#[tokio::test]
async fn a_device_that_vanishes_drops_out_of_the_call() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let login = async |user: &str| {
        let core =
            Core::with_session_store(fresh_data_dir(), SessionStoreKind::File).expect("core");
        let session = core
            .login(LoginRequest {
                homeserver: env.homeserver.clone(),
                username: user.into(),
                password: format!("{user}-password"),
            })
            .await
            .expect("login");
        (core, session)
    };
    let (alice_core, alice) = login("carol").await;
    let (_bob_core, bob) = login("bob").await;
    let carol_id = alice.user_id().expect("user id");

    let mut bob_rooms = bob.room_list().await.expect("bob room list");
    let mut rooms = Vector::new();
    drive(
        &mut rooms,
        async || bob_rooms.next().await,
        |rooms| rooms.iter().any(|r| r.name == SEEDED_ROOM),
    )
    .await;
    let general = rooms
        .iter()
        .find(|r| r.name == SEEDED_ROOM)
        .expect("general")
        .id
        .clone();

    alice.join_call(general.clone()).await.expect("join call");
    drive(
        &mut rooms,
        async || bob_rooms.next().await,
        |rooms| {
            rooms
                .iter()
                .any(|r| r.id == general && r.call_participants.contains(&carol_id))
        },
    )
    .await;

    // The app dies without hanging up (no leave sent): its runtime stops.
    tokio::task::spawn_blocking(move || {
        drop(alice);
        drop(alice_core);
    })
    .await
    .expect("drop");

    // The homeserver hangs up for it once it stops checking in.
    drive(
        &mut rooms,
        async || bob_rooms.next().await,
        |rooms| {
            rooms
                .iter()
                .any(|r| r.id == general && !r.call_participants.contains(&carol_id))
        },
    )
    .await;
}

#[tokio::test]
async fn call_media_keys_travel_encrypted_to_the_room() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let login = async |user: &str| {
        let core =
            Core::with_session_store(fresh_data_dir(), SessionStoreKind::File).expect("core");
        core.login(LoginRequest {
            homeserver: env.homeserver.clone(),
            username: user.into(),
            password: format!("{user}-password"),
        })
        .await
        .expect("login")
    };
    let find = async |session: &Session| {
        let mut updates = session.room_list().await.expect("room list");
        let mut rooms = Vector::new();
        drive(
            &mut rooms,
            async || updates.next().await,
            |rooms| rooms.iter().any(|r| r.name == "encrypted"),
        )
        .await;
        rooms
            .iter()
            .find(|r| r.name == "encrypted")
            .expect("encrypted room")
            .id
            .clone()
    };
    // Bob's device exists before alice sends, so her room key reaches it.
    let bob = login("bob").await;
    let room = find(&bob).await;
    let mut keys = bob.call_keys(room.clone()).await.expect("subscribe");

    let alice = login("alice").await;
    find(&alice).await;
    let key = format!("key-{}", std::process::id()).into_bytes();
    alice
        .send_call_key(room.clone(), key.clone())
        .await
        .expect("send key");

    let alice_id = alice.user_id().expect("alice");
    let alice_device = alice.device_id().expect("device");
    let got = timeout(WAIT, async {
        loop {
            let got = keys.next().await.expect("key stream");
            if got.user_id == alice_id && got.device_id == alice_device {
                return got;
            }
        }
    })
    .await
    .expect("alice's key arrives");
    assert_eq!(got.key, key);
    assert!(
        got.livekit_identities.len() >= 2,
        "identities to apply it to"
    );
}

/// Calling someone in a direct message rings them (MSC4075
/// `m.rtc.notification`); group rooms don't ring. Declining tells the caller.
#[tokio::test]
async fn a_call_in_a_direct_message_rings_and_can_be_declined() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let login = async |user: &str| {
        let core =
            Core::with_session_store(fresh_data_dir(), SessionStoreKind::File).expect("core");
        core.login(LoginRequest {
            homeserver: env.homeserver.clone(),
            username: user.into(),
            password: format!("{user}-password"),
        })
        .await
        .expect("login")
    };
    let alice = login("alice").await;
    let bob_dir = fresh_data_dir();
    let bob = Core::with_session_store(&bob_dir, SessionStoreKind::File)
        .expect("core")
        .login(LoginRequest {
            homeserver: env.homeserver.clone(),
            username: "bob".into(),
            password: "bob-password".into(),
        })
        .await
        .expect("login");
    let bob_id = bob.user_id().expect("bob");
    let mut alice_rooms = alice.room_list().await.expect("room list");
    let mut rooms = Vector::new();
    drive(
        &mut rooms,
        async || alice_rooms.next().await,
        |rooms| {
            rooms.iter().any(|r| r.name == SEEDED_ROOM)
                && rooms.iter().any(|r| r.dm_user.as_ref() == Some(&bob_id))
        },
    )
    .await;
    let general = rooms
        .iter()
        .find(|r| r.name == SEEDED_ROOM)
        .expect("general")
        .id
        .clone();
    // The DM with bob, with bob in it (earlier runs may have left one he
    // walked out of: messaging him asks him back).
    let dm = alice.dm_with(bob_id.clone()).await.expect("dm with bob");
    let mut bob_rooms = bob.room_list().await.expect("bob room list");
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while bob.join_room(dm.clone()).await.is_err() {
        assert!(
            std::time::Instant::now() < deadline,
            "bob could not join the DM"
        );
        let _ = timeout(Duration::from_millis(300), bob_rooms.next()).await;
    }
    let mut bob_rings = bob.ring_updates().await.expect("bob rings");
    let mut alice_rings = alice.ring_updates().await.expect("alice rings");
    // Let bob's sync get going.
    let _ = timeout(Duration::from_secs(5), bob_rooms.next()).await;

    // A group room's call rings nobody; the DM's rings bob.
    alice
        .join_call(general.clone())
        .await
        .expect("join general");
    alice.join_call(dm.clone()).await.expect("call bob");
    let ring = timeout(Duration::from_secs(20), async {
        loop {
            tokio::select! {
                ring = bob_rings.next() => match ring.expect("rings end") {
                    morpheus_core::Ringing::Incoming(call) => return call,
                    other => eprintln!("skipping {other:?}"),
                },
                _ = bob_rooms.next() => {}
            }
        }
    })
    .await
    .expect("bob's phone rings");
    assert_eq!(ring.room_id, dm, "the ring is the DM's, not #general's");
    assert_eq!(ring.caller, alice.user_id().expect("alice"));

    // A push for it, previewed with the app closed, reads as a call.
    let cold = Core::with_session_store(&bob_dir, SessionStoreKind::File).expect("cold core");
    let preview = cold
        .preview_notification(dm.to_string(), ring.notification.to_string())
        .await
        .expect("preview")
        .expect("something to show");
    assert!(preview.is_call, "{preview:?}");
    assert_eq!(preview.body, "Incoming call");
    assert!(preview.sender.contains("alice"), "{preview:?}");

    // Bob declines; alice hears.
    bob.decline_call(dm.clone(), ring.notification.clone())
        .await
        .expect("decline");
    let declined = timeout(Duration::from_secs(20), async {
        loop {
            if let morpheus_core::Ringing::Declined {
                room_id,
                by,
                notification,
            } = alice_rings.next().await.expect("rings end")
            {
                return (room_id, by, notification);
            }
        }
    })
    .await
    .expect("alice hears the decline");
    assert_eq!(declined, (dm.clone(), bob_id, ring.notification));

    alice.leave_call(dm).await.expect("hang up");
    alice.leave_call(general).await.expect("leave general");
}

/// Account data reads see a write at once, without waiting for sync: two
/// friends added back to back are both kept, and a restart before the
/// first sync still finds them.
#[tokio::test]
async fn friends_added_back_to_back_are_both_kept_and_seen_after_a_restart() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let dir = fresh_data_dir();
    let session = Core::with_session_store(&dir, SessionStoreKind::File)
        .expect("core")
        .login(LoginRequest {
            homeserver: env.homeserver.clone(),
            username: "carol".into(),
            password: "carol-password".into(),
        })
        .await
        .expect("login");
    let bob: morpheus_core::OwnedUserId = "@bob:localhost".parse().expect("bob");
    let alice: morpheus_core::OwnedUserId = "@alice:localhost".parse().expect("alice");
    for who in [&bob, &alice] {
        session.set_friend(who.clone(), false).await.expect("clear");
    }
    // No sync running: each read must come from the server.
    session
        .set_friend(bob.clone(), true)
        .await
        .expect("add bob");
    session
        .set_friend(alice.clone(), true)
        .await
        .expect("add alice");
    let friends = session.friends().await.expect("friends");
    assert!(
        friends.contains(&bob) && friends.contains(&alice),
        "{friends:?}"
    );
    drop(session);

    let restored = Core::with_session_store(&dir, SessionStoreKind::File)
        .expect("core")
        .restore_session()
        .await
        .expect("restore")
        .expect("a session");
    let friends = restored.friends().await.expect("friends");
    assert!(
        friends.contains(&bob) && friends.contains(&alice),
        "after a restart: {friends:?}"
    );
    for who in [&bob, &alice] {
        restored
            .set_friend(who.clone(), false)
            .await
            .expect("clean up");
    }
}

async fn sign_in_at(env: &TestEnv, dir: &std::path::Path, user: &str) -> Session {
    Core::with_session_store(dir, SessionStoreKind::File)
        .expect("core")
        .login(LoginRequest {
            homeserver: env.homeserver.clone(),
            username: user.into(),
            password: format!("{user}-password"),
        })
        .await
        .expect("login")
}

/// A local store that cannot be decrypted (its passphrase is not the one
/// saved with the account) is set aside with a clear message, and the
/// account can simply sign in again, instead of failing at every start.
#[tokio::test]
async fn an_unreadable_store_is_set_aside_and_the_account_signs_in_again() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let (dir, other) = (fresh_data_dir(), fresh_data_dir());
    drop(sign_in_at(&env, &dir, "carol").await);
    drop(sign_in_at(&env, &other, "carol").await);
    // The account's store swapped for one made with another passphrase.
    std::fs::remove_dir_all(dir.join("store")).expect("remove store");
    std::fs::rename(other.join("store"), dir.join("store")).expect("swap store");

    let core = Core::with_session_store(&dir, SessionStoreKind::File).expect("core");
    let err = match core.restore_session().await {
        Ok(_) => panic!("the store cannot be read"),
        Err(err) => err.to_string(),
    };
    assert!(err.contains("sign in again"), "{err}");
    assert!(!dir.join("store").exists(), "the store is set aside");
    assert!(
        std::fs::read_dir(&dir).expect("dir").flatten().any(|e| e
            .file_name()
            .to_string_lossy()
            .starts_with("store.unreadable-")),
        "kept, renamed"
    );
    // No dead end: nothing to restore, and signing in again works there.
    assert!(core.restore_session().await.expect("restore").is_none());
    let again = core
        .login(LoginRequest {
            homeserver: env.homeserver.clone(),
            username: "carol".into(),
            password: "carol-password".into(),
        })
        .await
        .expect("signs in again");
    assert!(dir.join("store").exists(), "in the same folder");
    drop(again);
}

/// A folder that still holds a local store is never taken by a new sign-in
/// (which starts with a fresh store), even when its credentials cannot be
/// found: they may only be out of reach for a moment.
#[tokio::test]
async fn a_new_sign_in_never_takes_a_folder_that_holds_a_store() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let dir = fresh_data_dir();
    drop(sign_in_at(&env, &dir, "carol").await);
    let before: Vec<_> = std::fs::read_dir(dir.join("store"))
        .expect("store")
        .flatten()
        .map(|e| e.file_name())
        .filter(|name| name.to_string_lossy().ends_with(".sqlite3"))
        .collect();
    // Credentials out of reach (a keyring that did not answer, say).
    std::fs::remove_file(dir.join("session.enc")).expect("hide credentials");

    drop(sign_in_at(&env, &dir, "bob").await);
    assert!(
        dir.join("accounts").join("1").join("store").exists(),
        "the new account gets a folder of its own"
    );
    let after: Vec<_> = std::fs::read_dir(dir.join("store"))
        .expect("store still there")
        .flatten()
        .map(|e| e.file_name())
        .filter(|name| name.to_string_lossy().ends_with(".sqlite3"))
        .collect();
    assert!(
        !before.is_empty() && before.iter().all(|f| after.contains(f)),
        "store untouched: {before:?} -> {after:?}"
    );
}

/// Messaging someone who left the DM with you asks them back (an invite),
/// instead of writing into a room no one else is in ("Empty Room").
#[tokio::test]
async fn messaging_someone_who_left_the_dm_invites_them_back() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let carol = sign_in_at(&env, &fresh_data_dir(), "carol").await;
    let alice = sign_in_at(&env, &fresh_data_dir(), "alice").await;
    // Carol's side syncs while this runs.
    let mut rooms = carol.room_list().await.expect("room list");
    let syncing = tokio::spawn(async move { while rooms.next().await.is_some() {} });
    let mut alice_rooms = alice.room_list().await.expect("room list");
    let alice_syncing = tokio::spawn(async move { while alice_rooms.next().await.is_some() {} });
    // Joins once the invite reaches alice's device (or fails: not invited).
    let join = async |room: morpheus_core::OwnedRoomId| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            match alice.join_room(room.clone()).await {
                Ok(()) => return Ok(()),
                Err(err) if std::time::Instant::now() > deadline => return Err(err),
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(200)).await,
            }
        }
    };
    let alice_id: morpheus_core::OwnedUserId = "@alice:localhost".parse().expect("user id");

    let dm = carol.dm_with(alice_id.clone()).await.expect("dm");
    join(dm.clone()).await.expect("alice joins");
    alice.leave_room(dm.clone()).await.expect("alice leaves");
    // Carol hears she left.
    timeout(WAIT, async {
        while carol
            .members(dm.clone())
            .await
            .is_ok_and(|members| members.iter().any(|m| m.user_id == alice_id))
        {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    })
    .await
    .expect("carol sees alice leave");

    // Messaging her again: she is asked back, so she can join.
    let again = carol.dm_with(alice_id.clone()).await.expect("dm again");
    join(again)
        .await
        .expect("alice was invited back and can join");
    syncing.abort();
    alice_syncing.abort();
}
