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
pub const APP_ID: &str = crate::view_model::win_toast::APP_ID;

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
        let set = store.SetValue(&PKEY_AppUserModel_ID, &value);
        // Its text is ours, not the shell's: dropping the value would free it with
        // CoTaskMemFree (PropVariantClear) and corrupt the heap.
        std::mem::forget(value);
        set?;
        store.Commit()?;
        let file: IPersistFile = link.cast()?;
        file.Save(&HSTRING::from(shortcut.as_os_str()), true)?;
    }
    Ok(())
}

/// The AppUserModelID this process has, as the shell reports it.
pub fn process_app_id() -> Option<String> {
    // SAFETY: the call returns a string the shell allocated; it is read, then
    // freed with CoTaskMemFree as documented.
    unsafe {
        let id = windows::Win32::UI::Shell::GetCurrentProcessExplicitAppUserModelID().ok()?;
        let text = id.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(id.0 as *const _));
        text
    }
}

/// The AppUserModelID stored in the shortcut at `shortcut`.
pub fn shortcut_app_id(shortcut: &Path) -> Option<String> {
    use windows::Win32::System::Com::STGM_READ;
    // SAFETY: COM calls on this thread; the string from PropVariantToStringAlloc
    // is read, then freed with CoTaskMemFree.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;
        file.Load(&HSTRING::from(shortcut.as_os_str()), STGM_READ).ok()?;
        let store: IPropertyStore = link.cast().ok()?;
        let value = store.GetValue(&PKEY_AppUserModel_ID).ok()?;
        let raw = &value.Anonymous.Anonymous;
        if raw.vt != VT_LPWSTR {
            return None;
        }
        raw.Anonymous.pwszVal.to_string().ok()
    }
}
