//! Sharing to Morpheus from another app (Android's share sheet): a "Share
//! to" dialog with what is shared, a message (the shared text), and the
//! rooms to pick from; picking one opens it and sends.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login(app: &mut App) {
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", "alice")
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", "alice-password")
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
}

/// Pick `room` in the open Share dialog (searching for it) and send.
async fn share_to(app: &mut App, room: &str) {
    app.type_into(".share-dialog input.share-search", room)
        .await
        .expect("search");
    app.wait_for_text(".share-dialog .share-room", room, DEFAULT_WAIT)
        .await
        .expect("the room offered");
    app.click_text(".share-dialog .share-room", room)
        .await
        .expect("pick it");
    app.click(".share-dialog .share-send").await.expect("send");
}

#[tokio::test]
async fn shared_text_and_pictures_go_to_the_room_picked() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    if !app.is_phone() {
        eprintln!("sharing from another app is Android's; skipping");
        return;
    }
    login(&mut app).await;

    // Text (a link, say): the message box of the dialog holds it.
    let text = format!("shared from elsewhere {}", std::process::id());
    app.share_from_outside(Some(&text), None)
        .expect("share text");
    app.wait_for(".share-dialog", DEFAULT_WAIT)
        .await
        .expect("the Share dialog");
    let message = app
        .eval("return document.querySelector('.share-dialog .share-message').value;")
        .await
        .expect("message");
    assert_eq!(message, serde_json::json!(text), "the shared text, to send");
    share_to(&mut app, "random").await;
    app.wait_for_text(".chat-header h2", "random", DEFAULT_WAIT)
        .await
        .expect("#random open");
    app.wait_for_text(".message .body", &text, DEFAULT_WAIT)
        .await
        .expect("sent there");

    // A picture from the phone's gallery, with a caption.
    let picture = morpheus_e2e::android_gallery_picture().expect("a picture in the gallery");
    let caption = format!("look at this {}", std::process::id());
    app.share_from_outside(Some(&caption), Some(&picture))
        .expect("share a picture");
    app.wait_for(".share-dialog .share-file", DEFAULT_WAIT)
        .await
        .expect("the picture listed");
    share_to(&mut app, "general").await;
    app.wait_for_text(".chat-header h2", "general", DEFAULT_WAIT)
        .await
        .expect("#general open");
    app.wait_for(".message.own img.image", DEFAULT_WAIT)
        .await
        .expect("the picture sent");
    app.wait_for_text(".message .body", &caption, DEFAULT_WAIT)
        .await
        .expect("and the caption");
}
