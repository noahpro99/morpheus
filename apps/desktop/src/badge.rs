//! The unread badge outside the window: the tray icon's red dot with the
//! count (every system), the taskbar button's overlay (Windows), the Dock
//! tile's label (macOS) and the launcher entry's count (Linux docks that
//! speak `com.canonical.Unity.LauncherEntry`). The count is the inbox
//! button's: rooms with something unread (`view_model::invites::unread_count`).
//! Only counts are shown, never names, so streamer mode needs no special case.
//!
//! The drawing and the labels are plain functions (unit tested below); the
//! system calls are small and only made when the count changes.

use std::sync::atomic::{AtomicUsize, Ordering};

use morpheus_core::RoomSummary;

/// The red of the dot.
const RED: [u8; 3] = [0xf2, 0x3f, 0x42];

/// The count now on show, for the e2e bridge and to skip repeated updates.
static SHOWN: AtomicUsize = AtomicUsize::new(0);

/// What the tray's dot says: nothing at 0, 1 to 9, then "9+".
pub fn tray_label(count: usize) -> Option<String> {
    match count {
        0 => None,
        1..=9 => Some(count.to_string()),
        _ => Some("9+".into()),
    }
}

/// What the Dock tile says (there is room for two digits): nothing at 0, up
/// to 99, then "99+".
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub fn dock_label(count: usize) -> Option<String> {
    match count {
        0 => None,
        1..=99 => Some(count.to_string()),
        _ => Some("99+".into()),
    }
}

/// 5x7 glyphs of the digits and the plus, one row per `u8` (bit 4 = left).
fn glyph(c: char) -> [u8; 7] {
    match c {
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        '+' => [0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000],
        _ => [0; 7],
    }
}

/// Blend `colour` over the pixel at `(x, y)` with `coverage` (0 to 1).
fn blend(rgba: &mut [u8], width: u32, x: u32, y: u32, colour: [u8; 3], coverage: f32) {
    let at = ((y * width + x) * 4) as usize;
    let px = &mut rgba[at..at + 4];
    let below = px[3] as f32 / 255.0;
    let out = coverage + below * (1.0 - coverage);
    if out <= 0.0 {
        return;
    }
    for (i, c) in colour.iter().enumerate() {
        let mixed = (*c as f32 * coverage + px[i] as f32 * below * (1.0 - coverage)) / out;
        px[i] = mixed.round().clamp(0.0, 255.0) as u8;
    }
    px[3] = (out * 255.0).round().clamp(0.0, 255.0) as u8;
}

/// A red dot of `radius` centred on `(cx, cy)` with `label` in white, over
/// `rgba` (`width` x `height`). `cell` is the size of one glyph pixel.
fn draw_dot(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    (cx, cy): (f32, f32),
    radius: f32,
    cell: f32,
    label: &str,
) {
    let y0 = (cy - radius - 1.0).floor().max(0.0) as u32;
    let y1 = ((cy + radius + 1.0).ceil() as u32).min(height);
    let x0 = (cx - radius - 1.0).floor().max(0.0) as u32;
    let x1 = ((cx + radius + 1.0).ceil() as u32).min(width);
    // The label: glyphs 5 cells wide with a cell between, centred in the dot.
    let chars: Vec<char> = label.chars().collect();
    let text_w = (chars.len() as f32 * 6.0 - 1.0) * cell;
    let text_h = 7.0 * cell;
    let left = cx - text_w / 2.0;
    let top = cy - text_h / 2.0;
    for y in y0..y1 {
        for x in x0..x1 {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let dist = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
            let coverage = (radius - dist + 0.5).clamp(0.0, 1.0);
            if coverage > 0.0 {
                blend(rgba, width, x, y, RED, coverage);
                let (tx, ty) = ((px - left) / cell, (py - top) / cell);
                if tx >= 0.0 && (0.0..7.0).contains(&ty) {
                    let (col, sub) = ((tx / 6.0) as usize, tx % 6.0);
                    if col < chars.len() && sub < 5.0 {
                        let rows = glyph(chars[col]);
                        if rows[ty as usize] >> (4 - sub as u32) & 1 == 1 {
                            blend(rgba, width, x, y, [255, 255, 255], coverage);
                        }
                    }
                }
            }
        }
    }
}

