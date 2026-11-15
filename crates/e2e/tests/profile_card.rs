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

/// `who` writes a message that starts a new group (someone else writes just
/// before it, so it has a header and avatar even when `who` wrote last in an
/// earlier run) and its avatar is clicked. Tests of this file share rooms and
/// run side by side, so another's message can land between the two: then the
/// message is written again.
async fn open_card_from_new_message(app: &mut App, hs: &str, room_id: &str, who: &str, text: &str) {
    for attempt in 0..5 {
        let text = format!("{text} #{attempt}");
        morpheus_e2e::send_as(hs, "alice", room_id, "break between speakers").expect("a break");
        morpheus_e2e::send_as(hs, who, room_id, &text).expect("they write");
        app.wait_for_text(".message .body", &text, DEFAULT_WAIT)
            .await
            .expect("their message");
        if app.click_in_message(&text, ".avatar-button").await.is_ok() {
            return;
        }
    }
    panic!("no message of {who} started a group");
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
    app.wait_for_text(".context-item", "Report user profile", DEFAULT_WAIT)
        .await
        .expect("a report item in the more menu");
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
    app.wait_for(
        ".profile-card .report-user .ok, .profile-card .report-user .error",
        DEFAULT_WAIT,
    )
    .await
    .expect("server answered the report");
}

