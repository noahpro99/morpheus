//! Shell layout and modal behaviour: the member list toggle in the chat
//! header, and Escape closing whichever modal is open.

// Desktop layout (member column beside the chat, window controls): phone runs keep it on desktop.
#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login_and_open(app: &mut App, room: &str) {
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
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("room listed");
    app.open_room(room).await.expect("open room");
}

#[tokio::test]
async fn member_toggle_hides_and_shows_the_member_list() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;

    app.wait_for_text(".members .member", "alice", DEFAULT_WAIT)
        .await
        .expect("members shown by default");

    app.click(".member-toggle").await.expect("toggle off");
    app.wait_for_js(
        "return document.querySelector('.members') === null && document.querySelector('.shell').classList.contains('no-side');",
        DEFAULT_WAIT,
    )
    .await
    .expect("member list hidden and the column collapsed");

    // The choice sticks across rooms.
    app.open_room("random").await.expect("switch room");
    assert_eq!(
        app.count(".members").await.expect("count"),
        0,
        "still hidden in the next room"
    );

    app.click(".member-toggle").await.expect("toggle on");
    app.wait_for_text(".members .member", "alice", DEFAULT_WAIT)
        .await
        .expect("member list back");
}

#[tokio::test]
async fn escape_closes_open_modals() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;

    // Settings.
    app.click(".settings-button").await.expect("open settings");
    app.wait_for(".settings", DEFAULT_WAIT)
        .await
        .expect("settings modal");
    app.press(".settings", "Escape").await.expect("escape");
    app.wait_for_js(
        "return document.querySelector('.settings') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("settings closed by Escape");

    // Quick switcher.
    app.shortcut(".shell", "k", true).await.expect("ctrl+k");
    app.wait_for(".switcher input", DEFAULT_WAIT)
        .await
        .expect("switcher");
    app.press(".switcher input", "Escape")
        .await
        .expect("escape");
    app.wait_for_js(
        "return document.querySelector('.switcher') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("switcher closed by Escape");
}

/// The profile modal opens from a message's sender name, a member row, and
/// the footer chip, shows display name plus Matrix ID, and closes on Escape.
#[tokio::test]
async fn profile_modal_opens_from_sender_member_and_footer() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;

    // From a message header.
    app.wait_for(".message button.sender", DEFAULT_WAIT)
        .await
        .expect("a sender name");
    let sender = app
        .text(".message button.sender")
        .await
        .expect("sender text")
        .unwrap_or_default();
    app.click(".message button.sender")
        .await
        .expect("click sender");
    app.wait_for_text(".profile-card .profile-name", &sender, DEFAULT_WAIT)
        .await
        .expect("profile shows the sender's name");
    app.wait_for_text(".profile-card .profile-id", ":localhost", DEFAULT_WAIT)
        .await
        .expect("profile shows the Matrix ID");
    app.press("body", "Escape").await.expect("escape");
    app.wait_for_js(
        "return document.querySelector('.profile-card') === null && document.querySelector('.profile-modal') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("profile closed by Escape");

    // From the member list.
    app.click_text(".members .member", "carol")
        .await
        .expect("click carol");
    app.wait_for_text(
        ".profile-card .profile-id",
        "@carol:localhost",
        DEFAULT_WAIT,
    )
    .await
    .expect("carol's profile");
    app.click(".card-overlay").await.expect("click outside");
    app.wait_for_js(
        "return document.querySelector('.profile-card') === null && document.querySelector('.profile-modal') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("profile closed by the button");

    // From the footer chip: its menu, then our own profile.
    app.click(".profile-chip").await.expect("click footer");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("account menu");
    app.click_text(".context-item", "View profile")
        .await
        .expect("view profile");
    app.wait_for_text(
        ".profile-card .profile-id",
        "@alice:localhost",
        DEFAULT_WAIT,
    )
    .await
    .expect("own profile");
    app.wait_for_text(".profile-card", "This is you", DEFAULT_WAIT)
        .await
        .expect("marked as own");
}

/// Navigating to a room focuses the composer so you can type immediately.
#[tokio::test]
async fn composer_is_focused_after_navigating() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;
    let focused =
        "return document.activeElement === document.querySelector('.composer .composer-input');";
    app.wait_for_js(focused, DEFAULT_WAIT)
        .await
        .expect("composer focused after opening general");
    app.open_room("random").await.expect("switch");
    app.wait_for_js(focused, DEFAULT_WAIT)
        .await
        .expect("composer focused after switching to random");
}

/// Big rooms render a page of members at a time; the rest come on request.
#[tokio::test]
async fn member_list_is_windowed_for_big_rooms() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login_and_open(&mut app, "crowd").await;
    app.wait_for_text(".members-header", "Members — 152", DEFAULT_WAIT)
        .await
        .expect("full count in the header");
    assert_eq!(
        app.count(".members .member").await.expect("count"),
        100,
        "only the first page of rows is rendered"
    );
    app.wait_for_text(".show-more-members", "53 hidden", DEFAULT_WAIT)
        .await
        .expect("show more button");
    app.click(".show-more-members").await.expect("show more");
    app.wait_for_js(
        "return document.querySelectorAll('.members .member').length === 153 && document.querySelector('.show-more-members') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("everyone listed after one click");
}
