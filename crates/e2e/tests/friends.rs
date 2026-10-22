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
async fn add_friend_by_username_from_the_friends_page_like_discord() {
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

    // The green button in the page's header opens the form.
    app.click(".friends-header .add-friend-tab")
        .await
        .expect("Add Friend");
    app.wait_for("form.add-friend input", DEFAULT_WAIT)
        .await
        .expect("the form");
    // Someone who does not exist: said so, nothing added.
    app.type_into("form.add-friend input", "nobody-here")
        .await
        .expect("type");
    app.submit("form.add-friend").await.expect("send");
    app.wait_for_text(
        ".add-friend .add-friend-result.error",
        "nobody-here",
        DEFAULT_WAIT,
    )
    .await
    .expect("an unknown name is refused");
    // A bare username means someone on this server.
    app.type_into("form.add-friend input", "alice")
        .await
        .expect("type");
    app.submit("form.add-friend").await.expect("send");
    app.wait_for_text(".add-friend .add-friend-result.ok", "alice", DEFAULT_WAIT)
        .await
        .expect("added");
    // A full Matrix ID works too.
    app.type_into("form.add-friend input", "@bob:localhost")
        .await
        .expect("type");
    app.submit("form.add-friend").await.expect("send");
    app.wait_for_text(".add-friend .add-friend-result.ok", "bob", DEFAULT_WAIT)
        .await
        .expect("added by ID");
    if let Some(path) = app.screenshot("add-friend").await.expect("screenshot") {
        eprintln!("screenshot: {}", path.display());
    }

    // Both on the All tab.
    app.click(".friends-header .all-friends-tab")
        .await
        .expect("All");
    app.wait_for_text(".friend-list .friend", "alice", DEFAULT_WAIT)
        .await
        .expect("alice listed");
    app.wait_for_text(".friend-list .friend", "bob", DEFAULT_WAIT)
        .await
        .expect("bob listed");
    // Removed again, so the others start with none.
    for _ in 0..2 {
        app.click_text(".friend-list .friend .friend-actions button", "Remove")
            .await
            .expect("Remove");
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
    app.wait_for_text(".friends-page .empty-state", "No friends yet", DEFAULT_WAIT)
        .await
        .expect("none left");
}
