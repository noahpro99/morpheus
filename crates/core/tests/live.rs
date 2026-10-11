#![allow(clippy::expect_used)]
//! End-to-end tests against a local homeserver.
//!
//! They self-skip unless `MORPHEUS_TEST_HOMESERVER` is set. Bring the
//! homeserver up with `just hs-up && just hs-seed`, then run `just test-live`.

use std::time::Duration;

use morpheus_core::{Core, LoginRequest, MessageKind, TimelineEntry, Vector, VectorDiff};
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(30);
const SEEDED_ROOM: &str = "general";
const SEEDED_MESSAGE: &str = "Welcome to Morpheus";

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
