//! What you type into a text input can be read: the colour it is drawn in
//! stands out from the input's background (`data-color` against `data-bg`,
//! a contrast of 4.5 or more), and, when the screen can be captured, the
//! typed letters really show in the input. (GPUI only.)

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, Bounds, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

/// `rrggbb` (or `#rrggbb`) as RGB.
fn hex(colour: &str) -> Option<[u8; 3]> {
    let digits = colour.trim_start_matches('#');
    let value = u32::from_str_radix(digits.get(..6)?, 16).ok()?;
    Some([(value >> 16) as u8, (value >> 8) as u8, value as u8])
}

/// WCAG's relative luminance and contrast ratio.
fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let lum = |c: [u8; 3]| {
        let ch = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.039_28 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * ch(c[0]) + 0.7152 * ch(c[1]) + 0.0722 * ch(c[2])
    };
    let (la, lb) = (lum(a), lum(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The screen can be captured: Hyprland, the session not locked.
fn can_capture() -> bool {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
        && !std::process::Command::new("pgrep")
            .args(["-x", "hyprlock"])
            .status()
            .is_ok_and(|s| s.success())
}

/// The window's top-left corner on the screen (by the app's pid).
fn window_at(pid: u32) -> Option<(f64, f64)> {
    let out = std::process::Command::new("hyprctl").args(["clients", "-j"]).output().ok()?;
    let clients: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let window = clients
        .as_array()?
        .iter()
        .find(|c| c["pid"].as_u64() == Some(u64::from(pid)))?
        .clone();
    Some((window["at"][0].as_f64()?, window["at"][1].as_f64()?))
}

/// The pixels (RGB) of `area` of the screen, at scale 1.
fn capture(area: &Bounds, at: (f64, f64)) -> Option<Vec<[u8; 3]>> {
    let geometry = format!(
        "{},{} {}x{}",
        (at.0 + area.x).round(),
        (at.1 + area.y).round(),
        area.w.round(),
        area.h.round()
    );
    let out = std::process::Command::new("grim")
        .args(["-s", "1", "-t", "ppm", "-g", &geometry, "-"])
        .output()
        .ok()?;
    let bytes = out.stdout;
    // P6\n<w> <h>\n255\n
    let mut fields = 0;
    let mut start = 0;
    for (i, b) in bytes.iter().enumerate() {
        if b.is_ascii_whitespace() {
            fields += 1;
            if fields == 4 {
                start = i + 1;
                break;
            }
        }
    }
    Some(bytes.get(start..)?.as_chunks::<3>().0.to_vec())
}

/// Type into `input` and note a miss if what was typed cannot be seen.
async fn check(app: &mut App, misses: &mut Vec<String>, input: &str) {
    app.click(input).await.expect("focus");
    app.type_into(input, "WMWMWMWM").await.expect("type");
    app.wait_for(&format!("{input}[value=WMWMWMWM]"), DEFAULT_WAIT).await.expect("typed");
    // A frame for the colour the text was drawn in to be noted.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let colour = app.attr(input, "data-color").await.expect("attr");
    let bg = app.attr(input, "data-bg").await.expect("attr");
    let (Some(colour), Some(bg)) = (colour.as_deref().and_then(hex), bg.as_deref().and_then(hex)) else {
        misses.push(format!("{input}: data-color {colour:?}, data-bg {bg:?}"));
        return;
    };
    let ratio = contrast(colour, bg);
    eprintln!("{input}: text {colour:?} on {bg:?}, contrast {ratio:.1}");
    if ratio < 4.5 {
        misses.push(format!("{input}: text {colour:?} on {bg:?} has a contrast of {ratio:.1}"));
    }
    if !can_capture() {
        eprintln!("screen locked or not Hyprland: no pixel check");
        return;
    }
    let bounds = app.bounds(input).await.expect("bounds").expect("input bounds");
    let Some(at) = app.pid().and_then(window_at) else {
        eprintln!("window not found: no pixel check");
        return;
    };
    // The inside, clear of the border and rounded corners.
    let inside = Bounds { x: bounds.x + 6.0, y: bounds.y + 4.0, w: bounds.w - 12.0, h: bounds.h - 8.0 };
    let Some(pixels) = capture(&inside, at) else {
        eprintln!("grim failed: no pixel check");
        return;
    };
    // Glyph pixels: far from the background (the caret is a thin bar of
    // ~2x20 px; eight capitals are several hundred).
    let ink = pixels
        .iter()
        .filter(|p| p.iter().zip(bg).map(|(a, b)| (i32::from(*a) - i32::from(b)).abs()).sum::<i32>() > 120)
        .count();
    eprintln!("{input}: {ink} of {} pixels inked", pixels.len());
    if ink < 150 {
        let shot = app.screenshot("input-text").await.ok().flatten();
        misses.push(format!("{input}: the typed text does not show ({ink} pixels inked; {shot:?})"));
    }
}

#[tokio::test]
async fn typed_text_shows_on_the_login_and_add_account_forms() {
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
        check(&mut app, &mut misses, &format!("form.login input[name={name}]")).await;
    }
    // Sign in (the homeserver field back to the test server's).
    app.type_into("form.login input[name=homeserver]", &hs).await.expect("homeserver");
    app.type_into("form.login input[name=username]", "alice").await.expect("username");
    app.type_into("form.login input[name=password]", "alice-password").await.expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");

    // Add an account: the same form, over a signed-in app.
    app.click(".profile-chip").await.expect("chip");
    app.wait_for(".context-menu", DEFAULT_WAIT).await.expect("account menu");
    app.click_submenu("Switch account", "Add an account").await.expect("add");
    app.wait_for_text("form.login button", "Back to", DEFAULT_WAIT).await.expect("login");
    for name in ["homeserver", "username", "password"] {
        check(&mut app, &mut misses, &format!("form.login input[name={name}]")).await;
    }
    app.click_text("form.login button", "Back to").await.expect("back");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    app.open_room("general").await.expect("general");

    // Other forms' inputs.
    app.click(".create-room-button").await.expect("plus");
    app.wait_for(".create-room-dialog input[name=name]", DEFAULT_WAIT).await.expect("dialog");
    for name in ["name", "topic"] {
        check(&mut app, &mut misses, &format!(".create-room-dialog input[name={name}]")).await;
    }
    app.press(".create-room-dialog", "Escape").await.expect("close");
    for (item, input) in [
        ("Edit profile", ".profile-editor input[name=displayname]"),
        ("Set status", ".status-editor input[name=status]"),
    ] {
        app.click(".profile-chip").await.expect("chip");
        app.wait_for(".context-menu", DEFAULT_WAIT).await.expect("account menu");
        app.click_text(".context-item", item).await.expect(item);
        app.wait_for(input, DEFAULT_WAIT).await.expect(item);
        check(&mut app, &mut misses, input).await;
        app.press(input, "Escape").await.expect("close");
    }
    assert!(misses.is_empty(), "{misses:#?}");
}
