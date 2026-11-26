//! Windows: toasts under the app's own identity. The app is registered
//! under its AppUserModelID (a name and an icon, for runs with no Start menu
//! shortcut: a portable copy, a dev build), a click is a `morpheus-notify:`
//! link (see `view_model::win_toast`), and the toast is shown with
//! `Windows.UI.Notifications` itself: the notification crate's toasts take
//! PowerShell's identity ("Windows PowerShell") and carry no click target.
#![allow(unsafe_code)]
use std::path::Path;

use windows::{
    Data::Xml::Dom::XmlDocument,
    UI::Notifications::{ToastNotification, ToastNotificationManager},
    Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx},
    core::HSTRING,
};

use crate::view_model::win_toast::{self, APP_ID, DISPLAY_NAME};

/// Name the app (and give it its icon) for toasts when no shortcut does:
/// `HKCU\Software\Classes\AppUserModelId\<APP_ID>`.
pub fn register_identity(icon: &Path) -> std::io::Result<()> {
    fn to_io(err: impl std::fmt::Display) -> std::io::Error {
        std::io::Error::other(err.to_string())
    }
    let key = windows_registry::CURRENT_USER
        .create(win_toast::app_id_key())
        .map_err(to_io)?;
    key.set_string("DisplayName", DISPLAY_NAME).map_err(to_io)?;
    key.set_string("IconUri", icon.to_string_lossy().as_ref())
        .map_err(to_io)?;
    Ok(())
}

/// What the registry holds for `APP_ID`: (display name, icon), for the check.
pub fn registered_identity() -> Option<(String, String)> {
    let key = windows_registry::CURRENT_USER
        .open(win_toast::app_id_key())
        .ok()?;
    Some((
        key.get_string("DisplayName").ok()?,
        key.get_string("IconUri").ok()?,
    ))
}

/// Make `morpheus-notify:` start `exe` (the launcher) with the link, so a
/// click on a toast reaches the running app, or starts it.
pub fn register_click_protocol(exe: &Path) {
    let key = win_toast::protocol_key();
    let Ok(root) = windows_registry::CURRENT_USER.create(&key) else {
        return;
    };
    let _ = root.set_string("", "URL:Morpheus notification");
    let _ = root.set_string("URL Protocol", "");
    if let Ok(command) = windows_registry::CURRENT_USER.create(format!(r"{key}\shell\open\command")) {
        let _ = command.set_string("", win_toast::protocol_command(exe));
    }
}

/// Show `toast` as the app.
pub fn show(toast: &win_toast::Toast) -> windows::core::Result<()> {
    // SAFETY: a plain call; a thread that is already set up answers with a
    // status (RPC_E_CHANGED_MODE) that does not matter here.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(toast.xml()))?;
    let notification = ToastNotification::CreateToastNotification(&doc)?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))?
        .Show(&notification)
}
