//! Calls, Discord-style: the phone button in a room's header joins its call;
//! everyone sees who is in it under the room in the sidebar (ringed while
//! they speak); the voice panel at the bottom of the sidebar mutes, deafens
//! and hangs up. Test runs send a tone instead of the microphone.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT, api_as};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

/// The seeded voice channels: #hangout, and #vault (encrypted).
const VOICE_CHANNELS: [&str; 2] = ["hangout", "vault"];

/// Open a room through the switcher (Ctrl+K), which shows it without joining
/// a voice channel's call.
/// The button that starts or joins a call: in a DM's top bar, or a voice
/// channel's Join (its page has no top bar).
const CALL_BUTTON: &str = ".chat-header .call-button, .voice-page-join .join-voice";

async fn open_without_joining(app: &mut App, room: &str) {
    app.key_combo("body", "k", true, false, false)
        .await
        .expect("ctrl+k");
    app.wait_for(".switcher input", DEFAULT_WAIT)
        .await
        .expect("switcher");
    app.type_into(".switcher input", room)
        .await
        .expect("filter");
    app.wait_for_text(".switcher-item.active", room, DEFAULT_WAIT)
        .await
        .expect("room offered");
    app.press(".switcher input", "Enter").await.expect("pick");
    // A voice channel's page has no top bar: its name is on the Join screen.
    app.wait_for_text(".chat-header h2, .voice-page-join h2", room, DEFAULT_WAIT)
        .await
        .expect("room open");
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
    // Calls belong to voice channels (and DMs). Clicking a voice channel's
    // row joins its call; the switcher only opens it, so a test can start or
    // join from the call button as a step of its own.
    if VOICE_CHANNELS.contains(&room) {
        open_without_joining(app, room).await;
        // A voice channel opens as a call page (no message box).
        return;
    } else if let Err(err) = app.open_room(room).await {
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
    // A voice channel of its own: nobody an earlier test left behind can
    // come or go in it (and chime).
    let channel = fresh_voice_channel(&hs);
    login_and_open(&mut alice, "alice", "general").await;
    open_without_joining(&mut alice, &channel).await;
    let hangout_id: String = alice
        .eval("return document.querySelector('.room.selected').dataset.room;")
        .await
        .expect("room id")
        .as_str()
        .expect("string")
        .to_owned();

    // Alice starts a call from the header.
    alice.click(CALL_BUTTON).await.expect("call");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("voice panel");
    alice
        .wait_for_text(".voice-panel", &channel, DEFAULT_WAIT)
        .await
        .expect("names the room");
    alice
        .wait_for_js(&in_call(&hangout_id, "alice"), DEFAULT_WAIT)
        .await
        .expect("alice listed under the room");
    alice
        .wait_for_js(&chimes_are(&["join"]), DEFAULT_WAIT)
        .await
        .expect("a chime as she joins");

    // Bob sees her there before joining, and joins.
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "general").await;
    open_without_joining(&mut bob, &channel).await;
    bob.wait_for_js(&in_call(&hangout_id, "alice"), DEFAULT_WAIT)
        .await
        .expect("bob sees alice in the call");
    bob.click(CALL_BUTTON).await.expect("join");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("bob connected");
    alice
        .wait_for_js(&in_call(&hangout_id, "bob"), DEFAULT_WAIT)
        .await
        .expect("alice sees bob join");
    if let Err(err) = alice
        .wait_for_js(&chimes_are(&["join", "join"]), DEFAULT_WAIT)
        .await
    {
        let heard = alice
            .eval("return (window.morpheusSounds ?? []).join(',');")
            .await;
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
    // (A voice channel's page shows the call; its chat opens beside it.)
    alice
        .click(".voice-chat-toggle")
        .await
        .expect("open the chat");
    alice
        .wait_for(".voice-chat .timeline", DEFAULT_WAIT)
        .await
        .expect("the chat");
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
        &format!("return !(() => {{ {} }})();", in_call(&hangout_id, "alice")),
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
    // #vault starts without calls allowed (the seed resets it): alice,
    // an admin, turns them on by joining, and bob can then join too.
    // Both devices exist before either sends, so room keys reach both.
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "vault").await;
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "vault").await;

    alice.click(CALL_BUTTON).await.expect("call");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("alice connected");
    bob.click(CALL_BUTTON).await.expect("join");
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
    login_and_open(&mut alice, "alice", "hangout").await;
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
            "return [...document.querySelectorAll('.voice-devices .device-choice:not([data-kind=camera]) .device-current')].map(s => s.dataset.id).join(',');",
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
    open_without_joining(&mut alice, "hangout").await;
    alice.click(CALL_BUTTON).await.expect("join");
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
    login_and_open(&mut alice, "alice", "hangout").await;
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "hangout").await;
    open_dm(&mut alice, "bob").await;

    // Alice calls; bob's app rings.
    alice.click(CALL_BUTTON).await.expect("call bob");
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
    alice.click(CALL_BUTTON).await.expect("call bob again");
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
    login_and_open(&mut alice, "alice", "vault").await;
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "vault").await;
    alice.click(CALL_BUTTON).await.expect("call");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("alice connected");
    bob.click(CALL_BUTTON).await.expect("join");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("bob connected");
    // No cameras yet: the call shows (a card each), but no video.
    assert_eq!(
        bob.count(".call-stage .video-tile").await.expect("count"),
        0
    );

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
        open_without_joining(&mut alice, "vault").await;
        // The call's own screen, where a phone shows the video.
        alice.click(CALL_BUTTON).await.expect("open the call");
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
            .wait_for(
                ".call-screen .call-tile[data-user=\"@bob:localhost\"]",
                DEFAULT_WAIT,
            )
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
        if let Some(path) = alice
            .screenshot("android-call-two")
            .await
            .expect("screenshot")
        {
            eprintln!("screenshot: {}", path.display());
        }
        assert_eq!(
            stacked,
            serde_json::json!(true),
            "the two tiles stack on a phone"
        );
    }

    // The other way: bob's camera reaches alice.
    bob.click(".voice-panel .camera-button")
        .await
        .expect("bob's camera on");
    alice
        .wait_for_js(&shows_test_pattern("@bob:localhost"), DEFAULT_WAIT)
        .await
        .expect("alice sees bob's camera");
    if let Some(path) = alice
        .screenshot("call-video-other-way")
        .await
        .expect("screenshot")
    {
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
        "return !document.querySelector('.call-stage .video-tile');",
        DEFAULT_WAIT,
    )
    .await
    .expect("her video goes when the camera does");
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
    login_and_open(&mut alice, "alice", "hangout").await;
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "hangout").await;
    alice.click(CALL_BUTTON).await.expect("call");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("alice connected");
    bob.click(CALL_BUTTON).await.expect("join");
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
    login_and_open(&mut alice, "alice", "hangout").await;
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "hangout").await;
    open_dm(&mut alice, "bob").await;
    alice.click(CALL_BUTTON).await.expect("call bob");
    bob.wait_for_text(".incoming-call", "alice", DEFAULT_WAIT)
        .await
        .expect("bob's app rings");

    // Bob goes to the DM and joins from its header instead.
    open_dm(&mut bob, "alice").await;
    bob.click(CALL_BUTTON).await.expect("join from the room");
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
    bob.wait_for_js(
        "return !document.querySelector('.voice-panel');",
        DEFAULT_WAIT,
    )
    .await
    .expect("bob out");
    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
    assert_eq!(
        (
            rings(&mut bob).await,
            bob.count(".incoming-call").await.expect("count")
        ),
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
    login_and_open(&mut alice, "alice", "hangout").await;
    alice.click(CALL_BUTTON).await.expect("alice in a call");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("alice connected");
    let before = rings(&mut alice).await;

    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "hangout").await;
    open_dm(&mut bob, "alice").await;
    bob.click(CALL_BUTTON).await.expect("bob calls alice");
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
    let (header, payload) = (
        parts.next().expect("header"),
        parts.next().expect("payload"),
    );
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
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
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
                    let Ok(n) = client.read(&mut buf).await else {
                        return;
                    };
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
                    let Ok(n) = client.read(&mut buf).await else {
                        return;
                    };
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&buf[..n]);
                }
                let head = String::from_utf8_lossy(&request[..head_end]).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let body = String::from_utf8_lossy(&request[head_end..]).to_string();
                let answer = tokio::process::Command::new("curl")
                    .args([
                        "-s",
                        "-w",
                        "\n%{http_code}",
                        "-X",
                        "POST",
                        "-H",
                        "Content-Type: application/json",
                    ])
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
    login_and_open(&mut alice, "alice", "vault").await;
    let mut bob = App::launch_with_env(&hs, &env).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "vault").await;

    alice.click(CALL_BUTTON).await.expect("call");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("alice connected");
    bob.click(CALL_BUTTON).await.expect("join");
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

    bob.click(".voice-panel .hang-up")
        .await
        .expect("bob hangs up");
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
    // A DM call: a voice channel's page is all call, with nothing to resize.
    login_and_open(&mut alice, "alice", "general").await;
    open_dm(&mut alice, "bob").await;
    alice.click(CALL_BUTTON).await.expect("call");
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
        .wait_for_js(
            "return !document.querySelector('.call-stage .video-tile');",
            DEFAULT_WAIT,
        )
        .await
        .expect("her video gone");
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
    if let Some(path) = alice
        .screenshot("call-stage-resized")
        .await
        .expect("screenshot")
    {
        eprintln!("screenshot: {}", path.display());
    }
    alice.click(".voice-panel .hang-up").await.expect("hang up");
    out_of_call(&mut alice).await;
}

