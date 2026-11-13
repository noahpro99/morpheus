//! The video player in messages (GPUI only; the Dioxus page uses the web
//! view's `<video>`): play in place, the controls, the full-window player,
//! GIF-like clips, encrypted rooms and formats it cannot play.
//!
//! The player (`.video-player`) says where it is for tests: `data-time` and
//! `data-duration` (ms), `data-frame` (pictures shown so far), `data-playing`,
//! `data-muted`, `data-volume` (0-100), `data-audio` (`on`, `muted`, `none`
//! for a silent file, `off` while paused), `data-loop`, `data-loops` and
//! `data-autoplay`.

#![allow(clippy::expect_used)]

mod common;

use std::time::{Duration, Instant};

use morpheus_e2e::{App, DEFAULT_WAIT, VideoMessage, api_as, new_account};

const AV: &[u8] = include_bytes!("../../video/fixtures/av.mp4");
const AV_THUMB: &[u8] = include_bytes!("../../video/fixtures/av-thumb.png");
const LOOP: &[u8] = include_bytes!("../../video/fixtures/loop.mp4");
const LOOP_THUMB: &[u8] = include_bytes!("../../video/fixtures/loop-thumb.png");
const WEBM: &[u8] = include_bytes!("../../video/fixtures/clip.webm");

const PLAYER: &str = ".video-card .video-player";

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

fn av(body: &str) -> VideoMessage<'_> {
    VideoMessage {
        bytes: AV,
        mime: "video/mp4",
        body,
        size: Some((320, 240)),
        duration_ms: Some(6000),
        thumbnail_png: Some(AV_THUMB),
    }
}

