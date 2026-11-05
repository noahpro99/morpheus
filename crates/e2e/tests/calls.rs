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
    if let Err(err) = app.open_room(room).await {
        let rows = app.eval("return [...document.querySelectorAll('.room')].map(r => r.textContent).join(' | ');").await;
        let _ = app.screenshot("open-room-failed").await;
        panic!("open room: {err}\nrows: {rows:?}");
    }
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
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
    if let Err(err) = alice
        .wait_for_js(&chimes_are(&["join", "join"]), DEFAULT_WAIT)
        .await
    {
        let heard = alice.eval("return (window.morpheusSounds ?? []).join(',');").await;
        panic!("a chime as bob joins: {err}; alice heard {heard:?}");
    }
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
        "const t = [...document.querySelectorAll('.video-tile:not(.screen)')].find(t => t.dataset.user === {user}); \
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
        // The call's own screen, where a phone shows the video.
        alice
            .click(".chat-header .call-button")
            .await
            .expect("open the call");
        alice
            .wait_for(".call-screen", DEFAULT_WAIT)
            .await
            .expect("the call screen");
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
    // Bob, camera off, has a tile too: his picture, beside her video.
    bob.wait_for(
        ".call-stage .call-tile[data-user=\"@bob:localhost\"] .avatar",
        DEFAULT_WAIT,
    )
    .await
    .expect("a picture tile for bob on the stage");
    if let Some(path) = bob.screenshot("call-video").await.expect("screenshot") {
        eprintln!("screenshot: {}", path.display());
    }
    // A phone stacks the two, one above the other, full width.
    if alice.is_phone() {
        alice
            .wait_for(".call-screen .call-tile[data-user=\"@bob:localhost\"]", DEFAULT_WAIT)
            .await
            .expect("bob's tile on the phone");
        let stacked = alice
            .eval(
                "const t = [...document.querySelectorAll('.call-tiles > *')].map(e => e.getBoundingClientRect()); \
                 const w = document.querySelector('.call-tiles').clientWidth; \
                 return t.length === 2 && t[1].top >= t[0].bottom - 1 && t.every(r => r.width > w * 0.8);",
            )
            .await
            .expect("measure");
        if let Some(path) = alice.screenshot("android-call-two").await.expect("screenshot") {
            eprintln!("screenshot: {}", path.display());
        }
        assert_eq!(stacked, serde_json::json!(true), "the two tiles stack on a phone");
    }

    // The other way: bob's camera reaches alice.
    bob.click(".voice-panel .camera-button")
        .await
        .expect("bob's camera on");
    alice
        .wait_for_js(&shows_test_pattern("@bob:localhost"), DEFAULT_WAIT)
        .await
        .expect("alice sees bob's camera");
    if let Some(path) = alice.screenshot("call-video-other-way").await.expect("screenshot") {
        eprintln!("screenshot: {}", path.display());
    }
    bob.click(".voice-panel .camera-button")
        .await
        .expect("bob's camera off");

    // Off again: her tile goes, and with no cameras on, the stage.
    let no_tile_for_alice = "return ![...document.querySelectorAll('.video-tile')].some(t => t.dataset.user === '@alice:localhost');";
    if alice.is_phone() {
        alice
            .click(".call-screen .camera-button")
            .await
            .expect("camera off");
    } else {
        alice
            .click(".voice-panel .camera-button")
            .await
            .expect("camera off");
    }
    bob.wait_for_js(
        "return !document.querySelector('.call-stage');",
        DEFAULT_WAIT,
    )
    .await
    .expect("the stage goes when the camera does");
    alice
        .wait_for_js(no_tile_for_alice, DEFAULT_WAIT)
        .await
        .expect("alice's self view goes too");
    if alice.is_phone() {
        alice
            .click(".call-screen .call-minimize")
            .await
            .expect("down to the bar");
        alice
            .click(".chat-header .pane-back")
            .await
            .expect("back to the list");
    }

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

/// How many times the ringtone has played in `app`.
async fn rings(app: &mut App) -> u64 {
    app.eval("return (window.morpheusSounds ?? []).filter(s => s === 'ring').length;")
        .await
        .expect("sounds")
        .as_u64()
        .unwrap_or_default()
}