/// How the stage's cards sit: each card's box, its picture's box, whose it
/// is, and the stage's box. Fails (returns why) unless every card is the same
/// size, wholly inside the stage, and a camera fills its card.
const STAGE_CARDS: &str = "
    const stage = document.querySelector('.call-stage');
    if (!stage) return 'no stage';
    const s = stage.getBoundingClientRect();
    const cards = [...stage.querySelectorAll('.video-tile, .call-tile')];
    if (cards.length < 2) return 'cards: ' + cards.length;
    const boxes = cards.map(c => c.getBoundingClientRect());
    const w = boxes[0].width, h = boxes[0].height;
    if (w < 80 || h < 45) return 'tiny cards ' + w + 'x' + h;
    for (const [i, b] of boxes.entries()) {
        if (Math.abs(b.width - w) > 1.5 || Math.abs(b.height - h) > 1.5)
            return 'card ' + i + ' is ' + b.width + 'x' + b.height + ', not ' + w + 'x' + h;
        if (b.left < s.left - 1 || b.right > s.right + 1 || b.top < s.top - 1 || b.bottom > s.bottom + 1)
            return 'card ' + i + ' ' + JSON.stringify(b) + ' outside the stage ' + JSON.stringify(s);
    }
    for (const t of stage.querySelectorAll('.video-tile')) {
        const b = t.getBoundingClientRect(), v = t.querySelector('img.video').getBoundingClientRect();
        if (Math.abs(v.width - b.width) > 1.5 || Math.abs(v.height - b.height) > 1.5)
            return 'a camera ' + v.width + 'x' + v.height + ' in a card ' + b.width + 'x' + b.height;
    }
    return 'ok ' + w + 'x' + h;";

/// With a camera on, everyone in the call is a card of the same size, as in
/// Discord: the camera shows inside its person's card, the cards fit the
/// stage, and still do (bigger) once the stage is dragged taller.
#[tokio::test]
async fn a_camera_shows_in_its_persons_card_among_equal_cards() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch_desktop(&hs).await.expect("launch alice");
    // A DM call (a voice channel's page is all call, with nothing to resize).
    login_and_open(&mut alice, "alice", "general").await;
    open_dm(&mut alice, "bob").await;
    let mut bob = App::launch_desktop(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "general").await;
    open_dm(&mut bob, "alice").await;
    alice.click(CALL_BUTTON).await.expect("call");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("alice connected");
    bob.click(CALL_BUTTON).await.expect("join");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("bob connected");
    bob.click(".voice-panel .camera-button")
        .await
        .expect("bob's camera");
    alice
        .wait_for_js(&shows_test_pattern("@bob:localhost"), DEFAULT_WAIT)
        .await
        .expect("alice sees bob's camera");
    alice
        .wait_for(
            ".call-stage .call-tile[data-user=\"@alice:localhost\"]",
            DEFAULT_WAIT,
        )
        .await
        .expect("alice's own card");
    // Short to begin with, so its height (not the window's width) sets the
    // cards' size.
    drag_stage(&mut alice, -400).await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let at_first = alice.eval(STAGE_CARDS).await.expect("cards");
    if let Some(path) = alice.screenshot("call-cards").await.expect("screenshot") {
        eprintln!("screenshot: {}", path.display());
    }
    let first: f64 = at_first
        .as_str()
        .and_then(|s| s.strip_prefix("ok "))
        .and_then(|s| s.split('x').next())
        .and_then(|w| w.parse().ok())
        .unwrap_or_else(|| panic!("the cards: {at_first}"));

    // Drag the stage 200 px taller: the cards grow and still fit.
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
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let resized = alice.eval(STAGE_CARDS).await.expect("cards");
    if let Some(path) = alice
        .screenshot("call-cards-resized")
        .await
        .expect("screenshot")
    {
        eprintln!("screenshot: {}", path.display());
    }
    let later: f64 = resized
        .as_str()
        .and_then(|s| s.strip_prefix("ok "))
        .and_then(|s| s.split('x').next())
        .and_then(|w| w.parse().ok())
        .unwrap_or_else(|| panic!("the cards after a resize: {resized}"));
    assert!(
        later > first,
        "bigger cards in a taller stage: {first} → {later}"
    );
    // Put the stage's size back for other tests.
    alice
        .eval("localStorage.removeItem('morpheus.callStageHeight'); return true;")
        .await
        .expect("forget the size");

    bob.click(".voice-panel .hang-up")
        .await
        .expect("bob hangs up");
    alice.click(".voice-panel .hang-up").await.expect("hang up");
    out_of_call(&mut bob).await;
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

