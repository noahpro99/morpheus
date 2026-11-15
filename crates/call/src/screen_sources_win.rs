//! Windows: the screens and windows a share can show, for the app's picker
//! (`screen_sources.rs`). The list is libwebrtc's desktop capturer's, so its
//! ids are the ones the share selects (a screen's display device index, a
//! window's handle). The pictures are GDI's: a screen copied from the
//! desktop, a window drawn by PrintWindow, both scaled down with HALFTONE.
//! Not libwebrtc's capture, which on Windows 10 puts a yellow border round
//! every window it captures, and would start one for every window listed.
//!
//! GDI through raw calls: this module may use `unsafe`.
#![allow(unsafe_code)]

use std::{ffi::c_void, sync::Mutex};

use livekit::webrtc::desktop_capturer::{
    DesktopCaptureSourceType, DesktopCapturer, DesktopCapturerOptions,
};
use windows::{
    Win32::{
        Foundation::{HWND, RECT},
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC,
            DEVMODEW, DIB_RGB_COLORS, DISPLAY_DEVICE_ACTIVE, DISPLAY_DEVICEW, DeleteDC,
            DeleteObject, ENUM_CURRENT_SETTINGS, EnumDisplayDevicesW, EnumDisplaySettingsW, GetDC,
            GetDIBits, HALFTONE, HBITMAP, HDC, ReleaseDC, SRCCOPY, SelectObject, SetBrushOrgEx,
            SetStretchBltMode, StretchBlt,
        },
        Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow},
        UI::WindowsAndMessaging::{GetWindowRect, IsHungAppWindow, IsIconic, IsWindow},
    },
    core::PCWSTR,
};

use crate::{ScreenSource, ScreenSourceKind, ScreenSourceProvider, Thumbnail, fit_thumbnail};

/// PrintWindow's PW_RENDERFULLCONTENT (Windows 8.1 and later): what DWM
/// composes too, so a GPU-drawn window is not black.
const PW_RENDERFULLCONTENT: PRINT_WINDOW_FLAGS = PRINT_WINDOW_FLAGS(2);

/// The capturers that list the screens and the windows, made once.
#[derive(Default)]
pub(crate) struct WindowsSources {
    listers: Mutex<Option<(Option<DesktopCapturer>, Option<DesktopCapturer>)>>,
}

impl ScreenSourceProvider for WindowsSources {
    fn sources(&self) -> Vec<ScreenSource> {
        let Ok(mut listers) = self.listers.lock() else {
            return Vec::new();
        };
        let (screens, windows) = listers.get_or_insert_with(|| {
            let make = |kind| DesktopCapturer::new(DesktopCapturerOptions::new(kind));
            (
                make(DesktopCaptureSourceType::Screen),
                make(DesktopCaptureSourceType::Window),
            )
        });
        let mut out = Vec::new();
        if let Some(screens) = screens {
            for (n, source) in screens.get_source_list().into_iter().enumerate() {
                let title = source.title();
                out.push(ScreenSource {
                    kind: ScreenSourceKind::Screen,
                    id: source.id(),
                    name: if title.trim().is_empty() {
                        format!("Screen {}", n + 1)
                    } else {
                        title
                    },
                });
            }
        }
        if let Some(windows) = windows {
            for source in windows.get_source_list() {
                let title = source.title();
                if title.trim().is_empty() {
                    continue;
                }
                out.push(ScreenSource {
                    kind: ScreenSourceKind::Window,
                    id: source.id(),
                    name: title,
                });
            }
        }
        out
    }

    fn thumbnail(
        &self,
        source: &ScreenSource,
        max_width: u32,
        max_height: u32,
    ) -> Option<Thumbnail> {
        match source.kind {
            ScreenSourceKind::Screen => screen_thumbnail(source.id, max_width, max_height),
            ScreenSourceKind::Window => window_thumbnail(source.id, max_width, max_height),
        }
    }
}

