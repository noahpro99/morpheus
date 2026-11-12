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
    app.wait_for(".composer .composer-input:not([disabled])", DEFAULT_WAIT)
        .await
        .expect("composer ready");
}

#[tokio::test]
async fn media_rich_text_reactions_and_members() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
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
    // (GPUI draws bold and code as styled runs of the body, not elements:
    // the text of the rich body is checked there.)
    let (bold, code) = if app.is_gpui() {
        ((".message .body.rich", "looks fast"), (".message .body.rich", "fn main"))
    } else {
        ((".message .rich strong", "fast"), (".message .rich pre code", "fn main"))
    };
    app.reveal_text(bold.0, bold.1, DEFAULT_WAIT)
        .await
        .expect("bold html");
    // Inline image thumbnail (just below the bold line).
    app.reveal_element(".message img.image", DEFAULT_WAIT)
        .await
        .expect("inline image");
    if app.is_gpui() {
        // Revealing scrolls to the top (the virtualised list draws only what
        // is near the view); the code block and the reactions are further
        // down: scroll down until each is drawn.
        let mut at = scroll_down_until(&mut app, code.0, code.1, None).await;
        at = scroll_down_until(&mut app, ".reaction", "👍", Some(at)).await;
        let _ = at;
    } else {
        app.reveal_text(code.0, code.1, DEFAULT_WAIT)
            .await
            .expect("code block");
    }

    // Seeded reactions with aggregated counts.
    app.wait_for_text(".reaction", "👍", DEFAULT_WAIT)
        .await
        .expect("thumbs up chip");
    wait_reaction(&mut app, "👍", Some("2"), false, true).await;

    // Members panel grouped by role.
    app.wait_for_text(".members-header", "Admins", DEFAULT_WAIT)
        .await
        .expect("admins group");
    for name in ["alice", "bob", "carol"] {
        app.wait_for_text(".member", name, DEFAULT_WAIT)
            .await
            .unwrap_or_else(|e| panic!("member {name}: {e}"));
    }
    // The seeded #general has grown past its first three members: the count
    // on the toggle shows at least those.
    let counted = app
        .text(".chat-header .member-toggle")
        .await
        .expect("member toggle")
        .unwrap_or_default();
    let digits: String = counted.chars().filter(char::is_ascii_digit).collect();
    assert!(
        digits.parse::<u32>().is_ok_and(|n| n >= 3),
        "member count {counted:?}"
    );

    // Earlier runs may have left our ❤️ on the server; start from "off".
    if heart_mine(&mut app).await {
        app.click_text(".reaction.mine", "❤️")
            .await
            .expect("clear leftover heart");
        wait_reaction(&mut app, "❤️", None, true, false).await;
    }

    // Toggle a reaction on via the hover bar, then off via the chip.
    app.click_in_message("react to this one", ".action[title=\"React with ❤️\"]")
        .await
        .expect("click heart action");
    app.wait_for_text(".reaction.mine", "❤️", DEFAULT_WAIT)
        .await
        .expect("own heart reaction");
    app.click_text(".reaction.mine", "❤️")
        .await
        .expect("click heart chip");
    wait_reaction(&mut app, "❤️", None, true, false).await;

    // Markdown is rendered on send.
    let body = format!("**bold** from e2e {}", std::process::id());
    app.type_into(".composer .composer-input", &body)
        .await
        .expect("type");
    app.press(".composer .composer-input", "Enter")
        .await
        .expect("enter");
    // (GPUI draws bold as a styled run of the body, not an element: its text
    // is checked instead.)
    let (rendered, needle) = if app.is_gpui() {
        (".message:not(.pending) .body.rich", "bold from e2e")
    } else {
        (".message:not(.pending) .rich strong", "bold")
    };
    app.wait_for_text(rendered, needle, DEFAULT_WAIT)
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
    if app.is_gpui() {
        // No CSS animation or data URLs here: the full-size picture is a
        // `.lightbox-image` once it has been fetched.
        app.wait_for(".lightbox-image", DEFAULT_WAIT)
            .await
            .expect("full-size image loaded");
        // The dimming covers the whole window, the member list too, and the
        // image is in the window's middle (not the chat's).
        let window = app.bounds("body").await.expect("body").expect("a window");
        let overlay = app
            .bounds(".lightbox-overlay")
            .await
            .expect("overlay")
            .expect("an overlay");
        let image = app
            .bounds(".lightbox-image")
            .await
            .expect("image")
            .expect("an image");
        assert!(
            overlay.x <= window.x
                && overlay.y <= window.y
                && overlay.right() >= window.right()
                && overlay.bottom() >= window.bottom(),
            "the dimming does not cover the window: {overlay:?} vs {window:?}"
        );
        assert!(
            (image.centre_x() - (window.x + window.w / 2.0)).abs() < 4.0,
            "the image is not in the window's middle: {image:?} in {window:?}"
        );
        // Esc closes it.
        app.press("body", "Escape").await.expect("escape");
        app.wait_for_count(".lightbox-overlay", 0, DEFAULT_WAIT)
            .await
            .expect("lightbox closes");
        return;
    }
    // It zooms in rather than popping.
    app.wait_for_js(
        "return getComputedStyle(document.querySelector('.lightbox')).animationName === 'lightbox-zoom'",
        DEFAULT_WAIT,
    )
    .await
    .expect("lightbox opens with an animation");
    app.wait_for_js(
        "const img = document.querySelector('.lightbox-image'); return !!img && img.src.startsWith('data:image/');",
        DEFAULT_WAIT,
    )
    .await
    .expect("full-size image loaded as a data URL");
    // The dimming covers the whole window, the member list too, and the
    // image is in the window's middle (not the chat's).
    if !app.is_phone() {
        app.wait_for(".members", DEFAULT_WAIT)
            .await
            .expect("the member list beside the chat");
    }
    app.wait_for_js(
        "const o = document.querySelector('.lightbox-overlay').getBoundingClientRect(), \
           i = document.querySelector('.lightbox-image').getBoundingClientRect(), \
           m = document.querySelector('.members'); \
         const top = m && document.elementFromPoint(m.getBoundingClientRect().left + 20, innerHeight / 2); \
         return o.left <= 0 && o.top <= 0 && o.right >= innerWidth && o.bottom >= innerHeight \
           && Math.abs((i.left + i.right) / 2 - innerWidth / 2) < 4 \
           && (!m || !!top?.closest('.lightbox-overlay'));",
        DEFAULT_WAIT,
    )
    .await
    .expect("dims the whole window, image centred");

    // Esc closes it.
    app.press("body", "Escape").await.expect("escape");
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
    // A room of its own (#random grows with every test run): 120 messages.
    let user = morpheus_e2e::new_account(&hs, "history").expect("account");
    let room = morpheus_e2e::api_as(
        &hs,
        &user,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": "filler" })),
    )
    .expect("a room");
    let room = room["room_id"].as_str().expect("room id").to_owned();
    let bodies: Vec<String> = (1..=120)
        .map(|n| format!("random filler message {n}"))
        .collect();
    morpheus_e2e::send_many_as(&hs, &user, &room, &bodies).expect("history");
    let mut app = App::launch(&hs).await.expect("launch app");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", &user)
        .await
        .expect("username");
    app.type_into(
        "form.login input[name=password]",
        &format!("{user}-password"),
    )
    .await
    .expect("password");
    app.submit("form.login").await.expect("submit");
    app.open_room("filler").await.expect("open the room");

    // The timeline has loaded (newer test traffic sits above the seeded
    // filler by now); the oldest filler message is not there yet.
    app.wait_for(".message .body", DEFAULT_WAIT)
        .await
        .expect("newest");
    let oldest_shown = |bodies: Vec<String>| bodies.iter().any(|b| b.trim() == "random filler message 1");
    assert!(
        !oldest_shown(app.texts(".message .body").await.expect("bodies")),
        "oldest loaded initially"
    );

    // Keep scrolling to the top, like a user would, until the room start is
    // reached (#random grows with every test run: give it time, not tries).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(240);
    while std::time::Instant::now() < deadline {
        let at_start = if app.is_gpui() {
            app.scroll_to(".timeline", morpheus_e2e::ScrollTo::Top)
                .await
                .expect("scroll");
            app.count(".timeline-start").await.expect("count") > 0
        } else {
            app.eval("const t = document.querySelector('.timeline'); t.scrollTop = -t.scrollHeight; return !!document.querySelector('.timeline-start');")
                .await
                .expect("scroll")
                .as_bool()
                == Some(true)
        };
        if at_start {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
    app.wait_for_exact_text(".message .body", "random filler message 1", DEFAULT_WAIT)
        .await
        .expect("oldest loaded");
    app.wait_for_text(".timeline-start", "beginning", DEFAULT_WAIT)
        .await
        .expect("start marker");
}

/// Whether our own ❤️ is among the reactions shown.
async fn heart_mine(app: &mut App) -> bool {
    app.texts(".reaction.mine")
        .await
        .expect("reactions")
        .iter()
        .any(|text| text.contains("❤️"))
}

/// Wait until a reaction chip with `key` (and `count`, if given) is shown
/// (`present`), or until none is. `mine` looks at our own chips only.
async fn wait_reaction(
    app: &mut App,
    key: &str,
    count: Option<&str>,
    mine: bool,
    present: bool,
) {
    let selector = if mine { ".reaction.mine" } else { ".reaction" };
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let shown = app
            .texts(selector)
            .await
            .expect("reactions")
            .iter()
            .any(|text| text.contains(key) && count.is_none_or(|c| text.contains(c)));
        if shown == present {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "reaction {key} {count:?}: wanted shown = {present}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// GPUI: from the top of a room, scrolls down a little at a time until an
/// element matching `selector` with `needle` in its text is drawn. Returns
/// the scroll position (pixels up from the bottom) it stopped at.
async fn scroll_down_until(
    app: &mut App,
    selector: &str,
    needle: &str,
    from: Option<f64>,
) -> f64 {
    let mut from_bottom = match from {
        Some(at) => at,
        None => app
            .scroll_state(".timeline")
            .await
            .expect("scroll state")
            .expect("a timeline")
            .range,
    };
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        if app
            .texts(selector)
            .await
            .expect("texts")
            .iter()
            .any(|t| t.contains(needle))
        {
            return from_bottom;
        }
        assert!(
            std::time::Instant::now() < deadline && from_bottom > 0.0,
            "{selector} with {needle:?} not drawn scrolling down from the top"
        );
        from_bottom = (from_bottom - 250.0).max(0.0);
        app.scroll_to(".timeline", morpheus_e2e::ScrollTo::FromBottom(from_bottom))
            .await
            .expect("down a little");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}