/// A call ringing a desktop is heard: the app plays the ringtone itself
/// (not its page, which can't from the tray), through a real audio stream
/// (test runs otherwise only note their sounds; this one has real audio).
#[tokio::test]
async fn a_ring_is_heard_on_the_desktop() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "hangout").await;
    let mut bob = App::launch_with_env(&hs, &[("MORPHEUS_CALL_AUDIO", "platform")])
        .await
        .expect("launch bob, with real sound");
    login_and_open(&mut bob, "bob", "hangout").await;
    open_dm(&mut alice, "bob").await;
    let hang_up = if alice.is_phone() {
        ".call-screen .hang-up"
    } else {
        ".voice-panel .hang-up"
    };
    alice.click(CALL_BUTTON).await.expect("alice calls bob");
    bob.wait_for_text(".incoming-call", "alice", DEFAULT_WAIT)
        .await
        .expect("bob is rung");
    let pid = bob.pid().expect("a desktop app");
    let played = bob.data_dir().join("played-sounds.log");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    let mut streams = Vec::new();
    while std::time::Instant::now() < deadline {
        let rang = std::fs::read_to_string(&played)
            .unwrap_or_default()
            .contains("ring");
        // The page no longer plays the ring: a stream now is the app's. The
        // ringtone goes through PipeWire's ALSA layer, which names it after
        // the program but records no process (bob is the one with sound on).
        streams = playing_streams(pid);
        streams.extend(alsa_streams_of("morpheus-desktop"));
        if rang && !streams.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
    alice.click(hang_up).await.expect("alice hangs up");
    out_of_call(&mut alice).await;
    assert!(
        std::fs::read_to_string(&played)
            .unwrap_or_default()
            .contains("ring"),
        "the app rang"
    );
    assert!(
        !streams.is_empty(),
        "no audio stream from the app while ringing; its log ends:\n{}",
        bob.log_tail(30)
    );
}

/// The app's processes (it and WebKit's helpers, which play the page's
/// audio) that have a playback stream open in PipeWire right now.
fn playing_streams(pid: u32) -> Vec<String> {
    let mut family = vec![pid];
    let mut at = 0;
    while at < family.len() {
        let parent = family[at];
        at += 1;
        if let Ok(children) =
            std::fs::read_to_string(format!("/proc/{parent}/task/{parent}/children"))
        {
            family.extend(
                children
                    .split_whitespace()
                    .filter_map(|c| c.parse::<u32>().ok()),
            );
        }
    }
    let Ok(out) = std::process::Command::new("pw-dump").output() else {
        return vec!["no pw-dump".into()];
    };
    let nodes: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    nodes
        .as_array()
        .into_iter()
        .flatten()
        .filter(|n| n["info"]["props"]["media.class"] == "Stream/Output/Audio")
        .filter(|n| {
            n["info"]["props"]["application.process.id"]
                .as_u64()
                .or_else(|| {
                    n["info"]["props"]["application.process.id"]
                        .as_str()?
                        .parse()
                        .ok()
                })
                .is_some_and(|p| family.contains(&(p as u32)))
        })
        .map(|n| {
            format!(
                "{} app {} channels {} volumes {} ({})",
                n["info"]["props"]["node.name"],
                n["info"]["props"]["application.name"],
                n["info"]["props"]["audio.channels"],
                n["info"]["params"]["Props"][0]["channelVolumes"],
                n["info"]["state"]
            )
        })
        .collect()
}

/// The desktop's sounds come out of the speakers: pressing mute plays its
/// chime through a real audio stream, not only a play() the page accepted.
#[tokio::test]
async fn a_chime_comes_out_of_the_speakers() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_with_env(&hs, &[("MORPHEUS_CALL_AUDIO", "platform")])
        .await
        .expect("launch, with real sound");
    login_and_open(&mut app, "alice", "general").await;
    let pid = app.pid().expect("a desktop app");
    app.click(".user-bar .mute-button").await.expect("mute");
    let mut seen = Vec::new();
    for _ in 0..100 {
        seen = playing_streams(pid)
            .into_iter()
            .filter(|s| s.ends_with("(\"running\")"))
            .collect();
        if !seen.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let plays = sound_plays(&mut app).await;
    app.click(".user-bar .mute-button").await.expect("unmute");
    assert!(
        !seen.is_empty(),
        "no audio stream from the app; the page said: {plays:?}; streams: {:?}",
        playing_streams(pid)
    );
    // Heard, not only played: a volume of 0 saved for the app (WirePlumber
    // keeps one per app, across restarts) must not silence it.
    assert!(
        seen.iter()
            .all(|s| !s.contains("volumes [0.0") && !s.contains("volumes [0]")),
        "the app's audio plays at no volume: {seen:?}"
    );
}

/// A voice channel (Element's video room: a room of type
/// org.matrix.msc3417.call) works as Discord's: listed with a speaker,
/// clicking it joins its call, clicking it again stays in, and others see
/// who is in it under it and join the same way.
#[tokio::test]
async fn clicking_a_voice_channel_joins_its_call() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "general").await;
    let lounge = "[...document.querySelectorAll('.room')].find(r => r.querySelector('.room-name')?.textContent.trim() === 'lounge')";
    alice
        .wait_for_js(&format!("return !!{lounge};"), DEFAULT_WAIT)
        .await
        .expect("the voice channel listed");
    assert_eq!(
        alice
            .eval(&format!("const r = {lounge}; return [r.classList.contains('voice'), !!r.querySelector('.voice-icon'), r.querySelector('.hash') === null];"))
            .await
            .expect("row"),
        serde_json::json!([true, true, true]),
        "listed with a speaker, not a #"
    );
    let room_id: String = alice
        .eval(&format!("return {lounge}.dataset.room;"))
        .await
        .expect("room id")
        .as_str()
        .expect("string")
        .to_owned();

    alice
        .eval(&format!("{lounge}.click();"))
        .await
        .expect("click it");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("clicking joined its call");
    alice
        .wait_for_text(".voice-panel", "lounge", DEFAULT_WAIT)
        .await
        .expect("in the lounge");
    alice
        .wait_for_js(&in_call(&room_id, "alice"), DEFAULT_WAIT)
        .await
        .expect("listed under it");
    // Clicking it again does not leave.
    alice
        .eval(&format!("{lounge}.click();"))
        .await
        .expect("click again");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    alice
        .wait_for_text(
            ".voice-panel",
            "Voice connected",
            std::time::Duration::from_secs(2),
        )
        .await
        .expect("still in");

    // Bob sees her in it, and joins the same way.
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "general").await;
    bob.wait_for_js(&in_call(&room_id, "alice"), DEFAULT_WAIT)
        .await
        .expect("bob sees alice in the voice channel");
    bob.eval(&format!("{lounge}.click();"))
        .await
        .expect("bob clicks it");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("bob joined");
    alice
        .wait_for_js(&in_call(&room_id, "bob"), DEFAULT_WAIT)
        .await
        .expect("alice sees bob join");

    for app in [&mut alice, &mut bob] {
        let _ = app.click(".voice-panel .hang-up").await;
        out_of_call(app).await;
    }
}

