//! Helpers shared by test files (`mod common;`), so no file has to include
//! another test file.

#![allow(dead_code, clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

/// Sign in through the login form and wait for the room list.
pub async fn login(app: &mut App, user: &str, password: &str) {
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", user)
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", password)
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("rooms listed");
}

/// The window's pixels (RGB rows) and size, from a `grim` capture of the
/// test window (found by the app's pid on Hyprland); `None` elsewhere.
pub fn capture(app: &App) -> Option<(Vec<u8>, usize, usize)> {
    // A locked session (hyprlock) is what grim sees: no pixels of the app.
    if std::process::Command::new("pgrep").args(["-x", "hyprlock"]).output().is_ok_and(|o| o.status.success()) {
        eprintln!("SKIPPED pixel check: the session is locked (hyprlock)");
        return None;
    }
    let pid = app.pid()?;
    let out = std::process::Command::new("hyprctl").args(["clients", "-j"]).output().ok()?;
    let clients: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let window = clients
        .as_array()?
        .iter()
        .find(|c| c["pid"].as_u64() == Some(u64::from(pid)))?
        .clone();
    let geometry = format!(
        "{},{} {}x{}",
        window["at"][0], window["at"][1], window["size"][0], window["size"][1]
    );
    let out = std::process::Command::new("grim")
        .args(["-t", "ppm", "-g", &geometry, "-"])
        .output()
        .ok()?;
    // P6\n<w> <h>\n255\n, then RGB bytes.
    let bytes = out.stdout;
    let mut fields = Vec::new();
    let mut at = 0;
    while fields.len() < 4 {
        let start = at;
        while !bytes.get(at)?.is_ascii_whitespace() {
            at += 1;
        }
        fields.push(String::from_utf8_lossy(&bytes[start..at]).into_owned());
        at += 1;
    }
    let (w, h) = (fields[1].parse().ok()?, fields[2].parse().ok()?);
    Some((bytes[at..].to_vec(), w, h))
}

/// A pixel of a capture: `(r, g, b)`.
pub fn pixel_at(capture: &(Vec<u8>, usize, usize), x: f64, y: f64) -> (u8, u8, u8) {
    let (pixels, w, h) = capture;
    let (x, y) = (x.round() as usize, y.round() as usize);
    assert!(x < *w && y < *h, "({x}, {y}) is outside the {w}x{h} capture");
    let i = (y * w + x) * 3;
    (pixels[i], pixels[i + 1], pixels[i + 2])
}

pub fn colour_distance(a: (u8, u8, u8), b: (u8, u8, u8)) -> u32 {
    u32::from(a.0.abs_diff(b.0)) + u32::from(a.1.abs_diff(b.1)) + u32::from(a.2.abs_diff(b.2))
}

/// Whether each of the four corners of `b` is rounded: the pixel just inside
/// the corner shows what is behind the panel, not the panel's own colour
/// there (sampled 30 px along the same edge). `which` is a mask of the
/// corners to check (top-left, top-right, bottom-left, bottom-right).
/// Returns the names of corners drawn square; `None` where there is no capture.
pub fn square_corners(app: &App, b: &morpheus_e2e::Bounds, which: [bool; 4]) -> Option<Vec<&'static str>> {
    let shot = capture(app)?;
    let inset = 2.5;
    let mut square = Vec::new();
    let corners = [
        ("top-left", b.x + inset, b.y + inset, 30.0),
        ("top-right", b.right() - inset, b.y + inset, -30.0),
        ("bottom-left", b.x + inset, b.bottom() - inset, 30.0),
        ("bottom-right", b.right() - inset, b.bottom() - inset, -30.0),
    ];
    for ((name, x, y, along), check) in corners.into_iter().zip(which) {
        if !check {
            continue;
        }
        let corner = pixel_at(&shot, x, y);
        let panel = pixel_at(&shot, x + along, y);
        eprintln!("{name}: corner {corner:?}, panel edge {panel:?}");
        if colour_distance(corner, panel) < 10 {
            square.push(name);
        }
    }
    Some(square)
}
