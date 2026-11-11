//! The profile card: mutual servers, a private note, messaging from the card,
//! the more menu (full profile, block), and the report form.

// Desktop layout (member column beside the chat, window controls): phone runs keep it on desktop.
#![allow(clippy::expect_used)]

use std::time::Duration;

use morpheus_e2e::{App, DEFAULT_WAIT, admin_join, api_as, new_account};

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
    let card = app
        .bounds(".profile-card")
        .await
        .expect("card bounds")
        .expect("a card");
    let shell = app
        .bounds(".shell")
        .await
        .expect("shell bounds")
        .expect("a shell");
    assert!(
        (card.centre_x() - shell.centre_x()).abs() > 20.0,
        "anchored at the click, not centred: {card:?} in {shell:?}"
    );
    app.wait_for_text(
        ".profile-card .mutual-servers",
        "mutual server",
        DEFAULT_WAIT,
    )
    .await
    .expect("mutual servers counted");
    app.wait_for_text(".profile-card .card-tags", "Member", DEFAULT_WAIT)
        .await
        .expect("role here");

    // The note persists across reopening.
    let note = format!("met at the conf {}", std::process::id());
    app.type_into(".profile-card textarea.note", &note)
        .await
        .expect("type note");
    app.blur(".profile-card textarea.note").await.expect("blur");
    app.wait_for_text(".profile-card .note-saved", "Saved", DEFAULT_WAIT)
        .await
        .expect("note saved");
    app.press("body", "Escape").await.expect("escape");
    app.wait_for_count(".profile-card", 0, DEFAULT_WAIT)
        .await
        .expect("closed");
    open_card_for(&mut app, "bob").await;
    app.wait_for_value(".profile-card textarea.note", &note, DEFAULT_WAIT)
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
    app.wait_for_count(".profile-modal", 0, DEFAULT_WAIT)
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
    app.click(".profile-card .card-button.card-mod-view")
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
    app.click(".profile-card .card-button.card-mod-view")
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

/// Opening someone's card shows their picture at once: the copy already on
/// screen stands in while a larger one loads, instead of a letter.
#[tokio::test]
async fn the_card_shows_the_picture_already_loaded_at_once() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch");
    login_and_open(&mut app, "alice", "general").await;
    app.wait_for_js(
        "return [...document.querySelectorAll('.members .member img.avatar')].some(i => i.src.startsWith('data:'));",
        DEFAULT_WAIT,
    )
    .await
    .expect("member pictures loaded");
    let first = app
        .eval(
            "const pic = [...document.querySelectorAll('.members .member img.avatar')].find(i => i.src.startsWith('data:')); \
             const shown = pic.src; pic.closest('.member').click(); \
             for (let i = 0; i < 600; i++) { \
               const a = document.querySelector('.profile-card .avatar'); \
               if (a) return a.tagName === 'IMG' && a.src ? (a.src === shown ? 'the same picture' : 'another picture') : 'a placeholder'; \
               await new Promise(r => requestAnimationFrame(r)); \
             } \
             return 'no card';",
        )
        .await
        .expect("first frame");
    assert_eq!(
        first.as_str(),
        Some("the same picture"),
        "what the card showed first"
    );
}

/// The card's buttons along its top are small squares (not a bar across it).
/// It counts the servers you share (no list); the count, like its picture,
/// opens the full profile, which lists those servers with their icons, and
/// clicking one opens it.
#[tokio::test]
async fn the_card_counts_mutual_servers_and_the_full_profile_lists_them() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch");
    login_and_open(&mut app, "alice", "general").await;
    open_card_for(&mut app, "bob").await;
    let buttons = app
        .eval(
            "return [...document.querySelectorAll('.profile-card .card-actions button')] \
               .map(b => { const r = b.getBoundingClientRect(); return [b.title, Math.round(r.width), Math.round(r.height)]; });",
        )
        .await
        .expect("buttons");
    for b in buttons.as_array().expect("list") {
        let (w, h) = (b[1].as_f64().unwrap_or(0.0), b[2].as_f64().unwrap_or(0.0));
        assert!(w <= 40.0 && (w - h).abs() <= 4.0, "not a small square: {b}");
    }
    app.wait_for_text(
        ".profile-card .mutual-servers",
        "mutual server",
        DEFAULT_WAIT,
    )
    .await
    .expect("a count of mutual servers");
    assert_eq!(
        app.count(".profile-card .mutual-room, .profile-card .mutual-server")
            .await
            .expect("count"),
        0,
        "no list on the card"
    );
    let counted: usize = app
        .eval("return parseInt(document.querySelector('.profile-card .mutual-servers').textContent, 10);")
        .await
        .expect("count")
        .as_u64()
        .expect("a number") as usize;
    assert!(counted >= 1, "alice and bob share Engineering");
    app.click(".profile-card .mutual-servers")
        .await
        .expect("the count");
    app.wait_for(".profile-modal", DEFAULT_WAIT)
        .await
        .expect("the full profile");
    app.wait_for_js(
        &format!(
            "const s = [...document.querySelectorAll('.profile-modal .mutual-server')]; \
             return s.length === {counted} && s.every(x => x.querySelector('.avatar')) && s.some(x => x.textContent.includes('Engineering'));"
        ),
        DEFAULT_WAIT,
    )
    .await
    .expect("every mutual server, with its icon");
    app.click_text(".profile-modal .mutual-server", "Engineering")
        .await
        .expect("open Engineering");
    app.wait_for(".rail-item.selected[title=\"Engineering\"]", DEFAULT_WAIT)
        .await
        .expect("in Engineering");
    app.wait_for_js(
        "return !document.querySelector('.profile-modal');",
        DEFAULT_WAIT,
    )
    .await
    .expect("closed");

    // The card's picture opens the full profile too.
    app.open_room("general").await.expect("general");
    open_card_for(&mut app, "bob").await;
    app.click(".profile-card .card-avatar")
        .await
        .expect("the card's picture");
    app.wait_for(".profile-modal", DEFAULT_WAIT)
        .await
        .expect("the picture opens the full profile");
}

