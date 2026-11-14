#![allow(clippy::expect_used)]
//! Media through a real LiveKit server (the dev one from `just hs-up`):
//! what one participant says, another hears, and muting stops it.
//! Self-skips unless `MORPHEUS_TEST_HOMESERVER` is set (the call services
//! come up with the homeserver); `MORPHEUS_TEST_LIVEKIT` overrides the URL.

use std::time::Duration;

use livekit_api::access_token::{AccessToken, VideoGrants};
use morpheus_call::{AudioMode, Call, CallEvent};
use tokio::{sync::mpsc::UnboundedReceiver, time::timeout};

fn livekit() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()?;
    Some(std::env::var("MORPHEUS_TEST_LIVEKIT").unwrap_or_else(|_| "ws://localhost:7880".into()))
}

/// The dev server's keys (`livekit-server --dev`).
fn token(room: &str, identity: &str) -> String {
    AccessToken::with_api_key("devkey", "secret")
        .with_identity(identity)
        .with_grants(VideoGrants {
            room_join: true,
            room: room.into(),
            ..Default::default()
        })
        .to_jwt()
        .expect("token")
}

/// Wait for an event matching `want`, skipping others.
async fn wait_for(
    events: &mut UnboundedReceiver<CallEvent>,
    what: &str,
    mut want: impl FnMut(&CallEvent) -> bool,
) {
    timeout(Duration::from_secs(20), async {
        loop {
            let event = events.recv().await.expect("call events end");
            if want(&event) {
                return;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
}

#[tokio::test]
async fn one_participant_hears_the_other_until_they_mute() {
    let Some(url) = livekit() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping media test");
        return;
    };
    let room = format!("media-test-{}", std::process::id());

    let (speaker, mut speaker_events) =
        Call::connect(&url, &token(&room, "speaker"), AudioMode::Tone)
            .await
            .expect("speaker connects");
    let (listener, mut events) = Call::connect(&url, &token(&room, "listener"), AudioMode::Tone)
        .await
        .expect("listener connects");

    // Each sees the other arrive.
    wait_for(
        &mut speaker_events,
        "the listener joining",
        |e| matches!(e, CallEvent::Joined(who) if who == "listener"),
    )
    .await;
    assert!(listener.participants().contains(&"speaker".to_owned()));

    // The listener hears the speaker's tone.
    wait_for(
        &mut events,
        "the speaker's audio",
        |e| matches!(e, CallEvent::Heard(who) if who == "speaker"),
    )
    .await;

    // Muted, they go quiet.
    speaker.set_muted(true);
    wait_for(
        &mut events,
        "the speaker going quiet",
        |e| matches!(e, CallEvent::Quiet(who) if who == "speaker"),
    )
    .await;

    // Leaving tells the others.
    speaker.leave().await;
    wait_for(
        &mut events,
        "the speaker leaving",
        |e| matches!(e, CallEvent::Left(who) if who == "speaker"),
    )
    .await;
    listener.leave().await;
}

#[tokio::test]
async fn encrypted_audio_is_heard_only_with_the_speakers_key() {
    let Some(url) = livekit() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping media test");
        return;
    };
    let room = format!("e2ee-test-{}", std::process::id());
    let speaker_key = b"0123456789abcdef".to_vec();

    // The others are there first, listening from the speaker's very first frame.
    let (friend, mut friend_events) = Call::connect_encrypted(
        &url,
        &token(&room, "friend"),
        AudioMode::Tone,
        b"fedcba9876543210".to_vec(),
    )
    .await
    .expect("friend connects");
    // The snoop runs in its own process: WebRTC shares media keys between
    // calls in one process, so an in-process snoop would borrow the friend's.
    let snoop = tokio::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            "snoop_without_the_key",
            "--ignored",
            "--nocapture",
        ])
        .env("MORPHEUS_SNOOP_ROOM", &room)
        .env("MORPHEUS_SNOOP_URL", &url)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("snoop process");
    // Let it join before the speaker publishes.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let (speaker, _speaker_events) = Call::connect_encrypted(
        &url,
        &token(&room, "speaker"),
        AudioMode::Tone,
        speaker_key.clone(),
    )
    .await
    .expect("speaker connects");

    // The friend was given the speaker's key (over Matrix, in the app).
    friend.set_key("speaker", speaker_key);
    wait_for(
        &mut friend_events,
        "the speaker's audio, decrypted",
        |e| matches!(e, CallEvent::Heard(who) if who == "speaker"),
    )
    .await;

    // Without it, the speaker's audio never decrypts into anything.
    let snooped = snoop.wait_with_output().await.expect("snoop finishes");
    let said = String::from_utf8_lossy(&snooped.stdout);
    assert!(
        said.contains("SNOOP SILENT"),
        "audio heard without the key: {said}"
    );
    speaker.leave().await;
}