async fn gone_js(app: &mut App, selector: &str) {
    app.wait_for_count(selector, 0, DEFAULT_WAIT)
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
    if !app.is_gpui()
        && let Some(path) = app.screenshot("mod-view").await.expect("screenshot")
    {
        eprintln!("screenshot: {}", path.display());
    }

    // A role, then a kick, in #general.
    // The roles she may give, highest first (she created the room, so
    // Admin too): Admin, Moderator, Member. Moderator is the second.
    app.choose_option(&format!("{} select.mod-role", row("general")), 1)
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
    if app.is_gpui() {
        // A member's picture is an `img` once fetched; the card, opened from
        // that row, has a picture (not a letter) in its very first frame.
        app.wait_for(".members .member img.avatar", DEFAULT_WAIT)
            .await
            .expect("member pictures loaded");
        app.click(".members .member img.avatar")
            .await
            .expect("open a card from a member with a picture");
        assert!(
            app.count(".profile-card img.avatar").await.expect("count") > 0,
            "the card showed a letter first"
        );
        return;
    }
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
    for b in app
        .all_bounds(".profile-card .card-actions button")
        .await
        .expect("buttons")
    {
        assert!(
            b.w <= 40.0 && (b.w - b.h).abs() <= 4.0,
            "not a small square: {b:?}"
        );
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
    // The count fills in as the card finds the servers you share.
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    let (counted, said): (usize, String) = loop {
        let said = app
            .text(".profile-card .mutual-servers")
            .await
            .expect("count")
            .unwrap_or_default();
        let n = said
            .split_whitespace()
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        if n >= 1 || std::time::Instant::now() > deadline {
            break (n, said);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert!(
        counted >= 1,
        "alice and bob share Engineering (the card says {said:?})"
    );
    app.click(".profile-card .mutual-servers")
        .await
        .expect("the count");
    app.wait_for(".profile-modal", DEFAULT_WAIT)
        .await
        .expect("the full profile");
    app.wait_for_count(".profile-modal .mutual-server", counted, DEFAULT_WAIT)
        .await
        .expect("every mutual server");
    assert_eq!(
        app.count(".profile-modal .mutual-server .avatar")
            .await
            .expect("count"),
        counted,
        "each with its icon"
    );
    app.wait_for_text(".profile-modal .mutual-server", "Engineering", DEFAULT_WAIT)
        .await
        .expect("Engineering among them");
    app.click_text(".profile-modal .mutual-server", "Engineering")
        .await
        .expect("open Engineering");
    app.wait_for(".rail-item.selected[title=\"Engineering\"]", DEFAULT_WAIT)
        .await
        .expect("in Engineering");
    app.wait_for_count(".profile-modal", 0, DEFAULT_WAIT)
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
    // Someone else writes last, so their picture is the lowest in the timeline.
    let room_id = app
        .attr(".room.selected", "data-room")
        .await
        .expect("room id")
        .expect("a selected room");
    let text = format!("lowest picture {}", std::process::id());
    open_card_from_new_message(&mut app, &hs, &room_id, "carol", &text).await;
    app.wait_for(".profile-card .mutual-servers", DEFAULT_WAIT)
        .await
        .expect("filled in");
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    let card = app
        .bounds(".profile-card")
        .await
        .expect("card")
        .expect("a card");
    let shell = app.bounds(".shell").await.expect("shell").expect("a shell");
    assert!(
        card.y >= shell.y - 1.0 && card.bottom() <= shell.bottom() + 1.0,
        "the card ran off the app: {card:?} in {shell:?}"
    );
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

/// The person's id on the card is a way to the full profile (as the avatar
/// and the mutual-servers count are).
#[tokio::test]
async fn clicking_the_id_on_the_card_opens_the_full_profile() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch");
    login_and_open(&mut app, "alice", "general").await;
    if !app.is_gpui() {
        eprintln!("skipped: the Dioxus card's id is plain text");
        return;
    }
    open_card_for(&mut app, "bob").await;
    app.click(".profile-card .profile-id")
        .await
        .expect("the id");
    app.wait_for_text(".profile-modal .profile-id", "@bob:localhost", DEFAULT_WAIT)
        .await
        .expect("the full profile");
    app.wait_for_count(".profile-card", 0, DEFAULT_WAIT)
        .await
        .expect("instead of the card");
}

/// On the full profile the id is the profile's link: a click copies it and
/// says so, as Share profile does.
#[tokio::test]
async fn clicking_the_id_on_the_full_profile_copies_the_link() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch");
    login_and_open(&mut app, "alice", "general").await;
    if !app.is_gpui() {
        eprintln!("skipped: the Dioxus full profile's id is plain text");
        return;
    }
    open_card_for(&mut app, "bob").await;
    app.click(".profile-card .card-button.more")
        .await
        .expect("more");
    app.click_text(".context-item", "View full profile")
        .await
        .expect("full");
    app.wait_for(".profile-modal .profile-id", DEFAULT_WAIT)
        .await
        .expect("modal");
    assert_eq!(
        app.count(".toast").await.expect("toasts"),
        0,
        "no toast yet"
    );
    app.click(".profile-modal .profile-id")
        .await
        .expect("the id");
    app.wait_for_text(".toast", "Link copied", DEFAULT_WAIT)
        .await
        .expect("a toast saying so");
    // It stays open: only the link was copied.
    assert_eq!(app.count(".profile-modal").await.expect("modal"), 1);
}

/// The banner's colour as `[r, g, b]`, from its `data-bg` ("" without a picture).
async fn banner_colour(app: &mut App) -> Option<[i32; 3]> {
    let bg = app
        .attr(".profile-card .profile-banner", "data-bg")
        .await
        .expect("attr")
        .unwrap_or_default();
    let bg = bg.trim_start_matches('#');
    if bg.len() != 6 {
        return None;
    }
    let n = i32::from_str_radix(bg, 16).ok()?;
    Some([(n >> 16) & 255, (n >> 8) & 255, n & 255])
}

#[tokio::test]
async fn the_banner_takes_the_colour_of_the_picture() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let red = new_account(&hs, "bannerred").expect("account");
    let plain = new_account(&hs, "bannerplain").expect("account");
    for user in [&red, &plain] {
        admin_join(&hs, user, "#general:localhost").expect("join general");
    }
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(64, 64, image::Rgba([230, 20, 20, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .expect("png");
    morpheus_e2e::set_avatar_as(&hs, &red, &png.into_inner()).expect("picture");

    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    login_and_open(&mut app, "alice", "general").await;
    app.wait_for_text(".members .member", &plain, DEFAULT_WAIT)
        .await
        .expect("listed");

    // Without a picture the banner keeps its usual colour (no averaged one).
    open_card_for(&mut app, &plain).await;
    assert_eq!(
        banner_colour(&mut app).await,
        None,
        "no picture, no averaged colour"
    );
    app.press(".profile-card", "Escape").await.expect("close");
    app.wait_for_count(".profile-card", 0, DEFAULT_WAIT)
        .await
        .expect("closed");

    // With one, the banner is that picture's colour, darkened: red stays red.
    open_card_for(&mut app, &red).await;
    let started = std::time::Instant::now();
    loop {
        if let Some([r, g, b]) = banner_colour(&mut app).await {
            assert!(
                r > g + 30 && r > b + 30,
                "a red picture gives a reddish banner, not {r},{g},{b}"
            );
            break;
        }
        assert!(
            started.elapsed() < DEFAULT_WAIT,
            "the banner never took the picture's colour"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The role label arrives after the card opens; its place is held, so the
/// card's sections sit where they will stay (GPUI only).
#[tokio::test]
async fn the_card_does_not_shift_when_the_role_arrives() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_with_env(&hs, &[("MORPHEUS_E2E_CARD_DELAY_MS", "2500")])
        .await
        .expect("launch");
    if !app.is_gpui() {
        return;
    }
    login_and_open(&mut app, "alice", "general").await;
    open_card_for(&mut app, "bob").await;
    let first = app
        .bounds(".profile-card .card-section")
        .await
        .expect("bounds")
        .expect("a section");
    app.wait_for(
        ".profile-card .card-tags .tag:not(.skeleton)",
        DEFAULT_WAIT * 2,
    )
    .await
    .expect("the role arrives");
    let after = app
        .bounds(".profile-card .card-section")
        .await
        .expect("bounds")
        .expect("a section");
    assert!(
        (first.y - after.y).abs() < 0.5,
        "the note section moved from {} to {} when the role arrived",
        first.y,
        after.y
    );
}

/// A direct message between `owner` and `guest`, marked as one in the
/// owner's account data (as a client does).
fn dm_of(hs: &str, owner: &str, guest: &str) -> String {
    let made = api_as(
        hs,
        owner,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "invite": [format!("@{guest}:localhost")], "is_direct": true, "preset": "trusted_private_chat" })),
    )
    .expect("a DM");
    let room = made["room_id"].as_str().expect("room id").to_owned();
    api_as(
        hs,
        guest,
        "POST",
        &format!("join/{}", escaped(&room)),
        Some(serde_json::json!({})),
    )
    .expect("join");
    room
}

fn escaped(id: &str) -> String {
    id.replace('!', "%21")
        .replace('@', "%40")
        .replace(':', "%3A")
}

/// `user`'s membership in `room`, with `avatar` as its picture (as a
/// per-room picture is set), leaving their global profile as it is.
fn set_member_avatar(hs: &str, user: &str, room: &str, avatar: &str) {
    let state = format!(
        "rooms/{}/state/m.room.member/{}",
        escaped(room),
        escaped(&format!("@{user}:localhost"))
    );
    // A profile change reaches the room's member events after a moment: set
    // this one once that has happened, so it is not overwritten.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let global = morpheus_e2e::profile_of(hs, user).expect("profile")["avatar_url"].clone();
        let current = api_as(hs, user, "GET", &state, None).expect("member event");
        if current["avatar_url"] == global || std::time::Instant::now() > deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    api_as(
        hs,
        user,
        "PUT",
        &state,
        Some(
            serde_json::json!({ "membership": "join", "displayname": user, "avatar_url": avatar }),
        ),
    )
    .expect("member avatar");
}

/// A person's picture is kept twice, in their global profile and in their
/// member event of each room, and either can point at a file its server no
/// longer has (404). Wherever they show (the DM list, the member list, the
/// profile card) the one that loads is used: ben's global picture is gone
/// but his member event's is fine; cal's member event's is gone but his
/// global picture is fine.
#[tokio::test]
async fn a_picture_gone_from_its_server_falls_back_to_the_persons_other_one() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    // Fresh accounts: the seeded users' profiles are left alone.
    let ann = new_account(&hs, "avfann").expect("account");
    let ben = new_account(&hs, "avfben").expect("account");
    let cal = new_account(&hs, "avfcal").expect("account");
    let with_ben = dm_of(&hs, &ann, &ben);
    let with_cal = dm_of(&hs, &ann, &cal);
    api_as(
        &hs,
        &ann,
        "PUT",
        &format!("user/%40{ann}%3Alocalhost/account_data/m.direct"),
        Some(serde_json::json!({
            format!("@{ben}:localhost"): [with_ben.clone()],
            format!("@{cal}:localhost"): [with_cal.clone()],
        })),
    )
    .expect("m.direct");
    let picture = include_bytes!("../fixtures/framed.png");
    let gone = |who: &str| format!("mxc://localhost/gone{who}");

    // Ben: a good picture in the DM, a dead one in his profile.
    morpheus_e2e::set_avatar_as(&hs, &ben, picture).expect("ben's picture");
    let good = morpheus_e2e::profile_of(&hs, &ben).expect("profile")["avatar_url"]
        .as_str()
        .expect("an avatar")
        .to_owned();
    api_as(
        &hs,
        &ben,
        "PUT",
        &format!(
            "profile/{}/avatar_url",
            escaped(&format!("@{ben}:localhost"))
        ),
        Some(serde_json::json!({ "avatar_url": gone(&ben) })),
    )
    .expect("dead global picture");
    set_member_avatar(&hs, &ben, &with_ben, &good);
    // Cal: a good picture in his profile, a dead one in the DM.
    morpheus_e2e::set_avatar_as(&hs, &cal, picture).expect("cal's picture");
    set_member_avatar(&hs, &cal, &with_cal, &gone(&cal));

    let mut app = App::launch(&hs).await.expect("launch");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", &ann)
        .await
        .expect("username");
    app.type_into(
        "form.login input[name=password]",
        &format!("{ann}-password"),
    )
    .await
    .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".rail-dms", DEFAULT_WAIT)
        .await
        .expect("signed in");
    app.click(".rail-dms").await.expect("the DMs");
    for (who, room) in [(&ben, &with_ben), (&cal, &with_cal)] {
        app.wait_for(
            &format!(".room.dm[data-room=\"{room}\"] img.avatar"),
            DEFAULT_WAIT,
        )
        .await
        .unwrap_or_else(|e| panic!("{who}'s picture in the DM list: {e}"));
        app.click(&format!(".room.dm[data-room=\"{room}\"]"))
            .await
            .expect("open the DM");
        let member = format!(".members .member[title=\"@{who}:localhost\"]");
        app.wait_for(".chat-header .room-head img.avatar", DEFAULT_WAIT)
            .await
            .unwrap_or_else(|e| panic!("{who}'s picture in the top bar: {e}"));
        app.wait_for(&format!("{member} img.avatar"), DEFAULT_WAIT)
            .await
            .unwrap_or_else(|e| panic!("{who}'s picture in the member list: {e}"));
        app.click(&member).await.expect("open the card");
        app.wait_for(".profile-card img.avatar", DEFAULT_WAIT)
            .await
            .unwrap_or_else(|e| panic!("{who}'s picture on the profile card: {e}"));
        app.press("body", "Escape").await.expect("close the card");
        app.wait_for_count(".profile-card", 0, DEFAULT_WAIT)
            .await
            .expect("card closed");
    }
}

/// A DM whose own (room-level) picture is a dead one: the DM list and the
/// top bar, which show the room's picture, use the partner's picture instead
/// (they go through the same fallback as the member list and the card).
#[tokio::test]
async fn a_dm_whose_room_picture_is_gone_shows_the_partners_in_the_list_and_the_top_bar() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let ann = new_account(&hs, "dmgann").expect("account");
    let dan = new_account(&hs, "dmgdan").expect("account");
    let room = dm_of(&hs, &ann, &dan);
    api_as(
        &hs,
        &ann,
        "PUT",
        &format!("user/%40{ann}%3Alocalhost/account_data/m.direct"),
        Some(serde_json::json!({ format!("@{dan}:localhost"): [room.clone()] })),
    )
    .expect("m.direct");
    morpheus_e2e::set_avatar_as(&hs, &dan, include_bytes!("../fixtures/framed.png"))
        .expect("dan's picture");
    api_as(
        &hs,
        &ann,
        "PUT",
        &format!("rooms/{}/state/m.room.avatar", escaped(&room)),
        Some(serde_json::json!({ "url": "mxc://localhost/gonedmroom" })),
    )
    .expect("a dead room picture");

    let mut app = App::launch(&hs).await.expect("launch");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", &ann)
        .await
        .expect("username");
    app.type_into(
        "form.login input[name=password]",
        &format!("{ann}-password"),
    )
    .await
    .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".rail-dms", DEFAULT_WAIT)
        .await
        .expect("signed in");
    app.click(".rail-dms").await.expect("the DMs");
    let item = format!(".room.dm[data-room=\"{room}\"]");
    app.wait_for(&format!("{item} img.avatar"), DEFAULT_WAIT)
        .await
        .unwrap_or_else(|e| panic!("the partner's picture in the DM list: {e}"));
    app.click(&item).await.expect("open the DM");
    app.wait_for(".chat-header .room-head img.avatar", DEFAULT_WAIT)
        .await
        .unwrap_or_else(|e| panic!("the partner's picture in the top bar: {e}"));
}

