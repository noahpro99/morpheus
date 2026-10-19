//! Desktop startup options (Settings → Startup): open Morpheus when you sign
//! in to the computer (an XDG autostart entry), optionally hidden in the tray.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

#[tokio::test]
async fn startup_options_write_the_autostart_entry() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    // A private config dir: never touch the developer's real autostart.
    let config = std::env::temp_dir().join(format!("morpheus-xdg-{}", std::process::id()));
    std::fs::create_dir_all(&config).expect("config dir");
    let entry = config.join("autostart").join("morpheus.desktop");
    let mut app =
        App::launch_with_env(&hs, &[("XDG_CONFIG_HOME", config.to_str().expect("utf-8"))])
            .await
            .expect("launch app");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", "carol")
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", "carol-password")
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");

    app.click(".settings-button").await.expect("gear");
    app.click_text(".settings-nav button", "Startup")
        .await
        .expect("Startup page");
    app.wait_for(".settings-page input[name=autostart]", DEFAULT_WAIT)
        .await
        .expect("autostart switch");
    assert!(!entry.exists(), "no entry before opting in");

    app.click(".settings-page input[name=autostart]")
        .await
        .expect("turn on");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while !entry.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "no autostart entry written"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    let text = std::fs::read_to_string(&entry).expect("entry");
    assert!(
        text.contains("[Desktop Entry]") && text.contains("Exec="),
        "{text}"
    );
    assert!(!text.contains("--hidden"), "not hidden yet: {text}");

    app.click(".settings-page input[name=start-hidden]")
        .await
        .expect("start hidden");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while !std::fs::read_to_string(&entry)
        .unwrap_or_default()
        .contains("--hidden")
    {
        assert!(
            std::time::Instant::now() < deadline,
            "entry not updated for --hidden"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    app.click(".settings-page input[name=autostart]")
        .await
        .expect("turn off");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    while entry.exists() {
        assert!(std::time::Instant::now() < deadline, "entry not removed");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    let _ = std::fs::remove_dir_all(&config);
}