/// Run by `encrypted_audio_is_heard_only_with_the_speakers_key` in its own
/// process: joins with a key of its own and reports whether it ever hears
/// the speaker's tone.
#[tokio::test]
#[ignore = "helper, run by the encrypted media test"]
async fn snoop_without_the_key() {
    let (Ok(room), Ok(url)) = (
        std::env::var("MORPHEUS_SNOOP_ROOM"),
        std::env::var("MORPHEUS_SNOOP_URL"),
    ) else {
        return;
    };
    let (_snoop, mut events) = Call::connect_encrypted(
        &url,
        &token(&room, "snoop"),
        AudioMode::Tone,
        b"snoopsnoopsnoop!".to_vec(),
    )
    .await
    .expect("snoop connects");
    let heard = timeout(Duration::from_secs(8), async {
        loop {
            match events.recv().await {
                Some(CallEvent::Heard(who)) if who == "speaker" => return,
                Some(_) => {}
                None => std::future::pending::<()>().await,
            }
        }
    })
    .await;
    println!(
        "{}",
        if heard.is_ok() {
            "SNOOP HEARD"
        } else {
            "SNOOP SILENT"
        }
    );
}

/// The colour at `(x, y)` of a JPEG, as (r, g, b).
fn pixel(jpeg: &[u8], x: usize, y: usize) -> (u8, u8, u8) {
    let mut decoder = zune_jpeg::JpegDecoder::new(std::io::Cursor::new(jpeg));
    let rgb = decoder.decode().expect("a JPEG");
    let (width, _) = decoder.dimensions().expect("dimensions");
    let at = (y * width + x) * 3;
    (rgb[at], rgb[at + 1], rgb[at + 2])
}

