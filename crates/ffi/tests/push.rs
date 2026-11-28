#![allow(clippy::expect_used)]
//! Push notifications through the FFI: a pusher is registered for an
//! endpoint, and a pushed event id is turned into what the notification
//! says without the app running (a cold start of the push service).

use std::time::Duration;

use morpheus_ffi::{Morpheus, preview_push};

struct Rows(std::sync::Mutex<tokio::sync::mpsc::UnboundedSender<Vec<morpheus_ffi::TimelineRow>>>);
impl morpheus_ffi::TimelineListener for Rows {
    fn on_rows(&self, rows: Vec<morpheus_ffi::TimelineRow>) {
        let _ = self.0.lock().expect("lock").send(rows);
    }
}

#[tokio::test]
async fn a_pushed_event_is_worded_without_the_app_and_a_pusher_is_registered() {
    use morpheus_core::{Core, LoginRequest, SessionStoreKind};
    let Ok(hs) = std::env::var("MORPHEUS_TEST_HOMESERVER") else {
        return;
    };
    let login = |user: &'static str| {
        let hs = hs.clone();
        async move {
            Core::with_session_store(tempfile::tempdir().expect("dir").keep(), SessionStoreKind::File)
                .expect("core")
                .login(LoginRequest { homeserver: hs, username: user.into(), password: format!("{user}-password") })
                .await
                .expect("login")
        }
    };
    // A room of alice's that bob is in.
    let alice = login("alice").await;
    let name = format!("push {}", std::process::id());
    let room_id = alice.create_room(name.clone(), String::new(), None).await.expect("room");
    let bob_core = login("bob").await;
    bob_core.join_room_via(room_id.clone(), vec!["localhost".into()]).await.expect("join");

    // Bob's phone: signed in once, then closed.
    let dir = tempfile::tempdir().expect("dir");
    let bob = Morpheus::new(dir.path().display().to_string()).expect("app");
    bob.login(hs.clone(), "bob".into(), "bob-password".into()).await.expect("bob");
    let endpoint = format!("http://localhost:9/up/{}", std::process::id());
    let gateway = bob.register_push(endpoint.clone(), "test phone".into()).await.expect("register");
    assert!(gateway.contains("/_matrix/push/v1/notify"), "{gateway}");
    bob.unregister_push(endpoint).await.expect("unregister");
    bob.suspend().await.expect("suspend");
    drop(bob);

    // Alice writes; the push says only ids.
    let opened = alice.timeline(room_id.clone()).await.expect("timeline");
    let text = format!("hello bob {}", std::process::id());
    opened.handle.send_text(text.clone()).await.expect("send");
    let mut updates = opened.updates;
    let mut entries = morpheus_core::Vector::new();
    let event_id = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let diffs = updates.next().await.expect("timeline ended");
            for diff in diffs {
                diff.apply(&mut entries);
            }
            let found = entries.iter().find_map(|e| match e {
                morpheus_core::TimelineEntry::Message(m) if m.kind.plain_body() == Some(text.as_str()) => match &m.id {
                    morpheus_core::MessageId::Event(id) => Some(id.to_string()),
                    _ => None,
                },
                _ => None,
            });
            if let Some(id) = found {
                return id;
            }
        }
    })
    .await
    .expect("the sent event");

    let notice = preview_push(dir.path().display().to_string(), room_id.to_string(), event_id.clone()).await.expect("a notice");
    assert_eq!(notice.body, text);
    assert!(notice.title.contains(&name) || notice.title.to_lowercase().contains("alice"), "{}", notice.title);
    assert!(!notice.call);
    assert!(preview_push(dir.path().display().to_string(), room_id.to_string(), "$nonexistent".into()).await.is_none());

    // The notification's buttons, with the app closed (#548): Mark as read sends a receipt; Reply sends a message alice sees.
    let data = dir.path().display().to_string();
    morpheus_ffi::read_from_push(data.clone(), room_id.to_string(), event_id.clone()).await.expect("mark as read");
    let answer = format!("on my way {}", std::process::id());
    morpheus_ffi::reply_from_push(data, room_id.to_string(), answer.clone()).await.expect("reply");
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let diffs = updates.next().await.expect("timeline ended");
            for diff in diffs {
                diff.apply(&mut entries);
            }
            let seen = entries.iter().any(|e| matches!(e, morpheus_core::TimelineEntry::Message(m) if m.kind.plain_body() == Some(answer.as_str())));
            if seen {
                return;
            }
        }
    })
    .await
    .expect("alice sees bob's reply sent from the notification");
}