/// A new room shared by two new people, `videos` sent into it by the second,
/// then `texts` messages after them; the first signed in with it open.
/// `None` on Dioxus.
async fn room_with(hs: &str, videos: &[VideoMessage<'_>], texts: usize, encrypted: bool) -> Option<(App, String, String)> {
    let (ann, ben) = (new_account(hs, "vidann").expect("account"), new_account(hs, "vidben").expect("account"));
    let mut create = serde_json::json!({ "name": "videoroom", "preset": "public_chat" });
    if encrypted {
        create["initial_state"] = serde_json::json!([{
            "type": "m.room.encryption",
            "state_key": "",
            "content": { "algorithm": "m.megolm.v1.aes-sha2" }
        }]);
    }
    let made = api_as(hs, &ann, "POST", "createRoom", Some(create)).expect("a room");
    let room = made["room_id"].as_str().expect("room id").to_owned();
    api_as(
        hs,
        &ben,
        "POST",
        &format!("join/{}", room.replace('!', "%21").replace(':', "%3A")),
        Some(serde_json::json!({})),
    )
    .expect("join");
    for video in videos {
        morpheus_e2e::send_video_as(hs, &ben, &room, video).expect("a video");
    }
    let texts: Vec<String> = (0..texts).map(|i| format!("chatter {i}")).collect();
    if !texts.is_empty() {
        morpheus_e2e::send_many_as(hs, &ben, &room, &texts).expect("messages");
    }
    let mut app = App::launch_desktop(hs).await.expect("launch app");
    if !app.is_gpui() {
        eprintln!("the video player is built in the GPUI app only; skipping");
        return None;
    }
    common::login(&mut app, &ann, &format!("{ann}-password")).await;
    app.open_room("videoroom").await.expect("open room");
    Some((app, room, ben))
}

async fn num(app: &mut App, selector: &str, name: &str) -> f64 {
    app.attr(selector, name)
        .await
        .expect("attr")
        .unwrap_or_else(|| panic!("no {name} on {selector}"))
        .parse()
        .unwrap_or_else(|_| panic!("{name} on {selector} is not a number"))
}

/// Waits until `name` on `selector` passes `ok`, returning it.
async fn wait_num(app: &mut App, selector: &str, name: &str, what: &str, ok: impl Fn(f64) -> bool) -> f64 {
    let deadline = Instant::now() + DEFAULT_WAIT;
    let mut last = None;
    while Instant::now() < deadline {
        if let Ok(Some(value)) = app.attr(selector, name).await
            && let Ok(value) = value.parse::<f64>()
        {
            if ok(value) {
                return value;
            }
            last = Some(value);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{what}: {name} on {selector} was {last:?}");
}

/// Clicks the play button of the (only) video and waits until it plays.
async fn play(app: &mut App) {
    app.reveal_element(".video-card .play-button", DEFAULT_WAIT).await.expect("the video");
    app.click(".video-card .play-button").await.expect("play");
    app.wait_for(&format!("{PLAYER}[data-playing=\"1\"]"), DEFAULT_WAIT)
        .await
        .expect("it plays");
}

/// The controls show while the pointer is over the player.
async fn controls(app: &mut App) {
    app.hover(PLAYER).await.expect("hover");
    app.wait_for(".video-card .video-controls", DEFAULT_WAIT).await.expect("the controls");
}

#[tokio::test]
async fn a_video_plays_in_the_message_and_its_controls_work() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let Some((mut app, _, _)) = room_with(&hs, &[av("sine.mp4")], 0, false).await else { return };
    // Before playing: the thumbnail, a play button and the length.
    app.wait_for(".video-card img.video-thumb", DEFAULT_WAIT).await.expect("thumbnail");
    app.wait_for_text(".video-card .video-duration", "0:06", DEFAULT_WAIT)
        .await
        .expect("length");
    assert_eq!(app.count(PLAYER).await.expect("count"), 0, "nothing decoded before play");

    play(&mut app).await;
    assert_eq!(app.count(".video-card .play-button").await.expect("count"), 0, "the button gives way");
    // Pictures and time move on together.
    let frame = wait_num(&mut app, PLAYER, "data-frame", "pictures shown", |f| f >= 3.0).await;
    let time = num(&mut app, PLAYER, "data-time").await;
    tokio::time::sleep(Duration::from_millis(600)).await;
    let (frame2, time2) = (num(&mut app, PLAYER, "data-frame").await, num(&mut app, PLAYER, "data-time").await);
    assert!(frame2 >= frame + 8.0, "pictures advance: {frame} -> {frame2} in 0.6 s (30 fps)");
    assert!(time2 - time > 400.0 && time2 - time < 900.0, "time advances with the clock: {time} -> {time2}");
    let length = num(&mut app, PLAYER, "data-duration").await;
    assert!((length - 6000.0).abs() < 100.0, "6 s long: {length}");
    assert_eq!(app.attr(PLAYER, "data-audio").await.expect("attr").as_deref(), Some("on"), "playing with sound");

    // The controls: time shown, pause stops everything.
    controls(&mut app).await;
    if std::env::var_os("MORPHEUS_E2E_SHOTS").is_some() {
        let _ = app.screenshot("video-controls").await;
    }
    let shown = app.text(".video-card .video-time").await.expect("text").unwrap_or_default();
    assert!(shown.ends_with("/ 0:06"), "time / length: {shown:?}");
    app.click(".video-card .video-toggle").await.expect("pause");
    app.wait_for(&format!("{PLAYER}[data-playing=\"0\"]"), DEFAULT_WAIT).await.expect("paused");
    let (frame, time) = (num(&mut app, PLAYER, "data-frame").await, num(&mut app, PLAYER, "data-time").await);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(num(&mut app, PLAYER, "data-frame").await, frame, "no pictures while paused");
    assert_eq!(num(&mut app, PLAYER, "data-time").await, time, "the clock stops");
    assert_eq!(app.attr(PLAYER, "data-audio").await.expect("attr").as_deref(), Some("off"));

    // Seek: a click in the middle of the bar lands near 3 s, the picture too.
    controls(&mut app).await;
    app.click(".video-card .video-seek").await.expect("seek to the middle");
    let at = wait_num(&mut app, PLAYER, "data-time", "seeked", |t| (t - 3000.0).abs() < 250.0).await;
    wait_num(&mut app, PLAYER, "data-frame-time", "the picture follows", |t| (t - at).abs() < 100.0).await;
    let played = app.attr(".video-card .video-seek-played", "data-fraction").await.expect("attr");
    assert!(
        played.as_deref().and_then(|p| p.parse::<f64>().ok()).is_some_and(|p| (p - 0.5).abs() < 0.05),
        "the bar fills to the middle: {played:?}"
    );
    assert_eq!(
        app.attr(".video-card .video-seek-buffered", "data-fraction").await.expect("attr").as_deref(),
        Some("1"),
        "the whole file is here"
    );
    let time = app.text(".video-card .video-time").await.expect("text").unwrap_or_default();
    assert!(time.starts_with("0:03") || time.starts_with("0:02"), "time shown after seeking: {time:?}");

    // Mute and unmute while playing.
    app.click(".video-card .video-toggle").await.expect("play again");
    app.wait_for(&format!("{PLAYER}[data-playing=\"1\"]"), DEFAULT_WAIT).await.expect("playing");
    controls(&mut app).await;
    app.click(".video-card .video-mute").await.expect("mute");
    app.wait_for(&format!("{PLAYER}[data-muted=\"1\"][data-audio=\"muted\"]"), DEFAULT_WAIT)
        .await
        .expect("muted");
    controls(&mut app).await;
    app.click(".video-card .video-mute").await.expect("unmute");
    app.wait_for(&format!("{PLAYER}[data-muted=\"0\"][data-audio=\"on\"]"), DEFAULT_WAIT)
        .await
        .expect("sound again");
    // The volume slider: a click near its left end turns it down.
    controls(&mut app).await;
    let bar = app.bounds(".video-card .video-volume").await.expect("bounds").expect("volume slider");
    app.click_at(".video-card .video-volume", (-bar.w * 0.3) as f32, 0.0, 1).await.expect("volume");
    wait_num(&mut app, PLAYER, "data-volume", "turned down", |v| v > 5.0 && v < 40.0).await;
    // The download button is there too.
    controls(&mut app).await;
    assert_eq!(app.count(".video-card .video-download").await.expect("count"), 1, "download offered");

    // Space on the focused player pauses and plays.
    app.press(PLAYER, "Space").await.expect("space");
    app.wait_for(&format!("{PLAYER}[data-playing=\"0\"]"), DEFAULT_WAIT).await.expect("space pauses");
    app.press(PLAYER, "Space").await.expect("space");
    app.wait_for(&format!("{PLAYER}[data-playing=\"1\"]"), DEFAULT_WAIT).await.expect("space plays");
}

#[tokio::test]
async fn fullscreen_opens_the_big_player_and_escape_closes_it() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let Some((mut app, _, _)) = room_with(&hs, &[av("big.mp4")], 0, false).await else { return };
    play(&mut app).await;
    let small = app.bounds(PLAYER).await.expect("bounds").expect("player");
    controls(&mut app).await;
    app.click(".video-card .video-fullscreen").await.expect("fullscreen");
    let big_player = ".video-viewer .video-player";
    app.wait_for(big_player, DEFAULT_WAIT).await.expect("the full-window player");
    if std::env::var_os("MORPHEUS_E2E_SHOTS").is_some() {
        let _ = app.screenshot("video-fullscreen").await;
    }
    let window = app.bounds("body").await.expect("bounds").expect("window");
    let big = app.bounds(big_player).await.expect("bounds").expect("big player");
    assert!(
        big.w > small.w * 1.5 && (big.w > window.w * 0.6 || big.h > window.h * 0.6),
        "fills the window: {big:?} in {window:?} (was {small:?})"
    );
    // It goes on playing where it was.
    let frame = num(&mut app, big_player, "data-frame").await;
    wait_num(&mut app, big_player, "data-frame", "still playing", |f| f > frame + 5.0).await;
    // Arrows seek 5 s; Space pauses.
    app.press(big_player, "Space").await.expect("space");
    app.wait_for(&format!("{big_player}[data-playing=\"0\"]"), DEFAULT_WAIT).await.expect("paused");
    let t = num(&mut app, big_player, "data-time").await;
    app.press(big_player, "ArrowRight").await.expect("right");
    wait_num(&mut app, big_player, "data-time", "5 s on", |x| (x - (t + 5000.0).min(6000.0)).abs() < 150.0).await;
    app.press(big_player, "ArrowLeft").await.expect("left");
    wait_num(&mut app, big_player, "data-time", "5 s back", |x| (x - (t + 5000.0).min(6000.0) + 5000.0).abs() < 150.0).await;
    // Escape closes it; the message's player is back.
    app.press(big_player, "Escape").await.expect("escape");
    app.wait_for_count(".video-viewer", 0, DEFAULT_WAIT).await.expect("closed");
    app.wait_for(PLAYER, DEFAULT_WAIT).await.expect("the player in the message");
}

#[tokio::test]
async fn a_short_silent_video_plays_by_itself_muted_and_loops() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let gif = VideoMessage {
        bytes: LOOP,
        mime: "video/mp4",
        body: "loop.mp4",
        size: Some((320, 240)),
        duration_ms: Some(2000),
        thumbnail_png: Some(LOOP_THUMB),
    };
    let Some((mut app, _, _)) = room_with(&hs, &[gif], 0, false).await else { return };
    let autoplay = format!("{PLAYER}[data-autoplay=\"1\"][data-loop=\"1\"][data-playing=\"1\"]");
    app.wait_for(&autoplay, DEFAULT_WAIT).await.expect("plays without a click");
    assert_eq!(app.count(".video-card .play-button").await.expect("count"), 0, "no play button");
    assert_eq!(app.attr(PLAYER, "data-audio").await.expect("attr").as_deref(), Some("none"), "silent");
    assert_eq!(app.attr(PLAYER, "data-muted").await.expect("attr").as_deref(), Some("1"), "muted");
    wait_num(&mut app, PLAYER, "data-loops", "went round", |n| n >= 1.0).await;
    assert_eq!(app.attr(PLAYER, "data-playing").await.expect("attr").as_deref(), Some("1"), "and goes on");
}

