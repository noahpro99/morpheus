//! The bottom-left voice controls: the microphone button's right-click menu
//! is about input only (device, volume), the headphones button's about output
//! only; "Voice Settings" has the gear. Volume sliders (those menus and
//! Settings > Voice & Video) have a round handle to grab, and a double click
//! sets them back to 100%.

#![allow(clippy::expect_used)]

mod common;

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn launch(hs: &str) -> Option<App> {
    let mut app = App::launch_desktop(hs).await.expect("launch");
    if !app.is_gpui() {
        eprintln!("the voice menus are in the GPUI app only; skipping");
        return None;
    }
    common::login(&mut app, "alice", "alice-password").await;
    Some(app)
}

/// A slider has a handle (a visible knob at the end of its fill), and a
/// double click puts it back at 100%.
async fn check_slider(app: &mut App, slider: &str, handle: &str) {
    app.wait_for(slider, DEFAULT_WAIT)
        .await
        .expect("the slider");
    app.wait_for(handle, DEFAULT_WAIT)
        .await
        .expect("a handle on the slider");
    let knob = app
        .bounds(handle)
        .await
        .expect("bounds")
        .expect("handle bounds");
    assert!(
        knob.w >= 10.0 && (knob.w - knob.h).abs() < 1.0,
        "a round knob: {knob:?}"
    );
    let bar = app
        .bounds(slider)
        .await
        .expect("bounds")
        .expect("bar bounds");
    // One click a quarter bar left of the middle: 50%.
    app.click_at(slider, (-0.25 * bar.w) as f32, 0.0, 1)
        .await
        .expect("click");
    wait_value(app, slider, "50").await;
    let knob = app
        .bounds(handle)
        .await
        .expect("bounds")
        .expect("handle bounds");
    let at = (knob.x + knob.w / 2.0 - bar.x) / bar.w;
    assert!(
        (at - 0.25).abs() < 0.04,
        "the handle sits at the value: {at}"
    );
    app.click_at(slider, (-0.25 * bar.w) as f32, 0.0, 2)
        .await
        .expect("double click");
    wait_value(app, slider, "100").await;
}

async fn wait_value(app: &mut App, slider: &str, want: &str) {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let got = app.attr(slider, "data-value").await.expect("attr");
        if got.as_deref() == Some(want) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{slider} at {got:?}, wanted {want}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn the_microphone_menu_is_input_only_with_a_handled_slider() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let Some(mut app) = launch(&hs).await else {
        return;
    };
    app.right_click(".user-pill .mute-button")
        .await
        .expect("right-click");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("menu");
    app.wait_for(".context-slider.input-volume", DEFAULT_WAIT)
        .await
        .expect("input volume");
    let text = app
        .text(".context-menu")
        .await
        .expect("text")
        .unwrap_or_default();
    assert!(text.contains("Input Device"), "{text}");
    assert!(!text.contains("Output"), "an input menu: {text}");
    assert_eq!(
        app.count(".context-slider.output-volume")
            .await
            .expect("count"),
        0
    );
    let icons = app
        .attrs(".context-item", "data-icon")
        .await
        .expect("icons");
    assert_eq!(
        icons.last().cloned().flatten().as_deref(),
        Some("settings"),
        "Voice Settings has the gear: {icons:?}"
    );
    check_slider(
        &mut app,
        ".context-slider.input-volume .menu-slider",
        ".context-slider.input-volume .slider-handle",
    )
    .await;
}

#[tokio::test]
async fn the_headphones_menu_is_output_only_with_a_handled_slider() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let Some(mut app) = launch(&hs).await else {
        return;
    };
    app.right_click(".user-pill .deafen-button")
        .await
        .expect("right-click");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("menu");
    app.wait_for(".context-slider.output-volume", DEFAULT_WAIT)
        .await
        .expect("output volume");
    let text = app
        .text(".context-menu")
        .await
        .expect("text")
        .unwrap_or_default();
    assert!(text.contains("Output Device"), "{text}");
    assert!(!text.contains("Input"), "an output menu: {text}");
    assert_eq!(
        app.count(".context-slider.input-volume")
            .await
            .expect("count"),
        0
    );
    let icons = app
        .attrs(".context-item", "data-icon")
        .await
        .expect("icons");
    assert_eq!(
        icons.last().cloned().flatten().as_deref(),
        Some("settings"),
        "Voice Settings has the gear: {icons:?}"
    );
    check_slider(
        &mut app,
        ".context-slider.output-volume .menu-slider",
        ".context-slider.output-volume .slider-handle",
    )
    .await;
}

#[tokio::test]
async fn the_settings_volume_sliders_have_handles_and_reset_on_double_click() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let Some(mut app) = launch(&hs).await else {
        return;
    };
    app.click(".settings-button").await.expect("gear");
    app.wait_for_text(".settings-nav button", "Voice & Video", DEFAULT_WAIT)
        .await
        .expect("settings");
    app.click_text(".settings-nav button", "Voice & Video")
        .await
        .expect("voice page");
    for kind in ["input-volume", "output-volume"] {
        check_slider(
            &mut app,
            &format!(".volume-slider.{kind}"),
            &format!(".settings-page .{kind} .slider-handle, .volume-slider.{kind} .slider-handle"),
        )
        .await;
    }
}
