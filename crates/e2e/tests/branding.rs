//! The app looks like morpheuschat.org: Inter and Unbounded bundled (not the
//! system's fonts), and the logo lockup as the site's header draws it.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

#[tokio::test]
async fn the_app_uses_the_sites_fonts_and_lockup() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");

    // The bundled faces load and the page uses them (GPUI: the elements note the
    // family they are drawn in, `data-font`).
    if app.is_gpui() {
        assert_eq!(
            app.attr(".app", "data-font").await.expect("font").as_deref(),
            Some("Inter"),
            "Inter first"
        );
        assert_eq!(
            app.attr("form.login h1", "data-font").await.expect("heading").as_deref(),
            Some("Unbounded"),
            "the Welcome back heading in Unbounded"
        );
        return gpui_lockup(&mut app).await;
    }
    app.wait_for_js(
        "return document.fonts.check('16px \"Inter Variable\"') && document.fonts.check('700 16px \"Unbounded Variable\"') \
           && [...document.fonts].some(f => f.family.includes('Inter') && f.status === 'loaded') \
           && [...document.fonts].some(f => f.family.includes('Unbounded') && f.status === 'loaded');",
        DEFAULT_WAIT,
    )
    .await
    .expect("Inter and Unbounded loaded");
    let body = app
        .eval("return getComputedStyle(document.body).fontFamily;")
        .await
        .expect("font");
    assert!(
        body.as_str()
            .unwrap_or_default()
            .trim_start_matches(['"', '\''])
            .starts_with("Inter Variable"),
        "Inter first: {body}"
    );
    let heading = app
        .eval("return getComputedStyle(document.querySelector('form.login h1, .login h1')).fontFamily;")
        .await
        .expect("heading");
    assert!(
        heading.as_str().unwrap_or_default().contains("Unbounded"),
        "the Welcome back heading in Unbounded: {heading}"
    );

    // The lockup, where a window bar shows it (desktop).
    if app.is_phone() {
        return;
    }
    let lockup = app
        .eval(
            "const b = document.querySelector('.brand'); if (!b) return null; \
             const svg = b.querySelector('svg'), name = b.querySelector('.brand-name'), tm = b.querySelector('.tm'); \
             const ns = getComputedStyle(name), size = parseFloat(ns.fontSize); \
             return [svg.getAttribute('viewBox'), \
               Math.abs(svg.getBoundingClientRect().width - 2.083 * size) < 1, \
               ns.fontFamily.includes('Unbounded'), ns.fontWeight, name.textContent.trim(), \
               !!tm && tm.textContent === '™' && getComputedStyle(tm).fontFamily.includes('Inter'), \
               getComputedStyle(svg).color];",
        )
        .await
        .expect("lockup");
    assert_eq!(
        lockup,
        serde_json::json!([
            "48 182 416 154",
            true,
            true,
            "700",
            "Morpheus",
            true,
            "rgb(184, 192, 255)"
        ]),
        "the glasses cropped tight, 2.083em wide, the name in Unbounded 700, ™, the moon colour"
    );
}

/// The lockup in the window bar as the GPUI app draws it: the glasses cropped
/// tight (`viewBox`), 2.083em wide, in the moon colour; the name in Unbounded
/// 700; a small ™ in Inter.
async fn gpui_lockup(app: &mut App) {
    let mark = ".brand svg";
    assert_eq!(
        app.attr(mark, "viewBox").await.expect("viewBox").as_deref(),
        Some("48 182 416 154"),
        "the glasses cropped tight"
    );
    let size: f64 = app
        .attr(".brand .brand-name", "data-size")
        .await
        .expect("size")
        .and_then(|s| s.parse().ok())
        .expect("the name's size");
    let width = app.bounds(mark).await.expect("mark").expect("drawn").w;
    assert!((width - 2.083 * size).abs() < 1.0, "2.083em wide: {width} for {size}");
    for (selector, attr, want) in [
        (".brand .brand-name", "data-font", "Unbounded"),
        (".brand .brand-name", "data-weight", "700"),
        (".brand .tm", "data-font", "Inter"),
        (mark, "data-color", "b8c0ff"),
    ] {
        assert_eq!(
            app.attr(selector, attr).await.expect("attr").as_deref(),
            Some(want),
            "{selector} {attr}"
        );
    }
    assert_eq!(app.text(".brand .brand-name").await.expect("name").as_deref(), Some("Morpheus"));
    assert_eq!(app.text(".brand .tm").await.expect("tm").as_deref(), Some("™"));
}
