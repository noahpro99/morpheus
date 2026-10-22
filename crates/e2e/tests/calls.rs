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

/// The chimes played so far, in order (test runs only note them).
fn chimes_are(expected: &[&str]) -> String {
    format!(
        "return (window.morpheusSounds ?? []).join(',') === {};",
        serde_json::to_string(&expected.join(",")).expect("json"),
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
    alice
        .wait_for_js(&chimes_are(&["join"]), DEFAULT_WAIT)
        .await
        .expect("a chime as she joins");

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
    alice
        .wait_for_js(&chimes_are(&["join", "join"]), DEFAULT_WAIT)
        .await
        .expect("a chime as bob joins");
    bob.wait_for_js(&chimes_are(&["join"]), DEFAULT_WAIT)
        .await
        .expect("bob's own join chime");

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
    alice
        .wait_for_js(&chimes_are(&["join", "join", "mute"]), DEFAULT_WAIT)
        .await
        .expect("a mute chime");
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
    alice
        .wait_for_js(
            &chimes_are(&["join", "join", "mute", "leave"]),
            DEFAULT_WAIT,
        )
        .await
        .expect("a chime as she hangs up");
    bob.wait_for_js(&chimes_are(&["join", "leave"]), DEFAULT_WAIT)
        .await
        .expect("a chime for bob as she leaves");
    bob.click(".voice-panel .hang-up")
        .await
        .expect("bob hangs up");
}

/// In an encrypted room the audio is encrypted end to end; each app shares
/// its key over Matrix (encrypted), so the two still hear each other.
#[tokio::test]
async fn encrypted_rooms_encrypt_the_call_and_both_sides_still_hear() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    // #encrypted starts without calls allowed (the seed resets it): alice,
    // an admin, turns them on by joining, and bob can then join too.
    // Both devices exist before either sends, so room keys reach both.
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "encrypted").await;
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "encrypted").await;

    alice
        .click(".chat-header .call-button")
        .await
        .expect("call");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("alice connected");
    bob.click(".chat-header .call-button").await.expect("join");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("bob connected");

    let hears = |who: &str| {
        format!(
            "return (document.querySelector('.voice-panel')?.dataset.heard || '').split(' ').includes('@{who}:localhost');"
        )
    };
    bob.wait_for_js(&hears("alice"), DEFAULT_WAIT)
        .await
        .expect("bob hears alice (decrypted)");
    alice
        .wait_for_js(&hears("bob"), DEFAULT_WAIT)
        .await
        .expect("alice hears bob (decrypted)");

    if alice.is_phone() {
        alice
            .click(".chat-header .pane-back")
            .await
            .expect("back to the list");
    }
    alice
        .click(".voice-panel .hang-up")
        .await
        .expect("alice hangs up");
    bob.click(".voice-panel .hang-up")
        .await
        .expect("bob hangs up");
    // Nobody left behind in the call.
    bob.wait_for_js(
        "return !document.querySelector('.room.selected + .room-call');",
        DEFAULT_WAIT,
    )
    .await
    .expect("the call empties");
}

/// Choose a device in a picker, as a person would: open it, pick one.
async fn choose(app: &mut App, picker: &str, id: &str) {
    app.click(&format!(
        ".voice-devices .device-choice[data-kind={picker}] .device-current"
    ))
    .await
    .expect("open the picker");
    let option = format!(".device-choice[data-kind={picker}] .device-option[data-id=\"{id}\"]");
    app.wait_for(&option, DEFAULT_WAIT)
        .await
        .expect("device listed");
    // Readable: the text is not the colour of the list behind it.
    let readable = app
        .eval(&format!(
            "const o = document.querySelector('{option}'), l = o.closest('.device-options'); \
             return getComputedStyle(o).color !== getComputedStyle(l).backgroundColor;"
        ))
        .await
        .expect("colours");
    assert_eq!(
        readable,
        serde_json::Value::Bool(true),
        "option text the colour of the list"
    );
    app.click(&option).await.expect("pick");
}

async fn open_voice_settings(app: &mut App) {
    app.click(".settings-button").await.expect("gear");
    app.wait_for_text(".settings-nav button", "Voice & Video", DEFAULT_WAIT)
        .await
        .expect("settings");
    app.click_text(".settings-nav button", "Voice & Video")
        .await
        .expect("voice page");
    // Test runs list stand-in devices (`AudioMode::Tone`).
    app.wait_for(
        ".voice-devices .device-choice[data-kind=microphone] .device-current",
        DEFAULT_WAIT,
    )
    .await
    .expect("microphones listed");
}