#[tokio::test]
async fn a_video_in_an_encrypted_room_plays() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let Some((mut app, _, _)) = room_with(&hs, &[], 0, true).await else { return };
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer");
    app.attach_file(false, "secret.mp4", "video/mp4", AV).await.expect("paste");
    app.wait_for(".composer .composer-attachment", DEFAULT_WAIT).await.expect("attached");
    app.press(".composer .composer-input", "Enter").await.expect("send");
    app.wait_for(".video-card .play-button", DEFAULT_WAIT).await.expect("the sent video");
    play(&mut app).await;
    wait_num(&mut app, PLAYER, "data-frame", "pictures decrypted and shown", |f| f >= 10.0).await;
    let length = num(&mut app, PLAYER, "data-duration").await;
    assert!((length - 6000.0).abs() < 100.0, "6 s long: {length}");
}

#[tokio::test]
async fn an_unsupported_format_offers_the_download() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let webm = VideoMessage {
        bytes: WEBM,
        mime: "video/webm",
        body: "clip.webm",
        size: Some((320, 240)),
        duration_ms: Some(2000),
        thumbnail_png: None,
    };
    let Some((mut app, _, _)) = room_with(&hs, &[webm], 0, false).await else { return };
    // Short, so it is looked at before any click: it cannot play.
    app.wait_for_text(".video-card .video-unsupported", "Can't play this format", DEFAULT_WAIT)
        .await
        .expect("says so");
    app.click(".video-card .video-download").await.expect("download");
    let saved = app.download_dir().join("clip.webm");
    let deadline = Instant::now() + DEFAULT_WAIT;
    while !saved.exists() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(std::fs::read(&saved).expect("downloaded"), WEBM, "the file itself");
}

