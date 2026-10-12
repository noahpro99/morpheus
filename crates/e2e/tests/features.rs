//! Phase 1/2 features through the real window: media, rich text, reactions,
//! members, markdown sending, and scroll-triggered history loading.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login_and_open(app: &mut App, room: &str) {
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
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("room listed");
    app.open_room(room).await.expect("open room");
    app.wait_for_text(".chat-header h2", room, DEFAULT_WAIT)
        .await
        .expect("room header");
    app.wait_for(".composer input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
}

#[tokio::test]
async fn media_rich_text_reactions_and_members() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;

    // Avatars resolve to real images for senders, and the room icon shows in the header area.
    app.wait_for(".message img.avatar", DEFAULT_WAIT)
        .await
        .expect("sender avatar image");
    app.wait_for(".member img.avatar", DEFAULT_WAIT)
        .await
        .expect("member avatar image");

    // Sanitised HTML bodies render as markup, not escaped text. The seeded
    // rows are old by now, so scroll up until they are revealed.
    app.reveal_text(".message .rich strong", "fast", DEFAULT_WAIT)
        .await
        .expect("bold html");
    app.reveal_text(".message .rich pre code", "fn main", DEFAULT_WAIT)
        .await
        .expect("code block");

    // Inline image thumbnail.
    app.reveal_element(".message img.image", DEFAULT_WAIT)
        .await
        .expect("inline image");

    // Seeded reactions with aggregated counts.
    app.wait_for_text(".reaction", "👍", DEFAULT_WAIT)
        .await
        .expect("thumbs up chip");
    app.wait_for_js(
        "return [...document.querySelectorAll('.reaction')].some(el => el.textContent.includes('👍') && el.textContent.includes('2'));",
        DEFAULT_WAIT,
    )
    .await
    .expect("two thumbs up");

    // Members panel grouped by role.
    app.wait_for_text(".members-header", "Admins", DEFAULT_WAIT)
        .await
        .expect("admins group");
    for name in ["alice", "bob", "carol"] {
        app.wait_for_text(".member", name, DEFAULT_WAIT)
            .await
            .unwrap_or_else(|e| panic!("member {name}: {e}"));
    }
    app.wait_for_text(".chat-header .member-toggle", "3", DEFAULT_WAIT)
        .await
        .expect("member count");

    // Earlier runs may have left our ❤️ on the server; start from "off".
    let heart_mine = "return [...document.querySelectorAll('.reaction.mine')].some(el => el.textContent.includes('❤️'));";
    if app.eval(heart_mine).await.expect("eval").as_bool() == Some(true) {
        app.eval("[...document.querySelectorAll('.reaction.mine')].find(b => b.textContent.includes('❤️')).click();")
            .await
            .expect("clear leftover heart");
        app.wait_for_js(
            &format!("{{ const on = (() => {{ {heart_mine} }})(); return !on; }}"),
            DEFAULT_WAIT,
        )
        .await
        .expect("leftover heart cleared");
    }

    // Toggle a reaction on via the hover bar, then off via the chip.
    app.eval(
        "const msg = [...document.querySelectorAll('.message')].find(m => m.textContent.includes('react to this one'));
         [...msg.querySelectorAll('.action')].find(b => b.textContent.includes('❤️')).click();",
    )
    .await
    .expect("click heart action");
    app.wait_for_text(".reaction.mine", "❤️", DEFAULT_WAIT)
        .await
        .expect("own heart reaction");
    app.eval("[...document.querySelectorAll('.reaction.mine')].find(b => b.textContent.includes('❤️')).click();")
        .await
        .expect("click heart chip");
    app.wait_for_js(
        "return ![...document.querySelectorAll('.reaction.mine')].some(el => el.textContent.includes('❤️'));",
        DEFAULT_WAIT,
    )
    .await
    .expect("heart reaction removed");

    // Markdown is rendered on send.
    let body = format!("**bold** from e2e {}", std::process::id());
    app.type_into(".composer input", &body).await.expect("type");
    app.press(".composer input", "Enter").await.expect("enter");
    app.wait_for_text(".message:not(.pending) .rich strong", "bold", DEFAULT_WAIT)
        .await
        .expect("markdown rendered");

    if let Some(path) = app.screenshot("features").await.expect("screenshot") {
        eprintln!("screenshot: {}", path.display());
    }
}

#[tokio::test]
async fn clicking_image_opens_lightbox() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "general").await;

    // Open the seeded inline image thumbnail (old by now: scroll up to it).
    app.reveal_element(".message img.image", DEFAULT_WAIT)
        .await
        .expect("inline image");
    app.click(".message img.image").await.expect("click image");

    // The lightbox appears with the full-resolution image as a data URL.
    app.wait_for(".lightbox-overlay", DEFAULT_WAIT)
        .await
        .expect("lightbox opens");
    app.wait_for_js(
        "const img = document.querySelector('.lightbox-image'); return !!img && img.src.startsWith('data:image/');",
        DEFAULT_WAIT,
    )
    .await
    .expect("full-size image loaded as a data URL");

    // Esc closes it.
    app.press(".lightbox-overlay", "Escape")
        .await
        .expect("escape");
    app.wait_for_js(
        "return document.querySelector('.lightbox-overlay') === null;",
        DEFAULT_WAIT,
    )
    .await
    .expect("lightbox closes");
}

#[tokio::test]
async fn scrolling_up_loads_older_history() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    login_and_open(&mut app, "random").await;

    // The newest page is there, the oldest message is not yet.
    app.wait_for_exact_text(".message .body", "random filler message 80", DEFAULT_WAIT)
        .await
        .expect("newest");
    app.wait_for_js(
        "return ![...document.querySelectorAll('.message .body')].some(el => el.textContent.trim() === 'random filler message 1');",
        DEFAULT_WAIT,
    )
    .await
    .expect("oldest not loaded initially");

    // Keep scrolling to the top, like a user would, until the room start is reached.
    for _ in 0..8 {
        app.eval("const t = document.querySelector('.timeline'); t.scrollTop = -t.scrollHeight;")
            .await
            .expect("scroll");
        if app
            .wait_for(".timeline-start", std::time::Duration::from_secs(5))
            .await
            .is_ok()
        {
            break;
        }
    }
    app.wait_for_exact_text(".message .body", "random filler message 1", DEFAULT_WAIT)
        .await
        .expect("oldest loaded");
    app.wait_for_text(".timeline-start", "beginning", DEFAULT_WAIT)
        .await
        .expect("start marker");
}
