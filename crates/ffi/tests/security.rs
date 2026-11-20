#![allow(clippy::expect_used)]
//! Verification and recovery through the FFI, the way the phone's screens
//! drive them. Self-skips unless `MORPHEUS_TEST_HOMESERVER` is set.

use std::{sync::Arc, time::Duration};

use morpheus_ffi::{
    IncomingVerificationListener, Morpheus, RecoveryKind, VerificationAction, VerificationFlow,
    VerificationListener, VerificationPanel,
};
use tokio::{sync::mpsc, time::timeout};

const WAIT: Duration = Duration::from_secs(60);

struct Panels(std::sync::Mutex<mpsc::UnboundedSender<VerificationPanel>>);
impl VerificationListener for Panels {
    fn on_panel(&self, panel: VerificationPanel) {
        let _ = self.0.lock().expect("lock").send(panel);
    }
}
struct Incoming(std::sync::Mutex<mpsc::UnboundedSender<Arc<VerificationFlow>>>);
impl IncomingVerificationListener for Incoming {
    fn on_incoming(&self, flow: Arc<VerificationFlow>) {
        let _ = self.0.lock().expect("lock").send(flow);
    }
}

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn app_as(homeserver: &str, user: &str, password: &str) -> (Arc<Morpheus>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("dir");
    let app = Morpheus::new(dir.path().display().to_string()).expect("app");
    app.login(homeserver.into(), user.into(), password.into()).await.expect("login");
    (app, dir)
}

/// A fresh account for one test run (through the admin API, as alice).
async fn new_account(homeserver: &str, name: &str) -> String {
    let http = matrix_sdk::reqwest::Client::new();
    let login = http
        .post(format!("{homeserver}/_matrix/client/v3/login"))
        .header("content-type", "application/json").body(serde_json::json!({
            "type": "m.login.password",
            "identifier": { "type": "m.id.user", "user": "alice" },
            "password": "alice-password",
        }).to_string())
        .send().await.expect("admin login").text().await.expect("body");
    let login: serde_json::Value = serde_json::from_str(&login).expect("json");
    let token = login["access_token"].as_str().expect("token").to_owned();
    let user = format!("{name}-{}", std::process::id());
    let status = http
        .put(format!("{homeserver}/_synapse/admin/v2/users/@{user}:localhost"))
        .bearer_auth(&token)
        .header("content-type", "application/json").body(serde_json::json!({ "password": format!("{user}-password") }).to_string())
        .send().await.expect("create user").status();
    assert!(status.is_success(), "creating {user}: {status}");
    let _ = http.post(format!("{homeserver}/_matrix/client/v3/logout")).bearer_auth(&token).send().await;
    user
}

async fn wait_recovery(app: &Morpheus, want: RecoveryKind) {
    timeout(WAIT, async {
        loop {
            let status = app.security_status().await.expect("status");
            if status.recovery == want {
                return;
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("recovery never became {want:?}"));
}

async fn panel_where(rx: &mut mpsc::UnboundedReceiver<VerificationPanel>, what: &str, want: impl Fn(&VerificationPanel) -> bool) -> VerificationPanel {
    timeout(WAIT, async {
        loop {
            let panel = rx.recv().await.expect("the flow ended");
            assert!(!panel.text.starts_with("Verification cancelled"), "{}", panel.text);
            if want(&panel) {
                return panel;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

#[tokio::test]
async fn recovery_key_unlocks_a_second_device() {
    let Some(hs) = homeserver() else { return };
    let user = new_account(&hs, "ffirecovery").await;
    let password = format!("{user}-password");
    let (first, _d1) = app_as(&hs, &user, &password).await;
    wait_recovery(&first, RecoveryKind::Disabled).await;
    let key = first.enable_recovery().await.expect("enable recovery");
    assert!(key.split_whitespace().count() >= 8, "a recovery key: {key:?}");
    wait_recovery(&first, RecoveryKind::Enabled).await;

    let (second, _d2) = app_as(&hs, &user, &password).await;
    wait_recovery(&second, RecoveryKind::NeedsKey).await;
    assert!(second.recover("not a key".into()).await.is_err(), "a wrong key is refused with a message");
    second.recover(key).await.expect("recover");
    wait_recovery(&second, RecoveryKind::Enabled).await;
    assert!(second.security_status().await.expect("status").device_verified);
}

#[tokio::test]
async fn emoji_verification_between_two_devices() {
    let Some(hs) = homeserver() else { return };
    let user = new_account(&hs, "ffiverify").await;
    let password = format!("{user}-password");
    let (a, _d1) = app_as(&hs, &user, &password).await;
    let (b, _d2) = app_as(&hs, &user, &password).await;
    // Both need to know of each other.
    timeout(WAIT, async {
        loop {
            if a.security_status().await.expect("status").other_devices > 0
                && b.security_status().await.expect("status").other_devices > 0
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    })
    .await
    .expect("devices see each other");

    let (tx, mut incoming_rx) = mpsc::unbounded_channel();
    let _watch = b.watch_incoming_verifications(Arc::new(Incoming(std::sync::Mutex::new(tx)))).await.expect("watch");

    let outgoing = a.request_verification().await.expect("request");
    let (tx_a, mut rx_a) = mpsc::unbounded_channel();
    let _wa = outgoing.watch(Arc::new(Panels(std::sync::Mutex::new(tx_a))));
    let waiting = panel_where(&mut rx_a, "the request", |p| !p.buttons.is_empty()).await;
    assert!(waiting.text.contains("Waiting"), "{}", waiting.text);

    let incoming = timeout(WAIT, incoming_rx.recv()).await.expect("incoming in time").expect("incoming");
    let (tx_b, mut rx_b) = mpsc::unbounded_channel();
    let _wb = incoming.watch(Arc::new(Panels(std::sync::Mutex::new(tx_b))));
    let ask = panel_where(&mut rx_b, "the question", |p| p.buttons.iter().any(|b| b.action == VerificationAction::Accept)).await;
    assert!(ask.text.contains("wants to verify"));
    incoming.act(VerificationAction::Accept).await.expect("accept");

    // A phone cannot scan yet: the request shows a code, and emoji instead.
    let with_emoji = |p: &VerificationPanel| p.emojis.len() == 7;
    let first = panel_where(&mut rx_a, "a code or emoji on A", |p| p.qr.is_some() || with_emoji(p)).await;
    let panel_a = if with_emoji(&first) {
        first
    } else {
        outgoing.act(VerificationAction::UseEmoji).await.expect("use emoji");
        panel_where(&mut rx_a, "emoji on A", with_emoji).await
    };
    let panel_b = panel_where(&mut rx_b, "emoji on B", with_emoji).await;
    let names = |p: &VerificationPanel| p.emojis.iter().map(|e| e.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&panel_a), names(&panel_b), "the same seven emoji");

    outgoing.act(VerificationAction::ConfirmMatch).await.expect("confirm a");
    incoming.act(VerificationAction::ConfirmMatch).await.expect("confirm b");
    let done = panel_where(&mut rx_a, "A done", |p| p.finished).await;
    assert!(done.text.starts_with("Verified"));
    panel_where(&mut rx_b, "B done", |p| p.finished).await;
}