/// In a call, its controls are part of the profile card at the bottom left
/// (which grows to hold them), and hanging up is red wherever it shows.
#[tokio::test]
async fn call_controls_live_in_the_profile_card_with_a_red_hang_up() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "general").await;
    let card_before = alice
        .eval("return document.querySelector('.user-pill').getBoundingClientRect().height;")
        .await
        .expect("card")
        .as_f64()
        .expect("height");
    let lounge = "[...document.querySelectorAll('.room')].find(r => r.querySelector('.room-name')?.textContent.trim() === 'lounge')";
    alice
        .wait_for_js(&format!("return !!{lounge};"), DEFAULT_WAIT)
        .await
        .expect("the voice channel listed");
    alice
        .eval(&format!("{lounge}.click();"))
        .await
        .expect("join the lounge");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("in the call");
    let card = alice
        .eval(
            "const panel = document.querySelector('.voice-panel'); \
             const pill = panel.closest('.user-pill'); \
             const probe = document.createElement('div'); probe.style.background = 'var(--danger)'; document.body.append(probe); \
             const danger = getComputedStyle(probe).backgroundColor; probe.remove(); \
             const hangUps = [...document.querySelectorAll('.hang-up')].filter(b => b.offsetParent !== null); \
             return { in_card: !!pill, height: pill ? pill.getBoundingClientRect().height : 0, danger, \
               hang_ups: hangUps.map(b => getComputedStyle(b).backgroundColor) };",
        )
        .await
        .expect("card");
    assert_eq!(
        card["in_card"], true,
        "call controls outside the profile card: {card}"
    );
    assert!(
        card["height"].as_f64().unwrap_or(0.0) > card_before + 20.0,
        "the profile card did not grow to hold them ({card_before}px before): {card}"
    );
    let hang_ups = card["hang_ups"].as_array().expect("hang-ups");
    assert!(!hang_ups.is_empty(), "no hang-up shown: {card}");
    assert!(
        hang_ups.iter().all(|c| c == &card["danger"]),
        "a hang-up that is not red: {card}"
    );
    alice.click(".voice-panel .hang-up").await.expect("hang up");
    alice
        .wait_for_js(
            "return !document.querySelector('.voice-panel');",
            DEFAULT_WAIT,
        )
        .await
        .expect("left the call");
}

/// Join #hangout by opening it without joining, then the call button.
async fn join_hangout(app: &mut App, user: &str) {
    login_and_open(app, user, "hangout").await;
    app.click(CALL_BUTTON).await.expect("join");
    app.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("in the call");
}

/// In any call the call's view shows, cameras or not: a card per person
/// showing whether they are muted or deafened, and the controls along the
/// bottom (mute, deafen, camera, screen, leave), as in Discord.
#[tokio::test]
async fn the_call_view_shows_in_any_call_with_its_controls() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    join_hangout(&mut alice, "alice").await;
    alice
        .wait_for(
            ".call-stage .call-tile[data-user=\"@alice:localhost\"]",
            DEFAULT_WAIT,
        )
        .await
        .expect("the call view, with no camera on");
    for control in [
        ".mute-button",
        ".deafen-button",
        ".camera-button",
        ".screen-button",
        ".hang-up",
    ] {
        alice
            .wait_for(
                &format!(".call-stage .call-controls {control}"),
                DEFAULT_WAIT,
            )
            .await
            .unwrap_or_else(|e| panic!("{control} along the bottom: {e}"));
    }
    let card = ".call-stage [data-user=\"@alice:localhost\"]";
    alice
        .click(".call-stage .call-controls .mute-button")
        .await
        .expect("mute");
    alice
        .wait_for(&format!("{card} .voice-state.muted"), DEFAULT_WAIT)
        .await
        .expect("her card shows her muted");
    alice
        .wait_for(".user-pill .mute-button.active", DEFAULT_WAIT)
        .await
        .expect("the same mute as the profile card's");
    alice
        .click(".call-stage .call-controls .deafen-button")
        .await
        .expect("deafen");
    alice
        .wait_for(&format!("{card} .voice-state.deafened"), DEFAULT_WAIT)
        .await
        .expect("her card shows her deafened");
    alice
        .click(".call-stage .call-controls .deafen-button")
        .await
        .expect("undeafen");
    alice
        .click(".call-stage .call-controls .mute-button")
        .await
        .expect("unmute");
    alice
        .wait_for_js(
            &format!("return !document.querySelector('{card} .voice-state');"),
            DEFAULT_WAIT,
        )
        .await
        .expect("no mark when talking");
    alice
        .click(".call-stage .call-controls .hang-up")
        .await
        .expect("leave");
    alice
        .wait_for_js(
            "return !document.querySelector('.call-stage');",
            DEFAULT_WAIT,
        )
        .await
        .expect("left");
}

/// Everyone sees each person's state: muted or deafened and their camera
/// on their card in the call, and the same, with LIVE for a shared screen,
/// beside their name in the sidebar's list of who is in the call (also for
/// someone not in it).
#[tokio::test]
async fn others_see_who_is_muted_deafened_on_camera_or_live() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    join_hangout(&mut alice, "alice").await;
    let mut carol = App::launch(&hs).await.expect("launch carol");
    login_and_open(&mut carol, "carol", "general").await;
    let mut bob = App::launch(&hs).await.expect("launch bob");
    join_hangout(&mut bob, "bob").await;

    let listed = "[...document.querySelectorAll('.room-call .call-participant')].find(p => p.textContent.includes('bob'))";
    let card = ".call-stage [data-user=\"@bob:localhost\"]";

    bob.click(".call-stage .call-controls .mute-button")
        .await
        .expect("bob mutes");
    alice
        .wait_for(&format!("{card} .voice-state.muted"), DEFAULT_WAIT)
        .await
        .expect("alice sees bob muted on his card");
    for app in [&mut alice, &mut carol] {
        app.wait_for_js(
            &format!("const p = {listed}; return !!p && !!p.querySelector('.voice-state.muted');"),
            DEFAULT_WAIT,
        )
        .await
        .expect("bob listed as muted");
    }

    bob.click(".call-stage .call-controls .deafen-button")
        .await
        .expect("bob deafens");
    carol
        .wait_for_js(
            &format!(
                "const p = {listed}; return !!p && !!p.querySelector('.voice-state.deafened');"
            ),
            DEFAULT_WAIT,
        )
        .await
        .expect("bob listed as deafened");

    bob.click(".call-stage .call-controls .camera-button")
        .await
        .expect("camera on");
    bob.click(".call-stage .call-controls .screen-button")
        .await
        .expect("share the screen");
    for app in [&mut alice, &mut carol] {
        app.wait_for_js(
            &format!("const p = {listed}; return !!p && !!p.querySelector('.media-camera') && !!p.querySelector('.media-live');"),
            DEFAULT_WAIT,
        )
        .await
        .expect("bob listed with his camera and LIVE");
    }
    carol
        .wait_for_text(
            ".room-call .call-participant .media-live",
            "LIVE",
            DEFAULT_WAIT,
        )
        .await
        .expect("says LIVE");

    bob.click(".call-stage .call-controls .hang-up")
        .await
        .expect("bob leaves");
    alice
        .click(".call-stage .call-controls .hang-up")
        .await
        .expect("alice leaves");
}