/// Display device `index`'s place on the desktop, as libwebrtc's
/// GetScreenRect finds it.
fn screen_rect(index: u64) -> Option<RECT> {
    let index = u32::try_from(index).ok()?;
    let mut device = DISPLAY_DEVICEW {
        cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
        ..Default::default()
    };
    // SAFETY: `device` is a DISPLAY_DEVICEW with its size set.
    if !unsafe { EnumDisplayDevicesW(PCWSTR::null(), index, &mut device, 0) }.as_bool()
        || !device.StateFlags.contains(DISPLAY_DEVICE_ACTIVE)
    {
        return None;
    }
    let mut mode = DEVMODEW {
        dmSize: std::mem::size_of::<DEVMODEW>() as u16,
        ..Default::default()
    };
    // SAFETY: the name is the NUL-terminated one just filled in; `mode` has
    // its size set.
    if !unsafe {
        EnumDisplaySettingsW(
            PCWSTR(device.DeviceName.as_ptr()),
            ENUM_CURRENT_SETTINGS,
            &mut mode,
        )
    }
    .as_bool()
    {
        return None;
    }
    // SAFETY: a display's settings fill in the position member of the union.
    let at = unsafe { mode.Anonymous1.Anonymous2.dmPosition };
    Some(RECT {
        left: at.x,
        top: at.y,
        right: at.x + mode.dmPelsWidth as i32,
        bottom: at.y + mode.dmPelsHeight as i32,
    })
}

fn screen_thumbnail(index: u64, max_width: u32, max_height: u32) -> Option<Thumbnail> {
    let rect = screen_rect(index)?;
    // SAFETY: the desktop's DC, released below.
    let desktop = unsafe { GetDC(None) };
    if desktop.is_invalid() {
        return None;
    }
    let thumb = scaled(desktop, rect.left, rect.top, &rect, max_width, max_height);
    // SAFETY: got above, for the whole desktop.
    unsafe { ReleaseDC(None, desktop) };
    thumb
}

fn window_thumbnail(id: u64, max_width: u32, max_height: u32) -> Option<Thumbnail> {
    let hwnd = HWND(id as usize as *mut c_void);
    // SAFETY: plain queries on a handle that may be stale (they say so).
    // A hung window would hold PrintWindow up: it is left as it is.
    if unsafe {
        !IsWindow(Some(hwnd)).as_bool()
            || IsIconic(hwnd).as_bool()
            || IsHungAppWindow(hwnd).as_bool()
    } {
        return None;
    }
    let mut rect = RECT::default();
    // SAFETY: `rect` is ours to fill.
    unsafe { GetWindowRect(hwnd, &mut rect) }.ok()?;
    let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
    if width <= 0 || height <= 0 {
        return None;
    }
    // The whole window drawn at its size, then scaled down.
    // SAFETY: every DC and bitmap made here is let go before returning.
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let full_dc = CreateCompatibleDC(Some(screen));
        let full = CreateCompatibleBitmap(screen, width, height);
        let old = SelectObject(full_dc, full.into());
        let drawn = PrintWindow(hwnd, full_dc, PW_RENDERFULLCONTENT).as_bool();
        let whole = RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        };
        let thumb = drawn
            .then(|| scaled(full_dc, 0, 0, &whole, max_width, max_height))
            .flatten();
        SelectObject(full_dc, old);
        let _ = DeleteObject(full.into());
        let _ = DeleteDC(full_dc);
        ReleaseDC(None, screen);
        thumb
    }
}

/// `rect`'s size of `source` from (`x`, `y`), scaled down to fit, as B, G,
/// R, A top row first.
fn scaled(
    source: HDC,
    x: i32,
    y: i32,
    rect: &RECT,
    max_width: u32,
    max_height: u32,
) -> Option<Thumbnail> {
    let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
    if width <= 0 || height <= 0 {
        return None;
    }
    let (w, h) = fit_thumbnail(width as u32, height as u32, max_width, max_height);
    let (w, h) = (w as i32, h as i32);
    // SAFETY: the DC and bitmap made here are let go before returning; the
    // pixels go into a buffer of exactly the size asked for.
    unsafe {
        let dc = CreateCompatibleDC(Some(source));
        if dc.is_invalid() {
            return None;
        }
        let bitmap: HBITMAP = CreateCompatibleBitmap(source, w, h);
        let old = SelectObject(dc, bitmap.into());
        SetStretchBltMode(dc, HALFTONE);
        let _ = SetBrushOrgEx(dc, 0, 0, None);
        let copied =
            StretchBlt(dc, 0, 0, w, h, Some(source), x, y, width, height, SRCCOPY).as_bool();
        SelectObject(dc, old);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                // Negative: top row first.
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bgra = vec![0u8; (w * h * 4) as usize];
        let rows = if copied {
            GetDIBits(
                dc,
                bitmap,
                0,
                h as u32,
                Some(bgra.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            )
        } else {
            0
        };
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        if rows != h {
            return None;
        }
        // GDI leaves alpha at 0.
        for pixel in bgra.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        Some(Thumbnail {
            width: w as u32,
            height: h as u32,
            bgra,
        })
    }
}
