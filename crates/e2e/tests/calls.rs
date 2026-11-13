//! Calls, Discord-style: the phone button in a room's header joins its call;
//! everyone sees who is in it under the room in the sidebar (ringed while
//! they speak); the voice panel at the bottom of the sidebar mutes, deafens
//! and hangs up. Test runs send a tone instead of the microphone.

#![allow(clippy::expect_used)]

mod common;

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

/// Wait until `name` is listed in `room`'s call in the sidebar.
async fn wait_in_call(app: &mut App, room: &str, name: &str) -> anyhow::Result<()> {
    app.wait_for_text(
        &format!(".room-call[data-room=\"{room}\"] .call-participant"),
        name,
        DEFAULT_WAIT,
    )
    .await
}

/// Wait until `name` is no longer listed in `room`'s call.
async fn wait_not_in_call(app: &mut App, room: &str, name: &str) -> anyhow::Result<()> {
    app.wait_for_text_gone(
        &format!(".room-call[data-room=\"{room}\"] .call-participant"),
        name,
        DEFAULT_WAIT,
    )
    .await
}

/// Wait until the chimes played so far are exactly `expected`, in order.
async fn wait_chimes(app: &mut App, expected: &[&str]) -> anyhow::Result<()> {
    let expected = expected.join(",");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let heard = if app.is_gpui() {
            app.texts(".sound-log li").await?.join(",")
        } else {
            app.eval("return (window.morpheusSounds ?? []).join(',');")
                .await?
                .as_str()
                .unwrap_or_default()
                .to_owned()
        };
        if heard == expected {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            anyhow::bail!("chimes {heard:?}, wanted {expected:?}");
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// Wait until whose audio the voice panel hears (decrypted) includes `who`.
async fn wait_hears(app: &mut App, who: &str) -> anyhow::Result<()> {
    let user = format!("@{who}:localhost");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let heard = app
            .attr(".voice-panel", "data-heard")
            .await?
            .unwrap_or_default();
        if heard.split(' ').any(|u| u == user) {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            anyhow::bail!("{who} not heard; the panel hears {heard:?}");
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
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
        .attr(".room.selected", "data-room")
        .await
        .expect("room id")
        .expect("a selected room");

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
    wait_in_call(&mut alice, &hangout_id, "alice")
        .await
        .expect("alice listed under the room");
    wait_chimes(&mut alice, &["join"])
        .await
        .expect("a chime as she joins");

    // Bob sees her there before joining, and joins.
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "general").await;
    open_without_joining(&mut bob, &channel).await;
    wait_in_call(&mut bob, &hangout_id, "alice")
        .await
        .expect("bob sees alice in the call");
    bob.click(CALL_BUTTON).await.expect("join");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("bob connected");
    wait_in_call(&mut alice, &hangout_id, "bob")
        .await
        .expect("alice sees bob join");
    wait_chimes(&mut alice, &["join", "join"])
        .await
        .expect("a chime as bob joins");
    wait_chimes(&mut bob, &["join"])
        .await
        .expect("bob's own join chime");

    // Both send a tone, so both are speaking.
    bob.wait_for_text(".call-participant.speaking", "alice", DEFAULT_WAIT)
        .await
        .expect("alice ringed as speaking");
    alice
        .wait_for_text(".call-participant.speaking", "bob", DEFAULT_WAIT)
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
    assert!(
        !alice
            .texts(".timeline")
            .await
            .expect("timeline text")
            .iter()
            .any(|t| t.contains("call.member")),
        "call membership rows in the timeline"
    );

    // Muted, she stops.
    alice.click(".user-bar .mute-button").await.expect("mute");
    alice
        .wait_for(".user-bar .mute-button.active", DEFAULT_WAIT)
        .await
        .expect("mute shows");
    wait_chimes(&mut alice, &["join", "join", "mute"])
        .await
        .expect("a mute chime");
    bob.wait_for_text_gone(".call-participant.speaking", "alice", DEFAULT_WAIT)
        .await
        .expect("alice no longer speaking");

    // Hanging up takes her out of the list for everyone.
    alice.click(".voice-panel .hang-up").await.expect("hang up");
    alice
        .wait_for_count(".voice-panel", 0, DEFAULT_WAIT)
        .await
        .expect("panel gone");
    wait_not_in_call(&mut bob, &hangout_id, "alice")
        .await
        .expect("alice gone from the call");
    wait_chimes(&mut alice, &["join", "join", "mute", "leave"])
        .await
        .expect("a chime as she hangs up");
    wait_chimes(&mut bob, &["join", "leave"])
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

    wait_hears(&mut bob, "alice")
        .await
        .expect("bob hears alice (decrypted)");
    wait_hears(&mut alice, "bob")
        .await
        .expect("alice hears bob (decrypted)");

    let vault_id = bob
        .attr(".room.selected", "data-room")
        .await
        .expect("room id")
        .expect("a selected room");
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
    bob.wait_for_count(
        &format!(".room-call[data-room=\"{vault_id}\"]"),
        0,
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
    let text = app.colours_of(&option, "color").await.expect("colours");
    let list = app
        .colours_of(&format!(".device-choice[data-kind={picker}] .device-options"), "background")
        .await
        .expect("colours");
    assert!(
        !text.is_empty() && !list.is_empty() && text[0] != list[0],
        "option text the colour of the list: {text:?} on {list:?}"
    );
    app.click(&option).await.expect("pick");
}

/// Wait until the open menu lists each of `items` (by what its labels contain).
async fn wait_menu_has(app: &mut App, items: &[&str]) -> anyhow::Result<()> {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let labels = app.texts(".context-menu .context-item").await?;
        if items
            .iter()
            .all(|wanted| labels.iter().any(|label| label.contains(wanted)))
        {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            anyhow::bail!("the menu lists {labels:?}, not {items:?}");
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
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
        .attrs(
            ".voice-devices .device-choice:not([data-kind=camera]) .device-current",
            "data-id",
        )
        .await
        .expect("read pickers")
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(shown, "test-microphone-2,test-speaker-1");

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
    wait_sound(&mut bob, "ring").await.expect("a ringtone");
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
    if alice.is_gpui() {
        // The line leads with a green phone.
        alice
            .wait_for(".message.system .state-icon.call-started[data-icon=phone][data-tone=good]", DEFAULT_WAIT)
            .await
            .expect("the call's line has its icon");
    }

    // Bob answers: he's in the call, and it stops ringing.
    bob.click(".incoming-call .accept-call")
        .await
        .expect("accept");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("bob connected");
    bob.wait_for_count(".incoming-call", 0, DEFAULT_WAIT)
        .await
        .expect("ringing stops");
    alice
        .wait_for_text(".call-participant.speaking", "bob", DEFAULT_WAIT)
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
        .wait_for_count(".voice-panel", 0, DEFAULT_WAIT)
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
    bob.wait_for_count(".incoming-call", 0, DEFAULT_WAIT)
        .await
        .expect("ringing stops");
    alice
        .wait_for_text(".toast", "bob declined the call", DEFAULT_WAIT)
        .await
        .expect("alice told");
    alice
        .wait_for_count(".voice-panel", 0, DEFAULT_WAIT)
        .await
        .expect("alice's call ends");
    assert!(
        !bob.texts("body")
            .await
            .expect("text")
            .iter()
            .any(|t| t.contains("Unsupported")),
        "rings or declines shown as unsupported"
    );
}

/// Wait until `user` has a screen tile and a camera tile in the call view,
/// each showing a picture.
async fn wait_pictures(app: &mut App, user: &str) -> anyhow::Result<()> {
    if app.is_gpui() {
        let tile = format!(".call-stage .video-tile[data-user=\"{user}\"]");
        app.wait_for(&format!("{tile}.screen .video[data-shown=true]"), DEFAULT_WAIT)
            .await?;
        return app
            .wait_for(
                &format!("{tile}:not(.screen) .video[data-shown=true]"),
                DEFAULT_WAIT,
            )
            .await;
    }
    app.wait_for_js(
        &format!(
            "const tiles = [...document.querySelectorAll('.call-stage .video-tile')].filter(t => t.dataset.user === '{user}'); \
             const shown = t => {{ const i = t.querySelector('img.video'); return !!i && i.complete && i.naturalWidth > 0; }}; \
             return tiles.some(t => t.classList.contains('screen') && shown(t)) && tiles.some(t => !t.classList.contains('screen') && shown(t));"
        ),
        DEFAULT_WAIT,
    )
    .await
}

/// Wait until `user`'s video tile shows the test pattern: red on the left,
/// blue on the right (what a test run's camera sends).
async fn wait_test_pattern(app: &mut App, user: &str) -> anyhow::Result<()> {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let shown = if app.is_gpui() {
            // The tile's picture notes the colours at a quarter and
            // three quarters across, as hex.
            let card = format!(".video-tile:not(.screen)[data-user=\"{user}\"]");
            let tile = format!("{card} .video");
            let mut left = app.attr(&tile, "data-left").await?.unwrap_or_default();
            let mut right = app.attr(&tile, "data-right").await?.unwrap_or_default();
            // Your own camera is drawn mirrored: the pattern is then reversed.
            if app.attr(&card, "data-mirrored").await?.as_deref() == Some("true") {
                std::mem::swap(&mut left, &mut right);
            }
            let rgb = |hex: &str| {
                (hex.len() == 6)
                    .then(|| u32::from_str_radix(hex, 16).ok())
                    .flatten()
                    .map(|v| ((v >> 16) & 255, (v >> 8) & 255, v & 255))
            };
            matches!((rgb(&left), rgb(&right)), (Some(l), Some(r)) if l.0 > 150 && l.2 < 100 && r.2 > 150 && r.0 < 100)
        } else {
            app.eval(&shows_test_pattern(user))
                .await?
                .as_bool()
                .unwrap_or(false)
        };
        if shown {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            anyhow::bail!("{user}'s tile does not show the test pattern");
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
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
    wait_test_pattern(&mut alice, "@alice:localhost")
        .await
        .expect("alice's self view");
    wait_test_pattern(&mut bob, "@alice:localhost")
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
    wait_test_pattern(&mut alice, "@bob:localhost")
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
    let alice_tile = ".video-tile[data-user=\"@alice:localhost\"]";
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
    bob.wait_for_count(".call-stage .video-tile", 0, DEFAULT_WAIT)
        .await
        .expect("her video goes when the camera does");
    alice
        .wait_for_count(alice_tile, 0, DEFAULT_WAIT)
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

/// Starts sharing this device's screen from the Share screen button `button`:
/// the GPUI app asks for the quality first (Go Live starts it).
async fn start_share(app: &mut App, button: &str) {
    app.click(button).await.expect("share");
    if app.is_gpui() {
        app.wait_for(".screen-quality .screen-go", DEFAULT_WAIT)
            .await
            .expect("the quality popover");
        app.click(".screen-quality .screen-go")
            .await
            .expect("go live");
    }
}

/// Wait until `user`'s shared screen shows the test run's screen pattern:
/// green on the left, yellow on the right.
async fn wait_screen_pattern(app: &mut App, user: &str) -> anyhow::Result<()> {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    let mut seen = String::new();
    loop {
        let shown = if app.is_gpui() {
            let tile = format!(".call-stage .video-tile.screen[data-user=\"{user}\"] .video");
            let left = app.attr(&tile, "data-left").await?.unwrap_or_default();
            let right = app.attr(&tile, "data-right").await?.unwrap_or_default();
            let tiles = app.count(&tile).await?;
            let shown = app.attr(&tile, "data-shown").await?.unwrap_or_default();
            let seq = app.attr(&tile, "data-seq").await?.unwrap_or_default();
            seen = format!("tiles {tiles}, shown {shown:?}, seq {seq:?}, left {left:?}, right {right:?}");
            let rgb = |hex: &str| {
                (hex.len() == 6)
                    .then(|| u32::from_str_radix(hex, 16).ok())
                    .flatten()
                    .map(|v| ((v >> 16) & 255, (v >> 8) & 255, v & 255))
            };
            matches!((rgb(&left), rgb(&right)), (Some(l), Some(r)) if l.1 > 150 && l.0 < 100 && r.0 > 150 && r.1 > 150 && r.2 < 100)
        } else {
            app.eval(&shows_screen_pattern(user))
                .await?
                .as_bool()
                .unwrap_or(false)
        };
        if shown {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            anyhow::bail!("{user}'s screen does not show the screen pattern ({seen})");
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
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

    start_share(&mut alice, ".voice-panel .screen-button").await;
    alice
        .wait_for(".voice-panel .screen-button.active", DEFAULT_WAIT)
        .await
        .expect("the button shows it shared");
    wait_screen_pattern(&mut bob, "@alice:localhost")
        .await
        .expect("bob sees alice's screen");
    bob.wait_for_text(
        ".video-tile.screen .video-name",
        "alice's screen",
        DEFAULT_WAIT,
    )
    .await
    .expect("named as her screen");
    wait_screen_pattern(&mut alice, "@alice:localhost")
        .await
        .expect("alice sees what she shares");
    // Her camera too: the screen stays first.
    alice
        .click(".voice-panel .camera-button")
        .await
        .expect("camera on");
    wait_test_pattern(&mut bob, "@alice:localhost")
        .await
        .expect("and her camera");
    let order: Vec<bool> = bob
        .texts(".call-stage .video-tile .video-name")
        .await
        .expect("order")
        .iter()
        .map(|name| name.contains("'s screen"))
        .collect();
    assert_eq!(order, [true, false], "screen first");
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
    bob.wait_for_count(".video-tile.screen", 0, DEFAULT_WAIT)
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

/// The sounds `app` has played so far, in order (test runs only note them).
async fn sounds_played(app: &mut App) -> Vec<String> {
    if app.is_gpui() {
        return app.texts(".sound-log li").await.expect("sounds");
    }
    serde_json::from_value(
        app.eval("return window.morpheusSounds ?? [];")
            .await
            .expect("sounds"),
    )
    .unwrap_or_default()
}

/// How many times `sound` has played in `app`.
async fn count_sound(app: &mut App, sound: &str) -> usize {
    sounds_played(app)
        .await
        .iter()
        .filter(|s| *s == sound)
        .count()
}

/// Wait until `sound` has played in `app`.
async fn wait_sound(app: &mut App, sound: &str) -> anyhow::Result<()> {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while count_sound(app, sound).await == 0 {
        if std::time::Instant::now() >= deadline {
            anyhow::bail!("{sound} never played; played {:?}", sounds_played(app).await);
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Ok(())
}

/// How many times the ringtone has played in `app`.
async fn rings(app: &mut App) -> u64 {
    count_sound(app, "ring").await as u64
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
    bob.wait_for_count(".voice-panel", 0, DEFAULT_WAIT)
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
    if alice.count(hang_up).await.expect("count") == 0 {
        eprintln!("alice is out of the call; her log:\n{}", alice.log_tail(60));
    }
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

    bob.wait_for_text(".call-participant.speaking", "alice", DEFAULT_WAIT)
        .await
        .expect("bob sees alice speak");
    alice
        .wait_for_text(".call-participant.speaking", "bob", DEFAULT_WAIT)
        .await
        .expect("alice sees bob speak");
    wait_hears(&mut bob, "alice")
        .await
        .expect("bob hears alice (decrypted)");
    wait_hears(&mut alice, "bob")
        .await
        .expect("alice hears bob (decrypted)");
    // And a camera turned on reaches the other side, decrypted.
    bob.click(".voice-panel .camera-button")
        .await
        .expect("bob's camera on");
    wait_test_pattern(&mut alice, "@bob:localhost")
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
    app.bounds(".call-stage")
        .await
        .expect("height")
        .expect("a stage")
        .h
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
        .wait_for(".call-stage", DEFAULT_WAIT)
        .await
        .expect("the stage");
    alice
        .wait_for(".call-stage-resize", DEFAULT_WAIT)
        .await
        .expect("an edge to drag");
    let before = stage_height(&mut alice).await;
    // Press on the edge, move the mouse 200 px down, let go.
    alice
        .drag(".call-stage-resize", 0, 200)
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
        .wait_for_count(".call-stage .video-tile", 0, DEFAULT_WAIT)
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

/// How the stage's cards sit. Fails (says why) unless every card is the same
/// size, wholly inside the stage, and a camera fills its card; else
/// `ok <width>x<height>` of a card.
async fn stage_cards(app: &mut App) -> String {
    let Some(stage) = app.bounds(".call-stage").await.expect("stage") else {
        return "no stage".into();
    };
    let boxes = app
        .all_bounds(".call-stage .video-tile, .call-stage .call-tile")
        .await
        .expect("cards");
    if boxes.len() < 2 {
        return format!("cards: {}", boxes.len());
    }
    let (w, h) = (boxes[0].w, boxes[0].h);
    if w < 80.0 || h < 45.0 {
        return format!("tiny cards {w}x{h}");
    }
    for (i, b) in boxes.iter().enumerate() {
        if (b.w - w).abs() > 1.5 || (b.h - h).abs() > 1.5 {
            return format!("card {i} is {}x{}, not {w}x{h}", b.w, b.h);
        }
        if b.x < stage.x - 1.0
            || b.right() > stage.right() + 1.0
            || b.y < stage.y - 1.0
            || b.bottom() > stage.bottom() + 1.0
        {
            return format!("card {i} {b:?} outside the stage {stage:?}");
        }
    }
    let videos = app
        .all_bounds(".call-stage .video-tile")
        .await
        .expect("video tiles");
    let pictures = app
        .all_bounds(".call-stage .video-tile .video")
        .await
        .expect("pictures");
    for (b, v) in videos.iter().zip(&pictures) {
        if (v.w - b.w).abs() > 1.5 || (v.h - b.h).abs() > 1.5 {
            return format!("a camera {}x{} in a card {}x{}", v.w, v.h, b.w, b.h);
        }
    }
    if videos.len() != pictures.len() {
        return format!("{} cameras with {} pictures", videos.len(), pictures.len());
    }
    format!("ok {w}x{h}")
}

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
    wait_test_pattern(&mut alice, "@bob:localhost")
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
    let at_first = serde_json::json!(stage_cards(&mut alice).await);
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
    drag_stage(&mut alice, 200).await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let resized = serde_json::json!(stage_cards(&mut alice).await);
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
    if !alice.is_gpui() {
        alice
            .eval("localStorage.removeItem('morpheus.callStageHeight'); return true;")
            .await
            .expect("forget the size");
    }

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
        .wait_for_count(".voice-panel, .call-screen", 0, DEFAULT_WAIT)
        .await;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
}

/// What the app's sounds did: "ring: played", or the reason one did not
/// play (a page may only play sound after a tap or click).
async fn sound_plays(app: &mut App) -> Vec<String> {
    if app.is_gpui() {
        return sounds_played(app).await;
    }
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
        streams.extend(alsa_streams_of(if bob.is_gpui() {
            "morpheus-desktop-e2e"
        } else {
            "morpheus-desktop"
        }));
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
        let mut streams = playing_streams(pid);
        // The GPUI app plays through PipeWire's ALSA layer (named after the
        // program, no process recorded), as the ringtone does.
        if app.is_gpui() {
            streams.extend(alsa_streams_of("morpheus-desktop-e2e"));
        }
        seen = streams
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
    alice
        .wait_for_text(".room .room-name", "lounge", DEFAULT_WAIT)
        .await
        .expect("the voice channel listed");
    assert!(
        alice
            .count(".room.voice .voice-icon")
            .await
            .expect("count")
            > 0,
        "listed with a speaker"
    );
    assert_eq!(
        alice.count(".room.voice .hash").await.expect("count"),
        0,
        "listed with a speaker, not a #"
    );
    let room_id = alice
        .attr_containing(".room.voice", "lounge", "data-room")
        .await
        .expect("room id")
        .expect("the lounge's id");

    alice
        .click_text(".room.voice", "lounge")
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
    wait_in_call(&mut alice, &room_id, "alice")
        .await
        .expect("listed under it");
    // Clicking it again does not leave.
    alice
        .click_text(".room.voice", "lounge")
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
    wait_in_call(&mut bob, &room_id, "alice")
        .await
        .expect("bob sees alice in the voice channel");
    bob.click_text(".room.voice", "lounge")
        .await
        .expect("bob clicks it");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("bob joined");
    wait_in_call(&mut alice, &room_id, "bob")
        .await
        .expect("alice sees bob join");

    for app in [&mut alice, &mut bob] {
        let _ = app.click(".voice-panel .hang-up").await;
        out_of_call(app).await;
    }
}

/// Joining is quick: from the click on a voice channel to "Voice connected"
/// (task 21). A first join after a quiet moment (what the app can prepare
/// by then is ready) and a second one, each timed to the poll; figures go to
/// target/join-latency.txt.
async fn join_figures(hs: &str, label: &str, bound_ms: u128) {
    let mut alice = App::launch(hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "general").await;
    alice
        .wait_for_text(".room .room-name", "lounge", DEFAULT_WAIT)
        .await
        .expect("listed");
    // A user takes a moment before clicking.
    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
    let mut figures = Vec::new();
    for round in 0..3 {
        let started = std::time::Instant::now();
        alice.click_text(".room.voice", "lounge").await.expect("join");
        let joining = loop {
            let panel = alice.texts(".voice-panel").await.unwrap_or_default();
            if panel.iter().any(|t| t.contains("Connecting")) {
                break started.elapsed();
            }
            assert!(started.elapsed().as_secs() < 30, "never joining");
        };
        let connected = loop {
            let panel = alice.texts(".voice-panel").await.unwrap_or_default();
            if panel.iter().any(|t| t.contains("Voice connected")) {
                break started.elapsed();
            }
            assert!(started.elapsed().as_secs() < 30, "never connected");
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        };
        figures.push((round, joining, connected));
        alice.click(".voice-panel .hang-up").await.expect("hang up");
        alice
            .wait_for_count(".voice-panel", 0, DEFAULT_WAIT)
            .await
            .expect("out");
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    let text: String = figures
        .iter()
        .map(|(round, joining, connected)| {
            format!(
                "{label} round {round}: joining shown {} ms, connected {} ms\n",
                joining.as_millis(),
                connected.as_millis()
            )
        })
        .collect();
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/join-latency.txt"))
        .and_then(|mut f| std::io::Write::write_all(&mut f, text.as_bytes()));
    eprintln!("{text}");
    for (round, joining, connected) in figures {
        assert!(
            joining.as_millis() < 300,
            "round {round}: the joining state took {} ms\n{text}",
            joining.as_millis()
        );
        assert!(
            connected.as_millis() < bound_ms,
            "{label} round {round}: click to connected took {} ms (bound {bound_ms})\n{text}",
            connected.as_millis()
        );
    }
}

/// Hanging up takes you off the voice channel's list at once, not after the
/// server has answered (task 21).
#[tokio::test]
async fn hanging_up_leaves_the_voice_channels_list_at_once() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "general").await;
    alice
        .wait_for_text(".room .room-name", "lounge", DEFAULT_WAIT)
        .await
        .expect("listed");
    let room_id = alice
        .attr_containing(".room.voice", "lounge", "data-room")
        .await
        .expect("room id")
        .expect("the lounge's id");
    alice.click_text(".room.voice", "lounge").await.expect("join");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("in");
    wait_in_call(&mut alice, &room_id, "alice").await.expect("listed under it");
    // Let the membership settle on the server, so the removal below is the
    // only thing in flight.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    alice.click(".voice-panel .hang-up").await.expect("hang up");
    // No waiting: the very next look.
    let listed = alice
        .texts(&format!(".room-call[data-room=\"{room_id}\"] .call-participant"))
        .await
        .expect("texts");
    assert!(
        listed.iter().all(|n| !n.contains("alice")),
        "still listed right after hanging up: {listed:?}"
    );
    // And it stays gone once the server has answered.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let listed = alice
        .texts(&format!(".room-call[data-room=\"{room_id}\"] .call-participant"))
        .await
        .expect("texts");
    assert!(listed.is_empty(), "listed again: {listed:?}");
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
        .bounds(".user-pill")
        .await
        .expect("card")
        .expect("a profile card")
        .h;
    alice
        .wait_for_text(".room .room-name", "lounge", DEFAULT_WAIT)
        .await
        .expect("the voice channel listed");
    alice
        .click_text(".room.voice", "lounge")
        .await
        .expect("join the lounge");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("in the call");
    assert!(
        alice.count(".user-pill .voice-panel").await.expect("count") > 0,
        "call controls outside the profile card"
    );
    let height = alice
        .bounds(".user-pill")
        .await
        .expect("card")
        .expect("a profile card")
        .h;
    assert!(
        height > card_before + 20.0,
        "the profile card did not grow to hold them ({card_before}px before, {height}px now)"
    );
    let hang_ups = alice
        .colours_of(".hang-up", "background")
        .await
        .expect("colours");
    assert!(!hang_ups.is_empty(), "no hang-up shown");
    assert!(
        hang_ups.iter().all(|c| c == "#ff6b6b"),
        "a hang-up that is not red: {hang_ups:?}"
    );
    alice.click(".voice-panel .hang-up").await.expect("hang up");
    alice
        .wait_for_count(".voice-panel", 0, DEFAULT_WAIT)
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
        .wait_for_count(&format!("{card} .voice-state"), 0, DEFAULT_WAIT)
        .await
        .expect("no mark when talking");
    alice
        .click(".call-stage .call-controls .hang-up")
        .await
        .expect("leave");
    alice
        .wait_for_count(".call-stage", 0, DEFAULT_WAIT)
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

    let listed = ".room-call .call-participant[data-user=\"@bob:localhost\"]";
    let card = ".call-stage [data-user=\"@bob:localhost\"]";

    bob.click(".call-stage .call-controls .mute-button")
        .await
        .expect("bob mutes");
    alice
        .wait_for(&format!("{card} .voice-state.muted"), DEFAULT_WAIT)
        .await
        .expect("alice sees bob muted on his card");
    for app in [&mut alice, &mut carol] {
        app.wait_for(&format!("{listed} .voice-state.muted"), DEFAULT_WAIT)
            .await
            .expect("bob listed as muted");
    }

    bob.click(".call-stage .call-controls .deafen-button")
        .await
        .expect("bob deafens");
    carol
        .wait_for(&format!("{listed} .voice-state.deafened"), DEFAULT_WAIT)
        .await
        .expect("bob listed as deafened");

    bob.click(".call-stage .call-controls .camera-button")
        .await
        .expect("camera on");
    start_share(&mut bob, ".call-stage .call-controls .screen-button").await;
    for app in [&mut alice, &mut carol] {
        app.wait_for(&format!("{listed} .media-camera"), DEFAULT_WAIT)
            .await
            .expect("bob listed with his camera");
        app.wait_for(&format!("{listed} .media-live"), DEFAULT_WAIT)
            .await
            .expect("bob listed as LIVE");
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
    start_share(&mut bob, ".voice-panel .screen-button").await;
    bob.wait_for(".voice-panel .screen-button.active", DEFAULT_WAIT)
        .await
        .expect("sharing");

    let mut alice = App::launch(&hs).await.expect("launch alice");
    join_hangout(&mut alice, "alice").await;
    wait_pictures(&mut alice, "@bob:localhost")
        .await
        .expect("bob's camera and screen on joining");
    for round in 1..=2 {
        alice.click(".voice-panel .hang-up").await.expect("leave");
        alice
            .wait_for_count(".voice-panel", 0, DEFAULT_WAIT)
            .await
            .expect("left");
        alice.click(CALL_BUTTON).await.expect("rejoin");
        alice
            .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
            .await
            .expect("back in");
        wait_pictures(&mut alice, "@bob:localhost")
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
    // The channels earlier runs left behind pile up in the accounts (hundreds
    // of rooms); leave them before this run adds its own.
    static CLEANED: std::sync::Once = std::sync::Once::new();
    CLEANED.call_once(|| {
        for user in ["alice", "bob"] {
            morpheus_e2e::remove_rooms_named(hs, user, "voice ");
        }
    });
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
    app.drag(".call-stage-resize", 0, dy)
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
    let lists = alice
        .count(".voice-devices .device-choice")
        .await
        .expect("lists");
    let icons = alice
        .count(".voice-devices .device-choice .device-label svg.icon")
        .await
        .expect("icons");
    assert!(lists > 0 && icons == lists, "device lists without an icon: {lists} lists, {icons} icons");
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
    alice
        .click(".user-pill .device-menu-button[title=Microphone]")
        .await
        .expect("the arrow beside the mic");
    wait_menu_has(&mut alice, &["Test microphone 1", "Test microphone 2"])
        .await
        .expect("microphones listed");
    alice
        .click_text(".context-menu .context-item", "Test microphone 2")
        .await
        .expect("pick");
    alice
        .wait_for_count(".context-menu", 0, DEFAULT_WAIT)
        .await
        .expect("menu closes");
    // Right-click works as the arrow does, and shows the pick.
    alice
        .right_click(".user-pill .mute-button")
        .await
        .expect("right-click the mic");
    alice
        .wait_for_text(
            ".context-menu .context-item.checked",
            "Test microphone 2",
            DEFAULT_WAIT,
        )
        .await
        .expect("the chosen microphone ticked");
    alice.press("body", "Escape").await.expect("close");

    alice
        .right_click(".user-pill .deafen-button")
        .await
        .expect("right-click the headphones");
    wait_menu_has(&mut alice, &["Test speaker 1", "Test speaker 2"])
        .await
        .expect("speakers listed");
    alice.press("body", "Escape").await.expect("close");
    assert_eq!(
        alice
            .count(".user-pill .device-pair .deafen-button")
            .await
            .expect("count"),
        1,
        "the headphones"
    );
    assert_eq!(
        alice
            .count(".user-pill .device-pair .device-menu-button")
            .await
            .expect("count"),
        2,
        "an arrow beside the mic and one beside the headphones"
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
    wait_menu_has(&mut alice, &["Test camera 1", "Test camera 2"])
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
        .wait_for_count(".device-options", 0, DEFAULT_WAIT)
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
    let mic = ".user-pill .device-pair.active .mute-button";
    let arrow = ".user-pill .device-pair.active .device-menu-button";
    for part in [mic, arrow] {
        assert_eq!(
            alice.colours_of(part, "color").await.expect("colour"),
            ["#ff6b6b"],
            "{part} is not red"
        );
        let background = alice.colours_of(part, "background").await.expect("bg");
        assert!(
            matches!(background.as_slice(), [bg] if !bg.is_empty() && bg != "#ff6b6b"),
            "{part} has no tint: {background:?}"
        );
    }
    let mic = alice.bounds(mic).await.expect("mic").expect("a mic");
    let arrow = alice.bounds(arrow).await.expect("arrow").expect("an arrow");
    assert!(mic.right() < arrow.x, "no gap between the mic and its arrow");
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
    let id = alice
        .attr_containing(".room.voice", &channel, "data-room")
        .await
        .expect("room id")
        .expect("the channel's id");
    // Click, then look straight away (before the call can connect).
    alice
        .click_text(".room.voice", &channel)
        .await
        .expect("click");
    let me = format!(".room-call[data-room=\"{id}\"] .call-participant[data-user=\"@alice:localhost\"]");
    alice
        .wait_for(
            &format!("{me}.connecting"),
            std::time::Duration::from_secs(5),
        )
        .await
        .expect("listed at once, greyed");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("connected");
    alice
        .wait_for(&format!("{me}:not(.connecting)"), DEFAULT_WAIT)
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
    alice
        .wait_for_text(".room .room-name", &channel, DEFAULT_WAIT)
        .await
        .expect("listed");
    let room_id = alice
        .attr_containing(".room.voice", &channel, "data-room")
        .await
        .expect("room id")
        .expect("the channel's id");
    alice
        .click_text(".room.voice", &channel)
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
        .click_text(".room.voice", &channel)
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
    let toggle = alice
        .bounds(".voice-chat-toggle")
        .await
        .expect("toggle")
        .expect("a chat button");
    let page = alice
        .bounds(".voice-channel")
        .await
        .expect("page")
        .expect("the voice channel");
    assert!(
        toggle.right() > page.right() - 64.0 && toggle.y < page.y + 64.0,
        "the chat button in the top right corner: {toggle:?} in {page:?}"
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
    let path = room_id.replace('!', "%21").replace(':', "%3A");
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
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        alice
            .scroll_to(".voice-chat .timeline", morpheus_e2e::ScrollTo::FromBottom(2000.0))
            .await
            .expect("scroll up");
        if alice.count(".voice-chat .jump-to-bottom").await.expect("count") > 0 {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "no Jump to present");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let jump = alice
        .bounds(".voice-chat .jump-to-bottom")
        .await
        .expect("jump")
        .expect("a jump button");
    let chat = alice
        .bounds(".voice-chat")
        .await
        .expect("chat")
        .expect("the chat");
    let box_ = alice
        .bounds(".voice-chat .composer")
        .await
        .expect("composer")
        .expect("the message box");
    let placed = [
        ((jump.x + jump.right()) / 2.0 - (chat.x + chat.right()) / 2.0).abs() < 4.0,
        jump.x >= chat.x && jump.right() <= chat.right(),
        jump.bottom() <= box_.y,
    ];
    assert_eq!(
        placed,
        [true, true, true],
        "centred in the chat, inside it, above the message box: {jump:?} {chat:?} {box_:?}"
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
    wait_sound(&mut alice, "ringback")
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
    let then = count_sound(&mut alice, "ringback").await;
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    assert_eq!(
        count_sound(&mut alice, "ringback").await,
        then,
        "still ringing back"
    );
    alice.click(".voice-panel .hang-up").await.expect("hang up");
    out_of_call(&mut alice).await;
    bob.click(".voice-panel .hang-up").await.expect("hang up");
    out_of_call(&mut bob).await;
    // Both gone from the DM's call before the next test calls in it.
    alice
        .wait_for_count(".room-call .call-participant", 0, DEFAULT_WAIT)
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
    alice
        .wait_for_text(".room .room-name", &channel, DEFAULT_WAIT)
        .await
        .expect("listed");
    alice
        .click_text(".room.voice", &channel)
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
        .key_combo("body", "V", true, true, true)
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
        .key_combo("body", "'", true, false, false)
        .await
        .expect("ctrl+'");
    bob.wait_for_text(".incoming-call", "alice", DEFAULT_WAIT)
        .await
        .expect("bob is rung");
    bob.press("body", "Escape").await.expect("escape");
    bob.wait_for_count(".incoming-call", 0, DEFAULT_WAIT)
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
/// and whether the focused one is much bigger: `(main, strip, bigger)`, or
/// `None` with nothing focused. A shared screen's card is `screen:<user>`.
async fn focus_state(app: &mut App) -> Option<(Option<String>, Vec<String>, bool)> {
    if app
        .count(".call-cards.focused, .call-tiles.focused")
        .await
        .expect("count")
        == 0
    {
        return None;
    }
    let user = app
        .attr(".call-focus-main > *", "data-user")
        .await
        .expect("main");
    let screen = app.count(".call-focus-main > .screen").await.expect("count") > 0;
    let main = user.map(|u| if screen { format!("screen:{u}") } else { u });
    let strip: Vec<String> = app
        .attrs(".call-strip > *", "data-user")
        .await
        .expect("strip")
        .into_iter()
        .flatten()
        .collect();
    let main_width = app
        .bounds(".call-focus-main > *")
        .await
        .expect("bounds")
        .map(|b| b.w);
    let strip_widths = app.all_bounds(".call-strip > *").await.expect("bounds");
    let bigger = main_width.is_some_and(|w| strip_widths.iter().all(|s| w > 2.0 * s.w));
    Some((main, strip, bigger))
}

/// Wait until the focused card is `main` (see [`focus_state`]).
async fn wait_focused(app: &mut App, main: &str) -> anyhow::Result<()> {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        if focus_state(app).await.and_then(|state| state.0).as_deref() == Some(main) {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            anyhow::bail!("{main} never focused; {:?}", focus_state(app).await);
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

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
        focus_state(&mut alice).await,
        Some((
            Some("@bob:localhost".to_owned()),
            vec!["@alice:localhost".to_owned()],
            true
        )),
        "bob large, alice in the strip"
    );
    alice
        .click(".call-focus-main [data-user=\"@bob:localhost\"]")
        .await
        .expect("click bob again");
    alice
        .wait_for_count(".call-cards.focused", 0, DEFAULT_WAIT)
        .await
        .expect("back to the grid");

    // A shared screen.
    start_share(&mut alice, ".voice-panel .screen-button").await;
    bob.wait_for(".call-stage .video-tile.screen", DEFAULT_WAIT)
        .await
        .expect("bob sees the screen");
    bob.click(".call-stage .video-tile.screen")
        .await
        .expect("click the screen");
    wait_focused(&mut bob, "screen:@alice:localhost")
        .await
        .expect("the screen focused");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(
        focus_state(&mut bob).await.map(|state| state.2),
        Some(true),
        "the screen much bigger than the cards in the strip"
    );
    bob.press("body", "Escape").await.expect("escape");
    bob.wait_for_count(".call-cards.focused", 0, DEFAULT_WAIT)
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
    if bob.is_gpui() {
        eprintln!("the GPUI app has no phone layout; skipping");
        return;
    }
    join_hangout(&mut bob, "bob").await;
    let mut alice = App::launch_sized(&hs, 400, 800)
        .await
        .expect("launch a phone-sized alice");
    login_and_open(&mut alice, "alice", "hangout").await;
    // On a phone a channel's row joins its call (its bar has no call
    // button); then the call's bar in the chat opens its screen.
    alice.click(".chat-header .pane-back").await.expect("back");
    alice
        .click_text(".room.voice", "hangout")
        .await
        .expect("join");
    alice
        .wait_for(".room.voice.in-call", DEFAULT_WAIT)
        .await
        .expect("in the call");
    alice
        .click_text(".room.voice", "hangout")
        .await
        .expect("open the channel");
    alice
        .wait_for(".chat .call-bar", DEFAULT_WAIT)
        .await
        .expect("the call's bar");
    alice.click(".chat .call-bar").await.expect("open the call");
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
    wait_focused(&mut alice, "@bob:localhost")
        .await
        .expect("bob focused");
    alice
        .click(".call-focus-main [data-user=\"@bob:localhost\"] .card-enlarge")
        .await
        .expect("shrink");
    alice
        .wait_for_count(".call-tiles.focused", 0, DEFAULT_WAIT)
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

/// Closing the window to the tray does not end a call: the other side still
/// sees you in it, and the reopened window shows the call you are in.
#[tokio::test]
async fn a_call_goes_on_with_the_window_closed() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let channel = fresh_voice_channel(&hs);
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "general").await;
    open_without_joining(&mut alice, &channel).await;
    let room_id: String = alice
        .attr(".room.selected", "data-room")
        .await
        .expect("room id")
        .expect("a selected room");

    let mut bob = App::launch_desktop(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "general").await;
    open_without_joining(&mut bob, &channel).await;
    bob.click(CALL_BUTTON).await.expect("bob joins");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("bob connected");
    wait_in_call(&mut alice, &room_id, "bob")
        .await
        .expect("alice sees bob");
    // Alice joins too, so there is audio to hear from bob's side.
    alice.click(CALL_BUTTON).await.expect("alice joins");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("alice connected");
    wait_hears(&mut alice, "bob").await.expect("alice hears bob");

    // Bob closes the window to the tray: the call must not end.
    bob.close_to_tray().await.expect("close");
    tokio::time::sleep(std::time::Duration::from_secs(8)).await;
    assert!(bob.is_running(), "closing the window keeps the app");
    assert!(
        alice
            .texts(&format!(
                ".room-call[data-room=\"{room_id}\"] .call-participant"
            ))
            .await
            .expect("alice's list")
            .iter()
            .any(|t| t.contains("bob")),
        "bob left the call when his window closed"
    );
    // His audio is still there for alice, not just his listing.
    wait_hears(&mut alice, "bob")
        .await
        .expect("alice still hears bob with his window closed");

    // Reopened (what the tray does), bob's window shows the call he is in.
    bob.show_window().await.expect("show window");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("the reopened window shows the call");
    wait_in_call(&mut bob, &room_id, "bob")
        .await
        .expect("bob listed in the call");
    bob.click(".voice-panel .hang-up").await.expect("bob leaves");
    wait_not_in_call(&mut alice, &room_id, "bob")
        .await
        .expect("bob hung up");
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
    bob.close_to_tray().await.expect("close");
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

/// On a phone a DM call rings as a real call, as Discord's do: the app in
/// the background hands it to Android (Telecom, ringing with the phone's
/// ringtone, a call notification with Answer and Decline, the full-screen
/// incoming call over the lock screen). Answer opens the app in the call;
/// Decline tells the caller; a caller giving up stops the ringing.
#[tokio::test]
async fn a_phone_rings_like_a_phone_call() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut bob = App::launch(&hs).await.expect("launch bob");
    if !bob.is_phone() {
        eprintln!("a phone's call handling (MORPHEUS_E2E_ANDROID); skipping");
        return;
    }
    login_and_open(&mut bob, "bob", "general").await;
    let mut alice = App::launch_desktop(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "general").await;
    open_dm(&mut alice, "bob").await;
    let ringing = || {
        morpheus_e2e::android_telecom_calls()
            .iter()
            .any(|s| s == "RINGING")
    };
    let active = || {
        morpheus_e2e::android_telecom_calls()
            .iter()
            .any(|s| s == "ACTIVE")
    };
    let wait = |what: &str, check: &dyn Fn() -> bool| {
        let deadline = std::time::Instant::now() + DEFAULT_WAIT;
        while !check() {
            assert!(
                std::time::Instant::now() < deadline,
                "{what}: {:?}",
                morpheus_e2e::android_telecom_calls()
            );
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    };

    // In the background, a call rings as the phone's.
    morpheus_e2e::android_home();
    alice.click(CALL_BUTTON).await.expect("alice calls");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("alice waiting");
    wait("ringing as a phone call", &ringing);

    // Answer, as from the notification: the app comes up, in the call.
    morpheus_e2e::android_tap_text("Answer").expect("tap Answer");
    wait("the call active", &active);
    bob.wait_for_js(
        "return !!document.querySelector('.call-screen, .call-bar, .voice-panel');",
        DEFAULT_WAIT,
    )
    .await
    .expect("bob in the call");
    alice
        .wait_for(".call-stage [data-user=\"@bob:localhost\"]", DEFAULT_WAIT)
        .await
        .expect("alice sees bob join");
    // Both hang up (a DM call stays up for whoever is still in it).
    alice
        .click(".voice-panel .hang-up")
        .await
        .expect("alice hangs up");
    bob.wait_for_js(
        "const b = document.querySelector('.call-screen .hang-up, .call-bar .hang-up, .voice-panel .hang-up'); if (!b) return false; b.click(); return true;",
        DEFAULT_WAIT,
    )
    .await
    .expect("bob hangs up");
    bob.wait_for_js(
        "return !document.querySelector('.call-screen, .call-bar');",
        DEFAULT_WAIT,
    )
    .await
    .expect("bob's call over");
    wait("Android's call over", &|| {
        morpheus_e2e::android_telecom_calls().is_empty()
    });

    // Declined from the phone: the caller is told.
    alice
        .wait_for(
            ".chat-header .call-button[title=\"Start a call\"]",
            DEFAULT_WAIT,
        )
        .await
        .expect("call empty");
    morpheus_e2e::android_home();
    alice.click(CALL_BUTTON).await.expect("alice calls again");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("alice waiting");
    wait("ringing again", &ringing);
    morpheus_e2e::android_tap_text("Decline").expect("tap Decline");
    alice
        .wait_for_text(".toast", "bob declined the call", DEFAULT_WAIT)
        .await
        .expect("alice told");
    wait("no call left", &|| {
        morpheus_e2e::android_telecom_calls().is_empty()
    });

    // The caller gives up: the phone stops ringing.
    alice
        .wait_for(
            ".chat-header .call-button[title=\"Start a call\"]",
            DEFAULT_WAIT,
        )
        .await
        .expect("call empty");
    alice
        .click(CALL_BUTTON)
        .await
        .expect("alice calls a third time");
    wait("ringing a third time", &ringing);
    alice
        .click(".voice-panel .hang-up")
        .await
        .expect("alice gives up");
    wait("ringing stops", &|| {
        morpheus_e2e::android_telecom_calls().is_empty()
    });
}

/// With the app closed (Android killed it), a DM call still rings the phone
/// (by push) and Decline still tells the caller at once: the phone signs in
/// from its saved session for the one send. Needs pushes (UnifiedPush and
/// internet), so it runs only with `MORPHEUS_E2E_PUSH=1`.
#[tokio::test]
async fn declining_with_the_app_closed_tells_the_caller() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    if std::env::var_os("MORPHEUS_E2E_PUSH").is_none() {
        eprintln!("needs pushes (MORPHEUS_E2E_PUSH); skipping");
        return;
    }
    let mut bob = App::launch(&hs).await.expect("launch bob");
    if !bob.is_phone() {
        eprintln!("a phone's (MORPHEUS_E2E_ANDROID); skipping");
        return;
    }
    login_and_open(&mut bob, "bob", "general").await;
    // Registered for pushes before it goes.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !morpheus_e2e::android_shell(&[
        "run-as",
        "dev.morpheus.mobile",
        "cat",
        "files/morpheus/push_status",
    ])
    .unwrap_or_default()
    .contains("registered")
    {
        assert!(
            std::time::Instant::now() < deadline,
            "push never registered"
        );
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    let mut alice = App::launch_desktop(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "general").await;
    open_dm(&mut alice, "bob").await;

    // Closed: in the background, then killed as Android reclaims memory (a
    // force-stop would block pushes too).
    morpheus_e2e::android_home();
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let pid = morpheus_e2e::android_shell(&["pidof", "dev.morpheus.mobile"]).unwrap_or_default();
    let _ =
        morpheus_e2e::android_shell(&["run-as", "dev.morpheus.mobile", "kill", "-9", pid.trim()]);
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    alice.click(CALL_BUTTON).await.expect("alice calls");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("alice waiting");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !morpheus_e2e::android_telecom_calls()
        .iter()
        .any(|s| s == "RINGING")
    {
        assert!(
            std::time::Instant::now() < deadline,
            "the push never rang the phone"
        );
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    let rang_at = std::time::Instant::now();
    morpheus_e2e::android_tap_text("Decline").expect("tap Decline");
    alice
        .wait_for_text(".toast", "bob declined the call", DEFAULT_WAIT)
        .await
        .expect("alice told");
    assert!(
        rang_at.elapsed() < std::time::Duration::from_secs(25),
        "told before the ring ran out by itself"
    );
}

/// Poll both apps until `.call-participant.speaking` lists `who` (`on`) or
/// stops listing them on each: the milliseconds since `since` for `first`
/// and for `second` (None at the deadline), polled in turns so neither
/// waits for the other.
async fn speaking_flips(
    first: &mut App,
    second: &mut App,
    who: &str,
    on: bool,
    since: std::time::Instant,
) -> (Option<u128>, Option<u128>) {
    let deadline = since + std::time::Duration::from_secs(8);
    let (mut a, mut b) = (None, None);
    while (a.is_none() || b.is_none()) && std::time::Instant::now() < deadline {
        for (app, seen) in [(&mut *first, &mut a), (&mut *second, &mut b)] {
            if seen.is_none() {
                let texts = app
                    .texts(".call-participant.speaking")
                    .await
                    .unwrap_or_default();
                if texts.iter().any(|t| t.contains(who)) == on {
                    *seen = Some(since.elapsed().as_millis());
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    (a, b)
}

/// How soon the green ring follows someone starting and stopping to talk
/// (Discord: from the client's own voice detection, so a fraction of a
/// second). Test runs send a tone while unmuted, so unmuting is starting to
/// speak and muting is stopping. Timed from just before the click on bob's
/// mute button, on bob's own card and on alice's view of bob; the click
/// itself (reported as `click`) is part of each figure.
///
/// Targets: own ring within ~150 ms of speaking, others' within ~300 ms of
/// the audio arriving, the ring off within ~500 ms of silence (a short hold
/// so it does not flicker between words). The bounds below are generous
/// multiples of those, with the bridge's own latency (tens of ms) in them.
#[tokio::test]
async fn the_speaking_ring_follows_the_voice_quickly() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    let channel = fresh_voice_channel(&hs);
    login_and_open(&mut alice, "alice", "general").await;
    open_without_joining(&mut alice, &channel).await;
    alice.click(CALL_BUTTON).await.expect("alice joins");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("alice connected");
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "general").await;
    open_without_joining(&mut bob, &channel).await;
    bob.click(CALL_BUTTON).await.expect("bob joins");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("bob connected");
    bob.wait_for_text(".call-participant.speaking", "bob", DEFAULT_WAIT)
        .await
        .expect("bob sees himself speak");
    alice
        .wait_for_text(".call-participant.speaking", "bob", DEFAULT_WAIT)
        .await
        .expect("alice sees bob speak");

    // (own on, other on, own off, other off, click) per round.
    let mut rounds: Vec<[u128; 5]> = Vec::new();
    for _ in 0..4 {
        // Stop: bob mutes.
        let t0 = std::time::Instant::now();
        bob.click(".user-bar .mute-button").await.expect("mute");
        let click = t0.elapsed().as_millis();
        let (own_off, other_off) = speaking_flips(&mut bob, &mut alice, "bob", false, t0).await;
        // Let the ring settle, then start: bob unmutes.
        tokio::time::sleep(std::time::Duration::from_millis(2_500)).await;
        let t0 = std::time::Instant::now();
        bob.click(".user-bar .mute-button").await.expect("unmute");
        let (own_on, other_on) = speaking_flips(&mut bob, &mut alice, "bob", true, t0).await;
        tokio::time::sleep(std::time::Duration::from_millis(1_500)).await;
        rounds.push([
            own_on.unwrap_or(99_999),
            other_on.unwrap_or(99_999),
            own_off.unwrap_or(99_999),
            other_off.unwrap_or(99_999),
            click,
        ]);
    }
    let report = format!(
        "[own on, other on, own off, other off, click] ms per round: {rounds:?}\n"
    );
    eprintln!("{report}");
    {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/speaking-latency.txt"))
        {
            let _ = write!(file, "{report}");
        }
    }
    // Generous bounds from the targets (the slowest of the rounds).
    let worst = |i: usize| rounds.iter().map(|r| r[i]).max().unwrap_or(0);
    assert!(worst(0) <= 220, "own ring on after {} ms\n{report}", worst(0));
    assert!(worst(1) <= 420, "alice's ring on after {} ms\n{report}", worst(1));
    assert!(worst(2) <= 650, "own ring off after {} ms\n{report}", worst(2));
    assert!(worst(3) <= 650, "alice's ring off after {} ms\n{report}", worst(3));
}

/// A camera's card shows the whole picture the camera sends, in its own
/// shape (the test camera is 4:3, the real one 640x480), with round corners
/// on the picture itself: gpui clips a card to a rectangle, so a picture
/// drawn to fill and crop it came out with square corners and its edges cut
/// off. Pixels are read from a screenshot of the test window.
#[tokio::test]
async fn a_cameras_card_shows_the_whole_picture_with_round_corners() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    let channel = fresh_voice_channel(&hs);
    login_and_open(&mut alice, "alice", "general").await;
    open_without_joining(&mut alice, &channel).await;
    alice.click(CALL_BUTTON).await.expect("alice joins");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("alice connected");
    alice
        .click(".voice-panel .camera-button")
        .await
        .expect("camera on");
    let tile = ".call-stage .video-tile:not(.screen)";
    alice
        .wait_for(&format!("{tile} .video[data-shown=true]"), DEFAULT_WAIT)
        .await
        .expect("her camera's picture");
    wait_test_pattern(&mut alice, "@alice:localhost")
        .await
        .expect("the test pattern");
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let card = alice
        .bounds(tile)
        .await
        .expect("bounds")
        .expect("the card");
    let Some((pixels, w, h)) = common::capture(&alice) else {
        eprintln!("no grim capture here; skipping the pixel check");
        return;
    };
    // The picture, in its own shape (the card has it too), as large as fits
    // the card, centred.
    let (shown_w, shown_h) = picture_size(&mut alice, tile).await;
    let ratio = shown_w / shown_h;
    let (pw, ph) = if card.w / card.h > ratio {
        (card.h * ratio, card.h)
    } else {
        (card.w, card.w / ratio)
    };
    let (px, py) = (card.x + (card.w - pw) / 2.0, card.y + (card.h - ph) / 2.0);
    let pixel = |x: f64, y: f64| {
        let (x, y) = (x.round() as usize, y.round() as usize);
        assert!(x < w && y < h, "({x}, {y}) is outside the {w}x{h} capture");
        let i = (y * w + x) * 3;
        (pixels[i], pixels[i + 1], pixels[i + 2])
    };
    let red = |p: (u8, u8, u8)| p.0 > 150 && p.1 < 100 && p.2 < 100;
    let blue = |p: (u8, u8, u8)| p.2 > 150 && p.0 < 100 && p.1 < 100;
    let (cx, cy) = (px + pw / 2.0, py + ph / 2.0);
    // The picture reaches the card's shape: nothing of its sides is cut
    // off (a white bar sweeps across it, so look at two places each).
    assert!(
        pw > card.w * 0.9 || ph > card.h * 0.9,
        "the picture ({pw}x{ph}) does not fill the card ({}x{})",
        card.w,
        card.h
    );
    // Her own camera is drawn mirrored: the pattern runs the other way round.
    let mirrored = alice
        .attr(".call-stage .video-tile:not(.screen)", "data-mirrored")
        .await
        .expect("attr")
        .as_deref()
        == Some("true");
    let (left_is, right_is): (&dyn Fn((u8, u8, u8)) -> bool, &dyn Fn((u8, u8, u8)) -> bool) =
        if mirrored { (&blue, &red) } else { (&red, &blue) };
    let left = [pixel(px + 4.0, cy), pixel(px + 24.0, cy)];
    let right = [pixel(px + pw - 4.0, cy), pixel(px + pw - 24.0, cy)];
    assert!(left.iter().any(|p| left_is(*p)), "the left edge is {left:?}, not the picture's red");
    assert!(right.iter().any(|p| right_is(*p)), "the right edge is {right:?}, not the picture's blue");
    // The middle of the top and bottom edges show the picture too.
    for (x, red_side) in [(px + pw * 0.25, !mirrored), (px + pw * 0.75, mirrored)] {
        for y in [py + 3.0, py + ph - 3.0] {
            let p = pixel(x, y);
            assert!(if red_side { red(p) } else { blue(p) }, "the edge at ({x}, {y}) is {p:?}");
        }
    }
    // The corners of the picture are the card's background, not picture.
    for (x, y) in [(px + 1.0, py + 1.0), (px + pw - 2.0, py + 1.0), (px + 1.0, py + ph - 2.0), (px + pw - 2.0, py + ph - 2.0)] {
        let p = pixel(x, y);
        assert!(!red(p) && !blue(p), "the picture's corner at ({x}, {y}) is {p:?}: it is not rounded (card {card:?}, picture {px},{py} {pw}x{ph})");
    }
}

/// Joining a voice channel on a local server: no network between.
#[tokio::test]
async fn joining_a_voice_channel_is_quick() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    join_figures(&hs, "local", 400).await;
}

/// A homeserver in front of which every request waits 100 ms (a server some
/// distance away; matrix.org's answers took longer): returns the proxy's URL.
async fn slow_homeserver(hs: &str, delay: std::time::Duration) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let target = hs.trim_start_matches("http://").trim_end_matches('/').to_owned();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    tokio::spawn(async move {
        while let Ok((client, _)) = listener.accept().await {
            let target = target.clone();
            tokio::spawn(async move {
                let Ok(server) = tokio::net::TcpStream::connect(&target).await else {
                    return;
                };
                let (mut client_read, mut client_write) = client.into_split();
                let (mut server_read, mut server_write) = server.into_split();
                let up = async move {
                    let mut buf = [0u8; 16384];
                    while let Ok(n) = client_read.read(&mut buf).await {
                        if n == 0 {
                            break;
                        }
                        tokio::time::sleep(delay).await;
                        if server_write.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                };
                let down = async move {
                    let mut buf = [0u8; 16384];
                    while let Ok(n) = server_read.read(&mut buf).await {
                        if n == 0 || client_write.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                };
                tokio::join!(up, down);
            });
        }
    });
    url
}

/// The same with every homeserver request 100 ms away: the steps that ran
/// one after another (the call service lookup, the OpenID token, the
/// delayed-leave request the server refuses, the membership) each cost a
/// round trip, and joining took about as many of them as it had steps.
#[tokio::test]
async fn joining_a_voice_channel_is_quick_when_the_server_is_far() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let far = slow_homeserver(&hs, std::time::Duration::from_millis(100)).await;
    join_figures(&far, "100 ms away", 450).await;
}

/// A call ending puts one line in the room's chat, "<last to leave> ended a
/// call" with the grey phone: not one for each person leaving, only the one
/// that leaves it empty (task 23). Matrix has no event for it: it is worked
/// out from the call.member events.
#[tokio::test]
async fn a_call_ending_leaves_one_ended_a_call_line_when_the_last_person_leaves() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let channel = fresh_voice_channel(&hs);
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "general").await;
    open_without_joining(&mut alice, &channel).await;
    let room: String = alice
        .attr(".room.selected", "data-room")
        .await
        .expect("room id")
        .expect("a selected room");
    alice.click(CALL_BUTTON).await.expect("call");
    alice
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("alice connected");
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "general").await;
    open_without_joining(&mut bob, &channel).await;
    bob.click(CALL_BUTTON).await.expect("join");
    bob.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("bob connected");
    wait_in_call(&mut alice, &room, "bob").await.expect("alice sees bob");
    alice.click(".voice-chat-toggle").await.expect("open the chat");
    alice
        .wait_for(".voice-chat .timeline", DEFAULT_WAIT)
        .await
        .expect("the chat");

    // Alice leaves, bob is still in: no ended line.
    alice.click(".voice-panel .hang-up").await.expect("alice hangs up");
    wait_not_in_call(&mut bob, &room, "alice").await.expect("alice gone");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    assert_eq!(
        alice.count(".voice-chat .message.system .state-icon.call-ended").await.expect("count"),
        0,
        "an ended line while bob is still in the call"
    );

    // Bob leaves, the call is empty: one line.
    bob.click(".voice-panel .hang-up").await.expect("bob hangs up");
    alice
        .wait_for(
            ".voice-chat .message.system .state-icon.call-ended[data-icon=phone][data-tone=quiet]",
            DEFAULT_WAIT,
        )
        .await
        .expect("an ended line with the grey phone");
    alice
        .wait_for_text(".voice-chat .timeline", "bob ended a call", DEFAULT_WAIT)
        .await
        .expect("it names who left last");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    assert_eq!(
        alice.count(".voice-chat .message.system .state-icon.call-ended").await.expect("count"),
        1,
        "one ended line for the call"
    );
}

/// The size (width, height) of the picture a tile shows, as the tile notes it.
async fn picture_size(app: &mut App, tile: &str) -> (f64, f64) {
    let size = app
        .attr(&format!("{tile} .video"), "data-size")
        .await
        .expect("attr")
        .unwrap_or_default();
    let (w, h) = size.split_once('x').unwrap_or(("0", "0"));
    (w.parse().unwrap_or(0.0), h.parse().unwrap_or(0.0))
}

/// Alice and bob in a fresh voice channel, alice's camera on and showing on
/// both: alice launched with `env`.
async fn two_in_a_call_with_alices_camera(
    hs: &str,
    env: &[(&str, &str)],
) -> (App, App) {
    let mut alice = App::launch_with_env(hs, env).await.expect("launch alice");
    let mut bob = App::launch(hs).await.expect("launch bob");
    let channel = fresh_voice_channel(hs);
    login_and_open(&mut alice, "alice", "general").await;
    login_and_open(&mut bob, "bob", "general").await;
    for app in [&mut alice, &mut bob] {
        open_without_joining(app, &channel).await;
        app.click(CALL_BUTTON).await.expect("joins");
        app.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
            .await
            .expect("connected");
    }
    alice
        .click(".voice-panel .camera-button")
        .await
        .expect("camera on");
    wait_test_pattern(&mut alice, "@alice:localhost")
        .await
        .expect("alice's self view");
    wait_test_pattern(&mut bob, "@alice:localhost")
        .await
        .expect("bob sees alice's camera");
    // Let the pictures settle: the tile follows the first picture's shape.
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    (alice, bob)
}

/// A video tile takes its picture's shape, whatever it is (a 4:3 camera
/// here): no bars, nothing cropped, in the self view and for others.
#[tokio::test]
async fn a_video_tile_takes_the_shape_of_its_picture() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (mut alice, mut bob) =
        two_in_a_call_with_alices_camera(&hs, &[("MORPHEUS_CALL_CAMERA_SIZE", "640x480")]).await;
    if !alice.is_gpui() {
        eprintln!("GPUI only (the Dioxus tile is a 16:9 box)");
        return;
    }
    let tile = ".call-stage .video-tile:not(.screen)[data-user=\"@alice:localhost\"]";
    for (who, app) in [("alice", &mut alice), ("bob", &mut bob)] {
        let (pw, ph) = picture_size(app, tile).await;
        assert!(pw > 0.0 && ph > 0.0, "{who}: no picture size");
        let card = app.bounds(tile).await.expect("bounds").expect("the card");
        let (picture, card_ratio) = (pw / ph, card.w / card.h);
        assert!(
            (card_ratio / picture - 1.0).abs() < 0.02,
            "{who}: the tile is {}x{} ({card_ratio:.3}) for a {pw}x{ph} picture ({picture:.3})",
            card.w,
            card.h
        );
        assert!(
            (picture / (4.0 / 3.0) - 1.0).abs() < 0.02,
            "{who}: the test camera should be 4:3, is {pw}x{ph}"
        );
    }
}

/// The camera is sent at 720p, 16:9 (the test camera's widest mode), with a
/// bitrate for it: what is published, and what the others get.
#[tokio::test]
async fn a_camera_is_sent_at_720p_widescreen_with_a_matching_bitrate() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (mut alice, mut bob) = two_in_a_call_with_alices_camera(&hs, &[]).await;
    if !alice.is_gpui() {
        eprintln!("GPUI only (the published settings are noted by its tiles)");
        return;
    }
    let tile = ".call-stage .video-tile:not(.screen)[data-user=\"@alice:localhost\"]";
    let published = alice
        .attr(&format!("{tile} .video"), "data-publish")
        .await
        .expect("attr")
        .unwrap_or_default();
    eprintln!("published: {published}");
    // "1280x720@30 bitrate=1700000 layers=3 ..."
    assert!(published.starts_with("1280x720@"), "published {published:?}");
    let bitrate: u64 = published
        .split_whitespace()
        .find_map(|part| part.strip_prefix("bitrate="))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    assert!(
        (1_200_000..=3_500_000).contains(&bitrate),
        "a 720p camera's bitrate is {bitrate}: {published}"
    );
    for (who, app) in [("alice", &mut alice), ("bob", &mut bob)] {
        let (pw, ph) = picture_size(app, tile).await;
        assert!(
            pw >= 1280.0 && (pw / ph / (16.0 / 9.0) - 1.0).abs() < 0.02,
            "{who} sees a {pw}x{ph} picture, not 1280x720"
        );
        let card = app.bounds(tile).await.expect("bounds").expect("the card");
        assert!(
            (card.w / card.h / (16.0 / 9.0) - 1.0).abs() < 0.02,
            "{who}: a 16:9 picture in a {}x{} tile",
            card.w,
            card.h
        );
    }
}

/// Alice and bob in a fresh voice channel, both connected.
async fn two_in_a_call(hs: &str, env: &[(&str, &str)]) -> (App, App) {
    let mut alice = App::launch_with_env(hs, env).await.expect("launch alice");
    let mut bob = App::launch(hs).await.expect("launch bob");
    let channel = fresh_voice_channel(hs);
    login_and_open(&mut alice, "alice", "general").await;
    login_and_open(&mut bob, "bob", "general").await;
    for app in [&mut alice, &mut bob] {
        open_without_joining(app, &channel).await;
        app.click(CALL_BUTTON).await.expect("joins");
        app.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
            .await
            .expect("connected");
    }
    (alice, bob)
}

/// Wait until the picture of `tile` in `app` is `w` x `h`.
async fn wait_picture_size(app: &mut App, tile: &str, w: f64, h: f64) {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let seen = picture_size(app, tile).await;
        if seen == (w, h) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{tile} still shows {seen:?}, not {w}x{h}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }
}

/// What a tile's track was published with: "1280x720@30 bitrate=… …".
async fn published(app: &mut App, tile: &str) -> String {
    app.attr(&format!("{tile} .video"), "data-publish")
        .await
        .expect("attr")
        .unwrap_or_default()
}

fn published_bitrate(published: &str) -> u64 {
    published
        .split_whitespace()
        .find_map(|part| part.strip_prefix("bitrate="))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// Pressing Share screen asks how first (resolution 720p / 1080p / Source,
/// 15 / 30 / 60 frames a second; Go Live starts it), remembers the choice,
/// and publishes the screen the way it says: no simulcast, a bitrate for
/// the size and speed, text-friendly below 60 fps. The test screen is 1440p.
#[tokio::test]
async fn sharing_the_screen_asks_for_its_quality_and_sends_that() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (mut alice, mut bob) = two_in_a_call(&hs, &[]).await;
    if !alice.is_gpui() {
        eprintln!("GPUI only (Dioxus shares at once)");
        return;
    }
    let button = ".voice-panel .screen-button";
    let mine = ".call-stage .video-tile.screen.self";
    let theirs = ".call-stage .video-tile.screen[data-user=\"@alice:localhost\"]";
    alice.click(button).await.expect("share");
    alice
        .wait_for(".screen-quality", DEFAULT_WAIT)
        .await
        .expect("the quality popover");
    assert_eq!(alice.count(".screen-quality .screen-res").await.expect("count"), 3);
    assert_eq!(alice.count(".screen-quality .screen-fps").await.expect("count"), 3);
    let labels = alice.texts(".screen-quality .screen-res").await.expect("texts");
    assert_eq!(labels, ["720p", "1080p", "Source"]);
    let labels = alice.texts(".screen-quality .screen-fps").await.expect("texts");
    assert_eq!(labels, ["15", "30", "60"]);
    // The usual choice first: 1080p at 30.
    alice
        .wait_for(".screen-res.active[data-value=\"1080\"]", DEFAULT_WAIT)
        .await
        .expect("1080p chosen");
    alice
        .wait_for(".screen-fps.active[data-value=\"30\"]", DEFAULT_WAIT)
        .await
        .expect("30 fps chosen");
    assert_eq!(alice.count(".screen-quality .screen-go").await.expect("count"), 1);
    // Nothing is shared until Go Live; Escape closes the popover.
    assert_eq!(alice.count(".video-tile.screen").await.expect("count"), 0);
    alice.press("body", "Escape").await.expect("escape");
    alice
        .wait_for_count(".screen-quality", 0, DEFAULT_WAIT)
        .await
        .expect("closed");
    assert_eq!(alice.count(&format!("{button}.active")).await.expect("count"), 0);

    // 720p at 15.
    alice.click(button).await.expect("share again");
    alice
        .click(".screen-quality .screen-res[data-value=\"720\"]")
        .await
        .expect("720p");
    alice
        .click(".screen-quality .screen-fps[data-value=\"15\"]")
        .await
        .expect("15 fps");
    alice.click(".screen-quality .screen-go").await.expect("go live");
    alice
        .wait_for_count(".screen-quality", 0, DEFAULT_WAIT)
        .await
        .expect("closed by Go Live");
    alice
        .wait_for(&format!("{mine} .video[data-shown=true]"), DEFAULT_WAIT)
        .await
        .expect("her own screen");
    let p = published(&mut alice, mine).await;
    eprintln!("720p15: {p}");
    assert!(p.starts_with("1280x720@15 "), "{p}");
    assert!(p.contains("simulcast=false"), "one full layer for a screen: {p}");
    assert!(p.contains("hint=detail"), "{p}");
    assert!(p.contains("degradation=maintain-resolution"), "{p}");
    let bitrate = published_bitrate(&p);
    assert!((800_000..=2_000_000).contains(&bitrate), "720p15 bitrate {bitrate}");
    wait_picture_size(&mut alice, mine, 1280.0, 720.0).await;
    wait_picture_size(&mut bob, theirs, 1280.0, 720.0).await;
    // Stopping needs no question.
    alice.click(button).await.expect("stop");
    alice
        .wait_for_count(".video-tile.screen", 0, DEFAULT_WAIT)
        .await
        .expect("stopped");
    assert_eq!(alice.count(".screen-quality").await.expect("count"), 0);

    // It remembers: 720p and 15 are chosen now, and kept for the next run.
    alice.click(button).await.expect("share");
    alice
        .wait_for(".screen-res.active[data-value=\"720\"]", DEFAULT_WAIT)
        .await
        .expect("720p remembered");
    alice
        .wait_for(".screen-fps.active[data-value=\"15\"]", DEFAULT_WAIT)
        .await
        .expect("15 remembered");
    let kept = std::fs::read_to_string(alice.data_dir().join("screen-quality.json"))
        .unwrap_or_default();
    assert!(kept.contains("720") && kept.contains("15"), "kept in {kept:?}");

    // 1080p at 60: more bits, and motion over text.
    alice
        .click(".screen-quality .screen-res[data-value=\"1080\"]")
        .await
        .expect("1080p");
    alice
        .click(".screen-quality .screen-fps[data-value=\"60\"]")
        .await
        .expect("60 fps");
    alice.click(".screen-quality .screen-go").await.expect("go live");
    alice
        .wait_for(&format!("{mine} .video[data-shown=true]"), DEFAULT_WAIT)
        .await
        .expect("her own screen");
    let p = published(&mut alice, mine).await;
    eprintln!("1080p60: {p}");
    assert!(p.starts_with("1920x1080@60 "), "{p}");
    assert!(p.contains("hint=motion"), "{p}");
    let bitrate = published_bitrate(&p);
    assert!((6_000_000..=12_000_000).contains(&bitrate), "1080p60 bitrate {bitrate}");
    wait_picture_size(&mut bob, theirs, 1920.0, 1080.0).await;
    alice.click(button).await.expect("stop");
    alice
        .wait_for_count(".video-tile.screen", 0, DEFAULT_WAIT)
        .await
        .expect("stopped");

    // Source: the screen's own size (1440p), no scaling down.
    alice.click(button).await.expect("share");
    alice
        .click(".screen-quality .screen-res[data-value=\"source\"]")
        .await
        .expect("source");
    alice
        .click(".screen-quality .screen-fps[data-value=\"15\"]")
        .await
        .expect("15 fps");
    alice.click(".screen-quality .screen-go").await.expect("go live");
    alice
        .wait_for(&format!("{mine} .video[data-shown=true]"), DEFAULT_WAIT)
        .await
        .expect("her own screen");
    let p = published(&mut alice, mine).await;
    eprintln!("source15: {p}");
    assert!(p.starts_with("2560x1440@15 "), "{p}");
    wait_picture_size(&mut bob, theirs, 2560.0, 1440.0).await;
    alice.click(button).await.expect("stop");
}

/// Turning the camera or the screen share on or off plays its sound (Discord's
/// pairs), once, for you; the others in the call hear a share start. Nothing
/// plays where the share was not started (Escape on the quality popover).
#[tokio::test]
async fn camera_and_screen_share_play_sounds() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (mut alice, mut bob) = two_in_a_call(&hs, &[]).await;
    if alice.is_phone() {
        eprintln!("phones do not share their screen yet; skipping");
        return;
    }
    let camera = ".voice-panel .camera-button";
    let screen = ".voice-panel .screen-button";
    for sound in ["camera-on", "camera-off", "screen-on", "screen-off"] {
        assert_eq!(count_sound(&mut alice, sound).await, 0, "{sound} before");
    }

    alice.click(camera).await.expect("camera on");
    wait_sound(&mut alice, "camera-on").await.expect("camera on sound");
    alice
        .wait_for(&format!("{camera}.active"), DEFAULT_WAIT)
        .await
        .expect("camera on");
    assert_eq!(count_sound(&mut alice, "camera-off").await, 0);
    alice.click(camera).await.expect("camera off");
    wait_sound(&mut alice, "camera-off").await.expect("camera off sound");
    assert_eq!(count_sound(&mut alice, "camera-on").await, 1);
    assert_eq!(count_sound(&mut alice, "camera-off").await, 1);

    // Cancelled: no sound.
    if alice.is_gpui() {
        alice.click(screen).await.expect("share");
        alice
            .wait_for(".screen-quality", DEFAULT_WAIT)
            .await
            .expect("the quality popover");
        alice.press("body", "Escape").await.expect("escape");
        alice
            .wait_for_count(".screen-quality", 0, DEFAULT_WAIT)
            .await
            .expect("closed");
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        assert_eq!(count_sound(&mut alice, "screen-on").await, 0);
        assert_eq!(count_sound(&mut alice, "screen-off").await, 0);
    }

    start_share(&mut alice, screen).await;
    wait_sound(&mut alice, "screen-on").await.expect("share sound");
    wait_sound(&mut bob, "screen-on").await.expect("bob hears the share start");
    assert_eq!(count_sound(&mut alice, "screen-off").await, 0);
    alice.click(screen).await.expect("stop");
    wait_sound(&mut alice, "screen-off").await.expect("stop sound");
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    assert_eq!(count_sound(&mut alice, "screen-on").await, 1);
    assert_eq!(count_sound(&mut alice, "screen-off").await, 1);
    assert_eq!(count_sound(&mut alice, "camera-on").await, 1);
    assert_eq!(count_sound(&mut alice, "camera-off").await, 1);
    assert_eq!(count_sound(&mut bob, "screen-on").await, 1);
    // Bob did nothing himself: no camera sounds, no stop sound.
    assert_eq!(count_sound(&mut bob, "camera-on").await, 0);
    assert_eq!(count_sound(&mut bob, "screen-off").await, 0);
}

/// Clicking someone under a voice channel in the sidebar opens their profile
/// beside that row, as clicking a member does (a click in the call grid
/// still focuses a card).
#[tokio::test]
async fn a_person_under_a_voice_channel_opens_their_profile_beside_the_row() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (mut alice, _bob) = two_in_a_call(&hs, &[]).await;
    if !alice.is_gpui() {
        eprintln!("GPUI only");
        return;
    }
    let row = ".room-call .call-participant[data-user=\"@bob:localhost\"]";
    alice.wait_for(row, DEFAULT_WAIT).await.expect("bob listed");
    alice.click(row).await.expect("click bob's row");
    alice
        .wait_for_text(".profile-card .profile-id", "@bob:localhost", DEFAULT_WAIT)
        .await
        .expect("bob's profile opens");
    let card = alice.bounds(".profile-card").await.expect("bounds").expect("card");
    let row_box = alice.bounds(row).await.expect("bounds").expect("row");
    assert!(
        card.x >= row_box.x + row_box.w - 1.0,
        "the card is beside the row: card {card:?}, row {row_box:?}"
    );
    assert!(
        card.y <= row_box.y + row_box.h && card.y + card.h >= row_box.y,
        "the card is level with the row: card {card:?}, row {row_box:?}"
    );
    // Still in the call: the click did not leave or re-join anything.
    assert_eq!(alice.count(".call-stage").await.expect("count"), 1);
}

/// What a tile's picture drew at a quarter and three quarters across.
async fn drawn(app: &mut App, tile: &str) -> (String, String) {
    let tile = format!("{tile} .video");
    let left = app.attr(&tile, "data-left").await.expect("attr").unwrap_or_default();
    let right = app.attr(&tile, "data-right").await.expect("attr").unwrap_or_default();
    (left, right)
}

/// Your own camera is drawn mirrored (as in Discord); nobody else sees it
/// that way, you see theirs as it is, and screens are never mirrored.
#[tokio::test]
async fn your_own_camera_is_mirrored_in_your_view_only() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (mut alice, mut bob) = two_in_a_call_with_alices_camera(&hs, &[]).await;
    if !alice.is_gpui() {
        eprintln!("GPUI only (the Dioxus self view is not mirrored)");
        return;
    }
    let mine = ".call-stage .video-tile:not(.screen).self";
    let theirs = ".call-stage .video-tile:not(.screen)[data-user=\"@alice:localhost\"]";
    assert_eq!(
        alice.attr(mine, "data-mirrored").await.expect("attr").as_deref(),
        Some("true"),
        "alice's self view is flagged mirrored"
    );
    assert_ne!(
        bob.attr(theirs, "data-mirrored").await.expect("attr").as_deref(),
        Some("true"),
        "bob sees alice's camera unmirrored"
    );
    // The test camera is red on the left and blue on the right as sent;
    // alice's own view has them the other way round.
    let red = |hex: &str| u32::from_str_radix(hex, 16).map_or(0, |v| (v >> 16) & 255);
    let blue = |hex: &str| u32::from_str_radix(hex, 16).map_or(0, |v| v & 255);
    let (l, r) = drawn(&mut alice, mine).await;
    assert!(
        blue(&l) > 150 && red(&r) > 150,
        "alice's own picture is drawn flipped: left {l}, right {r}"
    );
    let (l, r) = drawn(&mut bob, theirs).await;
    assert!(
        red(&l) > 150 && blue(&r) > 150,
        "bob gets the unflipped picture: left {l}, right {r}"
    );

    // Bob's own camera: alice sees it as sent.
    bob.click(".voice-panel .camera-button").await.expect("bob's camera");
    let bobs = ".call-stage .video-tile:not(.screen)[data-user=\"@bob:localhost\"]";
    alice
        .wait_for(&format!("{bobs} .video[data-shown=true]"), DEFAULT_WAIT)
        .await
        .expect("bob's camera shows");
    assert_ne!(
        alice.attr(bobs, "data-mirrored").await.expect("attr").as_deref(),
        Some("true"),
        "others' cameras are not mirrored"
    );
    // A shared screen is never mirrored.
    start_share(&mut alice, ".voice-panel .screen-button").await;
    wait_screen_pattern(&mut alice, "@alice:localhost")
        .await
        .expect("her own screen as it is");
    let screen = ".call-stage .video-tile.screen.self";
    assert_ne!(
        alice.attr(screen, "data-mirrored").await.expect("attr").as_deref(),
        Some("true"),
        "a screen is never mirrored"
    );
}

/// A focused card (`card` selector, clicked in `app`'s call view) fills the
/// stage as large as fits, in its picture's shape (`ratio`: 16:9 for a card
/// without one), the others in a strip under it; a second click goes back to
/// the grid.
async fn check_focus_fills_stage(app: &mut App, who: &str, card: &str, ratio: Option<f64>) {
    app.click(&format!(".call-stage {card}"))
        .await
        .unwrap_or_else(|e| panic!("{who}: click {card}: {e}"));
    app.wait_for(".call-stage .call-cards.focused", DEFAULT_WAIT)
        .await
        .unwrap_or_else(|e| panic!("{who}: {card} not focused: {e}"));
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let main = app
        .bounds(".call-focus-main")
        .await
        .expect("bounds")
        .expect("the stage of the focused card");
    let big = app
        .bounds(".call-focus-main > *")
        .await
        .expect("bounds")
        .expect("the focused card");
    let want = ratio.unwrap_or(16.0 / 9.0);
    assert!(
        main.h > 150.0,
        "{who}: the focused card's stage is {}x{}",
        main.w,
        main.h
    );
    assert!(
        big.w >= 0.9 * main.w || big.h >= 0.9 * main.h,
        "{who}: the focused {card} is {}x{} in a {}x{} stage: not as large as fits",
        big.w,
        big.h,
        main.w,
        main.h
    );
    assert!(
        (big.w / big.h / want - 1.0).abs() < 0.02,
        "{who}: the focused {card} is {}x{} ({:.3}), not {want:.3}",
        big.w,
        big.h,
        big.w / big.h
    );
    assert!(
        big.w <= main.w + 1.0 && big.h <= main.h + 1.0,
        "{who}: the focused card spills out of its stage"
    );
    let strip = app.all_bounds(".call-strip > *").await.expect("bounds");
    assert!(!strip.is_empty(), "{who}: the others are not in a strip");
    for s in &strip {
        assert!(
            s.y >= big.y + big.h - 1.0,
            "{who}: the strip is not under the focused card"
        );
        assert!(s.w < big.w, "{who}: a strip card is as big as the focused");
    }
    // Click the focused card: back to the grid.
    app.click(".call-focus-main > *").await.expect("click again");
    app.wait_for_count(".call-cards.focused", 0, DEFAULT_WAIT)
        .await
        .unwrap_or_else(|e| panic!("{who}: a second click did not unfocus: {e}"));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(
        app.count(".call-strip").await.expect("count"),
        0,
        "{who}: no strip in the grid"
    );
}

/// Focusing a card makes it fill the stage in its picture's shape (a camera,
/// a shared screen) or 16:9 (a card with only a picture of the person), with
/// the others in a strip under it; clicking it again returns to the grid.
#[tokio::test]
async fn a_focused_card_fills_the_stage_in_its_pictures_shape() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (mut alice, mut bob) =
        two_in_a_call_with_alices_camera(&hs, &[("MORPHEUS_CALL_CAMERA_SIZE", "640x480")]).await;
    if !alice.is_gpui() {
        eprintln!("GPUI only");
        return;
    }
    // A camera (4:3), in bob's view.
    let cam = ".video-tile:not(.screen)[data-user=\"@alice:localhost\"]";
    let (pw, ph) = picture_size(&mut bob, &format!(".call-stage {cam}")).await;
    assert!(pw > 0.0 && ph > 0.0, "no picture size");
    check_focus_fills_stage(&mut bob, "bob (camera)", cam, Some(pw / ph)).await;
    // Avatar only: bob has no camera, in alice's view.
    check_focus_fills_stage(
        &mut alice,
        "alice (avatar)",
        ".call-tile[data-user=\"@bob:localhost\"]",
        None,
    )
    .await;
    // A shared screen, in bob's view.
    start_share(&mut alice, ".voice-panel .screen-button").await;
    bob.wait_for(".call-stage .video-tile.screen", DEFAULT_WAIT)
        .await
        .expect("bob sees the screen");
    wait_screen_pattern(&mut bob, "@alice:localhost")
        .await
        .expect("the screen's picture");
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    let screen = ".video-tile.screen[data-user=\"@alice:localhost\"]";
    let (sw, sh) = picture_size(&mut bob, &format!(".call-stage {screen}")).await;
    assert!(sw > 0.0 && sh > 0.0, "no screen size");
    check_focus_fills_stage(&mut bob, "bob (screen)", screen, Some(sw / sh)).await;
    alice
        .click(".voice-panel .screen-button")
        .await
        .expect("stop sharing");
}