/// Answering a ring by joining the call some other way (the room's call
/// button) ends the ringing for good: not only while in the call, but
/// after hanging up with the caller still there.
#[tokio::test]
async fn joining_a_ringing_call_from_the_room_stops_the_ring_for_good() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "random").await;
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "random").await;
    open_dm(&mut alice, "bob").await;
    alice
        .click(".chat-header .call-button")
        .await
        .expect("call bob");
    bob.wait_for_text(".incoming-call", "alice", DEFAULT_WAIT)
        .await
        .expect("bob's app rings");

    // Bob goes to the DM and joins from its header instead.
    open_dm(&mut bob, "alice").await;
    bob.click(".chat-header .call-button")
        .await
        .expect("join from the room");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("bob connected");
    let joined = rings(&mut bob).await;
    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
    assert_eq!(rings(&mut bob).await, joined, "it rang on in the call");

    // Bob leaves, alice stays: still no ringing.
    bob.click(".voice-panel .hang-up")
        .await
        .expect("bob hangs up");
    bob.wait_for_js("return !document.querySelector('.voice-panel');", DEFAULT_WAIT)
        .await
        .expect("bob out");
    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
    assert_eq!(
        (rings(&mut bob).await, bob.count(".incoming-call").await.expect("count")),
        (joined, 0),
        "it rang again after hanging up"
    );
    // A phone hangs up from its call screen.
    let hang_up = if alice.is_phone() {
        ".call-screen .hang-up"
    } else {
        ".voice-panel .hang-up"
    };
    alice.click(hang_up).await.expect("alice hangs up");
    out_of_call(&mut alice).await;
}

/// Already in a call, a ring shows its card but plays no ringtone over the
/// call you are in.
#[tokio::test]
async fn a_ring_during_a_call_is_shown_but_silent() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch_desktop(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "random").await;
    alice
        .click(".chat-header .call-button")
        .await
        .expect("alice in a call");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("alice connected");
    let before = rings(&mut alice).await;

    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "random").await;
    open_dm(&mut bob, "alice").await;
    bob.click(".chat-header .call-button")
        .await
        .expect("bob calls alice");
    alice
        .wait_for_text(".incoming-call", "bob", DEFAULT_WAIT)
        .await
        .expect("alice sees bob calling");
    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
    assert_eq!(rings(&mut alice).await, before, "it rang over her call");

    let hang_up = if bob.is_phone() {
        ".call-screen .hang-up"
    } else {
        ".voice-panel .hang-up"
    };
    bob.click(hang_up).await.expect("bob hangs up");
    out_of_call(&mut bob).await;
    alice
        .click(".voice-panel .hang-up")
        .await
        .expect("alice hangs up");
    out_of_call(&mut alice).await;
}

/// HMAC-SHA256 (RFC 2104), for signing LiveKit tokens as the dev server does.
fn hmac_sha256(key: &[u8], message: &[u8]) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let mut block = [0u8; 64];
    block[..key.len()].copy_from_slice(key);
    let pad = |byte: u8| block.iter().map(|b| b ^ byte).collect::<Vec<u8>>();
    let inner = Sha256::new()
        .chain_update(pad(0x36))
        .chain_update(message)
        .finalize();
    Sha256::new()
        .chain_update(pad(0x5c))
        .chain_update(inner)
        .finalize()
        .to_vec()
}

/// A LiveKit token as the dev token service made it, with the identity
/// replaced by `identity` and signed again with the dev server's secret.
fn renamed(jwt: &str, identity: &str) -> String {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let mut parts = jwt.split('.');
    let (header, payload) = (parts.next().expect("header"), parts.next().expect("payload"));
    let mut claims: serde_json::Value =
        serde_json::from_slice(&b64.decode(payload).expect("payload")).expect("claims");
    claims["sub"] = serde_json::json!(identity);
    // Nor may anyone set their own LiveKit metadata or attributes there.
    if let Some(video) = claims["video"].as_object_mut() {
        video.remove("canUpdateOwnMetadata");
    }
    let signed = format!("{header}.{}", b64.encode(claims.to_string()));
    let signature = b64.encode(hmac_sha256(b"secret", signed.as_bytes()));
    format!("{signed}.{signature}")
}

