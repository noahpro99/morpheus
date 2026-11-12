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
