#![allow(clippy::expect_used)]
//! End-to-end tests against a local homeserver.
//!
//! They self-skip unless `MORPHEUS_TEST_HOMESERVER` is set. Bring the
//! homeserver up with `just hs-up && just hs-seed`, then run `just test-live`.

use std::time::Duration;

use morpheus_core::{
    Core, LoginRequest, MessageId, MessageKind, Session, TimelineEntry, Vector, VectorDiff,
    VerificationHandle, VerificationStep,
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

#[tokio::test]
async fn login_list_rooms_read_and_send() {
    let Some(env) = env() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping live test");
        return;
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter("morpheus=debug,warn")
        .try_init();

    let core = Core::new(fresh_data_dir()).expect("core");
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
    let opened = session
        .timeline(general.id.clone())
        .await
        .expect("open timeline");
    let mut updates = opened.updates;
    let mut items = Vector::new();
    let has_body = |items: &Vector<TimelineEntry>, body: &str| {
        items.iter().any(|e| matches!(e, TimelineEntry::Message(m) if matches!(&m.kind, MessageKind::Text { body: t, .. } if t == body)))
    };
    drive(
        &mut items,
        async || updates.next().await,
        |items| has_body(items, SEEDED_MESSAGE),
    )
    .await;

    // Send: our message must come back as a remote echo (not just the local one).
    let body = format!("live test {}", std::process::id());
    opened.handle.send_text(body.clone()).await.expect("send");
    drive(&mut items, async || updates.next().await, |items| {
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

    let core = Core::new(fresh_data_dir()).expect("core");
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
    let opened = session
        .timeline(general.clone())
        .await
        .expect("open timeline");
    let mut updates = opened.updates;
    let mut items = Vector::new();
    let is_root = |m: &morpheus_core::Message| matches!(&m.kind, MessageKind::Text { body, .. } if body == SEEDED_THREAD_ROOT);
    drive(
        &mut items,
        async || updates.next().await,
        |items| {
            items
                .iter()
                .any(|e| matches!(e, TimelineEntry::Message(m) if is_root(m)))
        },
    )
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
    let thread = session
        .thread(general.clone(), root_event_id.clone())
        .await
        .expect("open thread");
    let mut thread_updates = thread.updates;
    let mut thread_items = Vector::new();
    drive(&mut thread_items, async || thread_updates.next().await, |items| {
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
    drive(&mut thread_items, async || thread_updates.next().await, |items| {
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
            let core = Core::new(fresh_data_dir()).expect("core");
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

    let core = Core::new(fresh_data_dir()).expect("core");
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

    let core = Core::new(fresh_data_dir()).expect("core");
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

    let opened = session.timeline(general).await.expect("open timeline");
    let mut updates = opened.updates;
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
    drive(
        &mut items,
        async || updates.next().await,
        |items| find_image(items).is_some(),
    )
    .await;
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
            let core = Core::new(fresh_data_dir()).expect("core");
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
    let core = Core::new(fresh_data_dir()).expect("core");
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
        let core = Core::new(fresh_data_dir()).expect("core");
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
    let core = Core::new(fresh_data_dir()).expect("core");
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
    session.set_display_name(name.clone()).await.expect("set name");
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
    session.set_display_name("carol".into()).await.expect("restore name");
    let restored = session.own_profile().await.expect("profile");
    assert_eq!(restored.status, None);
    assert_eq!(restored.name, "carol");
}
