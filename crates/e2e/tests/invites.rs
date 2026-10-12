//! Pending invites: listed in the sidebar and the inbox, counted on the DM
//! rail button, shown as a card in the chat column; accept joins, decline
//! leaves.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login(app: &mut App, user: &str) {
    app.wait_for("form.login", DEFAULT_WAIT).await.expect("login form");
    app.type_into("form.login input[name=username]", user).await.expect("username");
    app.type_into("form.login input[name=password]", &format!("{user}-password")).await.expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("room listed");
}

/// Right-click the joined room row whose name contains `name`.
async fn room_menu(app: &mut App, name: &str) {
    let needle = serde_json::to_string(name).expect("json");
    let js = format!(
        "const el = [...document.querySelectorAll('.room:not(.invite-row)')].find(el => el.textContent.includes({needle})); \
         if (!el) throw new Error('no room ' + {needle}); const r = el.getBoundingClientRect(); \
         el.dispatchEvent(new MouseEvent('contextmenu', {{ bubbles: true, cancelable: true, clientX: r.left + 20, clientY: r.top + 10 }}));"
    );
    app.eval(&js).await.expect("right-click room");
    app.wait_for(".context-menu", DEFAULT_WAIT).await.expect("room menu");
}

async fn invite_bob(app: &mut App, room: &str) {
    room_menu(app, room).await;
    app.click_text(".context-item", "Invite people").await.expect("invite");
    app.wait_for(".invite-dialog input[name=user]", DEFAULT_WAIT).await.expect("invite dialog");
    app.type_into(".invite-dialog input[name=user]", "@bob:localhost").await.expect("user");
    app.submit(".invite-dialog").await.expect("send invite");
    app.wait_for_text(".invite-dialog .ok", "Invited @bob:localhost", DEFAULT_WAIT).await.expect("invited");
    app.press(".invite-dialog", "Escape").await.expect("escape");
}

/// JS predicate: an `.invite-row` whose text contains `name` exists (or not).
fn invite_row_js(name: &str, present: bool) -> String {
    let needle = serde_json::to_string(name).expect("json");
    let found = format!("[...document.querySelectorAll('.invite-row')].some(el => el.textContent.includes({needle}))");
    if present { format!("return {found}") } else { format!("return !({found})") }
}

#[tokio::test]
async fn invites_are_listed_declined_and_accepted() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut alice = App::launch(&hs).await.expect("launch alice");
    login(&mut alice, "alice").await;
    alice.open_room("general").await.expect("open general");
    let name = format!("e2e room invite {}", std::process::id());
    alice.click(".create-room-button").await.expect("plus");
    alice.wait_for(".create-room-dialog input[name=name]", DEFAULT_WAIT).await.expect("dialog");
    alice.type_into(".create-room-dialog input[name=name]", &name).await.expect("name");
    alice.submit(".create-room-dialog").await.expect("create");
    alice.wait_for_text(".chat-header h2", &name, DEFAULT_WAIT).await.expect("new room opened");

    let mut bob = App::launch(&hs).await.expect("launch bob");
    login(&mut bob, "bob").await;
    // Nothing pending before the invite.
    bob.wait_for_js(&invite_row_js(&name, false), DEFAULT_WAIT).await.expect("no invite yet");

    invite_bob(&mut alice, &name).await;

    // Sidebar row with the inviter, rail badge, card in the chat column.
    bob.wait_for_js(&invite_row_js(&name, true), DEFAULT_WAIT).await.expect("invite listed");
    bob.wait_for_text(".invite-row", "from alice", DEFAULT_WAIT).await.expect("inviter shown");
    bob.wait_for(".rail-dms .rail-badge", DEFAULT_WAIT).await.expect("dm badge counts the invite");
    bob.click_text(".invite-row", &name).await.expect("open invite");
    bob.wait_for_text(".invite-card h2", &name, DEFAULT_WAIT).await.expect("invite card");
    bob.wait_for_text(".invite-card", "alice invited you", DEFAULT_WAIT).await.expect("card text");
    assert_eq!(bob.count(".composer").await.expect("count"), 0, "no composer for an invite");
    // The inbox lists it too.
    bob.click(".titlebar .inbox-button").await.expect("open inbox");
    bob.wait_for_text(".inbox-invite", &name, DEFAULT_WAIT).await.expect("inbox invite");
    bob.press(".inbox-panel", "Escape").await.expect("close inbox");

    // Decline from the card: the row and the card go away.
    bob.click(".invite-card .invite-decline").await.expect("decline");
    bob.wait_for_js(&invite_row_js(&name, false), DEFAULT_WAIT).await.expect("invite gone");
    bob.wait_for_js("return !document.querySelector('.invite-card')", DEFAULT_WAIT).await.expect("card gone");

    // Invite again and accept from the sidebar row.
    invite_bob(&mut alice, &name).await;
    bob.wait_for_js(&invite_row_js(&name, true), DEFAULT_WAIT).await.expect("invited again");
    bob.click(".invite-row .invite-accept").await.expect("accept");
    bob.wait_for_js(&invite_row_js(&name, false), DEFAULT_WAIT).await.expect("no longer pending");
    bob.open_room(&name).await.expect("joined room opens");
    bob.wait_for(".composer input", DEFAULT_WAIT).await.expect("composer for the joined room");
}