/// A DM where the partner's member event has no picture at all (they set
/// theirs after joining, or only globally) but their global profile has one:
/// the DM list and the top bar show the global picture, as the member list does.
#[tokio::test]
async fn a_dm_whose_partner_has_no_member_picture_shows_their_global_one() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let ann = new_account(&hs, "dmnann").expect("account");
    let eve = new_account(&hs, "dmneve").expect("account");
    let made = api_as(&hs, &ann, "POST", "createRoom", Some(serde_json::json!({ "invite": [format!("@{eve}:localhost")], "is_direct": true, "preset": "trusted_private_chat" }))).expect("a DM");
    let room = made["room_id"].as_str().expect("room id").to_owned();
    api_as(
        &hs,
        &ann,
        "PUT",
        &format!("user/%40{ann}%3Alocalhost/account_data/m.direct"),
        Some(serde_json::json!({ format!("@{eve}:localhost"): [room.clone()] })),
    )
    .expect("m.direct");
    morpheus_e2e::set_avatar_as(&hs, &eve, include_bytes!("../fixtures/framed.png"))
        .expect("eve's picture");
    std::thread::sleep(Duration::from_secs(3));
    let member = api_as(
        &hs,
        &ann,
        "GET",
        &format!(
            "rooms/{}/state/m.room.member/{}",
            escaped(&room),
            escaped(&format!("@{eve}:localhost"))
        ),
        None,
    )
    .expect("member event");
    assert!(
        member.get("avatar_url").is_none(),
        "the member event still has a picture: {member}"
    );

    let mut app = App::launch(&hs).await.expect("launch");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", &ann)
        .await
        .expect("username");
    app.type_into(
        "form.login input[name=password]",
        &format!("{ann}-password"),
    )
    .await
    .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".rail-dms", DEFAULT_WAIT)
        .await
        .expect("signed in");
    app.click(".rail-dms").await.expect("the DMs");
    let item = format!(".room.dm[data-room=\"{room}\"]");
    app.wait_for(&item, DEFAULT_WAIT).await.expect("the DM");
    app.wait_for(&format!("{item} img.avatar"), DEFAULT_WAIT)
        .await
        .unwrap_or_else(|e| panic!("her global picture in the DM list: {e}"));
    app.click(&item).await.expect("open the DM");
    app.wait_for(".chat-header .room-head img.avatar", DEFAULT_WAIT)
        .await
        .unwrap_or_else(|e| panic!("her global picture in the top bar: {e}"));
}