/// Joining a call where someone already has their camera on and shares a
/// screen shows both straight away, and again after leaving and rejoining.
#[tokio::test]
async fn joining_and_rejoining_picks_up_cameras_and_screens() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut bob = App::launch(&hs).await.expect("launch bob");
    join_hangout(&mut bob, "bob").await;
    bob.click(".voice-panel .camera-button")
        .await
        .expect("camera on");
    bob.wait_for(".voice-panel .camera-button.active", DEFAULT_WAIT)
        .await
        .expect("camera on");
    bob.click(".voice-panel .screen-button")
        .await
        .expect("share");
    bob.wait_for(".voice-panel .screen-button.active", DEFAULT_WAIT)
        .await
        .expect("sharing");

    let mut alice = App::launch(&hs).await.expect("launch alice");
    let both = "const tiles = [...document.querySelectorAll('.call-stage .video-tile')].filter(t => t.dataset.user === '@bob:localhost'); \
        const shown = t => { const i = t.querySelector('img.video'); return !!i && i.complete && i.naturalWidth > 0; }; \
        return tiles.some(t => t.classList.contains('screen') && shown(t)) && tiles.some(t => !t.classList.contains('screen') && shown(t));";
    join_hangout(&mut alice, "alice").await;
    alice
        .wait_for_js(both, DEFAULT_WAIT)
        .await
        .expect("bob's camera and screen on joining");
    for round in 1..=2 {
        alice.click(".voice-panel .hang-up").await.expect("leave");
        alice
            .wait_for_js(
                "return !document.querySelector('.voice-panel');",
                DEFAULT_WAIT,
            )
            .await
            .expect("left");
        alice.click(CALL_BUTTON).await.expect("rejoin");
        alice
            .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
            .await
            .expect("back in");
        alice
            .wait_for_js(both, DEFAULT_WAIT)
            .await
            .unwrap_or_else(|e| panic!("bob's camera and screen after rejoining ({round}): {e}"));
    }
    alice.click(".voice-panel .hang-up").await.expect("leave");
    bob.click(".voice-panel .hang-up")
        .await
        .expect("bob leaves");
}

/// A new voice channel for alice and bob, named uniquely; its name.
fn fresh_voice_channel(hs: &str) -> String {
    let name = format!(
        "voice {} {}",
        std::process::id(),
        NEXT_CHANNEL.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    );
    let room = api_as(
        hs,
        "alice",
        "POST",
        "createRoom",
        Some(serde_json::json!({
            "name": name,
            "preset": "public_chat",
            "creation_content": { "type": "org.matrix.msc3417.call" },
            "power_level_content_override": { "events": { "org.matrix.msc3401.call.member": 0 } },
            "invite": ["@bob:localhost"],
        })),
    )
    .expect("a voice channel");
    let id = room["room_id"]
        .as_str()
        .expect("room id")
        .replace('!', "%21")
        .replace(':', "%3A");
    api_as(
        hs,
        "bob",
        "POST",
        &format!("rooms/{id}/join"),
        Some(serde_json::json!({})),
    )
    .expect("bob joins");
    name
}

/// Tells this run's fresh voice channels apart.
static NEXT_CHANNEL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Drag the call stage's bottom edge by `dy` pixels (down is taller).
async fn drag_stage(app: &mut App, dy: i32) {
    let step = if dy < 0 { -20 } else { 20 };
    app.eval(&format!(
        "const h = document.querySelector('.call-stage-resize').getBoundingClientRect(); \
         const x = h.left + h.width / 2, y = h.top + h.height / 2; \
         const at = (type, d, target) => target.dispatchEvent(new MouseEvent(type, {{ bubbles: true, cancelable: true, clientX: x, clientY: y + d, buttons: 1 }})); \
         at('mousedown', 0, document.querySelector('.call-stage-resize')); \
         for (let d = {step}; Math.abs(d) <= Math.abs({dy}); d += {step}) at('mousemove', d, window); \
         at('mouseup', {dy}, window); return true;"
    ))
    .await
    .expect("drag the stage");
}

/// Settings → Voice & Video: each device list has its icon, and the camera
/// is chosen there too (kept on the device).
#[tokio::test]
async fn voice_settings_have_icons_and_a_camera_choice() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "general").await;
    open_voice_settings(&mut alice).await;
    alice
        .wait_for(
            ".voice-devices .device-choice[data-kind=camera] .device-current",
            DEFAULT_WAIT,
        )
        .await
        .expect("a camera choice");
    let bare = alice
        .eval("return [...document.querySelectorAll('.voice-devices .device-choice')].filter(c => !c.querySelector('.device-label svg.icon')).map(c => c.dataset.kind);")
        .await
        .expect("icons");
    assert_eq!(bare, serde_json::json!([]), "device lists without an icon");
    choose(&mut alice, "camera", "test-camera-2").await;
    alice
        .wait_for(
            ".device-choice[data-kind=camera] .device-current[data-id=\"test-camera-2\"]",
            DEFAULT_WAIT,
        )
        .await
        .expect("chosen");
    let saved = alice.data_dir().join("audio_devices.json");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while !std::fs::read_to_string(&saved)
        .unwrap_or_default()
        .contains("test-camera-2")
    {
        assert!(std::time::Instant::now() < deadline, "camera not kept");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

/// The profile card's mic and headphones each have an arrow (and a
/// right-click) for a quick list of microphones or speakers; picking one
/// switches to it. Right-clicking the camera button in a call lists cameras.
#[tokio::test]
async fn quick_device_menus_on_the_mic_headphones_and_camera() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "general").await;
    let menu_has = |items: &[&str]| {
        format!(
            "const labels = [...document.querySelectorAll('.context-menu .context-item')].map(i => i.textContent.trim()); \
             return {}.every(w => labels.some(l => l.includes(w)));",
            serde_json::to_string(items).expect("json")
        )
    };

    alice
        .click(".user-pill .mute-button + .device-menu-button")
        .await
        .expect("the arrow beside the mic");
    alice
        .wait_for_js(
            &menu_has(&["Test microphone 1", "Test microphone 2"]),
            DEFAULT_WAIT,
        )
        .await
        .expect("microphones listed");
    alice
        .click_text(".context-menu .context-item", "Test microphone 2")
        .await
        .expect("pick");
    alice
        .wait_for_js(
            "return !document.querySelector('.context-menu');",
            DEFAULT_WAIT,
        )
        .await
        .expect("menu closes");
    // Right-click works as the arrow does, and shows the pick.
    alice
        .right_click(".user-pill .mute-button")
        .await
        .expect("right-click the mic");
    alice
        .wait_for_js(
            "return [...document.querySelectorAll('.context-menu .context-item.checked')].some(i => i.textContent.includes('Test microphone 2'));",
            DEFAULT_WAIT,
        )
        .await
        .expect("the chosen microphone ticked");
    alice.press("body", "Escape").await.expect("close");

    alice
        .right_click(".user-pill .deafen-button")
        .await
        .expect("right-click the headphones");
    alice
        .wait_for_js(
            &menu_has(&["Test speaker 1", "Test speaker 2"]),
            DEFAULT_WAIT,
        )
        .await
        .expect("speakers listed");
    alice.press("body", "Escape").await.expect("close");
    assert_eq!(
        alice
            .count(".user-pill .deafen-button + .device-menu-button")
            .await
            .expect("count"),
        1,
        "an arrow beside the headphones"
    );

    // In a call: the camera button's right-click lists cameras.
    open_without_joining(&mut alice, "hangout").await;
    alice.click(CALL_BUTTON).await.expect("join");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("in the call");
    alice
        .right_click(".voice-panel .camera-button")
        .await
        .expect("right-click the camera");
    alice
        .wait_for_js(&menu_has(&["Test camera 1", "Test camera 2"]), DEFAULT_WAIT)
        .await
        .expect("cameras listed");
    alice.press("body", "Escape").await.expect("close");
    alice.click(".voice-panel .hang-up").await.expect("leave");
}

