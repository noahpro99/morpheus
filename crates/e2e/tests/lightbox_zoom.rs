//! The picture viewer zooms like a good image viewer (GPUI only): wheel
//! around the pointer, drag to pan, double click, buttons and keys. The
//! zoom is `data-zoom` on `.lightbox-image` (1 = fit the window, 8 = the most)
//! and the pan `data-x`/`data-y` (pixels from the middle).

#![allow(clippy::expect_used)]

mod common;

use morpheus_e2e::{App, Bounds, DEFAULT_WAIT, api_as, new_account};

const WIDE_PNG: &[u8] = include_bytes!("../fixtures/wide.png");
const FRAMED_PNG: &[u8] = include_bytes!("../fixtures/framed.png");
const IMAGE: &str = ".lightbox-image";

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

/// A room with one picture of the declared size, opened in the viewer.
async fn viewer(hs: &str, png: &[u8], size: (u32, u32)) -> Option<App> {
    let (ann, ben) = (new_account(hs, "zmann").expect("account"), new_account(hs, "zmben").expect("account"));
    let made = api_as(
        hs,
        &ann,
        "POST",
        "createRoom",
        Some(serde_json::json!({ "name": "zoomroom", "preset": "public_chat" })),
    )
    .expect("a room");
    let room = made["room_id"].as_str().expect("room id").to_owned();
    api_as(
        hs,
        &ben,
        "POST",
        &format!("join/{}", room.replace('!', "%21").replace(':', "%3A")),
        Some(serde_json::json!({})),
    )
    .expect("join");
    morpheus_e2e::send_image_sized_as(hs, &ben, &room, png, "zoom.png", None, size).expect("a picture");
    let mut app = App::launch_desktop(hs).await.expect("launch app");
    if !app.is_gpui() {
        eprintln!("the zoom is built in the GPUI app only; skipping");
        return None;
    }
    common::login(&mut app, &ann, &format!("{ann}-password")).await;
    app.open_room("zoomroom").await.expect("open room");
    open(&mut app).await;
    Some(app)
}

async fn open(app: &mut App) {
    app.wait_for(".message img.image", DEFAULT_WAIT).await.expect("the picture");
    app.click(".message img.image").await.expect("open it");
    app.wait_for(IMAGE, DEFAULT_WAIT).await.expect("full-size picture");
}

async fn num(app: &mut App, name: &str) -> f64 {
    app.attr(IMAGE, name)
        .await
        .expect("attr")
        .unwrap_or_else(|| panic!("no {name} on the picture"))
        .parse()
        .expect("a number")
}

async fn zoom(app: &mut App) -> f64 {
    num(app, "data-zoom").await
}

async fn image(app: &mut App) -> Bounds {
    app.bounds(IMAGE).await.expect("bounds").expect("a picture")
}

async fn window(app: &mut App) -> Bounds {
    app.bounds("body").await.expect("bounds").expect("a window")
}

/// The picture's point under the pointer, as a fraction of its box.
fn at(b: &Bounds, x: f64, y: f64) -> (f64, f64) {
    ((x - b.x) / b.w, (y - b.y) / b.h)
}

#[tokio::test]
async fn the_wheel_zooms_around_the_pointer_between_fit_and_8x() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let Some(mut app) = viewer(&hs, WIDE_PNG, (1600, 800)).await else { return };
    assert!((zoom(&mut app).await - 1.0).abs() < 1e-6, "opens at fit");
    let fit = image(&mut app).await;
    // The pointer 60 right of and 30 above the middle; wheel up zooms in.
    let (px, py) = (fit.x + fit.w / 2.0 + 60.0, fit.y + fit.h / 2.0 - 30.0);
    let (u, v) = at(&fit, px, py);
    app.wheel_pixels_at(IMAGE, 60.0, -30.0, -600.0).await.expect("wheel up");
    let z = zoom(&mut app).await;
    assert!(z > 2.0 && z < 8.0, "wheel up zooms in: {z}");
    let big = image(&mut app).await;
    assert!((big.w / fit.w - z).abs() < 0.05, "drawn {z} times bigger: {fit:?} -> {big:?}");
    let (x, y) = (big.x + u * big.w, big.y + v * big.h);
    assert!(
        (x - px).abs() < 3.0 && (y - py).abs() < 3.0,
        "the point under the pointer moved from ({px}, {py}) to ({x}, {y})"
    );
    // Wheel down zooms out again, never past fit.
    app.wheel_pixels_at(IMAGE, 60.0, -30.0, 600.0).await.expect("wheel down");
    assert!((zoom(&mut app).await - 1.0).abs() < 1e-3, "back at fit");
    app.wheel_pixels_at(IMAGE, 0.0, 0.0, 3000.0).await.expect("wheel down more");
    let z = zoom(&mut app).await;
    assert!((z - 1.0).abs() < 1e-6, "no smaller than fit: {z}");
    let still = image(&mut app).await;
    assert!((still.w - fit.w).abs() < 1.0, "fit is the smallest: {still:?}");
    // And no bigger than 8x.
    app.wheel_pixels_at(IMAGE, 0.0, 0.0, -5000.0).await.expect("wheel up lots");
    let z = zoom(&mut app).await;
    assert!((z - 8.0).abs() < 1e-6, "8x at most: {z}");
}

