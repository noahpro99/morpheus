//! Desktop-agnostic wrappers around native file pickers and the system's
//! file opener, so components never call `rfd`/`open` directly.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

/// A platform's own file picker (Android's, through an activity): asked for
/// any file or (`true`) only pictures, it blocks until one is picked or the
/// picker is closed.
pub type FilePicker = Arc<dyn Fn(bool) -> Option<PathBuf> + Send + Sync>;

static PICKER: OnceLock<FilePicker> = OnceLock::new();

/// Installed once by a platform without `rfd` (the mobile app).
pub fn set_file_picker(picker: FilePicker) {
    let _ = PICKER.set(picker);
}

/// Open a native "choose a file to attach" dialog. Resolves to `None` if the
/// dialog could not be shown or the user cancelled.
pub async fn pick_attachment() -> Option<PathBuf> {
    pick(false).await
}

/// The same, for a picture (an avatar).
pub async fn pick_image() -> Option<PathBuf> {
    pick(true).await
}

async fn pick(images: bool) -> Option<PathBuf> {
    if let Some(picker) = PICKER.get().cloned() {
        return morpheus_core::rt::spawn_blocking(move || picker(images))
            .await
            .ok()
            .flatten();
    }
    desktop_pick(images).await
}

#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
async fn desktop_pick(images: bool) -> Option<PathBuf> {
    let mut dialog = rfd::AsyncFileDialog::new();
    dialog = if images {
        dialog
            .set_title("Choose a picture")
            .add_filter("Pictures", &["png", "jpg", "jpeg", "gif", "webp"])
    } else {
        dialog.set_title("Attach a file")
    };
    dialog
        .pick_file()
        .await
        .map(|handle| handle.path().to_path_buf())
}

/// iOS has no picker installed yet (its document picker is still to do); a
/// browser picks through the page (FILE_LISTENER_JS).
#[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
async fn desktop_pick(_images: bool) -> Option<PathBuf> {
    tracing::warn!("no file picker on this platform yet");
    None
}