/// While the card loads it does not claim "0 mutual servers": a skeleton of
/// the same size holds the place (and the status line's) until the count is
/// known (GPUI only).
#[tokio::test]
async fn the_card_shows_a_skeleton_not_zero_mutual_servers_while_loading() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_with_env(&hs, &[("MORPHEUS_E2E_CARD_DELAY_MS", "2500")])
        .await
        .expect("launch");
    if !app.is_gpui() {
        return;
    }
    login_and_open(&mut app, "alice", "general").await;
    open_card_for(&mut app, "bob").await;
    let card = app
        .text(".profile-card")
        .await
        .expect("text")
        .unwrap_or_default();
    assert!(!card.contains("0 mutual"), "the loading card says {card:?}");
    let first = app
        .bounds(".profile-card .mutual-servers")
        .await
        .expect("bounds")
        .expect("the mutual servers place");
    let status = app
        .count(".profile-card .profile-status.skeleton")
        .await
        .expect("count");
    assert_eq!(status, 1, "no skeleton for the status line while loading");
    app.wait_for(
        ".profile-card .mutual-servers:not(.skeleton)",
        DEFAULT_WAIT * 2,
    )
    .await
    .expect("the count arrives");
    let after = app
        .bounds(".profile-card .mutual-servers")
        .await
        .expect("bounds")
        .expect("the mutual servers");
    assert!(
        (first.y - after.y).abs() < 0.5 && (first.h - after.h).abs() < 0.5,
        "the line moved or changed size when the count arrived: {first:?} -> {after:?}"
    );
}

