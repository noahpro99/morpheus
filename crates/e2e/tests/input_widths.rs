//! Every text input fills its form's content width (as the login screen's
//! do): its width equals its container's width less the container's
//! padding, within 2 px. (GPUI only.)

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn sign_in(app: &mut App, user: &str) {
    app.wait_for("form.login", DEFAULT_WAIT).await.expect("login form");
    app.type_into("form.login input[name=username]", user).await.expect("username");
    app.type_into("form.login input[name=password]", &format!("{user}-password"))
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
}

/// Notes a miss when `input` is not `container`'s width less `inset` (both sides' padding).
async fn fill(app: &mut App, misses: &mut Vec<String>, container: &str, inset: f64, input: &str) {
    let c = app.bounds(container).await.expect("bounds").unwrap_or_else(|| panic!("no {container}"));
    let i = app.bounds(input).await.expect("bounds").unwrap_or_else(|| panic!("no {input}"));
    let want = c.w - inset;
    eprintln!("{input}: {:.1} wide, {container} content {:.1}", i.w, want);
    if (i.w - want).abs() > 2.0 {
        misses.push(format!("{input} is {:.1} wide, {container}'s content {want:.1}", i.w));
    }
}

async fn chip_item(app: &mut App, item: &str) {
    app.click(".profile-chip").await.expect("chip");
    app.wait_for(".context-menu", DEFAULT_WAIT).await.expect("account menu");
    app.click_text(".context-item", item).await.expect(item);
}

#[tokio::test]
async fn text_inputs_fill_their_forms() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch");
    if !app.is_gpui() {
        return;
    }
    let mut misses = Vec::new();
    app.wait_for("form.login", DEFAULT_WAIT).await.expect("login form");
    for name in ["homeserver", "username", "password"] {
        fill(&mut app, &mut misses, "form.login", 64.0, &format!("form.login input[name={name}]")).await;
    }
    sign_in(&mut app, "alice").await;
    app.open_room("general").await.expect("general");

    // Add an account: the same form, with a way back.
    app.click(".profile-chip").await.expect("chip");
    app.wait_for(".context-menu", DEFAULT_WAIT).await.expect("account menu");
    app.click_submenu("Switch account", "Add an account").await.expect("add");
    app.wait_for_text("form.login button", "Back to", DEFAULT_WAIT).await.expect("login");
    for name in ["homeserver", "username", "password"] {
        fill(&mut app, &mut misses, "form.login", 64.0, &format!("form.login input[name={name}]")).await;
    }
    app.click_text("form.login button", "Back to").await.expect("back");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    app.open_room("general").await.expect("general");

    // Create room.
    app.click(".create-room-button").await.expect("plus");
    app.wait_for(".create-room-dialog input[name=name]", DEFAULT_WAIT).await.expect("dialog");
    for name in ["name", "topic"] {
        fill(&mut app, &mut misses, ".create-room-dialog", 40.0, &format!(".create-room-dialog input[name={name}]")).await;
    }
    app.press(".create-room-dialog", "Escape").await.expect("close");

    // Edit profile and set status.
    chip_item(&mut app, "Edit profile").await;
    app.wait_for(".profile-editor input[name=displayname]", DEFAULT_WAIT).await.expect("editor");
    fill(&mut app, &mut misses, ".profile-editor", 40.0, ".profile-editor input[name=displayname]").await;
    app.press(".profile-editor", "Escape").await.expect("close");
    chip_item(&mut app, "Set status").await;
    app.wait_for(".status-editor input[name=status]", DEFAULT_WAIT).await.expect("status");
    fill(&mut app, &mut misses, ".status-editor", 40.0, ".status-editor input[name=status]").await;
    app.press(".status-editor", "Escape").await.expect("close");

    // Report a message.
    app.reveal_element(".message:not(.own):not(.system):not(.skeleton-row)", DEFAULT_WAIT)
        .await
        .expect("another sender's message");
    app.right_click(".message:not(.own):not(.system):not(.skeleton-row)").await.expect("menu");
    app.click_text(".context-item", "Report message").await.expect("report");
    app.wait_for(".report-dialog input[name=reason]", DEFAULT_WAIT).await.expect("report");
    fill(&mut app, &mut misses, ".report-dialog", 40.0, ".report-dialog input[name=reason]").await;
    app.press(".report-dialog", "Escape").await.expect("close");

    // A person's card: note, message, report, and the moderation sheet.
    app.click_text(".members .member", "bob").await.expect("member");
    app.wait_for(".profile-card [name=note]", DEFAULT_WAIT).await.expect("card");
    fill(&mut app, &mut misses, ".profile-card .card-section", 22.0, ".profile-card [name=note]").await;
    fill(&mut app, &mut misses, ".profile-card .card-section", 24.0, ".profile-card input[name=message]").await;
    app.click(".profile-card .card-button.card-mod-view").await.expect("mod view");
    app.wait_for(".mod-view input[name=reason]", DEFAULT_WAIT).await.expect("mod view");
    fill(&mut app, &mut misses, ".mod-view", 40.0, ".mod-view input[name=reason]").await;
    app.press("body", "Escape").await.expect("close the mod view");
    app.wait_for_count(".mod-view", 0, DEFAULT_WAIT).await.expect("closed");
    if app.count(".profile-card").await.expect("count") == 0 {
        app.click_text(".members .member", "bob").await.expect("member again");
        app.wait_for(".profile-card .card-button.more", DEFAULT_WAIT).await.expect("card");
    }
    app.click(".profile-card .card-button.more").await.expect("more");
    app.click_text(".context-item", "Report user profile").await.expect("report");
    app.wait_for(".profile-card .report-user input[name=reason]", DEFAULT_WAIT).await.expect("form");
    fill(&mut app, &mut misses, ".profile-card .report-user", 32.0, ".profile-card .report-user input[name=reason]").await;
    app.click_text(".profile-card .report-user button", "Cancel").await.expect("cancel");
    app.click_at(".card-overlay", -300.0, 0.0, 1).await.expect("click away");
    app.wait_for_count(".profile-card", 0, DEFAULT_WAIT).await.expect("card closed");

    // Settings: the recovery key field.
    app.click(".settings-button").await.expect("gear");
    app.wait_for(".settings", DEFAULT_WAIT).await.expect("settings");
    app.click_text(".settings-nav button", "Security & Privacy").await.expect("security");
    app.wait_for(".recovery-form input", DEFAULT_WAIT).await.expect("recovery form");
    fill(&mut app, &mut misses, ".recovery-form", 0.0, ".recovery-form input").await;

    assert!(misses.is_empty(), "inputs narrower than their form:\n{}", misses.join("\n"));
}
