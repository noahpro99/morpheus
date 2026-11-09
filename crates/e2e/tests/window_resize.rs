//! Resizing the desktop window never shows white: while the page catches up
//! with the new size, what shows around it is the app's dark background.

#![allow(clippy::expect_used)]

use std::time::{Duration, Instant};

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

fn hyprctl(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("hyprctl")
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The test window's Hyprland client (by the app's pid).
fn client(pid: u32) -> Option<serde_json::Value> {
    let clients: serde_json::Value = serde_json::from_str(&hyprctl(&["clients", "-j"])?).ok()?;
    clients
        .as_array()?
        .iter()
        .find(|c| c["pid"].as_u64() == Some(u64::from(pid)))
        .cloned()
}

/// The share of near-white pixels in the window right now, from a `grim`
/// capture of its whole area (PPM: a short header, then RGB bytes).
fn white_share(window: &serde_json::Value) -> Option<f64> {
    let geometry = format!(
        "{},{} {}x{}",
        window["at"][0], window["at"][1], window["size"][0], window["size"][1]
    );
    let out = std::process::Command::new("grim")
        .args(["-t", "ppm", "-g", &geometry, "-"])
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
    let pixels = bytes.get(start..)?;
    let total = pixels.len() / 3;
    if total == 0 {
        return None;
    }
    let white = pixels
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|p| p[0] > 230 && p[1] > 230 && p[2] > 230)
        .count();
    Some(white as f64 / total as f64)
}

#[tokio::test]
async fn resizing_the_window_shows_no_white() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
        eprintln!("not on Hyprland; skipping");
        return;
    }
    let mut app = App::launch(&hs).await.expect("launch");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("sign-in screen");
    let pid = app.pid().expect("pid");
    let window = client(pid).expect("the window");
    let address = window["address"].as_str().expect("address").to_owned();
    let target = format!("address:{address}");
    if window["floating"].as_bool() != Some(true) {
        hyprctl(&[
            "eval",
            &format!(
                "hl.dispatch(hl.dsp.window.float({{ action = \"enable\", window = \"{target}\" }}))"
            ),
        ]);
    }

    // Grow and shrink the window a few times, sampling it as it changes.
    let mut worst: f64 = 0.0;
    for (w, h) in [(1500, 950), (900, 650), (1600, 1000), (1000, 700)] {
        hyprctl(&[
            "eval",
            &format!(
                "hl.dispatch(hl.dsp.window.resize({{ x = {w}, y = {h}, window = \"{target}\" }}))"
            ),
        ]);
        let until = Instant::now() + Duration::from_millis(400);
        while Instant::now() < until {
            if let Some(window) = client(pid)
                && let Some(share) = white_share(&window)
            {
                worst = worst.max(share);
            }
        }
    }
    assert!(
        worst < 0.02,
        "{:.0}% of the window was white while it resized",
        worst * 100.0
    );
}