/// Settings → Voice & Video picks the microphone and speaker; the choice is
/// kept on the device, still there after a restart, and calls still connect.
#[tokio::test]
async fn choose_the_microphone_and_speaker_for_calls() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch_desktop(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "random").await;
    open_voice_settings(&mut alice).await;
    choose(&mut alice, "microphone", "test-microphone-2").await;
    choose(&mut alice, "speaker", "test-speaker-1").await;
    alice
        .click(".voice-devices .device-choice[data-kind=microphone] .device-current")
        .await
        .expect("reopen");
    alice
        .wait_for(".device-options", DEFAULT_WAIT)
        .await
        .expect("list open");
    let _ = alice.screenshot("voice-devices").await;
    alice
        .click(".voice-devices .device-choice[data-kind=microphone] .device-current")
        .await
        .expect("close");
    let saved = alice.data_dir().join("audio_devices.json");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let text = std::fs::read_to_string(&saved).unwrap_or_default();
        if text.contains("test-microphone-2") && text.contains("test-speaker-1") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "choice not kept: {text}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    // After a restart the pickers show the choice.
    let mut alice = alice.relaunch().await.expect("relaunch");
    alice.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    open_voice_settings(&mut alice).await;
    let shown = alice
        .eval(
            "return [...document.querySelectorAll('.voice-devices .device-current')].map(s => s.dataset.id).join(',');",
        )
        .await
        .expect("read pickers");
    assert_eq!(shown.as_str(), Some("test-microphone-2,test-speaker-1"));

    // Back to the system default.
    choose(&mut alice, "speaker", "").await;
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while std::fs::read_to_string(&saved)
        .unwrap_or_default()
        .contains("test-speaker-1")
    {
        assert!(std::time::Instant::now() < deadline, "default not kept");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    alice.press("body", "Escape").await.expect("close settings");

    // Calls still connect.
    alice.open_room("random").await.expect("open #random");
    alice.click(".call-button").await.expect("join");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("connected");
    alice.click(".voice-panel .hang-up").await.expect("hang up");
}

/// Into the direct message with `who`, from the DM list.
async fn open_dm(app: &mut App, who: &str) {
    app.click(".rail-item[title=\"Direct messages\"]")
        .await
        .expect("direct messages");
    app.open_room(who).await.expect("open the dm");
}