/// A token service that names each device in LiveKit its own way, as
/// matrix.org's does (no name there can be worked out from the user and
/// device), and lets nobody set their own LiveKit attributes: the dev
/// service in front, each token renamed. Returns its URL.
async fn renaming_token_service() -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    tokio::spawn(async move {
        let mut served = 0u32;
        while let Ok((mut client, _)) = listener.accept().await {
            served += 1;
            let identity = format!("renamed-{served}-{}", std::process::id());
            tokio::spawn(async move {
                // One request: headers, then the body its length says.
                let mut request = Vec::new();
                let mut buf = [0u8; 8192];
                let (head_end, length) = loop {
                    let Ok(n) = client.read(&mut buf).await else { return };
                    if n == 0 {
                        return;
                    }
                    request.extend_from_slice(&buf[..n]);
                    if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&request[..end]).to_lowercase();
                        let length = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        break (end + 4, length);
                    }
                };
                while request.len() < head_end + length {
                    let Ok(n) = client.read(&mut buf).await else { return };
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&buf[..n]);
                }
                let head = String::from_utf8_lossy(&request[..head_end]).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let body = String::from_utf8_lossy(&request[head_end..]).to_string();
                let answer = tokio::process::Command::new("curl")
                    .args(["-s", "-w", "\n%{http_code}", "-X", "POST", "-H", "Content-Type: application/json"])
                    .arg(format!("http://localhost:8080{path}"))
                    .args(["-d", &body])
                    .output()
                    .await
                    .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
                    .unwrap_or_default();
                let (text, status) = answer.rsplit_once('\n').unwrap_or(("", "502"));
                let text = match serde_json::from_str::<serde_json::Value>(text) {
                    Ok(mut answer) if answer["jwt"].is_string() => {
                        let jwt = answer["jwt"].as_str().unwrap_or_default().to_owned();
                        answer["jwt"] = serde_json::json!(renamed(&jwt, &identity));
                        answer.to_string()
                    }
                    _ => text.to_owned(),
                };
                let response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                );
                let _ = client.write_all(response.as_bytes()).await;
            });
        }
    });
    url
}

/// Two people in an encrypted call hear each other, and see each other
/// speak, however the token service names them in LiveKit. On matrix.org
/// the names are not the ones worked out from user and device, so each side
/// filed the other's key under a name nobody had: nothing decrypted, and
/// each saw only themselves speak.
#[tokio::test]
async fn encrypted_calls_work_whatever_the_token_service_names_people() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let service = renaming_token_service().await;
    let env = [("MORPHEUS_LIVEKIT_SERVICE", service.as_str())];
    let mut alice = App::launch_with_env(&hs, &env).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "encrypted").await;
    let mut bob = App::launch_with_env(&hs, &env).await.expect("launch bob");
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
    bob.wait_for_js(&speaking("alice"), DEFAULT_WAIT)
        .await
        .expect("bob sees alice speak");
    alice
        .wait_for_js(&speaking("bob"), DEFAULT_WAIT)
        .await
        .expect("alice sees bob speak");
    bob.wait_for_js(&hears("alice"), DEFAULT_WAIT)
        .await
        .expect("bob hears alice (decrypted)");
    alice
        .wait_for_js(&hears("bob"), DEFAULT_WAIT)
        .await
        .expect("alice hears bob (decrypted)");
    // And a camera turned on reaches the other side, decrypted.
    bob.click(".voice-panel .camera-button")
        .await
        .expect("bob's camera on");
    alice
        .wait_for_js(&shows_test_pattern("@bob:localhost"), DEFAULT_WAIT)
        .await
        .expect("alice sees bob's camera");

    bob.click(".voice-panel .hang-up").await.expect("bob hangs up");
    alice
        .click(".voice-panel .hang-up")
        .await
        .expect("alice hangs up");
    out_of_call(&mut alice).await;
}

/// The stage's height on screen.
async fn stage_height(app: &mut App) -> f64 {
    app.eval("return document.querySelector('.call-stage').getBoundingClientRect().height;")
        .await
        .expect("height")
        .as_f64()
        .expect("number")
}

