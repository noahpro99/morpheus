//! The member list shows skeleton rows (like the timeline's) while it loads,
//! then the real rows, and never again for a room it has loaded. (GPUI only.)

#![allow(clippy::expect_used)]

mod common;

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

#[tokio::test]
async fn the_member_list_shows_skeletons_while_loading_and_only_then() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    // The test build holds the first reading of each room's members back.
    let mut app = App::launch_with_env(&hs, &[("MORPHEUS_E2E_MEMBERS_DELAY_MS", "6000")])
        .await
        .expect("launch");
    if !app.is_gpui() {
        return;
    }
    common::login(&mut app, "alice", "alice-password").await;
    app.open_room("random").await.expect("random");

    // Waiting: rows shaped like members (a picture and a name), no real ones.
    app.wait_for(".members .skeleton-row", DEFAULT_WAIT)
        .await
        .expect("skeleton rows while the members load");
    assert!(app.count(".members .skeleton-row").await.expect("count") >= 3, "a few rows");
    assert_eq!(app.count(".members .member").await.expect("count"), 0, "real rows before they load");
    assert!(
        app.count(".members .skeleton-row .avatar.skeleton").await.expect("count") >= 3,
        "each row has a round picture like the timeline's"
    );
    let text = app.texts(".members").await.expect("text").join(" ");
    assert!(!text.contains("Loading"), "words instead of a skeleton: {text:?}");

    // Loaded: real rows replace them.
    app.wait_for(".members .member", DEFAULT_WAIT * 2).await.expect("members");
    assert_eq!(app.count(".members .skeleton-row").await.expect("count"), 0, "skeletons left over");

    // Another room, and back: the list it had is there at once, no skeleton.
    app.open_room("general").await.expect("general");
    app.open_room("random").await.expect("random again");
    for _ in 0..8 {
        assert_eq!(
            app.count(".members .skeleton-row").await.expect("count"),
            0,
            "a skeleton over a room's list that was loaded before"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(app.count(".members .member").await.expect("count") > 0, "the list is there at once");
}
