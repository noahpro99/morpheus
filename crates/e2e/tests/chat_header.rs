//! The bar over a chat, as Discord lays it out: it runs from the chat to
//! the window's right edge, with the side panel (members, a thread, search)
//! under it as a card; a call button only where a call belongs (DMs and
//! voice channels), at the right beside the pins and the search bar.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn login(app: &mut App, user: &str) {
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
}

#[tokio::test]
async fn the_top_bar_spans_to_the_right_over_a_side_card() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch");
    login(&mut app, "alice").await;
    app.open_room("general").await.expect("general");
    app.wait_for(".side-pane .members", DEFAULT_WAIT)
        .await
        .expect("members shown");
    let layout = app
        .eval(
            "const h = document.querySelector('.chat-header').getBoundingClientRect(); \
             const p = document.querySelector('.side-pane'); const s = p.getBoundingClientRect(); \
             const cs = getComputedStyle(p); \
             return { width: innerWidth, height: innerHeight, header_right: h.right, header_bottom: h.bottom, \
               side_top: s.top, side_right: s.right, side_bottom: s.bottom, \
               radii: [cs.borderTopLeftRadius, cs.borderTopRightRadius, cs.borderBottomRightRadius, cs.borderBottomLeftRadius].map(parseFloat) };",
        )
        .await
        .expect("layout");
    let num = |key: &str| layout[key].as_f64().expect("number");
    assert!(
        num("header_right") >= num("width") - 1.0,
        "the top bar stops short of the right edge: {layout}"
    );
    assert!(
        num("side_top") >= num("header_bottom"),
        "the side panel is not under the top bar: {layout}"
    );
    assert!(
        num("side_right") < num("width") && num("side_bottom") < num("height"),
        "the side panel is not inset like a card: {layout}"
    );
    let radii = layout["radii"].as_array().expect("radii");
    assert!(
        radii.iter().all(|r| r.as_f64().unwrap_or(0.0) >= 6.0),
        "the side panel is not rounded on all four corners: {layout}"
    );
}

#[tokio::test]
async fn a_call_button_only_where_calls_belong() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch");
    login(&mut app, "alice").await;

    // A text channel: no call button.
    app.open_room("general").await.expect("general");
    app.wait_for_text(".chat-header h2", "general", DEFAULT_WAIT)
        .await
        .expect("in general");
    assert_eq!(
        app.count(".chat-header .call-button").await.expect("count"),
        0,
        "a call button in a text channel"
    );

    // A DM: the call button at the right, by the pins, then the search bar.
    app.click(".rail-item[title=\"Direct messages\"]")
        .await
        .expect("direct messages");
    app.open_room("bob").await.expect("the dm");
    app.wait_for(".chat-header .call-button", DEFAULT_WAIT)
        .await
        .expect("a call button in a DM");
    let order = app
        .eval(
            "const header = document.querySelector('.chat-header'); \
             const kids = [...header.children]; \
             const at = sel => kids.indexOf(header.querySelector(':scope > ' + sel)); \
             const search = header.querySelector(':scope > .search-toggle'); \
             return { call: at('.call-button'), pin: at('.pin-toggle'), search: at('.search-toggle'), last: kids.length - 1, \
               search_width: search ? search.getBoundingClientRect().width : 0, \
               search_gap: search ? header.getBoundingClientRect().right - search.getBoundingClientRect().right : 999 };",
        )
        .await
        .expect("order");
    let at = |key: &str| order[key].as_i64().expect("index");
    assert_eq!(
        at("call") + 1,
        at("pin"),
        "the call button is not beside the pins: {order}"
    );
    assert_eq!(
        at("search"),
        at("last"),
        "the search bar is not at the right end: {order}"
    );
    assert!(
        order["search_width"].as_f64().unwrap_or(0.0) >= 140.0,
        "search is a button, not a bar: {order}"
    );
    assert!(
        order["search_gap"].as_f64().unwrap_or(999.0) <= 24.0,
        "the search bar is not at the top right: {order}"
    );
}