#[tokio::test]
async fn a_camera_turned_on_is_seen_by_the_other_side_until_turned_off() {
    let Some(url) = livekit() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping media test");
        return;
    };
    let room = format!("video-test-{}", std::process::id());
    let (alice_key, bob_key) = (b"0123456789abcdef".to_vec(), b"fedcba9876543210".to_vec());
    let (viewer, mut events) =
        Call::connect_encrypted(&url, &token(&room, "viewer"), AudioMode::Tone, bob_key)
            .await
            .expect("viewer connects");
    let (camera, _camera_events) = Call::connect_encrypted(
        &url,
        &token(&room, "camera"),
        AudioMode::Tone,
        alice_key.clone(),
    )
    .await
    .expect("camera connects");
    viewer.set_key("camera", alice_key);

    camera.set_camera(true).await.expect("camera on");
    assert!(camera.camera_on());
    // This device shows what it sends.
    timeout(Duration::from_secs(5), async {
        while morpheus_call::latest_frame(morpheus_call::LOCAL_CAMERA).is_none() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("a self view");

    let mut key = String::new();
    wait_for(&mut events, "the camera's video", |e| match e {
        CallEvent::Video {
            identity,
            key: k,
            screen: false,
            on: true,
        } if identity == "camera" => {
            key = k.clone();
            true
        }
        _ => false,
    })
    .await;
    // The picture arrives, decrypted: red on the left, blue on the right.
    let frame = timeout(Duration::from_secs(20), async {
        loop {
            if let Some(frame) = morpheus_call::latest_frame(&key) {
                let (w, h) = (frame.width as usize, frame.height as usize);
                let (left, right) = (
                    pixel(&frame.jpeg, w / 4, h / 2),
                    pixel(&frame.jpeg, 3 * w / 4, h / 2),
                );
                if left.0 > 150 && left.2 < 100 && right.2 > 150 && right.0 < 100 {
                    return frame;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the test pattern, decrypted, on the far side");
    let pattern = morpheus_call::CAMERA_PATTERN;
    // Shown no wider than a web view gets (640), in the camera's own shape.
    assert!(frame.width <= pattern.width);
    assert!(
        (frame.width * pattern.height).abs_diff(frame.height * pattern.width)
            <= pattern.width,
        "{}x{} is not the camera's shape",
        frame.width,
        frame.height
    );

    // Off, it goes away on both sides.
    camera.set_camera(false).await.expect("camera off");
    assert!(!camera.camera_on());
    assert!(morpheus_call::latest_frame(morpheus_call::LOCAL_CAMERA).is_none());
    wait_for(
        &mut events,
        "the camera's video stopping",
        |e| matches!(e, CallEvent::Video { identity, on: false, .. } if identity == "camera"),
    )
    .await;
    assert!(morpheus_call::latest_frame(&key).is_none());
    camera.leave().await;
    viewer.leave().await;
}

#[tokio::test]
async fn a_shared_screen_is_seen_as_a_screen_by_the_other_side() {
    let Some(url) = livekit() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping media test");
        return;
    };
    let room = format!("screen-test-{}", std::process::id());
    let (viewer, mut events) = Call::connect(&url, &token(&room, "viewer"), AudioMode::Tone)
        .await
        .expect("viewer connects");
    let (sharer, _sharer_events) = Call::connect(&url, &token(&room, "sharer"), AudioMode::Tone)
        .await
        .expect("sharer connects");

    sharer.set_screen_share(true).await.expect("share");
    assert!(sharer.sharing_screen());
    assert!(!sharer.camera_on(), "the screen is not the camera");
    let mut key = String::new();
    wait_for(&mut events, "the shared screen", |e| match e {
        CallEvent::Video {
            identity,
            key: k,
            screen: true,
            on: true,
        } if identity == "sharer" => {
            key = k.clone();
            true
        }
        _ => false,
    })
    .await;
    // Green on the left, yellow on the right.
    timeout(Duration::from_secs(20), async {
        loop {
            if let Some(frame) = morpheus_call::latest_frame(&key) {
                let (w, h) = (frame.width as usize, frame.height as usize);
                let left = pixel(&frame.jpeg, w / 4, h / 2);
                let right = pixel(&frame.jpeg, 3 * w / 4, h / 2);
                if left.1 > 150 && left.0 < 100 && right.0 > 150 && right.1 > 150 && right.2 < 100 {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the screen's pattern on the far side");

    sharer.set_screen_share(false).await.expect("stop sharing");
    wait_for(&mut events, "the screen going", |e| {
        matches!(e, CallEvent::Video { identity, screen: true, on: false, .. } if identity == "sharer")
    })
    .await;
    sharer.leave().await;
    viewer.leave().await;
}

/// What `who`'s shared screen sounds like here, once it does: loud enough
/// and as `want` says.
async fn wait_screen_audio(
    who: &str,
    what: &str,
    want: impl Fn(&morpheus_call::ScreenAudio) -> bool,
) -> morpheus_call::ScreenAudio {
    let mut last = None;
    let found = timeout(Duration::from_secs(20), async {
        loop {
            last = morpheus_call::screen_audio(who);
            if let Some(heard) = &last
                && want(heard)
            {
                return heard.clone();
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await;
    found.unwrap_or_else(|_| panic!("{what}: the screen's sound is {last:?}"))
}

/// A shared screen with its sound: the others get it as a track of its own
/// (not the sharer's voice: the test voice is 440 Hz, the screen's tone is
/// 660 Hz left and 990 Hz right, so stereo too), at the volume each of them
/// gives it, silent while deafened, gone with the share. Sharing without
/// sound sends none.
#[tokio::test]
async fn a_shared_screens_sound_is_heard_at_its_own_volume_until_the_share_stops() {
    let Some(url) = livekit() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping media test");
        return;
    };
    let room = format!("screen-audio-test-{}", std::process::id());
    let sharer_id = format!("sharer-{}", std::process::id());
    let (viewer, mut events) = Call::connect(&url, &token(&room, "viewer"), AudioMode::Tone)
        .await
        .expect("viewer connects");
    let (sharer, _sharer_events) = Call::connect(&url, &token(&room, &sharer_id), AudioMode::Tone)
        .await
        .expect("sharer connects");
    let with_audio = morpheus_call::ScreenQuality {
        audio: true,
        ..Default::default()
    };
    sharer
        .set_screen_share_with(true, with_audio)
        .await
        .expect("share with sound");
    wait_for(&mut events, "the screen's sound arriving", |e| {
        matches!(e, CallEvent::ScreenAudio { identity, on: true } if *identity == sharer_id)
    })
    .await;
    let near = |hz: f32, want: f32| (hz - want).abs() < 20.0;
    let full = wait_screen_audio(&sharer_id, "its tone, stereo", |a| {
        a.level > 0.05
            && near(a.left_hz, morpheus_call::SCREEN_TONE_LEFT)
            && near(a.right_hz, morpheus_call::SCREEN_TONE_RIGHT)
    })
    .await;
    // The first reading may include the silence before it began.
    let _ = full;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let full = morpheus_call::screen_audio(&sharer_id).expect("still sounding");
    eprintln!("at full volume: {full:?}");
    assert!(full.level > 0.1, "a steady tone: {full:?}");

    // Half the volume: half the level.
    viewer.set_screen_volume(&sharer_id, 0.5);
    let half = wait_screen_audio(&sharer_id, "at half volume", |a| {
        a.level > full.level * 0.35 && a.level < full.level * 0.65
    })
    .await;
    eprintln!("at half volume: {half:?}");
    // WebRTC itself plays it at half (its level, after the volume, is a
    // perceptual scale: lower, not half). Read again once it has updated.
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let half = morpheus_call::screen_audio(&sharer_id).expect("still sounding");
    eprintln!("played at full {}, at half {}", full.played, half.played);
    assert!(full.played > 0.05, "played at all: {full:?}");
    assert!(
        half.played < full.played * 0.8 && half.played > full.played * 0.2,
        "WebRTC's own level at half volume: {} (full {})",
        half.played,
        full.played
    );
    // Deafened: nothing.
    viewer.set_deafened(true);
    wait_screen_audio(&sharer_id, "deafened", |a| a.level < 0.005).await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let deaf = morpheus_call::screen_audio(&sharer_id).expect("still arriving");
    assert!(deaf.played < 0.01, "WebRTC plays it while deafened: {deaf:?}");
    viewer.set_deafened(false);
    wait_screen_audio(&sharer_id, "undeafened", |a| a.level > full.level * 0.35).await;

    // Stopping the share ends its sound.
    sharer
        .set_screen_share_with(false, with_audio)
        .await
        .expect("stop");
    wait_for(&mut events, "the screen's sound going", |e| {
        matches!(e, CallEvent::ScreenAudio { identity, on: false } if *identity == sharer_id)
    })
    .await;
    assert!(morpheus_call::screen_audio(&sharer_id).is_none());

    // Without sound: the screen, and no sound track.
    sharer
        .set_screen_share_with(
            true,
            morpheus_call::ScreenQuality {
                audio: false,
                ..Default::default()
            },
        )
        .await
        .expect("share without sound");
    wait_for(&mut events, "the screen again", |e| {
        matches!(e, CallEvent::Video { identity, screen: true, on: true, .. } if *identity == sharer_id)
    })
    .await;
    let sound = timeout(Duration::from_secs(3), async {
        loop {
            if let Some(CallEvent::ScreenAudio { identity, on: true }) = events.recv().await
                && identity == sharer_id
            {
                return;
            }
        }
    })
    .await;
    assert!(sound.is_err(), "a sound track came with a share without sound");
    sharer.leave().await;
    viewer.leave().await;
}
