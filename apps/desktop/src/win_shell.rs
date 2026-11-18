//! Windows: the app's identity for the taskbar and the Start menu.
//!
//! One AppUserModelID (`dev.morpheus.Morpheus`) names the app for every
//! process and shortcut, so its windows group under the taskbar pin (and
//! the Start menu shortcut) instead of a button of their own. The pin and
//! the shortcut point at the launcher (`Morpheus.exe`), never at the app,
//! which an update replaces.
#![allow(unsafe_code)]
use std::mem::ManuallyDrop;
use std::path::Path;

use windows::{
    Win32::{
        Storage::EnhancedStorage::PKEY_AppUserModel_ID,
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                IPersistFile,
                StructuredStorage::{PROPVARIANT, PROPVARIANT_0_0, PROPVARIANT_0_0_0},
            },
            Variant::VT_LPWSTR,
        },
        UI::Shell::{
            IShellLinkW, PropertiesSystem::IPropertyStore, SetCurrentProcessExplicitAppUserModelID,
            ShellLink,
        },
    },
    core::{HSTRING, Interface, PWSTR},
};

/// What names the app to the shell.
pub const APP_ID: &str = "dev.morpheus.Morpheus";

/// Name this process's windows `APP_ID`. First thing, before any window.
pub fn set_app_id() {
    // SAFETY: a plain call with a string that outlives it.
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(APP_ID));
    }
}

/// Make the Start menu shortcut to `launcher` (with `APP_ID` on it, which is
/// what ties the pin and the windows together) unless it is there.
pub fn ensure_start_menu_shortcut(launcher: &Path, shortcut: &Path) -> windows::core::Result<()> {
    if shortcut.is_file() {
        return Ok(());
    }
    if let Some(dir) = shortcut.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let dir = launcher.parent().unwrap_or(launcher);
    // SAFETY: COM calls on this thread; the string behind the property value
    // outlives `SetValue`, which copies it, and the value is never cleared (it
    // does not own the text).
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(launcher.as_os_str()))?;
        link.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()))?;
        link.SetIconLocation(&HSTRING::from(launcher.as_os_str()), 0)?;
        link.SetDescription(&HSTRING::from("Morpheus"))?;
        let store: IPropertyStore = link.cast()?;
        let mut text: Vec<u16> = APP_ID.encode_utf16().chain(Some(0)).collect();
        let mut value = PROPVARIANT::default();
        value.Anonymous.Anonymous = ManuallyDrop::new(PROPVARIANT_0_0 {
            vt: VT_LPWSTR,
            Anonymous: PROPVARIANT_0_0_0 {
                pwszVal: PWSTR(text.as_mut_ptr()),
            },
            ..Default::default()
        });
        store.SetValue(&PKEY_AppUserModel_ID, &value)?;
        store.Commit()?;
        let file: IPersistFile = link.cast()?;
        file.Save(&HSTRING::from(shortcut.as_os_str()), true)?;
    }
    Ok(())
}