/// The card does not move or resize when its content finishes loading: not
/// for someone with a status, nor for someone without one, and not when it
/// is opened low down in the window, where a taller or shorter card would be
/// placed differently (GPUI only).
#[tokio::test]
async fn the_card_does_not_move_or_resize_when_it_finishes_loading() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app =
        App::launch_sized_with_env(&hs, 1280, 720, &[("MORPHEUS_E2E_CARD_DELAY_MS", "3000")])
            .await
            .expect("launch");
    if !app.is_gpui() {
        return;
    }
    // Carol has a status line, bob none.
    api_as(
        &hs,
        "carol",
        "PUT",
        "presence/%40carol%3Alocalhost/status",
        Some(serde_json::json!({ "presence": "online", "status_msg": "out for lunch" })),
    )
    .expect("a status");
    login_and_open(&mut app, "alice", "general").await;
    let room_id = app
        .attr(".room.selected", "data-room")
        .await
        .expect("room id")
        .expect("a selected room");
    for (who, has_status) in [("carol", true), ("bob", false)] {
        // Their message is the lowest, so the card opens near the window's bottom.
        let text = format!("low {who} {}", std::process::id());
        open_card_from_new_message(&mut app, &hs, &room_id, who, &text).await;
        app.wait_for(".profile-card .mutual-servers.skeleton", DEFAULT_WAIT)
            .await
            .expect("the card is still loading");
        let first = app
            .bounds(".profile-card")
            .await
            .expect("bounds")
            .expect("a card");
        app.wait_for(
            ".profile-card .mutual-servers:not(.skeleton)",
            DEFAULT_WAIT * 2,
        )
        .await
        .expect("it loads");
        let shown = app
            .count(".profile-card .profile-status:not(.skeleton)")
            .await
            .expect("count");
        assert_eq!(shown > 0, has_status, "{who}: the status line");
        let after = app
            .bounds(".profile-card")
            .await
            .expect("bounds")
            .expect("a card");
        assert!(
            (first.x - after.x).abs() < 0.5
                && (first.y - after.y).abs() < 0.5
                && (first.w - after.w).abs() < 0.5
                && (first.h - after.h).abs() < 0.5,
            "{who}: the card moved or resized on loading: {first:?} -> {after:?}"
        );
        app.press("body", "Escape").await.expect("close");
        app.wait_for_count(".profile-card", 0, DEFAULT_WAIT)
            .await
            .expect("closed");
    }
}