/// Calling someone in a DM rings them: a popup with the caller, a
/// ringtone, accept and decline. Declining tells the caller and ends it.
#[tokio::test]
async fn a_call_in_a_dm_rings_the_other_person_who_can_answer_or_decline() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "random").await;
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "random").await;
    open_dm(&mut alice, "bob").await;

    // Alice calls; bob's app rings.
    alice
        .click(".chat-header .call-button")
        .await
        .expect("call bob");
    bob.wait_for_text(".incoming-call", "alice", DEFAULT_WAIT)
        .await
        .expect("bob sees alice calling");
    bob.wait_for_js(
        "return (window.morpheusSounds ?? []).includes('ring');",
        DEFAULT_WAIT,
    )
    .await
    .expect("a ringtone");
    // Bob's window is in the background: the system is told too (test runs
    // log the notification instead of showing it).
    if !bob.is_phone() {
        let log = bob.data_dir().join("app.log");
        let deadline = std::time::Instant::now() + DEFAULT_WAIT;
        while !std::fs::read_to_string(&log)
            .unwrap_or_default()
            .contains("alice is calling")
        {
            assert!(
                std::time::Instant::now() < deadline,
                "no ring notification; log: {}",
                std::fs::read_to_string(&log)
                    .unwrap_or_default()
                    .lines()
                    .filter(|l| l.contains("ring") || l.contains("notif") || l.contains("WARN"))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    }
    // The DM says so.
    alice
        .wait_for_text(".timeline", "alice started a call", DEFAULT_WAIT)
        .await
        .expect("started a call, in the timeline");

    // Bob answers: he's in the call, and it stops ringing.
    bob.click(".incoming-call .accept-call")
        .await
        .expect("accept");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("bob connected");
    bob.wait_for_js(
        "return !document.querySelector('.incoming-call');",
        DEFAULT_WAIT,
    )
    .await
    .expect("ringing stops");
    alice
        .wait_for_js(&speaking("bob"), DEFAULT_WAIT)
        .await
        .expect("alice hears bob");
    bob.click(".voice-panel .hang-up")
        .await
        .expect("bob hangs up");
    alice
        .click(".voice-panel .hang-up")
        .await
        .expect("alice hangs up");
    alice
        .wait_for_js(
            "return !document.querySelector('.voice-panel');",
            DEFAULT_WAIT,
        )
        .await
        .expect("alice out");
    // Once the room shows nobody in the call (the header redraws as it changes).
    alice
        .wait_for(
            ".chat-header .call-button[title=\"Start a call\"]",
            DEFAULT_WAIT,
        )
        .await
        .expect("call empty");

    // She calls again; this time bob declines, and her call ends.
    alice
        .click(".chat-header .call-button")
        .await
        .expect("call bob again");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("alice waiting in the call");
    bob.wait_for(".incoming-call .decline-call", DEFAULT_WAIT)
        .await
        .expect("rings again");
    bob.click(".incoming-call .decline-call")
        .await
        .expect("decline");
    bob.wait_for_js(
        "return !document.querySelector('.incoming-call');",
        DEFAULT_WAIT,
    )
    .await
    .expect("ringing stops");
    alice
        .wait_for_text(".toast", "bob declined the call", DEFAULT_WAIT)
        .await
        .expect("alice told");
    alice
        .wait_for_js(
            "return !document.querySelector('.voice-panel');",
            DEFAULT_WAIT,
        )
        .await
        .expect("alice's call ends");
    assert_eq!(
        bob.eval("return document.body.textContent.includes('Unsupported');")
            .await
            .expect("text"),
        serde_json::Value::Bool(false),
        "rings or declines shown as unsupported"
    );
}

/// Whether `user`'s video tile shows the test pattern: red on the left,
/// blue on the right (what a test run's camera sends).
fn shows_test_pattern(user: &str) -> String {
    format!(
        "const t = [...document.querySelectorAll('.call-stage .video-tile:not(.screen)')].find(t => t.dataset.user === {user}); \
         const i = t && t.querySelector('img.video'); \
         if (!i || !i.complete || !i.naturalWidth) return false; \
         const c = document.createElement('canvas'); c.width = i.naturalWidth; c.height = i.naturalHeight; \
         const g = c.getContext('2d'); g.drawImage(i, 0, 0); \
         const at = x => g.getImageData(Math.floor(c.width * x), Math.floor(c.height / 2), 1, 1).data; \
         const l = at(0.25), r = at(0.75); \
         return l[0] > 150 && l[2] < 100 && r[2] > 150 && r[0] < 100;",
        user = serde_json::to_string(user).expect("json"),
    )
}

#[tokio::test]
async fn a_camera_turned_on_shows_to_everyone_in_the_call_until_turned_off() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    // An encrypted room: the video is end-to-end encrypted like the audio.
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "encrypted").await;
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "encrypted").await;
    alice
        .click(".chat-header .call-button")
        .await
        .expect("call");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("alice connected");
    bob.click(".chat-header .call-button").await.expect("join");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("bob connected");
    // No cameras yet, so no stage.
    assert_eq!(bob.count(".call-stage").await.expect("count"), 0);

    // Alice turns hers on (the voice panel is with the room list on a phone).
    if alice.is_phone() {
        alice
            .click(".chat-header .pane-back")
            .await
            .expect("back to the list");
    }
    alice
        .click(".voice-panel .camera-button")
        .await
        .expect("camera on");
    alice
        .wait_for(".voice-panel .camera-button.active", DEFAULT_WAIT)
        .await
        .expect("the button shows it on");
    if alice.is_phone() {
        alice
            .open_room("encrypted")
            .await
            .expect("back to the room");
    }
    // She sees herself, and bob sees her: the picture arrived, decrypted.
    alice
        .wait_for_js(&shows_test_pattern("@alice:localhost"), DEFAULT_WAIT)
        .await
        .expect("alice's self view");
    bob.wait_for_js(&shows_test_pattern("@alice:localhost"), DEFAULT_WAIT)
        .await
        .expect("bob sees alice's camera");
    bob.wait_for_text(".video-tile .video-name", "alice", DEFAULT_WAIT)
        .await
        .expect("her tile is named");
    if let Some(path) = bob.screenshot("call-video").await.expect("screenshot") {
        eprintln!("screenshot: {}", path.display());
    }

    // Off again: her tile goes, and with no cameras on, the stage.
    if alice.is_phone() {
        alice
            .click(".chat-header .pane-back")
            .await
            .expect("back to the list");
    }
    alice
        .click(".voice-panel .camera-button")
        .await
        .expect("camera off");
    bob.wait_for_js(
        "return !document.querySelector('.call-stage');",
        DEFAULT_WAIT,
    )
    .await
    .expect("the stage goes when the camera does");
    alice
        .wait_for_js(
            "return !document.querySelector('.call-stage');",
            DEFAULT_WAIT,
        )
        .await
        .expect("alice's self view goes too");

    alice
        .click(".voice-panel .hang-up")
        .await
        .expect("alice hangs up");
    if bob.is_phone() {
        bob.click(".chat-header .pane-back").await.expect("back");
    }
    bob.click(".voice-panel .hang-up")
        .await
        .expect("bob hangs up");
}

