//! The app updates itself: it checks the releases by itself (no menu), says
//! when a newer version is out, and Update downloads this platform's build,
//! puts it in place of the running one, and starts it.
//!
//! The app runs from a folder of its own here (a copy, as an unzipped
//! release is), and the "releases" are a feed served by the test in GitHub's
//! format. The newer build in it is a script that notes it was started, so
//! the test sees the swap and the restart without a second real build.

#![allow(clippy::expect_used)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use morpheus_e2e::{App, DEFAULT_WAIT, app_binary};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

/// Serve `routes` (path → body) over HTTP, forever (the thread ends with
/// the test process).
fn serve(listener: TcpListener, routes: Vec<(String, Vec<u8>)>) {
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut request = [0u8; 4096];
            let n = stream.read(&mut request).unwrap_or(0);
            let line = String::from_utf8_lossy(&request[..n]);
            let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
            let (status, body) = match routes.iter().find(|(p, _)| *p == path) {
                Some((_, body)) => ("200 OK", body.clone()),
                None => ("404 Not Found", Vec::new()),
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(&body);
        }
    });
}

/// A release zip as the release scripts make them: one folder holding the
/// app and its assets. The app here is a script noting it was started.
fn release_zip(work: &Path, name: &str, exe: &str, marker: &Path) -> Vec<u8> {
    let folder = work.join(name);
    std::fs::create_dir_all(folder.join("assets")).expect("folder");
    std::fs::write(folder.join("assets/new-version.txt"), "99.0.0").expect("asset");
    let script = folder.join(exe);
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\necho \"started after $MORPHEUS_UPDATED_FROM\" > '{}'\n",
            marker.display()
        ),
    )
    .expect("script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("mode");
    }
    let zip = work.join(format!("{name}.zip"));
    let status = std::process::Command::new("zip")
        .current_dir(work)
        .args(["-qr", &format!("{name}.zip"), name])
        .status()
        .expect("zip");
    assert!(status.success(), "zip failed");
    std::fs::read(zip).expect("zip bytes")
}

/// The app's folder as a release unpacks: its program and assets, linked
/// from the test build (same disk, so nothing big is copied).
fn install_copy(dir: &Path) -> PathBuf {
    let built = app_binary().expect("the test build");
    let built_dir = built.parent().expect("its folder");
    std::fs::create_dir_all(dir).expect("install dir");
    let exe = dir.join(built.file_name().expect("name"));
    std::fs::hard_link(&built, &exe).expect("link the app");
    let status = std::process::Command::new("cp")
        .arg("-r")
        .arg(built_dir.join("assets"))
        .arg(dir.join("assets"))
        .status()
        .expect("cp");
    assert!(status.success());
    exe
}

#[tokio::test]
async fn the_app_updates_itself_from_a_newer_release() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    // Beside the test build, on the same disk (for the link).
    let root = app_binary()
        .expect("build")
        .ancestors()
        .nth(6)
        .expect("target")
        .join(format!("e2e-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let install = root.join("install");
    let exe = install_copy(&install);
    let marker = root.join("started.txt");

    let name = "morpheus-99.0.0-linux-x86_64";
    let zip = release_zip(&root, name, "morpheus-desktop", &marker);
    // GitHub's latest-release document, with the assets of every platform.
    let feed = |base: &str| {
        serde_json::json!({
            "tag_name": "v99.0.0",
            "html_url": format!("{base}/release"),
            "assets": [
                { "name": "morpheus-99.0.0-windows-x86_64.zip", "browser_download_url": format!("{base}/windows.zip") },
                { "name": "morpheus-99.0.0-android-arm64.apk", "browser_download_url": format!("{base}/android.apk") },
                { "name": format!("{name}.zip"), "browser_download_url": format!("{base}/{name}.zip") },
            ]
        })
        .to_string()
        .into_bytes()
    };
    // Bind first to learn the address, then serve the feed that names it.
    let probe = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = probe.local_addr().expect("addr").port();
    drop(probe);
    let base = format!("http://127.0.0.1:{port}");
    serve(
        TcpListener::bind(("127.0.0.1", port)).expect("rebind"),
        vec![
            ("/feed".to_string(), feed(&base)),
            (format!("/{name}.zip"), zip),
        ],
    );

    let feed_url = format!("{base}/feed");
    let exe_str = exe.to_string_lossy().into_owned();
    let mut app = App::launch_with_env(
        &hs,
        &[
            ("MORPHEUS_E2E_BIN", exe_str.as_str()),
            ("MORPHEUS_UPDATE_FEED", feed_url.as_str()),
        ],
    )
    .await
    .expect("launch the installed copy");
    let pid = app.pid().expect("pid");
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

    // Found by itself, without asking.
    app.wait_for_text(".update-card", "99.0.0", DEFAULT_WAIT)
        .await
        .expect("the app says a newer version is out");
    app.click(".update-card .update-button")
        .await
        .expect("Update");

    // The new build is started (after this one), in this one's place.
    let deadline = Instant::now() + Duration::from_secs(60);
    while !marker.exists() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let started = std::fs::read_to_string(&marker).unwrap_or_default();
    assert_eq!(
        started.trim(),
        format!("started after {pid}"),
        "the new version started, told to wait for this one ({})",
        app.log_tail(30)
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.is_running() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(!app.is_running(), "the old version quit");
    let now = std::fs::read_to_string(&exe).unwrap_or_default();
    assert!(
        now.contains("started after"),
        "the program in the app's folder is the new one"
    );
    assert_eq!(
        std::fs::read_to_string(install.join("assets/new-version.txt"))
            .ok()
            .as_deref(),
        Some("99.0.0"),
        "and so are its assets"
    );
    let _ = std::fs::remove_dir_all(&root);
}
