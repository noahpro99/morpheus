#![allow(clippy::expect_used)]
//! End-to-end tests against a local homeserver.
//!
//! They self-skip unless `MORPHEUS_TEST_HOMESERVER` is set. Bring the
//! homeserver up with `just hs-up && just hs-seed`, then run `just test-live`.

use std::time::Duration;

use morpheus_core::{
    Core, LoginRequest, MessageKind, Session, TimelineEntry, Vector, VectorDiff,
    VerificationHandle, VerificationStep,
};
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
    assert!(result.is_ok(), "timed out waiting for verification to finish");
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

    let a_devices = device_a.own_devices().await.expect("A's devices");
    let b_devices = device_b.own_devices().await.expect("B's devices");
    assert!(
        a_devices.iter().any(|d| !d.is_current && d.verified),
        "A should see B as verified: {a_devices:?}"
    );
    assert!(
        b_devices.iter().any(|d| !d.is_current && d.verified),
        "B should see A as verified: {b_devices:?}"
    );
}