/// The stage (everyone's video above the chat) is resized by dragging its
/// bottom edge, and keeps that size the next time it shows.
#[tokio::test]
async fn the_call_stage_is_resized_by_dragging_its_edge() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch_desktop(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "random").await;
    alice
        .click(".chat-header .call-button")
        .await
        .expect("call");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("connected");
    alice
        .click(".voice-panel .camera-button")
        .await
        .expect("camera on");
    alice
        .wait_for(".call-stage + .call-stage-resize", DEFAULT_WAIT)
        .await
        .expect("the stage, with an edge to drag");
    let before = stage_height(&mut alice).await;
    // Press on the edge, move the mouse 200 px down, let go.
    alice
        .eval(
            "const h = document.querySelector('.call-stage-resize').getBoundingClientRect(); \
             const x = h.left + h.width / 2, y = h.top + h.height / 2; \
             const at = (type, dy, target) => target.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y + dy, buttons: 1 })); \
             at('mousedown', 0, document.querySelector('.call-stage-resize')); \
             for (let dy = 20; dy <= 200; dy += 20) at('mousemove', dy, window); \
             at('mouseup', 200, window); return true;",
        )
        .await
        .expect("drag");
    let after = stage_height(&mut alice).await;
    assert!(
        (after - before - 200.0).abs() < 12.0,
        "dragging the edge 200 px down made the stage {before} → {after} px"
    );

    // Off and on again: the same size.
    alice
        .click(".voice-panel .camera-button")
        .await
        .expect("camera off");
    alice
        .wait_for_js("return !document.querySelector('.call-stage');", DEFAULT_WAIT)
        .await
        .expect("stage gone");
    alice
        .click(".voice-panel .camera-button")
        .await
        .expect("camera on again");
    alice
        .wait_for(".call-stage .video-tile", DEFAULT_WAIT)
        .await
        .expect("stage again");
    let again = stage_height(&mut alice).await;
    assert!(
        (again - after).abs() < 12.0,
        "the stage forgot its size: {after} → {again} px"
    );
    if let Some(path) = alice.screenshot("call-stage-resized").await.expect("screenshot") {
        eprintln!("screenshot: {}", path.display());
    }
    alice
        .click(".voice-panel .hang-up")
        .await
        .expect("hang up");
    out_of_call(&mut alice).await;
}

/// Hung up for good: the voice panel gone, and a moment for the leave to
/// reach the call server. A test ending sooner kills the app mid-leave, and
/// its ghost stays in the room until the server gives up on it, chiming
/// "leave" in whichever test comes next.
async fn out_of_call(app: &mut App) {
    let _ = app
        .wait_for_js(
            "return !document.querySelector('.voice-panel') && !document.querySelector('.call-screen');",
            DEFAULT_WAIT,
        )
        .await;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
}

/// What the app's sounds did: "ring: played", or the reason one did not
/// play (a page may only play sound after a tap or click).
async fn sound_plays(app: &mut App) -> Vec<String> {
    serde_json::from_value(
        app.eval("return window.morpheusSoundPlays ?? [];")
            .await
            .expect("sounds"),
    )
    .unwrap_or_default()
}

/// A call ringing a desktop is heard: the ringtone really plays (test runs
/// otherwise only note their sounds; this one has real audio).
#[tokio::test]
async fn a_ring_is_heard_on_the_desktop() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "random").await;
    let mut bob = App::launch_with_env(&hs, &[("MORPHEUS_CALL_AUDIO", "platform")])
        .await
        .expect("launch bob, with real sound");
    login_and_open(&mut bob, "bob", "random").await;
    open_dm(&mut alice, "bob").await;
    let hang_up = if alice.is_phone() {
        ".call-screen .hang-up"
    } else {
        ".voice-panel .hang-up"
    };
    alice
        .click(".chat-header .call-button")
        .await
        .expect("alice calls bob");
    bob.wait_for_text(".incoming-call", "alice", DEFAULT_WAIT)
        .await
        .expect("bob is rung");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    let plays = loop {
        let plays = sound_plays(&mut bob).await;
        if plays.iter().any(|p| p.starts_with("ring: ")) {
            break plays;
        }
        assert!(std::time::Instant::now() < deadline, "no ringtone tried: {plays:?}");
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    };
    alice.click(hang_up).await.expect("alice hangs up");
    out_of_call(&mut alice).await;
    assert!(
        plays.iter().any(|p| p == "ring: played") && !plays.iter().any(|p| p.starts_with("ring: ") && p != "ring: played"),
        "the ringtone did not play: {plays:?}"
    );
}