#[tokio::test]
async fn dragging_pans_a_zoomed_picture_and_it_cannot_leave_the_window() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let Some(mut app) = viewer(&hs, WIDE_PNG, (1600, 800)).await else { return };
    let win = window(&mut app).await;
    // Fit: nothing to pan.
    app.drag_at(IMAGE, 0.0, 0.0, 50, 30).await.expect("drag at fit");
    assert_eq!(app.count(".lightbox-overlay").await.expect("count"), 1, "still open");
    let fit = image(&mut app).await;
    app.wheel_pixels_at(IMAGE, 0.0, 0.0, -900.0).await.expect("zoom in");
    let before = image(&mut app).await;
    assert!(before.w > fit.w * 3.0, "zoomed: {before:?}");
    app.drag_at(IMAGE, 0.0, 0.0, -80, -40).await.expect("drag");
    let after = image(&mut app).await;
    assert!(
        (after.x - before.x + 80.0).abs() < 3.0 && (after.y - before.y + 40.0).abs() < 3.0,
        "dragged by (-80, -40): {before:?} -> {after:?}"
    );
    // Far past the edge: the picture still covers the window.
    app.drag_at(IMAGE, 0.0, 0.0, 6000, 6000).await.expect("drag far");
    let far = image(&mut app).await;
    assert!(
        far.x <= win.x + 1.0 && far.y <= win.y + 1.0,
        "the picture's top left was dragged into the window: {far:?} in {win:?}"
    );
    app.drag_at(IMAGE, 0.0, 0.0, -6000, -6000).await.expect("drag far the other way");
    let far = image(&mut app).await;
    assert!(
        far.right() >= win.right() - 1.0 && far.bottom() >= win.bottom() - 1.0,
        "the picture's bottom right was dragged into the window: {far:?} in {win:?}"
    );
    assert_eq!(app.count(".lightbox-overlay").await.expect("count"), 1, "still open");
}

#[tokio::test]
async fn double_click_toggles_between_fit_and_actual_size_at_the_pointer() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let Some(mut app) = viewer(&hs, WIDE_PNG, (1600, 800)).await else { return };
    let fit = image(&mut app).await;
    assert!(fit.w < 1500.0, "fit is smaller than the picture: {fit:?}");
    let (px, py) = (fit.x + fit.w / 2.0 + 100.0, fit.y + fit.h / 2.0);
    let (u, v) = at(&fit, px, py);
    app.click_at(IMAGE, 100.0, 0.0, 2).await.expect("double click");
    let big = image(&mut app).await;
    assert!((big.w - 1600.0).abs() < 4.0, "the picture's own 1600 px wide: {big:?}");
    let (x, y) = (big.x + u * big.w, big.y + v * big.h);
    assert!((x - px).abs() < 3.0 && (y - py).abs() < 3.0, "the point moved to ({x}, {y}) from ({px}, {py})");
    assert_eq!(app.count(".lightbox-overlay").await.expect("count"), 1, "still open");
    app.click_at(IMAGE, 100.0, 0.0, 2).await.expect("double click again");
    assert!((zoom(&mut app).await - 1.0).abs() < 1e-6, "back at fit");
    let back = image(&mut app).await;
    assert!((back.w - fit.w).abs() < 1.0, "fit again: {back:?}");
}

#[tokio::test]
async fn buttons_and_keys_zoom() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let Some(mut app) = viewer(&hs, WIDE_PNG, (1600, 800)).await else { return };
    app.click(".lightbox-zoom-in").await.expect("plus");
    let one = zoom(&mut app).await;
    assert!(one > 1.1, "+ zooms in: {one}");
    app.click(".lightbox-zoom-in").await.expect("plus");
    let two = zoom(&mut app).await;
    assert!(two > one, "+ again: {two}");
    app.click(".lightbox-zoom-out").await.expect("minus");
    assert!((zoom(&mut app).await - one).abs() < 1e-6, "- steps back");
    app.click(".lightbox-zoom-reset").await.expect("reset");
    assert!((zoom(&mut app).await - 1.0).abs() < 1e-6, "reset is fit");
    app.click(".lightbox-zoom-out").await.expect("minus at fit");
    assert!((zoom(&mut app).await - 1.0).abs() < 1e-6, "no smaller than fit");
    assert_eq!(app.count(".lightbox-overlay").await.expect("count"), 1, "the buttons do not close it");
    app.press("body", "+").await.expect("key +");
    assert!(zoom(&mut app).await > 1.1, "the + key zooms in");
    app.press("body", "+").await.expect("key +");
    app.press("body", "-").await.expect("key -");
    assert!((zoom(&mut app).await - one).abs() < 1e-6, "the - key steps out");
    app.press("body", "0").await.expect("key 0");
    assert!((zoom(&mut app).await - 1.0).abs() < 1e-6, "0 is fit");
    app.press("body", "Escape").await.expect("escape");
    app.wait_for_count(".lightbox-overlay", 0, DEFAULT_WAIT).await.expect("Esc closes");
}