/// A device list in Settings closes when you click elsewhere.
#[tokio::test]
async fn a_device_list_closes_on_a_click_elsewhere() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "general").await;
    open_voice_settings(&mut alice).await;
    alice
        .click(".voice-devices .device-choice[data-kind=microphone] .device-current")
        .await
        .expect("open the list");
    alice
        .wait_for(".device-options", DEFAULT_WAIT)
        .await
        .expect("open");
    alice
        .click(".settings-page h2")
        .await
        .expect("click elsewhere");
    alice
        .wait_for_js(
            "return !document.querySelector('.device-options');",
            DEFAULT_WAIT,
        )
        .await
        .expect("the list closes");
}

/// Muted (or deafened), the profile card's button and its arrow are one red
/// thing: both icons red on a pale red background spanning the two, with a
/// thin line of the card's grey between them.
#[tokio::test]
async fn muted_marks_the_mic_and_its_arrow_red_together() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "general").await;
    alice.click(".user-pill .mute-button").await.expect("mute");
    alice
        .wait_for(".user-pill .device-pair.active .mute-button", DEFAULT_WAIT)
        .await
        .expect("the pair marked");
    let look = alice
        .eval(
            "const pair = document.querySelector('.user-pill .device-pair.active'); \
             const probe = document.createElement('div'); probe.style.color = 'var(--danger)'; document.body.append(probe); \
             const danger = getComputedStyle(probe).color; probe.remove(); \
             const mic = getComputedStyle(pair.querySelector('.mute-button')).color; \
             const arrow = getComputedStyle(pair.querySelector('.device-menu-button')).color; \
             const tint = b => { const c = getComputedStyle(b).backgroundColor; return c !== 'rgba(0, 0, 0, 0)' && c !== danger; }; \
             const bg = tint(pair.querySelector('.mute-button')) && tint(pair.querySelector('.device-menu-button')); \
             const gap = pair.querySelector('.mute-button').getBoundingClientRect().right < pair.querySelector('.device-menu-button').getBoundingClientRect().left; \
             return { red: mic === danger && arrow === danger, bg, gap };",
        )
        .await
        .expect("look");
    assert_eq!(
        look,
        serde_json::json!({ "red": true, "bg": true, "gap": true }),
        "muted look"
    );
}

/// Clicking a voice channel lists you under it at once, greyed until the
/// call connects.
#[tokio::test]
async fn joining_lists_you_at_once_greyed_until_connected() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "general").await;
    let channel = fresh_voice_channel(&hs);
    alice
        .wait_for_text(".room .room-name", &channel, DEFAULT_WAIT)
        .await
        .expect("the channel listed");
    // Click, then look straight away (before the call can connect).
    let first = alice
        .eval(&format!(
            "const row = [...document.querySelectorAll('.room')].find(r => r.querySelector('.room-name')?.textContent.trim() === {name}); \
             const id = row.dataset.room; row.click(); \
             for (let i = 0; i < 60; i++) {{ \
               const me = [...document.querySelectorAll('.room-call .call-participant')].find(p => p.dataset.user === '@alice:localhost' && p.closest('.room-call').dataset.room === id); \
               if (me) return me.classList.contains('connecting') ? 'greyed' : 'listed'; \
               await new Promise(r => requestAnimationFrame(r)); \
             }} \
             return 'missing';",
            name = serde_json::to_string(&channel).expect("json")
        ))
        .await
        .expect("look");
    assert_eq!(first.as_str(), Some("greyed"), "on clicking");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("connected");
    alice
        .wait_for_js(
            "const me = [...document.querySelectorAll('.room-call .call-participant')].find(p => p.dataset.user === '@alice:localhost'); return !!me && !me.classList.contains('connecting');",
            DEFAULT_WAIT,
        )
        .await
        .expect("not greyed once connected");
    alice.click(".voice-panel .hang-up").await.expect("leave");
}

/// A server's voice channel, as Discord has it: a click joins its call and
/// leaves you where you were; clicking it again while in the call opens it,
/// all call (no timeline, no resize handle), with a chat button at the top
/// left for its text chat.
#[tokio::test]
async fn a_voice_channel_joins_in_place_and_opens_as_a_call_page() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "general").await;
    let channel = fresh_voice_channel(&hs);
    let row = format!(
        "[...document.querySelectorAll('.room')].find(r => r.querySelector('.room-name')?.textContent.trim() === {})",
        serde_json::to_string(&channel).expect("json")
    );
    alice
        .wait_for_js(&format!("return !!{row};"), DEFAULT_WAIT)
        .await
        .expect("listed");
    alice
        .eval(&format!("{row}.click(); return true;"))
        .await
        .expect("click");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("joined");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    alice
        .wait_for_text(".chat-header h2", "general", DEFAULT_WAIT)
        .await
        .expect("still in #general");
    assert_eq!(
        alice.count(".call-stage").await.expect("count"),
        0,
        "no call view over #general"
    );

    alice
        .eval(&format!("{row}.click(); return true;"))
        .await
        .expect("click again");
    alice
        .wait_for(".call-stage.voice-page", DEFAULT_WAIT)
        .await
        .expect("all call");
    assert_eq!(
        alice
            .count(".call-stage-resize, .chat > .timeline, .chat .composer")
            .await
            .expect("count"),
        0,
        "no resize handle, timeline or message box"
    );
    assert_eq!(
        alice.count(".chat-header").await.expect("count"),
        0,
        "no top bar over the call"
    );
    let corner = alice
        .eval("const b = document.querySelector('.voice-chat-toggle'), p = document.querySelector('.voice-channel'); if (!b || !p) return false; const r = b.getBoundingClientRect(), q = p.getBoundingClientRect(); return r.right > q.right - 64 && r.top < q.top + 64;")
        .await
        .expect("toggle");
    assert_eq!(
        corner,
        serde_json::json!(true),
        "the chat button in the top right corner"
    );
    // A voice channel has no member list: the call shows who is there.
    assert_eq!(
        alice
            .count(".members, .member-toggle")
            .await
            .expect("count"),
        0,
        "no member list or its button"
    );
    alice
        .key_combo("body", "u", true, false, false)
        .await
        .expect("Ctrl+U");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(
        alice.count(".members").await.expect("count"),
        0,
        "Ctrl+U opens no member list here"
    );
    alice
        .click(".voice-chat-toggle")
        .await
        .expect("open the chat");
    alice
        .wait_for(".voice-chat .composer .composer-input", DEFAULT_WAIT)
        .await
        .expect("its text chat");

    // Scrolled up in the chat, Jump to present sits in the chat's middle,
    // above its message box (not placed against the whole window).
    let room_id = alice
        .eval(&format!("return {row}.dataset.room;"))
        .await
        .expect("room id");
    let path = room_id
        .as_str()
        .expect("id")
        .replace('!', "%21")
        .replace(':', "%3A");
    for n in 0..80 {
        api_as(
            &hs,
            "bob",
            "PUT",
            &format!(
                "rooms/{path}/send/m.room.message/fill-{}-{n}",
                std::process::id()
            ),
            Some(serde_json::json!({ "msgtype": "m.text", "body": format!("filler {n}") })),
        )
        .expect("send");
    }
    alice
        .wait_for_text(".voice-chat .message .body", "filler 79", DEFAULT_WAIT)
        .await
        .expect("the messages");
    alice
        .wait_for_js("const t = document.querySelector('.voice-chat .timeline'); t.scrollTop = -2000; return !!document.querySelector('.voice-chat .jump-to-bottom');", DEFAULT_WAIT)
        .await
        .expect("Jump to present");
    let placed = alice
        .eval("const b = document.querySelector('.voice-chat .jump-to-bottom').getBoundingClientRect(), c = document.querySelector('.voice-chat').getBoundingClientRect(), m = document.querySelector('.voice-chat .composer').getBoundingClientRect(); return [Math.abs((b.left + b.right) / 2 - (c.left + c.right) / 2) < 4, b.left >= c.left && b.right <= c.right, b.bottom <= m.top];")
        .await
        .expect("placed");
    assert_eq!(
        placed,
        serde_json::json!([true, true, true]),
        "centred in the chat, inside it, above the message box"
    );
    alice.click(".voice-panel .hang-up").await.expect("leave");
}

