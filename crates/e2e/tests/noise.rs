//! Noise suppression and the mic test, as in Discord: a button in the call's
//! controls (beside the camera) shows the mode and opens a card above it with
//! Off / Standard / Strong and a Mic Test; Settings → Voice & Video has the
//! same. Strong is a neural suppressor (DeepFilterNet), Standard WebRTC's.
//!
//! Test runs send a tone instead of the microphone: the tone stands for the
//! voice, over a faint noise floor that goes through the suppression as a
//! real microphone would. The call's mic reports what processing it runs
//! (`.voice-panel[data-noise]`), and the meter its level (`data-level`).

#![allow(clippy::expect_used)]

mod common;

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

/// Into #hangout (a voice channel) and its call.
async fn join_hangout(app: &mut App, user: &str) {
    common::login(app, user, &format!("{user}-password")).await;
    app.key_combo("body", "k", true, false, false)
        .await
        .expect("ctrl+k");
    app.wait_for(".switcher input", DEFAULT_WAIT)
        .await
        .expect("switcher");
    app.type_into(".switcher input", "hangout")
        .await
        .expect("filter");
    app.wait_for_text(".switcher-item.active", "hangout", DEFAULT_WAIT)
        .await
        .expect("room offered");
    app.press(".switcher input", "Enter").await.expect("pick");
    app.click(".voice-page-join .join-voice")
        .await
        .expect("join");
    app.wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("in the call");
}

async fn open_voice_settings(app: &mut App) {
    app.click(".settings-button").await.expect("gear");
    app.wait_for_text(".settings-nav button", "Voice & Video", DEFAULT_WAIT)
        .await
        .expect("settings");
    app.click_text(".settings-nav button", "Voice & Video")
        .await
        .expect("voice page");
    app.wait_for(".voice-noise .noise-mode", DEFAULT_WAIT)
        .await
        .expect("the noise suppression choice on the voice page");
}

/// The mode shown as chosen in `scope` (the card or the settings page).
async fn active_mode(app: &mut App, scope: &str) -> Option<String> {
    app.attr(&format!("{scope} .noise-mode.active"), "data-value")
        .await
        .expect("read the choice")
}

async fn wait_active_mode(app: &mut App, scope: &str, mode: &str) {
    app.wait_for(
        &format!("{scope} .noise-mode.active[data-value={mode}]"),
        DEFAULT_WAIT,
    )
    .await
    .unwrap_or_else(|e| {
        panic!("{scope} does not show {mode}: {e}");
    });
}

