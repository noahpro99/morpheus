//! Friends: people you keep close (a private list in account data, as
//! Matrix has no friendship). Added from a person's menu, listed on the
//! Friends page at the top of the DMs, with a way to message them.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn member_menu(app: &mut App, member: &str) {
    app.wait_for_text(".members .member", member, DEFAULT_WAIT)
        .await
        .expect("member listed");
    let js = format!(
        "const el = [...document.querySelectorAll('.members .member')].find(el => el.textContent.includes({})); \
         const r = el.getBoundingClientRect(); \
         el.dispatchEvent(new MouseEvent('contextmenu', {{ bubbles: true, cancelable: true, clientX: r.left + 10, clientY: r.top + 10 }}));",
        serde_json::to_string(member).expect("json")
    );
    app.eval(&js).await.expect("right-click member");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("menu");
}

async fn open_friends(app: &mut App) {
    app.click(".rail-dms").await.expect("DMs");
    app.wait_for(".sidebar .friends-row", DEFAULT_WAIT)
        .await
        .expect("Friends row");
    app.click(".sidebar .friends-row").await.expect("Friends");
    app.wait_for(".friends-page", DEFAULT_WAIT)
        .await
        .expect("Friends page");
}

#[tokio::test]
async fn friends_are_added_listed_messaged_and_removed() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", "carol")
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", "carol-password")
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    app.open_room("general").await.expect("open general");

    member_menu(&mut app, "bob").await;
    app.click_text(".context-menu .context-item", "Add friend")
        .await
        .expect("Add friend");

    open_friends(&mut app).await;
    app.wait_for_text(".friends-page .friend", "bob", DEFAULT_WAIT)
        .await
        .expect("bob listed");

    // Kept in the account.
    let mut app = app.relaunch().await.expect("relaunch");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    open_friends(&mut app).await;
    app.wait_for_text(".friends-page .friend", "bob", DEFAULT_WAIT)
        .await
        .expect("bob still listed");

    // Message opens the DM with them.
    app.click_text(".friends-page .friend button", "Message")
        .await
        .expect("Message bob");
    app.wait_for(".chat-header", DEFAULT_WAIT)
        .await
        .expect("a chat");
    app.wait_for_js(
        "return !document.querySelector('.friends-page');",
        DEFAULT_WAIT,
    )
    .await
    .expect("left the Friends page");

    // Remove.
    open_friends(&mut app).await;
    app.click_text(".friends-page .friend button", "Remove")
        .await
        .expect("Remove");
    app.wait_for_js(
        "return ![...document.querySelectorAll('.friends-page .friend')].some(f => f.textContent.includes('bob'));",
        DEFAULT_WAIT,
    )
    .await
    .expect("bob gone");
}

#[tokio::test]
async fn new_message_by_username_opens_a_dm() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", "carol")
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", "carol-password")
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    open_friends(&mut app).await;

    // No "Add Friend" (Matrix has no friend requests): New Message instead.
    assert_eq!(app.count(".add-friend-tab").await.expect("count"), 0);
    app.click(".friends-header .new-message-tab")
        .await
        .expect("New Message");
    app.wait_for("form.new-message input", DEFAULT_WAIT)
        .await
        .expect("the form");
    // Someone who does not exist: said so.
    app.type_into("form.new-message input", "nobody-here")
        .await
        .expect("type");
    app.submit("form.new-message").await.expect("send");
    app.wait_for_text(
        ".new-message .new-message-result.error",
        "nobody-here",
        DEFAULT_WAIT,
    )
    .await
    .expect("an unknown name is refused");
    if let Some(path) = app.screenshot("new-message").await.expect("screenshot") {
        eprintln!("screenshot: {}", path.display());
    }
    // A bare username: the DM with them opens, ready to type in.
    app.type_into("form.new-message input", "alice")
        .await
        .expect("type");
    app.submit("form.new-message").await.expect("send");
    app.wait_for_js(
        "return !document.querySelector('.friends-page') && !!document.querySelector('.chat-header h2') \
         && document.querySelector('.chat-header h2').textContent.includes('alice') \
         && !document.querySelector('.chat-header h2').textContent.includes('Empty Room');",
        DEFAULT_WAIT,
    )
    .await
    .unwrap_or_else(|e| panic!("the DM with alice opens: {e}"));
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("ready to type");
}

/// On the Friends page the whole row is the Message button, as in Discord:
/// clicking a friend's name opens the DM; Remove still only removes.
#[tokio::test]
async fn a_friends_whole_row_opens_the_dm() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", "carol")
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", "carol-password")
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    app.open_room("general").await.expect("open general");
    member_menu(&mut app, "bob").await;
    app.click_text(".context-menu .context-item", "Add friend")
        .await
        .expect("Add friend");
    open_friends(&mut app).await;
    app.wait_for_text(".friends-page .friend", "bob", DEFAULT_WAIT)
        .await
        .expect("bob listed");
    app.click(".friends-page .friend .friend-name")
        .await
        .expect("click the row");
    app.wait_for_js(
        "return !document.querySelector('.friends-page') && !!document.querySelector('.chat-header');",
        DEFAULT_WAIT,
    )
    .await
    .expect("the row opened the DM");

    open_friends(&mut app).await;
    app.click_text(".friends-page .friend button", "Remove")
        .await
        .expect("Remove");
    app.wait_for_js(
        "return ![...document.querySelectorAll('.friends-page .friend')].some(f => f.textContent.includes('bob'));",
        DEFAULT_WAIT,
    )
    .await
    .expect("bob gone");
    assert_eq!(
        app.count(".friends-page").await.expect("count"),
        1,
        "Remove didn't also open the DM"
    );
}

/// DMs in the list are tall rows, as in Discord (42px, a 32px picture),
/// and the Friends row matches them.
#[tokio::test]
async fn dm_rows_are_tall() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
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
    app.wait_for(".rail-dms", DEFAULT_WAIT).await.expect("rail");
    app.click(".rail-dms").await.expect("DMs");
    app.wait_for(".sidebar .room.dm", DEFAULT_WAIT)
        .await
        .expect("a DM listed");
    let sizes = app
        .eval(
            "const dm = document.querySelector('.sidebar .room.dm'); \
             const pic = dm.querySelector('.avatar, img'); \
             const friends = document.querySelector('.sidebar .friends-row'); \
             return [dm.getBoundingClientRect().height, pic.getBoundingClientRect().height, friends.getBoundingClientRect().height];",
        )
        .await
        .expect("sizes");
    let sizes: Vec<f64> = serde_json::from_value(sizes).expect("numbers");
    assert!(sizes[0] >= 42.0, "a DM row is 42px tall: {sizes:?}");
    assert!(sizes[1] >= 32.0, "with a 32px picture: {sizes:?}");
    assert!(sizes[2] >= 42.0, "and so is Friends: {sizes:?}");
}