/// The tray icon for `count`: the base icon (`rgba`, `width` x `height`)
/// with a red dot and the count in its bottom-right corner; the base itself
/// at 0.
pub fn tray_icon(rgba: &[u8], width: u32, height: u32, count: usize) -> Vec<u8> {
    let mut out = rgba.to_vec();
    let Some(label) = tray_label(count) else {
        return out;
    };
    let radius = width.min(height) as f32 * 0.34;
    let centre = (width as f32 - radius, height as f32 - radius);
    // Glyph cells scale with the icon (3 px at 64).
    let cell = width.min(height) as f32 * 3.0 / 64.0;
    draw_dot(&mut out, width, height, centre, radius, cell, &label);
    out
}

/// A badge picture of `size` x `size` filling the square: a red disc with
/// the count (the Windows taskbar button's overlay icon). `None` at 0.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn overlay_icon(size: u32, count: usize) -> Option<Vec<u8>> {
    let label = tray_label(count)?;
    let mut out = vec![0u8; (size * size * 4) as usize];
    let half = size as f32 / 2.0;
    // Two glyphs ("9+") are 11 cells wide: 2 px cells in 32.
    let cell = size as f32 / 16.0;
    draw_dot(&mut out, size, size, (half, half), half, cell, &label);
    Some(out)
}

/// The count of the inbox button for these rooms (locked rooms are hidden).
pub fn count_of(rooms: &[RoomSummary]) -> usize {
    morpheus_view_model::invites::unread_count(&crate::locks::unlocked(rooms))
}

/// The count on show now.
pub fn shown() -> usize {
    SHOWN.load(Ordering::Relaxed)
}

/// Show `count` everywhere it goes, if it is not showing already.
pub fn set(count: usize, cx: &mut gpui::App) {
    if SHOWN.swap(count, Ordering::Relaxed) == count {
        return;
    }
    // Test runs have no tray, Dock or taskbar of their own to change.
    if !crate::tray::wanted() {
        return;
    }
    crate::tray::show_badge(count);
    system::set(count, cx);
}

/// Windows re-applies the overlay when the taskbar button is new (the window
/// came back from the tray); called every few seconds.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn tick(cx: &mut gpui::App) {
    system::tick(cx);
}

#[cfg(target_os = "linux")]
mod system {
    use std::collections::HashMap;

    /// The launcher entry the dock matches the app by: the desktop file's name.
    const APP_URI: &str = "application://morpheus.desktop";

    pub fn set(count: usize, _cx: &mut gpui::App) {
        // A D-Bus round trip: not on the UI thread.
        std::thread::spawn(move || {
            use zbus::zvariant::Value;
            let Ok(connection) = zbus::blocking::Connection::session() else {
                return;
            };
            let mut props: HashMap<&str, Value<'_>> = HashMap::new();
            props.insert("count", Value::I64(count.min(i64::MAX as usize) as i64));
            props.insert("count-visible", Value::Bool(count > 0));
            let _ = connection.emit_signal(
                None::<&str>,
                "/com/canonical/unity/launcherentry/morpheus",
                "com.canonical.Unity.LauncherEntry",
                "Update",
                &(APP_URI, props),
            );
        });
    }

    pub fn tick(_cx: &mut gpui::App) {}
}

#[cfg(target_os = "macos")]
mod system {
    pub fn set(count: usize, _cx: &mut gpui::App) {
        use objc2::MainThreadMarker;
        use objc2_app_kit::NSApplication;
        use objc2_foundation::NSString;
        // GPUI calls on the main thread; so does this.
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let label = super::dock_label(count);
        let tile = NSApplication::sharedApplication(mtm).dockTile();
        tile.setBadgeLabel(label.as_deref().map(NSString::from_str).as_deref());
    }

    pub fn tick(_cx: &mut gpui::App) {}
}

#[cfg(target_os = "windows")]
mod system {
    use std::sync::Mutex;

    /// The window and count the overlay was last put on.
    static APPLIED: Mutex<Option<(isize, usize)>> = Mutex::new(None);

    pub fn set(count: usize, cx: &mut gpui::App) {
        apply(count, cx);
    }

    pub fn tick(cx: &mut gpui::App) {
        apply(super::SHOWN.load(std::sync::atomic::Ordering::Relaxed), cx);
    }

