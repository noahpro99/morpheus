//! Calls, Discord-style: the phone button in a room's header joins its call;
//! everyone sees who is in it under the room in the sidebar (ringed while
//! they speak); the voice panel at the bottom of the sidebar mutes, deafens
//! and hangs up. Test runs send a tone instead of the microphone.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login_and_open(app: &mut App, user: &str, room: &str) {
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", user)
        .await
        .expect("username");
    app.type_into(
        "form.login input[name=password]",
        &format!("{user}-password"),
    )
    .await
    .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    app.open_room(room).await.expect("open room");
    app.wait_for(".composer input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer");
}

/// The people listed in a room's call, in the sidebar.
fn in_call(room: &str, name: &str) -> String {
    format!(
        "return [...document.querySelectorAll('.room-call')].some(c => c.dataset.room === {room} \
           && [...c.querySelectorAll('.call-participant')].some(p => p.textContent.includes({name})));",
        room = serde_json::to_string(room).expect("json"),
        name = serde_json::to_string(name).expect("json"),
    )
}

fn speaking(name: &str) -> String {
    format!(
        "return [...document.querySelectorAll('.call-participant.speaking')].some(p => p.textContent.includes({}));",
        serde_json::to_string(name).expect("json"),
    )
}

#[tokio::test]
async fn join_a_call_see_who_is_in_it_mute_and_hang_up() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "random").await;
    let random_id: String = alice
        .eval("return document.querySelector('.room.selected').dataset.room;")
        .await
        .expect("room id")
        .as_str()
        .expect("string")
        .to_owned();

    // Alice starts a call from the header.
    alice
        .click(".chat-header .call-button")
        .await
        .expect("call");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("voice panel");
    alice
        .wait_for_text(".voice-panel", "random", DEFAULT_WAIT)
        .await
        .expect("names the room");
    alice
        .wait_for_js(&in_call(&random_id, "alice"), DEFAULT_WAIT)
        .await
        .expect("alice listed under the room");

    // Bob sees her there before joining, and joins.
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "random").await;
    bob.wait_for_js(&in_call(&random_id, "alice"), DEFAULT_WAIT)
        .await
        .expect("bob sees alice in the call");
    bob.click(".chat-header .call-button").await.expect("join");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("bob connected");
    alice
        .wait_for_js(&in_call(&random_id, "bob"), DEFAULT_WAIT)
        .await
        .expect("alice sees bob join");

    // Both send a tone, so both are speaking.
    bob.wait_for_js(&speaking("alice"), DEFAULT_WAIT)
        .await
        .expect("alice ringed as speaking");
    alice
        .wait_for_js(&speaking("bob"), DEFAULT_WAIT)
        .await
        .expect("bob speaking");
    // Joins and leaves stay out of the timeline.
    assert_eq!(
        alice
            .eval("return document.querySelector('.timeline').textContent.includes('call.member');")
            .await
            .expect("timeline text"),
        serde_json::Value::Bool(false),
        "call membership rows in the timeline"
    );

    // Muted, she stops.
    alice.click(".user-bar .mute-button").await.expect("mute");
    alice
        .wait_for(".user-bar .mute-button.active", DEFAULT_WAIT)
        .await
        .expect("mute shows");
    bob.wait_for_js(
        &format!("return !(() => {{ {} }})();", speaking("alice")),
        DEFAULT_WAIT,
    )
    .await
    .expect("alice no longer speaking");

    // Hanging up takes her out of the list for everyone.
    alice.click(".voice-panel .hang-up").await.expect("hang up");
    alice
        .wait_for_js(
            "return !document.querySelector('.voice-panel');",
            DEFAULT_WAIT,
        )
        .await
        .expect("panel gone");
    bob.wait_for_js(
        &format!("return !(() => {{ {} }})();", in_call(&random_id, "alice")),
        DEFAULT_WAIT,
    )
    .await
    .expect("alice gone from the call");
    bob.click(".voice-panel .hang-up")
        .await
        .expect("bob hangs up");
}
