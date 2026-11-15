//! A direct message with nothing in it yet offers a "Say hi! 👋" button that
//! sends "Hi! 👋"; once there is a message, the button is gone.

#![allow(clippy::expect_used)]

mod common;

use morpheus_e2e::{App, DEFAULT_WAIT, api_as, new_account};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

#[tokio::test]
async fn an_empty_dm_offers_a_say_hi_button_that_sends_hi() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let (ann, ben) = (
        new_account(&hs, "hiann").expect("account"),
        new_account(&hs, "hiben").expect("account"),
    );
    let made = api_as(
        &hs,
        &ann,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "invite": [format!("@{ben}:localhost")], "is_direct": true, "preset": "trusted_private_chat" })),
    )
    .expect("a DM");
    let room = made["room_id"].as_str().expect("room id").to_owned();
    api_as(
        &hs,
        &ben,
        "POST",
        &format!("join/{}", room.replace('!', "%21").replace(':', "%3A")),
        Some(serde_json::json!({})),
    )
    .expect("join");
    api_as(
        &hs,
        &ann,
        "PUT",
        &format!("user/%40{ann}%3Alocalhost/account_data/m.direct"),
        Some(serde_json::json!({ format!("@{ben}:localhost"): [room] })),
    )
    .expect("m.direct");
    // A room with messages never shows it.
    api_as(
        &hs,
        &ann,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": "hi elsewhere" })),
    )
    .expect("room");

    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    common::login(&mut app, &ann, &format!("{ann}-password")).await;
    app.open_room("hi elsewhere").await.expect("the other room");
    app.wait_for(".timeline-start", DEFAULT_WAIT)
        .await
        .expect("an empty room");
    assert_eq!(
        app.count(".say-hi").await.expect("count"),
        0,
        "only DMs offer it"
    );

    app.click(".rail-dms").await.expect("DMs");
    app.open_room(&ben).await.expect("the DM");
    app.wait_for(".say-hi", DEFAULT_WAIT)
        .await
        .expect("the button");
    app.wait_for_text(".say-hi", "Say hi! 👋", DEFAULT_WAIT)
        .await
        .expect("its label");
    app.click(".say-hi").await.expect("click it");
    app.wait_for_text(".message .body", "Hi! 👋", DEFAULT_WAIT)
        .await
        .expect("the greeting is sent");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while app.count(".say-hi").await.expect("count") > 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "the button should go once there is a message"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
