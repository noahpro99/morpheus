//! The profile card: mutual rooms, a private note, messaging from the card,
//! the more menu (full profile, block), and the report form.

// Desktop layout (member column beside the chat, window controls): phone runs keep it on desktop.
#![allow(clippy::expect_used)]

use std::time::Duration;

use morpheus_e2e::{App, DEFAULT_WAIT, admin_join, new_account};

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

async fn open_card_for(app: &mut App, name: &str) {
    app.click_text(".members .member", name)
        .await
        .expect("click member");
    app.wait_for_text(
        ".profile-card .profile-id",
        &format!("@{name}:localhost"),
        DEFAULT_WAIT,
    )
    .await
    .expect("card for the member");
}

#[tokio::test]
async fn card_shows_mutual_rooms_saves_a_note_and_messages() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login_and_open(&mut app, "alice", "general").await;
    open_card_for(&mut app, "bob").await;

    // Pops up where the click was (not centred), with mutual rooms and the role here.
    app.wait_for_js(
        "const c = document.querySelector('.profile-card'); return !!c && c.style.left !== '50%';",
        Duration::from_secs(5),
    )
    .await
    .expect("anchored at the click");
    app.wait_for_text(".profile-card .mutual", "general", DEFAULT_WAIT)
        .await
        .expect("mutual rooms");
    app.wait_for_text(".profile-card .card-tags", "Member", DEFAULT_WAIT)
        .await
        .expect("role here");

    // The note persists across reopening.
    let note = format!("met at the conf {}", std::process::id());
    app.type_into(".profile-card textarea.note", &note)
        .await
        .expect("type note");
    app.eval(
        "document.querySelector('.profile-card textarea.note').dispatchEvent(new Event('blur'));",
    )
    .await
    .expect("blur");
    app.wait_for_text(".profile-card .note-saved", "Saved", DEFAULT_WAIT)
        .await
        .expect("note saved");
    app.press("body", "Escape").await.expect("escape");
    app.wait_for_js(
        "return document.querySelector('.profile-card') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("closed");
    open_card_for(&mut app, "bob").await;
    app.wait_for_js(
        &format!(
            "return document.querySelector('.profile-card textarea.note').value === {};",
            serde_json::to_string(&note).expect("json")
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("note remembered");

    // The more menu opens the full profile.
    app.click(".profile-card .card-button.more")
        .await
        .expect("more");
    app.wait_for_text(".context-item", "View full profile", DEFAULT_WAIT)
        .await
        .expect("more menu");
    app.click_text(".context-item", "View full profile")
        .await
        .expect("full");
    app.wait_for_text(".profile-modal .profile-id", "@bob:localhost", DEFAULT_WAIT)
        .await
        .expect("full profile modal");
    app.press("body", "Escape").await.expect("escape");
    app.wait_for_js(
        "return document.querySelector('.profile-modal') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("closed");

    // Messaging from the card lands in the DM.
    open_card_for(&mut app, "bob").await;
    let text = format!("from the card {}", std::process::id());
    app.type_into(".profile-card input.card-message", &text)
        .await
        .expect("type");
    app.press(".profile-card input.card-message", "Enter")
        .await
        .expect("send");
    app.wait_for_text(".chat-header h2", "bob", Duration::from_secs(30))
        .await
        .expect("DM opened");
    app.wait_for_text(".message.own .body", &text, DEFAULT_WAIT)
        .await
        .expect("message in the DM");
}

#[tokio::test]
async fn card_mod_view_and_report_form() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    // #general, not #moderation: the moderation tests kick carol from there.
    login_and_open(&mut app, "alice", "general").await;
    open_card_for(&mut app, "carol").await;

    // Mod view opens this person's moderation sheet (see the test below).
    app.click(".profile-card .card-button.mod-view")
        .await
        .expect("mod view");
    app.wait_for_text(".mod-view .mod-room", "general", DEFAULT_WAIT)
        .await
        .expect("the mod view");
    app.press("body", "Escape").await.expect("escape");
    gone_js(&mut app, ".mod-view").await;
    open_card_for(&mut app, "carol").await;

    // Report from the more menu shows an inline form.
    app.click(".profile-card .card-button.more")
        .await
        .expect("more");
    if app
        .wait_for_text(".context-item", "Report user profile", DEFAULT_WAIT)
        .await
        .is_err()
    {
        let shot = app.screenshot("card-more").await.ok().flatten();
        let items = app
            .eval("return [...document.querySelectorAll('.context-item')].map(e => e.textContent).join('|');")
            .await;
        panic!("no report item: {items:?} {shot:?}");
    }
    app.click_text(".context-item", "Report user profile")
        .await
        .expect("report");
    app.wait_for(
        ".profile-card .report-user input[name=reason]",
        DEFAULT_WAIT,
    )
    .await
    .expect("report form");
    app.type_into(
        ".profile-card .report-user input[name=reason]",
        "e2e report",
    )
    .await
    .expect("reason");
    app.submit(".profile-card .report-user")
        .await
        .expect("submit");
    app.wait_for_js(
        "const f = document.querySelector('.profile-card .report-user'); return !!f && (f.querySelector('.ok') !== null || f.querySelector('.error') !== null);",
        DEFAULT_WAIT,
    )
    .await
    .expect("server answered the report");
}

async fn gone_js(app: &mut App, selector: &str) {
    app.wait_for_js(
        &format!(
            "return !document.querySelector({});",
            serde_json::to_string(selector).expect("json")
        ),
        DEFAULT_WAIT,
    )
    .await
    .unwrap_or_else(|e| panic!("{selector} still there: {e}"));
}

/// The row for `room` in the mod view.
fn row(room: &str) -> String {
    format!(
        ".mod-view .mod-room[data-name={}]",
        serde_json::to_string(room).expect("json")
    )
}

/// Discord's mod view: one sheet for a person, every room you share with
/// their standing there, and the moderation you may do in each.
#[tokio::test]
async fn mod_view_moderates_someone_across_rooms() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    // Someone fresh, so banning them leaves the seeded people alone.
    let them = new_account(&hs, "modee").expect("account");
    admin_join(&hs, &them, "#general:localhost").expect("join general");
    admin_join(&hs, &them, "#random:localhost").expect("join random");

    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login_and_open(&mut app, "alice", "general").await;
    app.wait_for_text(".members .member", &them, DEFAULT_WAIT)
        .await
        .expect("listed in general");
    open_card_for(&mut app, &them).await;
    app.click(".profile-card .card-button.mod-view")
        .await
        .expect("mod view");
    app.wait_for_text(
        ".mod-view .mod-view-id",
        &format!("@{them}:localhost"),
        DEFAULT_WAIT,
    )
    .await
    .expect("the sheet names them");
    for room in ["general", "random"] {
        app.wait_for(&standing(room, "Member"), DEFAULT_WAIT)
            .await
            .unwrap_or_else(|e| panic!("{room} listed with their role: {e}"));
    }
    if let Some(path) = app.screenshot("mod-view").await.expect("screenshot") {
        eprintln!("screenshot: {}", path.display());
    }

    // A role, then a kick, in #general.
    app.eval(&format!(
        "const s = document.querySelector({}); s.value = 'Moderator'; s.dispatchEvent(new Event('change', {{ bubbles: true }}));",
        serde_json::to_string(&format!("{} select.mod-role", row("general"))).expect("json")
    ))
    .await
    .expect("make moderator");
    app.wait_for(&standing("general", "Moderator"), DEFAULT_WAIT)
        .await
        .expect("now a moderator");
    app.type_into(".mod-view input[name=reason]", "e2e mod view")
        .await
        .expect("reason");
    app.click(&format!("{} .mod-kick", row("general")))
        .await
        .expect("kick");
    app.wait_for(&standing("general", "Not in the room"), DEFAULT_WAIT)
        .await
        .expect("kicked from general");

    // Ban, then unban, in #random.
    app.click(&format!("{} .mod-ban", row("random")))
        .await
        .expect("ban");
    app.wait_for(&standing("random", "Banned"), DEFAULT_WAIT)
        .await
        .expect("banned from random");
    app.click(&format!("{} .mod-unban", row("random")))
        .await
        .expect("unban");
    app.wait_for(&standing("random", "Not in the room"), DEFAULT_WAIT)
        .await
        .expect("unbanned");
    // Still in #random? No: unbanning lets them back, it does not bring them.
    assert_eq!(
        app.count(&format!("{} .mod-ban", row("random")))
            .await
            .expect("count"),
        1,
        "they can be banned again before they come back"
    );

    app.press("body", "Escape").await.expect("escape");
    gone_js(&mut app, ".mod-view").await;
}

/// The row for `room` showing them as `standing` (a role, Invited, Banned
/// or Not in the room).
fn standing(room: &str, standing: &str) -> String {
    format!(
        "{}[data-standing={}]",
        row(room),
        serde_json::to_string(standing).expect("json")
    )
}
