//! Windows: what the diagnostics ask the system (memory of this process,
//! Windows build, refresh rate) and the taskbar flash for a new message.
//! Small unsafe calls kept here, as in `win_badge.rs`.
#![allow(unsafe_code)]
use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::{
    Foundation::HWND,
    Graphics::Gdi::{DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW},
    System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX},
        Threading::GetCurrentProcess,
    },
    UI::WindowsAndMessaging::{FLASHW_TIMERNOFG, FLASHW_TRAY, FLASHWINFO, FlashWindowEx},
};

/// The working set and the private bytes of this process, in KB.
pub fn memory_kb() -> (u64, u64) {
    let mut counters = PROCESS_MEMORY_COUNTERS_EX::default();
    counters.cb = size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    // SAFETY: the pseudo handle of this process and a correctly sized
    // struct (the _EX one starts with the plain one) it fills in.
    let ok = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
            counters.cb,
        )
    }
    .is_ok();
    if ok {
        (
            counters.WorkingSetSize as u64 / 1024,
            counters.PrivateUsage as u64 / 1024,
        )
    } else {
        (0, 0)
    }
}

/// The Windows build and its display version ("24H2"), from the registry.
pub fn windows_build() -> (u32, String) {
    let key = windows_registry::LOCAL_MACHINE
        .open(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        .ok();
    let get = |name: &str| key.as_ref().and_then(|k| k.get_string(name).ok());
    let build = get("CurrentBuild").and_then(|b| b.trim().parse().ok()).unwrap_or(0);
    let display = get("DisplayVersion").or_else(|| get("ReleaseId")).unwrap_or_default();
    (build, display)
}

/// The primary display's current refresh rate in Hz.
pub fn refresh_rate_hz() -> Option<u32> {
    let mut mode = DEVMODEW::default();
    mode.dmSize = size_of::<DEVMODEW>() as u16;
    // SAFETY: a null device name means the primary display; `mode` is sized.
    let ok = unsafe {
        EnumDisplaySettingsW(windows::core::PCWSTR::null(), ENUM_CURRENT_SETTINGS, &mut mode)
    }
    .as_bool();
    (ok && mode.dmDisplayFrequency > 1).then_some(mode.dmDisplayFrequency)
}

/// The main window, noted by the UI thread for the notifier's thread.
static WINDOW: AtomicIsize = AtomicIsize::new(0);

pub fn note_window(hwnd: isize) {
    WINDOW.store(hwnd, Ordering::Relaxed);
}

/// Flash the taskbar button until the window is in front again.
pub fn flash_taskbar() {
    let hwnd = WINDOW.load(Ordering::Relaxed);
    if hwnd == 0 {
        return;
    }
    let info = FLASHWINFO {
        cbSize: size_of::<FLASHWINFO>() as u32,
        hwnd: HWND(hwnd as *mut std::ffi::c_void),
        dwFlags: FLASHW_TRAY | FLASHW_TIMERNOFG,
        uCount: u32::MAX,
        dwTimeout: 0,
    };
    // SAFETY: a plain call with a filled struct; a stale window just fails.
    unsafe {
        let _ = FlashWindowEx(&info);
    }
}