#[tokio::test]
async fn one_video_plays_with_sound_and_one_scrolled_away_pauses() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    // Messages after the videos, so they can be scrolled out of view.
    let Some((mut app, _, _)) = room_with(&hs, &[av("first.mp4"), av("second.mp4")], 40, false).await else { return };
    let card = |name: &str| format!(".video-card[data-body=\"{name}\"]");
    app.reveal_element(&format!("{} .play-button", card("first.mp4")), DEFAULT_WAIT).await.expect("first");
    app.reveal_element(&format!("{} .play-button", card("second.mp4")), DEFAULT_WAIT).await.expect("second");
    // Both in view: the first is paused by the second, not by scrolling.
    let view = app.bounds(".timeline").await.expect("bounds").expect("timeline");
    let b = app.bounds(&card("second.mp4")).await.expect("bounds").expect("card");
    let below = (b.y + b.h) - (view.y + view.h);
    if below > 0.0 {
        let state = app.scroll_state(".timeline").await.expect("scroll").expect("timeline");
        app.scroll_to(".timeline", morpheus_e2e::ScrollTo::FromBottom(state.from_bottom - below - 20.0))
            .await
            .expect("scroll a little");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    for name in ["first.mp4", "second.mp4"] {
        let b = app.bounds(&card(name)).await.expect("bounds").expect("card");
        assert!(b.y >= view.y && b.y + b.h <= view.y + view.h, "{name} in view: {b:?} in {view:?}");
    }
    app.click(&format!("{} .play-button", card("first.mp4"))).await.expect("play first");
    app.wait_for(&format!("{} .video-player[data-playing=\"1\"]", card("first.mp4")), DEFAULT_WAIT)
        .await
        .expect("first plays");
    app.click(&format!("{} .play-button", card("second.mp4"))).await.expect("play second");
    app.wait_for(&format!("{} .video-player[data-playing=\"1\"]", card("second.mp4")), DEFAULT_WAIT)
        .await
        .expect("second plays");
    app.wait_for(&format!("{} .video-player[data-playing=\"0\"]", card("first.mp4")), DEFAULT_WAIT)
        .await
        .expect("the first one paused for it");
    let second = format!("{} .video-player", card("second.mp4"));
    assert_eq!(app.attr(&second, "data-playing").await.expect("attr").as_deref(), Some("1"), "the second plays on");

    // Scrolled out of view, the second pauses too, and is still paused when
    // back in view.
    let started = num(&mut app, &second, "data-time").await;
    app.scroll_to(".timeline", morpheus_e2e::ScrollTo::Bottom).await.expect("scroll down");
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(app.count(&second).await.expect("count"), 0, "out of view");
    app.reveal_element(&second, DEFAULT_WAIT).await.expect("back to the video");
    assert_eq!(app.attr(&second, "data-playing").await.expect("attr").as_deref(), Some("0"), "paused out of view");
    let t = num(&mut app, &second, "data-time").await;
    assert!(t - started < 1200.0, "it stopped when it left the view: {started} -> {t} ms");
}