    fn apply(count: usize, cx: &mut gpui::App) {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let hwnd = cx.windows().first().and_then(|window| {
            window
                .update(cx, |_, window, _| match window.window_handle().ok()?.as_raw() {
                    RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
                    _ => None,
                })
                .ok()
                .flatten()
        });
        let Some(hwnd) = hwnd else {
            // No window: no taskbar button; the next tick finds the new one.
            *APPLIED.lock().unwrap() = None;
            return;
        };
        let mut applied = APPLIED.lock().unwrap();
        if *applied == Some((hwnd, count)) {
            return;
        }
        let icon = super::overlay_icon(32, count);
        if crate::win_badge::set_overlay(hwnd, icon.as_deref(), 32, count) {
            *applied = Some((hwnd, count));
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod system {
    pub fn set(_count: usize, _cx: &mut gpui::App) {}
    pub fn tick(_cx: &mut gpui::App) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 64x64 stand-in for the base icon: opaque white in the middle.
    fn base() -> Vec<u8> {
        let mut rgba = vec![0u8; 64 * 64 * 4];
        for y in 8..56 {
            for x in 8..56 {
                let at = (y * 64 + x) * 4;
                rgba[at..at + 4].copy_from_slice(&[240, 240, 240, 255]);
            }
        }
        rgba
    }

    fn is_red(px: &[u8]) -> bool {
        px[3] == 255 && px[0] > 200 && px[1] < 90 && px[2] < 90
    }

    #[test]
    fn labels_run_to_nine_plus_and_ninety_nine_plus() {
        assert_eq!(tray_label(0), None);
        assert_eq!(tray_label(1).as_deref(), Some("1"));
        assert_eq!(tray_label(9).as_deref(), Some("9"));
        assert_eq!(tray_label(10).as_deref(), Some("9+"));
        assert_eq!(tray_label(500).as_deref(), Some("9+"));
        assert_eq!(dock_label(0), None);
        assert_eq!(dock_label(42).as_deref(), Some("42"));
        assert_eq!(dock_label(100).as_deref(), Some("99+"));
    }

    #[test]
    fn zero_leaves_the_base_icon_as_it_was() {
        assert_eq!(tray_icon(&base(), 64, 64, 0), base());
    }

    #[test]
    fn a_count_paints_a_red_dot_bottom_right_and_nothing_else() {
        let before = base();
        let after = tray_icon(&before, 64, 64, 3);
        let red: Vec<(usize, usize)> = (0..64)
            .flat_map(|y| (0..64).map(move |x| (x, y)))
            .filter(|(x, y)| is_red(&after[(y * 64 + x) * 4..][..4]))
            .collect();
        assert!(red.len() > 150, "a dot, not a speck: {}", red.len());
        assert!(
            red.iter().all(|(x, y)| *x >= 16 && *y >= 16),
            "in the bottom-right corner"
        );
        // The top-left corner is the base.
        for y in 0..19 {
            for x in 0..19 {
                let at = (y * 64 + x) * 4;
                assert_eq!(after[at..at + 4], before[at..at + 4]);
            }
        }
        // The digit is white inside the dot.
        let white = (32..64)
            .flat_map(|y| (32..64).map(move |x| (x, y)))
            .filter(|(x, y)| after[(y * 64 + x) * 4..][..4] == [255, 255, 255, 255])
            .count();
        assert!(white > 10, "digit pixels: {white}");
    }

    #[test]
    fn drawing_is_deterministic_and_the_count_shows() {
        assert_eq!(tray_icon(&base(), 64, 64, 7), tray_icon(&base(), 64, 64, 7));
        assert_ne!(tray_icon(&base(), 64, 64, 7), tray_icon(&base(), 64, 64, 8));
        assert_ne!(tray_icon(&base(), 64, 64, 9), tray_icon(&base(), 64, 64, 12));
        // Past nine it says "9+" whatever the number.
        assert_eq!(tray_icon(&base(), 64, 64, 12), tray_icon(&base(), 64, 64, 99));
    }

    #[test]
    fn the_overlay_is_a_red_disc_with_a_label_and_nothing_at_zero() {
        assert!(overlay_icon(32, 0).is_none());
        let icon = overlay_icon(32, 4).unwrap();
        assert_eq!(icon.len(), 32 * 32 * 4);
        // Corners transparent, centre-left red or white, some of each.
        assert_eq!(icon[3], 0);
        let reds = icon.chunks(4).filter(|p| is_red(p)).count();
        let whites = icon.chunks(4).filter(|p| p[..4] == [255, 255, 255, 255]).count();
        assert!(reds > 300 && whites > 10, "{reds} red, {whites} white");
    }
}