/// A card never runs off the app, even as its rooms and roles arrive after
/// it opened (opened low down, from the newest message's picture).
#[tokio::test]
async fn the_card_stays_on_screen_as_it_fills_in() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_sized(&hs, 1280, 720).await.expect("launch");
    login_and_open(&mut app, "bob", "general").await;
    app.wait_for(".message .avatar-button", DEFAULT_WAIT)
        .await
        .expect("messages");
    // The lowest picture in the timeline, someone else's.
    app.eval(
        "const b = [...document.querySelectorAll('.message:not(.own) .avatar-button')].sort((x, y) => y.getBoundingClientRect().top - x.getBoundingClientRect().top)[0]; \
         const r = b.getBoundingClientRect(); \
         b.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, clientX: r.left + 10, clientY: r.top + 10 })); return true;",
    )
    .await
    .expect("open a card low down");
    app.wait_for(".profile-card .mutual-servers", DEFAULT_WAIT)
        .await
        .expect("filled in");
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    let inside = app
        .eval(
            "const c = document.querySelector('.profile-card').getBoundingClientRect(); \
             const s = document.querySelector('.shell').getBoundingClientRect(); \
             return [c.top >= s.top - 1 && c.bottom <= Math.min(s.bottom, innerHeight) + 1, Math.round(c.top), Math.round(c.bottom), Math.round(s.bottom)];",
        )
        .await
        .expect("bounds");
    assert_eq!(inside[0], true, "the card ran off the app: {inside}");
}

/// Mutual servers are counted in a server with many members too (whose
/// members this device has not all loaded).
#[tokio::test]
async fn mutual_servers_count_in_a_busy_server() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let owner = new_account(&hs, "busyowner").expect("owner");
    let enc = |id: &str| id.replace('!', "%21").replace(':', "%3A");
    let space = api_as(
        &hs,
        &owner,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": "Busy server", "preset": "public_chat", "creation_content": { "type": "m.space" } })),
    )
    .expect("space");
    let space = space["room_id"].as_str().expect("id").to_owned();
    let room = api_as(
        &hs,
        &owner,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": "busy lobby", "preset": "public_chat" })),
    )
    .expect("room");
    let room = room["room_id"].as_str().expect("id").to_owned();
    api_as(
        &hs,
        &owner,
        "PUT",
        &format!("rooms/{}/state/m.space.child/{room}", enc(&space)),
        Some(serde_json::json!({ "via": ["localhost"] })),
    )
    .expect("child");
    // The person whose card is opened joined long ago: many have joined
    // since, so their joining is not among the server's recent events.
    let mut last = String::new();
    for n in 0..26 {
        let member = new_account(&hs, &format!("busy{n}")).expect("member");
        // Spaced out: the homeserver limits how fast joins come.
        std::thread::sleep(std::time::Duration::from_millis(400));
        api_as(
            &hs,
            &member,
            "POST",
            &format!("join/{}", enc(&space)),
            Some(serde_json::json!({})),
        )
        .expect("join space");
        if n == 0 {
            last = member;
        }
    }
    api_as(
        &hs,
        &last,
        "POST",
        &format!("join/{}", enc(&room)),
        Some(serde_json::json!({})),
    )
    .expect("join lobby");

    let mut app = App::launch(&hs).await.expect("launch");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", &owner)
        .await
        .expect("username");
    app.type_into(
        "form.login input[name=password]",
        &format!("{owner}-password"),
    )
    .await
    .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".shell", DEFAULT_WAIT)
        .await
        .expect("signed in");
    app.open_room("busy lobby").await.expect("lobby");
    app.click_text(".members .member", &last)
        .await
        .expect("their card");
    app.wait_for_text(
        ".profile-card .mutual-servers",
        "1 mutual server",
        DEFAULT_WAIT,
    )
    .await
    .expect("the busy server counted");
}

/// Share profile copies the profile's link and says so.
#[tokio::test]
async fn share_profile_says_the_link_was_copied() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch");
    login_and_open(&mut app, "alice", "general").await;
    open_card_for(&mut app, "bob").await;
    app.click(".profile-card .card-button.more")
        .await
        .expect("more");
    app.wait_for_text(".context-item", "Share profile", DEFAULT_WAIT)
        .await
        .expect("the menu");
    app.click_text(".context-item", "Share profile")
        .await
        .expect("share");
    app.wait_for_text(".toast", "Link copied", DEFAULT_WAIT)
        .await
        .expect("a toast saying so");
}