#[tokio::test]
async fn a_drag_ending_on_the_backdrop_does_not_close_and_reopening_is_at_fit() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let Some(mut app) = viewer(&hs, WIDE_PNG, (1600, 800)).await else { return };
    let win = window(&mut app).await;
    // At fit: press on the picture, let go on the dimmed window beside it.
    let fit = image(&mut app).await;
    let dx = (win.right() - 6.0 - fit.centre_x()) as i32;
    assert!(fit.right() < win.right() - 10.0, "room beside the picture: {fit:?}");
    app.drag_at(IMAGE, 0.0, 0.0, dx, 0).await.expect("drag out");
    assert_eq!(app.count(".lightbox-overlay").await.expect("count"), 1, "a drag onto the backdrop closed it");
    // Zoomed a little (taller than wide room: backdrop above and below).
    app.wheel_pixels_at(IMAGE, 0.0, 0.0, -170.0).await.expect("zoom a little");
    let b = image(&mut app).await;
    assert!(b.bottom() < win.bottom() - 10.0, "room below the zoomed picture: {b:?} in {win:?}");
    let dy = (win.bottom() - 6.0 - (b.y + b.h / 2.0)) as i32;
    app.drag_at(IMAGE, 0.0, 0.0, 0, dy).await.expect("drag out zoomed");
    assert_eq!(app.count(".lightbox-overlay").await.expect("count"), 1, "a pan ending on the backdrop closed it");
    // A plain click on the backdrop still closes it.
    // Reopening: at fit again.
    assert!(zoom(&mut app).await > 1.1, "zoomed before closing");
    app.press("body", "Escape").await.expect("escape");
    app.wait_for_count(".lightbox-overlay", 0, DEFAULT_WAIT).await.expect("closes");
    open(&mut app).await;
    assert!((zoom(&mut app).await - 1.0).abs() < 1e-6, "reopens at fit");
    assert!(num(&mut app, "data-x").await.abs() < 1e-6, "and centred");
    // Click on the dimmed window outside the picture closes it.
    let fit = image(&mut app).await;
    app.click_at(IMAGE, win.right() as f32 - 6.0 - fit.centre_x() as f32, 0.0, 1)
        .await
        .expect("click the backdrop");
    app.wait_for_count(".lightbox-overlay", 0, DEFAULT_WAIT).await.expect("a click outside closes");
}

#[tokio::test]
async fn the_pictures_corners_are_rounded() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    // Blue sides and a red middle, shown at its own 200 x 100 (zoom a bit so
    // the corners are big enough to see).
    let Some(mut app) = viewer(&hs, FRAMED_PNG, (200, 100)).await else { return };
    app.click(".lightbox-zoom-in").await.expect("plus");
    app.click(".lightbox-zoom-in").await.expect("plus");
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let b = image(&mut app).await;
    let Some((pixels, w, h)) = common::capture(&app) else {
        eprintln!("no grim capture here; skipping the pixel check");
        return;
    };
    let pixel = |x: f64, y: f64| {
        let (x, y) = (x.round() as usize, y.round() as usize);
        assert!(x < w && y < h, "({x}, {y}) is outside the {w}x{h} capture");
        let i = (y * w + x) * 3;
        (pixels[i], pixels[i + 1], pixels[i + 2])
    };
    let blue = |p: (u8, u8, u8)| p.2 > 150 && p.0 < 100 && p.1 < 100;
    // Well inside the picture, at its left edge: blue.
    let inside = pixel(b.x + 12.0, b.y + b.h / 2.0);
    assert!(blue(inside), "the picture's left edge is {inside:?}, not blue: {b:?}");
    for (x, y) in [(b.x, b.y), (b.right() - 1.0, b.y), (b.x, b.bottom() - 1.0), (b.right() - 1.0, b.bottom() - 1.0)] {
        let p = pixel(x, y);
        assert!(!blue(p), "the picture's corner at ({x}, {y}) is {p:?}: square, not rounded");
    }
}
