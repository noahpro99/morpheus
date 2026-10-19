//! The "new messages" divider: bob posts while alice is looking elsewhere,
//! and switching into the room should show the line above his message.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login_and_open(app: &mut App, user: &str, password: &str, room: &str) {
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", user)
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", password)
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("room listed");
    app.open_room(room).await.expect("open room");
    app.wait_for_text(".chat-header h2", room, DEFAULT_WAIT)
        .await
        .expect("room header");
    app.wait_for(".composer input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
}

/// JS that returns true once `.new-divider` and a timeline row containing
/// `needle` both exist, with the divider sitting above the message. Rows are
/// rendered newest-first (CSS flips it visually), so "above" means the
/// divider must appear *after* the message in document order.
fn marker_above_message_js(needle: &str) -> String {
    format!(
        "const rows = [...document.querySelectorAll('.timeline > *')];
         const dividerIdx = rows.findIndex(el => el.classList.contains('new-divider'));
         const msgIdx = rows.findIndex(el => el.textContent && el.textContent.includes({needle}));
         return dividerIdx !== -1 && msgIdx !== -1 && msgIdx < dividerIdx;",
        needle = serde_json::to_string(needle).expect("json"),
    )
}

#[tokio::test]
async fn new_message_marker_appears_above_unread_and_clears_on_send() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };

    // Alice catches up on #general first (earlier tests leave unread messages
    // there, and the divider rightly sits above the oldest of those), then
    // looks at #dev while bob posts in #general. Leaving a room marks it read.
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "alice-password", "general").await;
    alice
        .wait_for(".message .body", DEFAULT_WAIT)
        .await
        .expect("general loaded");
    alice.open_room("dev").await.expect("open dev");
    alice
        .wait_for_text(".chat-header h2", "dev", DEFAULT_WAIT)
        .await
        .expect("dev header");
    // The fully-read marker goes out when #general is left; let it land.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    // And from elsewhere (alice's other device) it moved on to a reaction
    // she took back: an event with no row of its own. It must still count.
    fully_read_on_a_redaction(&hs, "general");

    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "bob-password", "general").await;

    let body = format!("marker test {}", std::process::id());
    bob.type_into(".composer input", &body).await.expect("type");
    bob.press(".composer input", "Enter").await.expect("send");
    bob.wait_for_text(".message:not(.pending) .body", &body, DEFAULT_WAIT)
        .await
        .expect("bob's message echoed");

    // Alice switches into #general: the NEW divider should sit above bob's message.
    alice.open_room("general").await.expect("open general");
    alice
        .wait_for_text(".chat-header h2", "general", DEFAULT_WAIT)
        .await
        .expect("general header");
    alice
        .wait_for_text(".message .body", &body, DEFAULT_WAIT)
        .await
        .expect("bob's message visible");
    if let Err(err) = alice
        .wait_for_js(&marker_above_message_js(&body), DEFAULT_WAIT)
        .await
    {
        // What the newest rows were instead (this has failed only in long runs).
        let rows = alice
            .eval(
                "const all = [...document.querySelectorAll('.timeline > *')]; \
                 const d = all.findIndex(r => r.classList.contains('new-divider')); \
                 return 'divider at ' + d + ' of ' + all.length + '\\n' + all.slice(0, 12).concat(d > 11 ? all.slice(d - 2, d + 3) : []) \
                 .map(r => r.className + ': ' + r.textContent.trim().slice(0, 60)).join('\\n');",
            )
            .await
            .unwrap_or_default();
        panic!(
            "NEW divider shown above bob's message: {err}\nnewest rows:\n{}",
            rows.as_str().unwrap_or_default()
        );
    }

    // Sending a message means alice is caught up: the divider drops immediately.
    let own_body = format!("caught up {}", std::process::id());
    alice
        .type_into(".composer input", &own_body)
        .await
        .expect("type");
    alice.press(".composer input", "Enter").await.expect("send");
    alice
        .wait_for_text(".message:not(.pending) .body", &own_body, DEFAULT_WAIT)
        .await
        .expect("alice's message echoed");
    alice
        .wait_for_js(
            "return document.querySelector('.new-divider') === null;",
            DEFAULT_WAIT,
        )
        .await
        .expect("divider cleared after sending");
}

/// As alice on another device (`curl`): react to the newest message in
/// `room`, take the reaction back, and mark the redaction fully read, which
/// is what reading a room whose last event is a removed reaction leaves.
fn fully_read_on_a_redaction(hs: &str, room: &str) {
    let curl = |args: &[&str]| -> serde_json::Value {
        let out = std::process::Command::new("curl")
            .arg("-sf")
            .args(args)
            .output()
            .expect("curl");
        assert!(out.status.success(), "curl {args:?}");
        serde_json::from_slice(&out.stdout).unwrap_or_default()
    };
    let login = curl(&[
        "-X",
        "POST",
        &format!("{hs}/_matrix/client/v3/login"),
        "-d",
        r#"{"type":"m.login.password","identifier":{"type":"m.id.user","user":"alice"},"password":"alice-password"}"#,
    ]);
    let auth = format!(
        "Authorization: Bearer {}",
        login["access_token"].as_str().expect("token")
    );
    let room_id = curl(&[&format!(
        "{hs}/_matrix/client/v3/directory/room/%23{room}:localhost"
    )])["room_id"]
        .as_str()
        .expect("room id")
        .to_owned();
    let newest = curl(&[
        "-H",
        &auth,
        &format!(
            "{hs}/_matrix/client/v3/rooms/{room_id}/messages?dir=b&limit=1&filter=%7B%22types%22%3A%5B%22m.room.message%22%5D%7D"
        ),
    ])["chunk"][0]["event_id"]
        .as_str()
        .expect("newest message")
        .to_owned();
    let txn = std::process::id();
    let reaction = curl(&[
        "-X",
        "PUT",
        "-H",
        &auth,
        &format!("{hs}/_matrix/client/v3/rooms/{room_id}/send/m.reaction/r{txn}"),
        "-d",
        &serde_json::json!({ "m.relates_to": { "rel_type": "m.annotation", "event_id": newest, "key": "👀" } })
            .to_string(),
    ])["event_id"]
        .as_str()
        .expect("reaction")
        .to_owned();
    let redaction = curl(&[
        "-X",
        "PUT",
        "-H",
        &auth,
        &format!("{hs}/_matrix/client/v3/rooms/{room_id}/redact/{reaction}/x{txn}"),
        "-d",
        "{}",
    ])["event_id"]
        .as_str()
        .expect("redaction")
        .to_owned();
    curl(&[
        "-X",
        "POST",
        "-H",
        &auth,
        &format!("{hs}/_matrix/client/v3/rooms/{room_id}/read_markers"),
        "-d",
        &serde_json::json!({ "m.fully_read": redaction, "m.read": redaction }).to_string(),
    ]);
    curl(&[
        "-X",
        "POST",
        "-H",
        &auth,
        &format!("{hs}/_matrix/client/v3/logout"),
    ]);
}
