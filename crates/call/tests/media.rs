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