/// The full profile, opened while it loads, does not claim "Mutual servers — 0"
/// or "None yet": a skeleton holds the place until the servers are known
/// (GPUI only).
#[tokio::test]
async fn the_full_profile_shows_a_skeleton_not_zero_mutual_servers_while_loading() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_with_env(&hs, &[("MORPHEUS_E2E_CARD_DELAY_MS", "2500")])
        .await
        .expect("launch");
    if !app.is_gpui() {
        return;
    }
    login_and_open(&mut app, "alice", "general").await;
    open_card_for(&mut app, "bob").await;
    app.click(".profile-card .profile-id")
        .await
        .expect("the id");
    app.wait_for(".profile-modal", DEFAULT_WAIT)
        .await
        .expect("the full profile");
    let loading = app
        .text(".profile-modal")
        .await
        .expect("text")
        .unwrap_or_default();
    assert!(
        !loading.contains("None yet") && !loading.contains("servers — 0"),
        "the loading profile says {loading:?}"
    );
    assert_eq!(
        app.count(".profile-modal .mutual-skeleton")
            .await
            .expect("count"),
        1,
        "no skeleton for the mutual servers while loading"
    );
    app.wait_for(".profile-modal .mutual-server", DEFAULT_WAIT * 2)
        .await
        .expect("the servers arrive");
    assert_eq!(
        app.count(".profile-modal .mutual-skeleton")
            .await
            .expect("count"),
        0,
        "the skeleton stays"
    );
}