/// Whether `user`'s shared screen shows the test run's screen pattern:
/// green on the left, yellow on the right.
fn shows_screen_pattern(user: &str) -> String {
    format!(
        "const t = [...document.querySelectorAll('.call-stage .video-tile.screen')].find(t => t.dataset.user === {user}); \
         const i = t && t.querySelector('img.video'); \
         if (!i || !i.complete || !i.naturalWidth) return false; \
         const c = document.createElement('canvas'); c.width = i.naturalWidth; c.height = i.naturalHeight; \
         const g = c.getContext('2d'); g.drawImage(i, 0, 0); \
         const at = x => g.getImageData(Math.floor(c.width * x), Math.floor(c.height / 2), 1, 1).data; \
         const l = at(0.25), r = at(0.75); \
         return l[1] > 150 && l[0] < 100 && r[0] > 150 && r[1] > 150 && r[2] < 100;",
        user = serde_json::to_string(user).expect("json"),
    )
}

#[tokio::test]
async fn a_shared_screen_shows_to_everyone_in_the_call_until_stopped() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    if alice.is_phone() {
        eprintln!("phones do not share their screen yet; skipping");
        return;
    }
    login_and_open(&mut alice, "alice", "random").await;
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "random").await;
    alice
        .click(".chat-header .call-button")
        .await
        .expect("call");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("alice connected");
    bob.click(".chat-header .call-button").await.expect("join");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("bob connected");

    alice
        .click(".voice-panel .screen-button")
        .await
        .expect("share");
    alice
        .wait_for(".voice-panel .screen-button.active", DEFAULT_WAIT)
        .await
        .expect("the button shows it shared");
    bob.wait_for_js(&shows_screen_pattern("@alice:localhost"), DEFAULT_WAIT)
        .await
        .expect("bob sees alice's screen");
    bob.wait_for_text(
        ".video-tile.screen .video-name",
        "alice's screen",
        DEFAULT_WAIT,
    )
    .await
    .expect("named as her screen");
    alice
        .wait_for_js(&shows_screen_pattern("@alice:localhost"), DEFAULT_WAIT)
        .await
        .expect("alice sees what she shares");
    // Her camera too: the screen stays first.
    alice
        .click(".voice-panel .camera-button")
        .await
        .expect("camera on");
    bob.wait_for_js(&shows_test_pattern("@alice:localhost"), DEFAULT_WAIT)
        .await
        .expect("and her camera");
    let order = bob
        .eval("return [...document.querySelectorAll('.call-stage .video-tile')].map(t => t.classList.contains('screen'));")
        .await
        .expect("order");
    assert_eq!(order, serde_json::json!([true, false]), "screen first");
    if let Some(path) = bob
        .screenshot("call-screen-share")
        .await
        .expect("screenshot")
    {
        eprintln!("screenshot: {}", path.display());
    }

    alice
        .click(".voice-panel .screen-button")
        .await
        .expect("stop sharing");
    bob.wait_for_js(
        "return !document.querySelector('.video-tile.screen');",
        DEFAULT_WAIT,
    )
    .await
    .expect("her screen goes");
    assert_eq!(
        bob.count(".call-stage .video-tile").await.expect("count"),
        1,
        "her camera stays"
    );
    alice
        .click(".voice-panel .hang-up")
        .await
        .expect("alice hangs up");
    bob.click(".voice-panel .hang-up")
        .await
        .expect("bob hangs up");
}