/// Desktop DM calls ring both ways: the caller hears a ringback while it
/// rings (stopping once the other joins), and the one called gets a
/// notification that rings (the window may be hidden, its own ringtone
/// silent then).
#[tokio::test]
async fn a_dm_call_rings_back_for_the_caller_and_the_notification_rings() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut bob = App::launch_desktop(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "general").await;
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "general").await;
    open_dm(&mut alice, "bob").await;
    alice.click(CALL_BUTTON).await.expect("call bob");
    let ringbacks = "return (window.morpheusSounds ?? []).filter(s => s === 'ringback').length;";
    alice
        .wait_for_js(
            &format!("return (() => {{ {ringbacks} }})() > 0;"),
            DEFAULT_WAIT,
        )
        .await
        .expect("alice hears it ring");

    // Bob's notification rings.
    let shown = bob.data_dir().join("shown-notifications.jsonl");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let text = std::fs::read_to_string(&shown).unwrap_or_default();
        if let Some(line) = text.lines().find(|l| l.contains("is calling")) {
            let notice: serde_json::Value = serde_json::from_str(line).expect("json");
            assert_eq!(notice["sound"], "phone-incoming-call", "{notice}");
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no ring notification: {text}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }

    // Bob answers: the ringback stops.
    open_dm(&mut bob, "alice").await;
    bob.click(CALL_BUTTON).await.expect("answer");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("bob in");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let then = alice.eval(ringbacks).await.expect("count");
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    assert_eq!(
        alice.eval(ringbacks).await.expect("count"),
        then,
        "still ringing back"
    );
    alice.click(".voice-panel .hang-up").await.expect("hang up");
    out_of_call(&mut alice).await;
    bob.click(".voice-panel .hang-up").await.expect("hang up");
    out_of_call(&mut bob).await;
    // Both gone from the DM's call before the next test calls in it.
    alice
        .wait_for_js(
            "return !document.querySelector('.room-call .call-participant');",
            DEFAULT_WAIT,
        )
        .await
        .expect("the DM's call empty");
}

/// Ctrl+Enter answers a call ringing, as in Discord.
#[tokio::test]
async fn ctrl_enter_answers_a_ringing_call() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "general").await;
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "general").await;
    open_dm(&mut alice, "bob").await;
    alice.click(CALL_BUTTON).await.expect("alice calls");
    bob.wait_for_text(".incoming-call", "alice", DEFAULT_WAIT)
        .await
        .expect("bob is rung");
    bob.key_combo("body", "Enter", true, false, false)
        .await
        .expect("ctrl+enter");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("answered");
    bob.click(".voice-panel .hang-up").await.expect("hang up");
    out_of_call(&mut bob).await;
    alice.click(".voice-panel .hang-up").await.expect("hang up");
    out_of_call(&mut alice).await;
}

/// Discord's call shortcuts: Ctrl+Shift+Alt+V goes to the call you are in;
/// Ctrl+' calls the DM open; Esc declines a call ringing.
#[tokio::test]
async fn go_to_the_call_start_a_dm_call_and_decline_by_keyboard() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "general").await;
    let channel = fresh_voice_channel(&hs);
    let row = format!(
        "[...document.querySelectorAll('.room')].find(r => r.querySelector('.room-name')?.textContent.trim() === {})",
        serde_json::to_string(&channel).expect("json")
    );
    alice
        .wait_for_js(&format!("return !!{row};"), DEFAULT_WAIT)
        .await
        .expect("listed");
    alice
        .eval(&format!("{row}.click(); return true;"))
        .await
        .expect("join");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("joined");
    alice
        .wait_for_text(".chat-header h2", "general", DEFAULT_WAIT)
        .await
        .expect("still in #general");
    alice
        .eval("document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'V', code: 'KeyV', ctrlKey: true, shiftKey: true, altKey: true, bubbles: true })); return true;")
        .await
        .expect("ctrl+shift+alt+v");
    alice
        .wait_for(".call-stage.voice-page", DEFAULT_WAIT)
        .await
        .expect("its call page");
    alice.click(".voice-panel .hang-up").await.expect("hang up");
    out_of_call(&mut alice).await;

    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "general").await;
    open_dm(&mut alice, "bob").await;
    alice
        .eval("document.body.dispatchEvent(new KeyboardEvent('keydown', { key: \"'\", code: 'Quote', ctrlKey: true, bubbles: true })); return true;")
        .await
        .expect("ctrl+'");
    bob.wait_for_text(".incoming-call", "alice", DEFAULT_WAIT)
        .await
        .expect("bob is rung");
    bob.press("body", "Escape").await.expect("escape");
    bob.wait_for_js(
        "return !document.querySelector('.incoming-call');",
        DEFAULT_WAIT,
    )
    .await
    .expect("declined");
    assert_eq!(
        bob.count(".voice-panel").await.expect("count"),
        0,
        "bob did not join"
    );
    alice.click(".voice-panel .hang-up").await.expect("hang up");
    out_of_call(&mut alice).await;
}

/// The card focused in the call's view, the cards in the strip beside it,
/// and whether the focused one is much bigger: `[main, strip, bigger]`.
const FOCUS_STATE: &str = "const c = document.querySelector('.call-cards.focused, .call-tiles.focused'); \
     if (!c) return null; \
     const main = c.querySelector('.call-focus-main > *'), strip = [...c.querySelectorAll('.call-strip > *')]; \
     const w = el => el.getBoundingClientRect().width; \
     return [main ? (main.classList.contains('screen') ? 'screen:' : '') + main.dataset.user : null, \
       strip.map(s => s.dataset.user), \
       !!main && strip.every(s => w(main) > 2 * w(s))];";