/// Wait until the call's microphone is processed as `mode`.
async fn wait_published(app: &mut App, mode: &str) {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let now = app
            .attr(".voice-panel", "data-noise")
            .await
            .expect("read the call's processing");
        if now.as_deref() == Some(mode) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the call's mic is processed as {now:?}, not {mode}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

async fn meter_level(app: &mut App, scope: &str) -> f64 {
    app.attr(&format!("{scope} .mic-meter"), "data-level")
        .await
        .expect("read the meter")
        .and_then(|l| l.parse().ok())
        .unwrap_or(-1.0)
}

/// Wait until the meter in `scope` reads over `over` (or, `None`, zero).
async fn wait_meter(app: &mut App, scope: &str, over: Option<f64>) {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let level = meter_level(app, scope).await;
        let ok = match over {
            Some(min) => level > min,
            None => level == 0.0,
        };
        if ok {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the meter in {scope} reads {level}, wanted {over:?}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

async fn loopback(app: &mut App, scope: &str) -> Option<String> {
    app.attr(&format!("{scope} .mic-meter"), "data-loopback")
        .await
        .expect("read the loopback")
}

async fn deafened(app: &mut App) -> bool {
    app.count(".user-pill .deafen-button.active")
        .await
        .expect("read deafen")
        == 1
}

async fn wait_deafened(app: &mut App, on: bool) {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while deafened(app).await != on {
        assert!(
            std::time::Instant::now() < deadline,
            "the deafen button never showed deafened={on}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

/// The level the meter reads for the test tone (8000 of 32768 peak: RMS
/// about 0.17), with room for the noise floor and rounding.
const TONE_LEVEL: f64 = 0.08;

/// The call's button opens a card above it with the modes; a choice there
/// shows in Settings and the reverse, the call's microphone is processed as
/// chosen at once, Escape and a click elsewhere close the card, and the
/// choice is kept across a restart. Strong is the default.
#[tokio::test]
async fn noise_suppression_is_chosen_in_the_call_or_settings_and_kept() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    join_hangout(&mut alice, "alice").await;

    // Beside the camera, showing the mode; the call's mic is processed so.
    let button = ".voice-panel .noise-button";
    alice
        .wait_for(&format!("{button}[data-mode=strong]"), DEFAULT_WAIT)
        .await
        .expect("the noise button, Strong by default");
    let order = alice
        .attrs(".voice-panel .ghost", "class")
        .await
        .expect("controls");
    let noise_at = order
        .iter()
        .position(|c| c.as_deref().is_some_and(|c| c.contains("noise-button")));
    let camera_at = order
        .iter()
        .position(|c| c.as_deref().is_some_and(|c| c.contains("camera-button")));
    assert!(
        noise_at.is_some() && camera_at.is_some() && noise_at.map(|n| n + 1) == camera_at,
        "the noise button sits just before the camera: {order:?}"
    );
    wait_published(&mut alice, "strong").await;

    // The card opens above the button.
    alice.click(button).await.expect("open the card");
    alice
        .wait_for(".noise-card", DEFAULT_WAIT)
        .await
        .expect("the card");
    let card = alice
        .bounds(".noise-card")
        .await
        .expect("card")
        .expect("card");
    let under = alice.bounds(button).await.expect("button").expect("button");
    assert!(
        card.y + card.h <= under.y + 1.0,
        "the card is above its button: card {card:?}, button {under:?}"
    );
    assert_eq!(
        active_mode(&mut alice, ".noise-card").await.as_deref(),
        Some("strong")
    );
    let modes = alice
        .attrs(".noise-card .noise-mode", "data-value")
        .await
        .expect("modes");
    assert_eq!(
        modes.into_iter().flatten().collect::<Vec<_>>(),
        ["off", "standard", "strong"]
    );

    // Standard: chosen, applied to the call's mic, shown on the button.
    alice
        .click(".noise-card .noise-mode[data-value=standard]")
        .await
        .expect("standard");
    wait_active_mode(&mut alice, ".noise-card", "standard").await;
    wait_published(&mut alice, "standard").await;
    alice
        .wait_for(&format!("{button}[data-mode=standard]"), DEFAULT_WAIT)
        .await
        .expect("the button shows Standard");
    alice.press("body", "Escape").await.expect("escape");
    alice
        .wait_for_count(".noise-card", 0, DEFAULT_WAIT)
        .await
        .expect("Escape closes the card");
    assert_eq!(
        alice.count(".voice-panel").await.expect("panel"),
        1,
        "Escape closed the card, not the call"
    );

    // Settings shows the same, and a choice there is the call's too.
    open_voice_settings(&mut alice).await;
    wait_active_mode(&mut alice, ".voice-noise", "standard").await;
    alice
        .click(".voice-noise .noise-mode[data-value=off]")
        .await
        .expect("off");
    wait_active_mode(&mut alice, ".voice-noise", "off").await;
    alice.press("body", "Escape").await.expect("close settings");
    wait_published(&mut alice, "off").await;
    alice
        .wait_for(&format!("{button}[data-mode=off]"), DEFAULT_WAIT)
        .await
        .expect("the button shows Off");
    alice.click(button).await.expect("open the card again");
    wait_active_mode(&mut alice, ".noise-card", "off").await;
    // A click away from the card closes it.
    alice
        .click_at(".noise-card-backdrop", 0.0, -250.0, 1)
        .await
        .expect("click away");
    alice
        .wait_for_count(".noise-card", 0, DEFAULT_WAIT)
        .await
        .expect("a click elsewhere closes the card");

    // Kept on the device, and after a restart.
    let saved = alice.data_dir().join("noise-suppression.json");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while !std::fs::read_to_string(&saved)
        .unwrap_or_default()
        .contains("off")
    {
        assert!(
            std::time::Instant::now() < deadline,
            "the choice was not kept"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let mut alice = alice.relaunch().await.expect("relaunch");
    alice.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    open_voice_settings(&mut alice).await;
    wait_active_mode(&mut alice, ".voice-noise", "off").await;
    // Strong again, for the call after.
    alice
        .click(".voice-noise .noise-mode[data-value=strong]")
        .await
        .expect("strong");
    wait_active_mode(&mut alice, ".voice-noise", "strong").await;
    alice.press("body", "Escape").await.expect("close settings");
}

/// The card's Mic Test: deafens you in the call (shown on the deafen
/// button), plays your processed microphone back and shows its level; Stop
/// puts deafen back as it was, also when you had deafened yourself.
#[tokio::test]
async fn the_mic_test_in_a_call_deafens_plays_you_back_and_restores_deafen() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    join_hangout(&mut alice, "alice").await;
    let card = ".noise-card";
    assert!(!deafened(&mut alice).await, "not deafened to begin with");
    alice
        .click(".voice-panel .noise-button")
        .await
        .expect("open the card");
    alice.wait_for(card, DEFAULT_WAIT).await.expect("the card");
    assert_eq!(
        meter_level(&mut alice, card).await,
        0.0,
        "no level before the test"
    );
    assert_eq!(loopback(&mut alice, card).await.as_deref(), Some("off"));

    alice
        .click(&format!("{card} .mic-test"))
        .await
        .expect("start the mic test");
    wait_deafened(&mut alice, true).await;
    wait_meter(&mut alice, card, Some(TONE_LEVEL)).await;
    assert_eq!(loopback(&mut alice, card).await.as_deref(), Some("on"));
    alice
        .wait_for_text(&format!("{card} .mic-test"), "Stop", DEFAULT_WAIT)
        .await
        .expect("the button stops the test");
    // The meter follows the signal while it runs, not a single reading.
    for _ in 0..10 {
        let level = meter_level(&mut alice, card).await;
        assert!(
            level > TONE_LEVEL && level < 0.6,
            "level {level} for the tone"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    // Stop: not deafened again, nothing played back, the meter at rest.
    alice
        .click(&format!("{card} .mic-test"))
        .await
        .expect("stop the mic test");
    wait_deafened(&mut alice, false).await;
    wait_meter(&mut alice, card, None).await;
    assert_eq!(loopback(&mut alice, card).await.as_deref(), Some("off"));
    assert_eq!(
        alice
            .count(".user-pill .mute-button.active")
            .await
            .expect("mute"),
        0,
        "unmuted as before the test"
    );

    // Deafened by hand first: the test leaves you deafened when it stops.
    alice.press("body", "Escape").await.expect("close the card");
    alice
        .wait_for_count(card, 0, DEFAULT_WAIT)
        .await
        .expect("closed");
    alice
        .click(".user-pill .deafen-button")
        .await
        .expect("deafen");
    wait_deafened(&mut alice, true).await;
    alice
        .click(".voice-panel .noise-button")
        .await
        .expect("open the card");
    alice
        .click(&format!("{card} .mic-test"))
        .await
        .expect("start");
    wait_meter(&mut alice, card, Some(TONE_LEVEL)).await;
    assert!(deafened(&mut alice).await);
    alice
        .click(&format!("{card} .mic-test"))
        .await
        .expect("stop");
    wait_meter(&mut alice, card, None).await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        deafened(&mut alice).await,
        "still deafened, as before the test"
    );
    alice.press("body", "Escape").await.expect("close the card");
    alice.click(".voice-panel .hang-up").await.expect("hang up");
}

/// Settings → Voice & Video has the same Mic Test, also outside a call; it
/// stops when the settings close.
#[tokio::test]
async fn the_mic_test_in_settings_works_outside_a_call() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    common::login(&mut alice, "alice", "alice-password").await;
    assert_eq!(
        alice.count(".voice-panel").await.expect("panel"),
        0,
        "not in a call"
    );
    open_voice_settings(&mut alice).await;
    let page = ".voice-noise";
    assert_eq!(meter_level(&mut alice, page).await, 0.0);
    alice
        .click(&format!("{page} .mic-test"))
        .await
        .expect("start");
    wait_meter(&mut alice, page, Some(TONE_LEVEL)).await;
    assert_eq!(loopback(&mut alice, page).await.as_deref(), Some("on"));
    alice
        .click(&format!("{page} .mic-test"))
        .await
        .expect("stop");
    wait_meter(&mut alice, page, None).await;
    assert_eq!(loopback(&mut alice, page).await.as_deref(), Some("off"));

    // Running when the settings close: it stops.
    alice
        .click(&format!("{page} .mic-test"))
        .await
        .expect("start again");
    wait_meter(&mut alice, page, Some(TONE_LEVEL)).await;
    alice.press("body", "Escape").await.expect("close settings");
    alice
        .wait_for_count(".settings-nav", 0, DEFAULT_WAIT)
        .await
        .expect("closed");
    open_voice_settings(&mut alice).await;
    wait_meter(&mut alice, page, None).await;
    assert_eq!(loopback(&mut alice, page).await.as_deref(), Some("off"));
}

/// `#rrggbb` as RGB.
fn rgb_of(colour: &str) -> Option<[u8; 3]> {
    let value = u32::from_str_radix(colour.trim_start_matches('#').get(..6)?, 16).ok()?;
    Some([(value >> 16) as u8, (value >> 8) as u8, value as u8])
}

/// WCAG's contrast ratio of two colours.
fn contrast_of(a: [u8; 3], b: [u8; 3]) -> f64 {
    let lum = |c: [u8; 3]| {
        let ch = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.039_28 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * ch(c[0]) + 0.7152 * ch(c[1]) + 0.0722 * ch(c[2])
    };
    let (la, lb) = (lum(a), lum(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The chosen option of a segmented chooser is white on the accent (a
/// contrast of 4.5 or more); the others are muted on nothing.
async fn check_segments(app: &mut App, option: &str, what: &str) {
    let active = format!("{option}.active");
    app.wait_for(&active, DEFAULT_WAIT).await.unwrap_or_else(|e| panic!("{what}: {e}"));
    let fg = app.colours_of(&active, "color").await.expect("fg").remove(0);
    let bg = app.colours_of(&active, "background").await.expect("bg").remove(0);
    let (Some(f), Some(b)) = (rgb_of(&fg), rgb_of(&bg)) else {
        panic!("{what}: the chosen option's colours are {fg:?} on {bg:?}");
    };
    let ratio = contrast_of(f, b);
    assert!(ratio >= 4.5, "{what}: the chosen option is {fg} on {bg}, a contrast of {ratio:.1}");
    assert_eq!(fg.to_lowercase(), "#ffffff", "{what}: the chosen option's text is {fg}");
    let others = format!("{option}:not(.active)");
    let fgs = app.colours_of(&others, "color").await.expect("fg");
    let bgs = app.colours_of(&others, "background").await.expect("bg");
    assert!(!fgs.is_empty(), "{what}: no other options");
    for (f, b) in fgs.iter().zip(&bgs) {
        assert!(b.is_empty(), "{what}: an unchosen option has a background {b:?}");
        assert!(rgb_of(f).is_some() && *f != fg, "{what}: an unchosen option's text is {f:?}");
    }
}

/// The segmented choosers (noise suppression in the call card and in
/// Settings, the screen share quality) show the chosen option readably.
#[tokio::test]
async fn the_chosen_option_of_a_segmented_chooser_can_be_read() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    if !alice.is_gpui() {
        return;
    }
    join_hangout(&mut alice, "alice").await;
    alice.click(".voice-panel .noise-button").await.expect("open the card");
    alice.wait_for(".noise-card", DEFAULT_WAIT).await.expect("the card");
    check_segments(&mut alice, ".noise-card .noise-mode", "the noise card").await;
    alice.click(".noise-card .noise-mode[data-value=standard]").await.expect("pick");
    check_segments(&mut alice, ".noise-card .noise-mode", "the noise card, Standard").await;
    alice.click_at(".noise-card-backdrop", 0.0, -250.0, 1).await.expect("close");
    alice.wait_for_count(".noise-card", 0, DEFAULT_WAIT).await.expect("closed");

    alice.click(".voice-panel .screen-button").await.expect("share");
    alice.wait_for(".screen-quality", DEFAULT_WAIT).await.expect("the popover");
    check_segments(&mut alice, ".screen-quality .screen-res", "the screen resolution").await;
    check_segments(&mut alice, ".screen-quality .screen-fps", "the screen frame rate").await;
    alice.press("body", "Escape").await.expect("close the popover");

    open_voice_settings(&mut alice).await;
    check_segments(&mut alice, ".voice-noise .noise-mode", "Settings").await;
}

/// The mic test button (white text) is readable on its fill: a contrast of
/// 4.5 or more, in the dark themes too.
#[tokio::test]
async fn the_mic_test_button_is_readable() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    if !alice.is_gpui() {
        return;
    }
    common::login(&mut alice, "alice", "alice-password").await;
    open_voice_settings(&mut alice).await;
    alice.wait_for(".voice-noise .mic-test", DEFAULT_WAIT).await.expect("the button");
    let fg = alice.attr(".voice-noise .mic-test", "data-color").await.expect("fg").expect("fg");
    let bg = alice.attr(".voice-noise .mic-test", "data-bg").await.expect("bg").expect("bg");
    let (Some(f), Some(b)) = (rgb_of(&fg), rgb_of(&bg)) else {
        panic!("colours {fg:?} on {bg:?}");
    };
    let ratio = contrast_of(f, b);
    assert!(ratio >= 4.5, "the mic test is {fg} on {bg}, a contrast of {ratio:.1}");
}
