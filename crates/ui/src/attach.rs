//! Desktop-agnostic wrappers around native file pickers and the system's
//! file opener, so components never call `rfd`/`open` directly.

use std::path::{Path, PathBuf};

/// Open a native "choose a file to attach" dialog. Resolves to `None` if the
/// dialog could not be shown or the user cancelled.
pub async fn pick_attachment() -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title("Attach a file")
        .pick_file()
        .await
        .map(|handle| handle.path().to_path_buf())
}

/// Open `path` with whatever the OS considers the default handler for it.
pub fn open_with_system(path: &Path) -> Result<(), String> {
    open::that(path).map_err(|err| err.to_string())
}