/// CPU of the whole app while a video of your own plays in a message (not
/// run by default: `MORPHEUS_VIDEO_BENCH=path.mp4 just e2e video_player
/// cpu -- --ignored`): the app's CPU time over 5 s paused, then 5 s playing,
/// as a share of one core.
#[tokio::test]
#[ignore = "needs MORPHEUS_VIDEO_BENCH"]
async fn cpu_while_a_video_plays() {
    let (Some(hs), Ok(path)) = (homeserver(), std::env::var("MORPHEUS_VIDEO_BENCH")) else { return };
    let bytes = std::fs::read(&path).expect("the video");
    let video = VideoMessage {
        bytes: &bytes,
        mime: "video/mp4",
        body: "bench.mp4",
        size: None,
        duration_ms: None,
        thumbnail_png: None,
    };
    let Some((mut app, _, _)) = room_with(&hs, &[video], 0, false).await else { return };
    let pid = app.pid().expect("pid");
    let ticks = || -> f64 {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).expect("stat");
        let fields: Vec<&str> = stat.rsplit_once(')').expect("stat").1.split_whitespace().collect();
        fields[11].parse::<f64>().expect("utime") + fields[12].parse::<f64>().expect("stime")
    };
    let hz = 100.0;
    app.reveal_element(".video-card .play-button", DEFAULT_WAIT).await.expect("the video");
    let measure = |from: f64, to: f64, secs: f64| (to - from) / hz / secs * 100.0;
    let t0 = ticks();
    tokio::time::sleep(Duration::from_secs(5)).await;
    let idle = measure(t0, ticks(), 5.0);
    play(&mut app).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let (f0, t0) = (num(&mut app, PLAYER, "data-frame").await, ticks());
    tokio::time::sleep(Duration::from_secs(5)).await;
    let (f1, t1) = (num(&mut app, PLAYER, "data-frame").await, ticks());
    eprintln!(
        "CPU: {idle:.1}% of a core idle, {:.1}% playing; {:.1} pictures/s shown",
        measure(t0, t1, 5.0),
        (f1 - f0) / 5.0
    );
}
