//! Member menu: moderation (roles, kick, ban) for admins, and mention,
//! message, block for everyone. `#moderation` is reset by `just hs-seed`
//! (bob and carol are unbanned and rejoined) so the kick and ban stick only
//! until the next seed run.

// Desktop layout (member column beside the chat, window controls): phone runs keep it on desktop.
#![allow(clippy::expect_used)]

use std::time::Duration;

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login_and_open(app: &mut App, user: &str, room: &str) {
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", user)
        .await
        .expect("username");
    app.type_into(
        "form.login input[name=password]",
        &format!("{user}-password"),
    )
    .await
    .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("room listed");
    app.open_room(room).await.expect("open room");
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
}

/// Right-click the member row whose name contains `name`.
async fn member_menu(app: &mut App, name: &str) {
    app.wait_for_text(".members .member", name, DEFAULT_WAIT)
        .await
        .expect("member listed");
    let js = format!(
        "const el = [...document.querySelectorAll('.members .member')].find(el => el.textContent.includes({})); \
         const r = el.getBoundingClientRect(); \
         el.dispatchEvent(new MouseEvent('contextmenu', {{ bubbles: true, cancelable: true, clientX: r.left + 10, clientY: r.top + 10 }}));",
        serde_json::to_string(name).expect("json")
    );
    app.eval(&js).await.expect("right-click member");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("member menu");
}

async fn menu_labels(app: &mut App) -> String {
    app.eval("return [...document.querySelectorAll('.context-menu > .context-item')].map(e => e.textContent).join('|');")
        .await
        .expect("labels")
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// Hover a submenu item and click one of its children.
async fn click_submenu(app: &mut App, parent: &str, child: &str) {
    app.click_submenu(parent, child)
        .await
        .expect("submenu click");
}

#[tokio::test]
async fn admin_changes_roles_kicks_and_bans() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login_and_open(&mut app, "alice", "moderation").await;

    // Alice created the room: full moderation menu for carol.
    member_menu(&mut app, "carol").await;
    let labels = menu_labels(&mut app).await;
    for needle in [
        "Profile",
        "Mention",
        "Message",
        "Invite to server",
        "Block",
        "Roles",
        "Kick carol",
        "Ban carol",
        "Copy user ID",
    ] {
        assert!(labels.contains(needle), "missing {needle} in {labels}");
    }
    // Her own row offers no moderation.
    app.press(".context-menu", "Escape").await.expect("escape");
    member_menu(&mut app, "alice").await;
    let own = menu_labels(&mut app).await;
    assert!(
        !own.contains("Kick") && !own.contains("Roles") && !own.contains("Block"),
        "own row: {own}"
    );
    app.press(".context-menu", "Escape").await.expect("escape");

    // Promote carol to moderator, then back.
    member_menu(&mut app, "carol").await;
    click_submenu(&mut app, "Roles", "Moderator").await;
    app.wait_for_js(
        "const h = [...document.querySelectorAll('.members-header')].find(h => h.textContent.includes('Moderators')); return !!h && h.nextElementSibling?.textContent.includes('carol');",
        DEFAULT_WAIT,
    )
    .await
    .expect("carol listed under Moderators");
    member_menu(&mut app, "carol").await;
    click_submenu(&mut app, "Roles", "Member").await;
    app.wait_for_js(
        "return ![...document.querySelectorAll('.members-header')].some(h => h.textContent.includes('Moderators'));",
        DEFAULT_WAIT,
    )
    .await
    .expect("carol back to member");

    // Kick carol: she leaves the list.
    member_menu(&mut app, "carol").await;
    app.click_text(".context-item.danger", "Kick carol")
        .await
        .expect("kick");
    app.wait_for_js(
        "return ![...document.querySelectorAll('.members .member')].some(el => el.textContent.includes('carol'));",
        DEFAULT_WAIT,
    )
    .await
    .expect("carol gone after kick");

    // Ban bob: same, and the count drops.
    member_menu(&mut app, "bob").await;
    app.click_text(".context-item.danger", "Ban bob")
        .await
        .expect("ban");
    app.wait_for_js(
        "return ![...document.querySelectorAll('.members .member')].some(el => el.textContent.includes('bob'));",
        DEFAULT_WAIT,
    )
    .await
    .expect("bob gone after ban");
    app.wait_for_text(".chat-header .member-toggle", "1", DEFAULT_WAIT)
        .await
        .expect("member count follows");
}

#[tokio::test]
async fn member_menu_mentions_messages_and_blocks() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login_and_open(&mut app, "carol", "general").await;

    // Carol is a plain member: no moderation on bob.
    member_menu(&mut app, "bob").await;
    let labels = menu_labels(&mut app).await;
    assert!(
        !labels.contains("Kick") && !labels.contains("Ban") && !labels.contains("Roles"),
        "{labels}"
    );

    // Mention drops the name into the composer.
    app.click_text(".context-item", "Mention")
        .await
        .expect("mention");
    app.wait_for_js(
        "return document.querySelector('.composer .composer-input').value.includes('@bob');",
        DEFAULT_WAIT,
    )
    .await
    .expect("composer holds the mention");

    // Block, then unblock: the row is tagged and the menu flips. A failed
    // earlier run may have left bob blocked, so start from unblocked.
    member_menu(&mut app, "bob").await;
    if menu_labels(&mut app).await.contains("Unblock") {
        app.click_text(".context-item", "Unblock")
            .await
            .expect("reset block");
        app.wait_for_js(
            "return document.querySelector('.members .member.blocked') === null;",
            DEFAULT_WAIT,
        )
        .await
        .expect("reset to unblocked");
        member_menu(&mut app, "bob").await;
    }
    app.click_text(".context-item", "Block")
        .await
        .expect("block");
    app.wait_for_text(".members .member.blocked", "bob", DEFAULT_WAIT)
        .await
        .expect("bob marked blocked");
    member_menu(&mut app, "bob").await;
    app.click_text(".context-item", "Unblock")
        .await
        .expect("unblock");
    app.wait_for_js(
        "return document.querySelector('.members .member.blocked') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("block lifted");

    // Message opens (creating if needed) the DM and moves the rail to DMs.
    member_menu(&mut app, "bob").await;
    app.click_text(".context-item", "Message")
        .await
        .expect("message");
    app.wait_for_text(".chat-header h2", "bob", Duration::from_secs(30))
        .await
        .expect("DM with bob opened");
    app.wait_for(
        ".rail-item.selected[title=\"Direct messages\"]",
        DEFAULT_WAIT,
    )
    .await
    .expect("rail on DMs");
}
