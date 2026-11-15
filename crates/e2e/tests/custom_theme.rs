//! A custom theme colour: Settings > Appearance takes one hex colour (or a pick from a
//! grid of colours); the whole palette follows from it, text turns black on a light colour,
//! a live preview shows it, and it is kept across a restart. (GPUI desktop only.)

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn open_appearance(app: &mut App) {
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    app.click(".settings-button").await.expect("gear");
    app.wait_for(".settings", DEFAULT_WAIT)
        .await
        .expect("settings");
    app.click_text(".settings-nav button", "Appearance")
        .await
        .expect("Appearance");
    app.wait_for(".custom-colour input", DEFAULT_WAIT)
        .await
        .expect("the hex box");
}

async fn wait_attr(app: &mut App, selector: &str, name: &str, want: &str) {
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let got = app.attr(selector, name).await.expect("attr");
        if got.as_deref() == Some(want) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{selector} {name} is {got:?}, not {want}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

fn sum(hex: &str) -> u32 {
    let v = u32::from_str_radix(hex, 16).expect("hex");
    (v >> 16) + ((v >> 8) & 0xff) + (v & 0xff)
}

#[tokio::test]
async fn a_custom_colour_themes_the_whole_app_and_is_kept() {
    if !morpheus_e2e::gpui() {
        eprintln!("custom theme colour: GPUI desktop only; skipping");
        return;
    }
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login");
    app.type_into("form.login input[name=username]", "carol")
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", "carol-password")
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    open_appearance(&mut app).await;

    // A dark blue: the chat takes it, the rail and sidebar are darker shades, text stays white.
    app.type_into(".custom-colour input", "#2a4d8f")
        .await
        .expect("type a colour");
    wait_attr(&mut app, ".app", "data-bg", "2a4d8f").await;
    let rail = app
        .attr(".rail", "data-bg")
        .await
        .expect("rail")
        .expect("rail bg");
    let side = app
        .attr(".sidebar", "data-bg")
        .await
        .expect("sidebar")
        .expect("sidebar bg");
    assert!(
        sum(&rail) < sum(&side) && sum(&side) < sum("2a4d8f"),
        "{rail} {side}"
    );
    wait_attr(&mut app, ".app", "data-color", "ffffff").await;

    // The preview: a room item and message rows, one in a thread, in the same colours.
    for part in [
        ".custom-preview .preview-room",
        ".custom-preview .preview-message",
        ".custom-preview .preview-thread",
    ] {
        app.wait_for(part, DEFAULT_WAIT).await.expect(part);
    }
    wait_attr(&mut app, ".custom-preview", "data-bg", "2a4d8f").await;

    // A light colour: text flips to black.
    app.type_into(".custom-colour input", "#f2e9c9")
        .await
        .expect("type a light colour");
    wait_attr(&mut app, ".app", "data-bg", "f2e9c9").await;
    wait_attr(&mut app, ".app", "data-color", "000000").await;
    wait_attr(&mut app, ".custom-preview", "data-color", "000000").await;

    // Picking from the grid sets the colour too.
    app.click(".custom-colour .colour-swatch")
        .await
        .expect("pick");
    let picked = app
        .attr(".custom-colour .colour-swatch", "data-colour")
        .await
        .expect("swatch")
        .expect("colour");
    wait_attr(&mut app, ".app", "data-bg", &picked).await;
    app.type_into(".custom-colour input", "#f2e9c9")
        .await
        .expect("type again");
    wait_attr(&mut app, ".app", "data-bg", "f2e9c9").await;

    // Kept across a restart.
    let mut app = app.relaunch().await.expect("relaunch");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    wait_attr(&mut app, ".app", "data-bg", "f2e9c9").await;
    wait_attr(&mut app, ".app", "data-color", "000000").await;

    // Put Dark back for other tests.
    open_appearance(&mut app).await;
    app.click_text(".theme-option", "Dark").await.expect("Dark");
    wait_attr(&mut app, ".app", "data-bg", "1a1e29").await;
}

/// WCAG contrast of two `rrggbb` colours.
fn contrast(a: &str, b: &str) -> f64 {
    let lum = |hex: &str| {
        let v = u32::from_str_radix(hex, 16).expect("hex");
        let lin = |c: u32| {
            let c = f64::from(c) / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(v >> 16) + 0.7152 * lin((v >> 8) & 0xff) + 0.0722 * lin(v & 0xff)
    };
    let (la, lb) = (lum(a), lum(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The colour picker's own labels stay readable on a light and a dark custom colour (and
/// the awkward ones in between): its heading, its description, what is typed in the hex box.
#[tokio::test]
async fn the_pickers_labels_read_on_light_and_dark_custom_colours() {
    if !morpheus_e2e::gpui() {
        eprintln!("custom theme colour: GPUI desktop only; skipping");
        return;
    }
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch_desktop(&hs).await.expect("launch app");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login");
    app.type_into("form.login input[name=username]", "carol")
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", "carol-password")
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    open_appearance(&mut app).await;
    for colour in ["f2e9c9", "ffff00", "1a2b4c", "808080", "000000", "ffffff"] {
        app.type_into(".custom-colour input", &format!("#{colour}"))
            .await
            .expect("type a colour");
        wait_attr(&mut app, ".app", "data-bg", colour).await;
        // The boxes repaint a frame or two after the colour is applied.
        let deadline = std::time::Instant::now() + DEFAULT_WAIT;
        loop {
            let mut failure = None;
            for (label, selector) in [
                (
                    "heading",
                    ".settings-section[data-setting=custom-colour] h3",
                ),
                ("description", ".custom-colour p.muted"),
                ("hex box", ".custom-colour input"),
            ] {
                let fg = app
                    .attr(selector, "data-color")
                    .await
                    .expect("attr")
                    .unwrap_or_else(|| panic!("{colour}: the {label} has no colour"));
                let bg = app
                    .attr(selector, "data-bg")
                    .await
                    .expect("attr")
                    .unwrap_or_else(|| panic!("{colour}: the {label} has no background"));
                let ratio = contrast(&fg, &bg);
                if ratio < 4.5 {
                    failure = Some(format!(
                        "{colour}: the {label} is {fg} on {bg}, a contrast of {ratio:.2}"
                    ));
                }
            }
            let Some(failure) = failure else { break };
            assert!(std::time::Instant::now() < deadline, "{failure}");
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
    // Put Dark back for other tests.
    app.click_text(".theme-option", "Dark").await.expect("Dark");
    wait_attr(&mut app, ".app", "data-bg", "1a1e29").await;
}
