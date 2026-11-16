//! Windows: the taskbar button's overlay icon (`ITaskbarList3::SetOverlayIcon`),
//! the red badge with the unread count. The only unsafe code of the badge.
#![allow(unsafe_code)]
use std::cell::RefCell;
use std::ffi::c_void;

use windows::{
    Win32::{
        Foundation::HWND,
        Graphics::Gdi::{CreateBitmap, DeleteObject, HGDIOBJ},
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        },
        UI::{
            Shell::{ITaskbarList3, TaskbarList},
            WindowsAndMessaging::{CreateIconIndirect, DestroyIcon, HICON, ICONINFO},
        },
    },
    core::{HSTRING, PCWSTR},
};

thread_local! {
    /// The shell object, made once on the main thread.
    static TASKBAR: RefCell<Option<ITaskbarList3>> = const { RefCell::new(None) };
}

fn taskbar() -> Option<ITaskbarList3> {
    TASKBAR.with_borrow_mut(|slot| {
        if slot.is_none() {
            // SAFETY: plain COM calls on the main thread; GPUI has usually set
            // COM up already (this then answers "changed mode", ignored).
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                let list: ITaskbarList3 =
                    CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER).ok()?;
                list.HrInit().ok()?;
                *slot = Some(list);
            }
        }
        slot.clone()
    })
}

/// An icon from non-premultiplied RGBA, `size` x `size`.
fn make_icon(rgba: &[u8], size: u32) -> Option<HICON> {
    let bgra: Vec<u8> = rgba
        .chunks_exact(4)
        .flat_map(|p| [p[2], p[1], p[0], p[3]])
        .collect();
    // SAFETY: the pixel buffer outlives the call, which copies it; the
    // bitmaps are ours to delete once the icon has its own copies.
    unsafe {
        let color = CreateBitmap(
            size as i32,
            size as i32,
            1,
            32,
            Some(bgra.as_ptr() as *const c_void),
        );
        let mask = CreateBitmap(size as i32, size as i32, 1, 1, None);
        let info = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(HGDIOBJ(color.0));
        let _ = DeleteObject(HGDIOBJ(mask.0));
        icon
    }
}

/// Put the badge (`rgba` of `size` x `size`) on the button of the window
/// `hwnd`, or clear it with `None`. `false` if the shell would not take it
/// (no button yet): the caller tries again later.
pub fn set_overlay(hwnd: isize, rgba: Option<&[u8]>, size: u32, count: usize) -> bool {
    let Some(list) = taskbar() else {
        return false;
    };
    let hwnd = HWND(hwnd as *mut c_void);
    let description = HSTRING::from(format!("{count} unread"));
    match rgba {
        Some(rgba) => {
            let Some(icon) = make_icon(rgba, size) else {
                return false;
            };
            // SAFETY: valid window and icon; the shell keeps its own copy.
            let done =
                unsafe { list.SetOverlayIcon(hwnd, icon, PCWSTR(description.as_ptr())) }.is_ok();
            // SAFETY: the icon is ours and no longer used.
            unsafe {
                let _ = DestroyIcon(icon);
            }
            done
        }
        // SAFETY: as above; no icon clears the overlay.
        None => unsafe { list.SetOverlayIcon(hwnd, HICON::default(), PCWSTR::null()) }.is_ok(),
    }
}