/// Clicking a card in a call focuses it, as in Discord: large, with the
/// others in a strip. Clicking it again, or Escape, goes back to the grid.
/// A shared screen focuses the same way.
#[tokio::test]
async fn clicking_a_card_in_a_call_focuses_it() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    if alice.is_phone() {
        eprintln!("phones focus with Enlarge (the next test); skipping");
        return;
    }
    join_hangout(&mut alice, "alice").await;
    let mut bob = App::launch(&hs).await.expect("launch bob");
    join_hangout(&mut bob, "bob").await;
    alice
        .wait_for(".call-stage [data-user=\"@bob:localhost\"]", DEFAULT_WAIT)
        .await
        .expect("bob's card");

    alice
        .click(".call-stage [data-user=\"@bob:localhost\"]")
        .await
        .expect("click bob");
    alice
        .wait_for(".call-stage .call-cards.focused", DEFAULT_WAIT)
        .await
        .expect("focused");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(
        alice.eval(FOCUS_STATE).await.expect("state"),
        serde_json::json!(["@bob:localhost", ["@alice:localhost"], true]),
        "bob large, alice in the strip"
    );
    alice
        .click(".call-focus-main [data-user=\"@bob:localhost\"]")
        .await
        .expect("click bob again");
    alice
        .wait_for_js(
            "return !document.querySelector('.call-cards.focused');",
            DEFAULT_WAIT,
        )
        .await
        .expect("back to the grid");

    // A shared screen.
    alice
        .click(".voice-panel .screen-button")
        .await
        .expect("share");
    bob.wait_for(".call-stage .video-tile.screen", DEFAULT_WAIT)
        .await
        .expect("bob sees the screen");
    bob.click(".call-stage .video-tile.screen")
        .await
        .expect("click the screen");
    bob.wait_for_js(
        &format!("return (() => {{ {FOCUS_STATE} }})()?.[0] === 'screen:@alice:localhost';"),
        DEFAULT_WAIT,
    )
    .await
    .expect("the screen focused");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(
        bob.eval(FOCUS_STATE).await.expect("state")[2],
        serde_json::json!(true),
        "the screen much bigger than the cards in the strip"
    );
    bob.press("body", "Escape").await.expect("escape");
    bob.wait_for_js(
        "return !document.querySelector('.call-cards.focused');",
        DEFAULT_WAIT,
    )
    .await
    .expect("Escape: back to the grid");

    alice
        .click(".voice-panel .screen-button")
        .await
        .expect("stop sharing");
    bob.click(".voice-panel .hang-up")
        .await
        .expect("bob leaves");
    alice
        .click(".voice-panel .hang-up")
        .await
        .expect("alice leaves");
}

/// On a phone a tap on a card shows its buttons: Enlarge, at its top right,
/// focuses it (and then shrinks it back).
#[tokio::test]
async fn on_a_phone_a_card_is_enlarged_from_its_button() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut bob = App::launch(&hs).await.expect("launch bob");
    if bob.is_phone() {
        eprintln!("needs a desktop beside the phone; skipping");
        return;
    }
    join_hangout(&mut bob, "bob").await;
    let mut alice = App::launch_sized(&hs, 400, 800)
        .await
        .expect("launch a phone-sized alice");
    login_and_open(&mut alice, "alice", "hangout").await;
    alice.click(CALL_BUTTON).await.expect("join");
    alice
        .wait_for(
            ".call-screen .call-tiles [data-user=\"@bob:localhost\"]",
            DEFAULT_WAIT,
        )
        .await
        .expect("bob's card on the call screen");
    let card = ".call-screen .call-tiles [data-user=\"@bob:localhost\"]";
    assert_eq!(
        alice
            .eval(&format!("const e = document.querySelector('{card} .card-enlarge'); return !!e && getComputedStyle(e).display !== 'none' && getComputedStyle(e).visibility !== 'hidden' && getComputedStyle(e).opacity !== '0';"))
            .await
            .expect("hidden"),
        serde_json::json!(false),
        "no Enlarge before a tap"
    );
    alice.click(card).await.expect("tap bob's card");
    alice
        .wait_for_js(
            &format!(
            "const c = document.querySelector('{card}'), e = c.querySelector('.card-enlarge'); if (!e) return null; \
             const s = getComputedStyle(e), r = e.getBoundingClientRect(), q = c.getBoundingClientRect(); \
             return s.display !== 'none' && s.visibility !== 'hidden' && s.opacity !== '0' && r.right > q.right - 56 && r.top < q.top + 56;"
            ),
            DEFAULT_WAIT,
        )
        .await
        .expect("Enlarge at the card's top right");
    alice
        .click(&format!("{card} .card-enlarge"))
        .await
        .expect("enlarge");
    alice
        .wait_for_js(
            &format!("return (() => {{ {FOCUS_STATE} }})()?.[0] === '@bob:localhost';"),
            DEFAULT_WAIT,
        )
        .await
        .expect("bob focused");
    alice
        .click(".call-focus-main [data-user=\"@bob:localhost\"] .card-enlarge")
        .await
        .expect("shrink");
    alice
        .wait_for_js(
            "return !document.querySelector('.call-tiles.focused');",
            DEFAULT_WAIT,
        )
        .await
        .expect("back to the grid");
    alice
        .click(".call-screen .hang-up")
        .await
        .expect("alice leaves");
    bob.click(".voice-panel .hang-up")
        .await
        .expect("bob leaves");
}

/// Closed to the tray, a desktop still rings for a DM call, again and again
/// until the call stops: the ringtone plays outside the window (a hidden
/// window's page is suspended, and notification daemons often ignore a
/// notification's sound). Test runs write the plays to `played-sounds.log`.
#[tokio::test]
async fn a_desktop_in_the_tray_still_rings() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut bob = App::launch_desktop(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "general").await;
    // To the tray: the window hides, the app runs on.
    bob.eval("setTimeout(() => document.querySelector('.titlebar button.close').click(), 50); return true;")
        .await
        .expect("close");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    assert!(bob.is_running(), "closing the window keeps the app");
    let played = bob.data_dir().join("played-sounds.log");
    let rings = || {
        std::fs::read_to_string(&played)
            .unwrap_or_default()
            .lines()
            .filter(|l| *l == "ring")
            .count()
    };

    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "general").await;
    open_dm(&mut alice, "bob").await;
    alice.click(CALL_BUTTON).await.expect("call bob");
    alice
        .wait_for(".voice-panel .hang-up", DEFAULT_WAIT)
        .await
        .expect("alice's call is up");
    // Rings of an earlier call (still within its half minute) don't count.
    let before = rings();
    // It rings, and keeps ringing.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while rings() < before + 3 && std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    assert!(
        rings() >= before + 3,
        "bob's tray app rang {} times",
        rings() - before
    );

    // Alice gives up: the ringing stops.
    alice.click(".voice-panel .hang-up").await.expect("hang up");
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    let after = rings();
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    assert_eq!(rings(), after, "it stops ringing when the call stops");
}

/// Playback streams PipeWire's ALSA layer opened for `program`.
fn alsa_streams_of(program: &str) -> Vec<String> {
    let Ok(out) = std::process::Command::new("pw-dump").output() else {
        return Vec::new();
    };
    let nodes: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    let name = format!("alsa_playback.{program}");
    nodes
        .as_array()
        .into_iter()
        .flatten()
        .filter(|n| n["info"]["props"]["media.class"] == "Stream/Output/Audio")
        .filter(|n| n["info"]["props"]["node.name"] == name.as_str())
        .map(|n| {
            format!(
                "{} ({})",
                n["info"]["props"]["node.name"], n["info"]["state"]
            )
        })
        .collect()
}
