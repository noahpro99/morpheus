//! Nicknames: your own names for people (private account data, so every
//! device of yours sees them and nobody else does). They replace display
//! names in messages and the member list.

#![allow(clippy::expect_used)]

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
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    app.open_room(room).await.expect("open room");
    app.wait_for(".composer input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer");
}

async fn set_nickname(app: &mut App, member: &str, nickname: &str) {
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
    app.click_text(".context-menu .context-item", "Set nickname")
        .await
        .expect("Set nickname");
    app.wait_for(".nickname-editor input[name=nickname]", DEFAULT_WAIT)
        .await
        .expect("nickname editor");
    app.type_into(".nickname-editor input[name=nickname]", nickname)
        .await
        .expect("type nickname");
    app.submit(".nickname-editor").await.expect("save");
    app.wait_for_js(
        "return !document.querySelector('.nickname-editor');",
        DEFAULT_WAIT,
    )
    .await
    .expect("editor closes");
}

/// The sender shown on bob's message `text`.
fn sender_of(text: &str) -> String {
    format!(
        "const rows = [...document.querySelectorAll('.timeline .message')]; \
         const i = rows.findIndex(m => m.textContent.includes({})); \
         for (let j = i; j >= 0; j--) {{ const s = rows[j].querySelector('.sender'); if (s) return s.textContent.trim(); }} \
         return null;",
        serde_json::to_string(text).expect("json")
    )
}

#[tokio::test]
async fn nicknames_replace_display_names_and_follow_you() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut bob = App::launch(&hs).await.expect("launch bob");
    login_and_open(&mut bob, "bob", "general").await;
    let text = format!("call me something {}", std::process::id());
    bob.type_into(".composer input", &text).await.expect("type");
    bob.press(".composer input", "Enter").await.expect("send");

    let mut alice = App::launch(&hs).await.expect("launch alice");
    login_and_open(&mut alice, "alice", "general").await;
    alice
        .reveal_message(&text, DEFAULT_WAIT)
        .await
        .expect("bob's message");
    let nickname = format!("Bobby {}", std::process::id());
    set_nickname(&mut alice, "bob", &nickname).await;

    alice
        .wait_for_js(
            &format!(
                "return ({{ f: () => {{ {} }} }}).f() === {};",
                sender_of(&text),
                serde_json::to_string(&nickname).expect("json")
            ),
            DEFAULT_WAIT,
        )
        .await
        .expect("bob's message shows the nickname");
    alice
        .wait_for_text(".members .member", &nickname, DEFAULT_WAIT)
        .await
        .expect("member list shows the nickname");
    // The profile card names them by it too (their own name alongside).
    alice
        .click_text(".members .member", &nickname)
        .await
        .expect("open bob's card");
    alice
        .wait_for_text(".profile-card", &nickname, DEFAULT_WAIT)
        .await
        .expect("card shows the nickname");
    alice.press("body", "Escape").await.expect("close card");
    // And so does the DM with them, in the room list.
    alice.click(".rail-dms").await.expect("DMs");
    alice
        .wait_for_text(".room.dm", &nickname, DEFAULT_WAIT)
        .await
        .expect("the DM is listed under the nickname");
    alice.open_room(&nickname).await.expect("back to a room");
    alice
        .wait_for_text(".chat-header h2", &nickname, DEFAULT_WAIT)
        .await
        .expect("the DM's header too");

    // Kept in the account: a restart still shows it.
    let mut alice = alice.relaunch().await.expect("relaunch");
    alice.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    alice.open_room("general").await.expect("open general");
    alice
        .wait_for_text(".members .member", &nickname, DEFAULT_WAIT)
        .await
        .expect("nickname after restart");

    // Clearing it brings the display name back.
    set_nickname(&mut alice, &nickname, "").await;
    alice
        .wait_for_js(
            "return [...document.querySelectorAll('.members .member')].some(m => m.textContent.trim() === 'bob') \
                 && ![...document.querySelectorAll('.members .member')].some(m => m.textContent.includes('Bobby'));",
            DEFAULT_WAIT,
        )
        .await
        .expect("bob again");
}
