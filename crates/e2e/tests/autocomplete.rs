//! Composer helpers: `:code` emoji autocomplete, `@name` mention
//! autocomplete, and the emoji panel.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login_and_open(app: &mut App, room: &str) {
    app.wait_for("form.login", DEFAULT_WAIT).await.expect("login form");
    app.type_into("form.login input[name=username]", "alice").await.expect("username");
    app.type_into("form.login input[name=password]", "alice-password").await.expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms listed");
    app.open_room(room).await.expect("open room");
    app.wait_for(".composer input:not([disabled])", DEFAULT_WAIT).await.expect("composer ready");
}

#[tokio::test]
async fn emoji_and_mention_autocomplete_and_emoji_panel() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;

    // :fir → 🔥 fire, Enter inserts the emoji in place of the code.
    app.type_into(".composer input", "hot :fir").await.expect("type code");
    app.wait_for_text(".autocomplete-item", "fire", DEFAULT_WAIT).await.expect("emoji suggested");
    app.press(".composer input", "Enter").await.expect("accept");
    app.wait_for_js("return document.querySelector('.composer input').value === 'hot 🔥 '", DEFAULT_WAIT).await.expect("emoji inserted");
    assert_eq!(app.count(".autocomplete").await.expect("count"), 0, "suggestions closed");

    // @bo → bob from the member list; Down then Enter picks the second row if any, Tab accepts.
    app.type_into(".composer input", "hey @bo").await.expect("type mention");
    app.wait_for_text(".autocomplete-item", "bob", DEFAULT_WAIT).await.expect("member suggested");
    app.press(".composer input", "Tab").await.expect("accept with tab");
    app.wait_for_js("return document.querySelector('.composer input').value === 'hey @bob '", DEFAULT_WAIT).await.expect("mention inserted");

    // Escape closes suggestions without inserting.
    app.type_into(".composer input", ":hea").await.expect("type code");
    app.wait_for(".autocomplete-item", DEFAULT_WAIT).await.expect("suggested");
    app.press(".composer input", "Escape").await.expect("escape");
    app.wait_for_js("return !document.querySelector('.autocomplete')", DEFAULT_WAIT).await.expect("closed");
    app.wait_for_js("return document.querySelector('.composer input').value === ':hea'", DEFAULT_WAIT).await.expect("draft kept");

    // The emoji panel inserts at the end and closes.
    app.type_into(".composer input", "").await.expect("clear");
    app.click(".emoji-button").await.expect("open panel");
    app.wait_for(".emoji-panel", DEFAULT_WAIT).await.expect("panel");
    app.click(".emoji-panel button[title=rocket]").await.expect("pick rocket");
    app.wait_for_js("return document.querySelector('.composer input').value === '🚀 '", DEFAULT_WAIT).await.expect("rocket inserted");
    app.wait_for_js("return !document.querySelector('.emoji-panel')", DEFAULT_WAIT).await.expect("panel closed");
}
