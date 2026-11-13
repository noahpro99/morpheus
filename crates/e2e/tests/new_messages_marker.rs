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
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
}

/// Whether `.new-divider` and the message containing `needle` are both
/// there, the divider just above the message (found by where they sit, so
/// it does not depend on how the app orders its rows).
async fn marker_above_message(app: &mut App, needle: &str) -> bool {
    let (Some(divider), Some(message)) = (
        app.bounds(".new-divider").await.ok().flatten(),
        app.bounds_containing(".message", needle)
            .await
            .ok()
            .flatten(),
    ) else {
        return false;
    };
    divider.bottom() <= message.y + 1.0 && message.y - divider.bottom() < 60.0
}

/// Wait until [`marker_above_message`] holds.
async fn wait_for_marker_above(app: &mut App, needle: &str) -> Result<(), String> {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        if marker_above_message(app, needle).await {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!("no NEW divider just above {needle:?}"));
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
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
    bob.type_into(".composer .composer-input", &body)
        .await
        .expect("type");
    bob.press(".composer .composer-input", "Enter")
        .await
        .expect("send");
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
    if let Err(err) = wait_for_marker_above(&mut alice, &body).await {
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
        .type_into(".composer .composer-input", &own_body)
        .await
        .expect("type");
    alice
        .press(".composer .composer-input", "Enter")
        .await
        .expect("send");
    alice
        .wait_for_text(".message:not(.pending) .body", &own_body, DEFAULT_WAIT)
        .await
        .expect("alice's message echoed");
    alice
        .wait_for_count(".new-divider", 0, DEFAULT_WAIT)
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

/// The same as the first half of the test above, without the message box:
/// bob's message goes in through the API, and the divider is found by where
/// it sits (so it runs on the GPUI app too).
#[tokio::test]
async fn new_message_marker_appears_above_unread_messages() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let general = morpheus_e2e::api_as(
        &hs,
        "alice",
        "GET",
        "directory/room/%23general:localhost",
        None,
    )
    .expect("general");
    let general = general["room_id"].as_str().expect("room id").to_owned();

    // Alice catches up on #general first, then looks at #dev while bob posts.
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login(&mut alice).await;
    alice.open_room("general").await.expect("open general");
    alice
        .wait_for(".message .body", DEFAULT_WAIT)
        .await
        .expect("general loaded");
    alice.open_room("dev").await.expect("open dev");
    // The fully-read marker goes out when #general is left; let it land.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    fully_read_on_a_redaction(&hs, "general");

    let body = format!("marker port {}", std::process::id());
    morpheus_e2e::send_as(&hs, "bob", &general, &body).expect("bob posts");

    alice.open_room("general").await.expect("open general");
    alice
        .wait_for_text(".message .body", &body, DEFAULT_WAIT)
        .await
        .expect("bob's message visible");
    alice
        .wait_for(".new-divider", DEFAULT_WAIT)
        .await
        .expect("the NEW divider");
    let divider = alice
        .bounds(".new-divider")
        .await
        .expect("divider")
        .expect("divider box");
    let message = alice
        .bounds_containing(".message", &body)
        .await
        .expect("message")
        .expect("message box");
    assert!(
        divider.bottom() <= message.y + 1.0 && message.y - divider.bottom() < 60.0,
        "the divider is not just above bob's message: divider {divider:?}, message {message:?}"
    );

    // Discord's shape: a red line across the whole timeline, the red NEW pill
    // at its right end, centred on the line.
    let timeline = alice.bounds(".timeline").await.expect("bounds").expect("timeline");
    let line = alice
        .bounds(".new-divider .new-line")
        .await
        .expect("line")
        .expect("the line");
    let pill = alice
        .bounds(".new-divider .new-pill")
        .await
        .expect("pill")
        .expect("the NEW pill");
    assert!(
        line.x - timeline.x <= 24.0,
        "the line starts at {} but the timeline at {}",
        line.x,
        timeline.x
    );
    assert!(
        timeline.right() - pill.right() <= 24.0,
        "the pill ends at {} but the timeline at {}",
        pill.right(),
        timeline.right()
    );
    assert!(
        (line.right() - pill.x).abs() <= 2.0,
        "the line {line:?} does not run to the pill {pill:?}"
    );
    let line_mid = line.y + line.h / 2.0;
    let pill_mid = pill.y + pill.h / 2.0;
    assert!(
        (line_mid - pill_mid).abs() <= 1.0,
        "the pill is centred at {pill_mid} but the line at {line_mid}"
    );
}

async fn login(app: &mut App) {
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", "alice")
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", "alice-password")
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("room listed");
}
