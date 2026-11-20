#![allow(clippy::expect_used)]
//! Settings through the FFI: the look (theme, custom colour, theme codes),
//! the language, what notifies by default. The look and the language need no
//! homeserver; the notification defaults do (self-skips without one).

use morpheus_ffi::Morpheus;

#[tokio::test]
async fn the_look_is_picked_shared_as_a_code_and_kept() {
    let dir = tempfile::tempdir().expect("dir");
    let app = Morpheus::new(dir.path().display().to_string()).expect("app");

    let start = app.appearance();
    assert_eq!(start.theme, "dark");
    assert!(start.colour.is_none() && !start.palette.is_light);
    let keys: Vec<_> = app.themes().into_iter().map(|t| t.key).collect();
    assert_eq!(keys, ["dark", "midnight", "light"]);

    // A light theme draws light surfaces; midnight has black ones.
    let light = app.set_theme("light".into()).expect("light");
    assert!(light.palette.is_light);
    assert_ne!(light.palette.surface, start.palette.surface);
    assert_eq!(app.set_theme("midnight".into()).expect("midnight").palette.background, 0x000000);

    // A custom colour re-derives the palette and a bad one is refused in words.
    let custom = app.set_custom_colour("#2a4d8f".into()).expect("custom");
    assert_eq!(custom.colour.as_deref(), Some("2a4d8f"));
    assert_ne!(custom.palette.accent, start.palette.accent);
    let bad = app.set_custom_colour("blue-ish".into()).expect_err("not a colour");
    assert!(bad.to_string().contains("not a colour"), "{bad}");

    // The code carries the look to someone else, and a wrong one says why.
    let code = app.theme_code();
    assert!(code.starts_with("morpheus-theme/1;"), "{code}");
    let other_dir = tempfile::tempdir().expect("dir");
    let other = Morpheus::new(other_dir.path().display().to_string()).expect("app");
    let got = other.apply_theme_code(format!("  {code}  ")).expect("apply");
    assert_eq!(got.colour, custom.colour);
    assert_eq!(got.palette.background, custom.palette.background);
    let wrong = other.apply_theme_code("hello".into()).expect_err("not a code");
    assert!(wrong.to_string().contains("not a Morpheus theme code"), "{wrong}");

    // Kept: a new launch finds the look again.
    drop(app);
    let again = Morpheus::new(dir.path().display().to_string()).expect("app again");
    assert_eq!(again.appearance().colour.as_deref(), Some("2a4d8f"));
}

#[tokio::test]
async fn the_language_is_picked_and_kept_and_words_follow_it() {
    let dir = tempfile::tempdir().expect("dir");
    let app = Morpheus::new(dir.path().display().to_string()).expect("app");
    let codes: Vec<_> = app.languages().into_iter().map(|l| l.code).collect();
    assert_eq!(codes[0], "system");
    assert!(codes.contains(&"es".to_string()));
    app.set_language("es".into()).expect("es");
    assert_eq!(app.translate("Appearance".into()), "Apariencia");
    assert_eq!(app.language_choice(), "es");
    app.set_language("en".into()).expect("en");
    assert_eq!(app.translate("Appearance".into()), "Appearance");

    let again = Morpheus::new(dir.path().display().to_string()).expect("app again");
    assert_eq!(again.language_choice(), "en");
}

#[tokio::test]
async fn notification_defaults_are_read_and_changed() {
    let Ok(homeserver) = std::env::var("MORPHEUS_TEST_HOMESERVER") else {
        return;
    };
    let dir = tempfile::tempdir().expect("dir");
    let app = Morpheus::new(dir.path().display().to_string()).expect("app");
    app.login(homeserver, "carol".into(), "carol-password".into()).await.expect("login");
    let before = app.notify_defaults().await.expect("defaults");
    let flipped = if before.rooms == "all" { "mentions" } else { "all" };
    app.set_notify_default(false, flipped.into()).await.expect("set");
    assert_eq!(app.notify_defaults().await.expect("defaults").rooms, flipped);
    app.set_notify_default(false, before.rooms.clone()).await.expect("put back");
    assert_eq!(app.notify_defaults().await.expect("defaults").rooms, before.rooms);
}
