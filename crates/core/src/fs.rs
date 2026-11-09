//! The little the core keeps on disk (the account list, sessions), through
//! one door: `std::fs` on desktops and phones, the page's `localStorage`
//! in a browser, keyed by path. The SDK's own stores (SQLite, IndexedDB)
//! are chosen in `session.rs`.
//!
//! Files picked in a browser (a new picture, an attachment) have no path on
//! disk: [`stash`] holds their bytes in memory under a made-up one.

use std::{io, path::Path};

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::{io, path::Path};

    pub fn read(path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }
    pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        std::fs::write(path, bytes)
    }
    pub fn remove_file(path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }
    pub fn remove_dir_all(path: &Path) -> io::Result<()> {
        std::fs::remove_dir_all(path)
    }
    pub fn create_dir_all(path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }
    pub fn exists(path: &Path) -> bool {
        path.exists()
    }
    pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }
    /// Names of what is directly inside `dir`.
    pub fn list(dir: &Path) -> io::Result<Vec<String>> {
        Ok(std::fs::read_dir(dir)?
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect())
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use std::{cell::RefCell, collections::HashMap, io, path::Path};

    use base64::Engine;

    const PREFIX: &str = "morpheus-fs:";

    thread_local! {
        /// Picked files, by their made-up path.
        pub(super) static STASH: RefCell<HashMap<String, Vec<u8>>> = RefCell::new(HashMap::new());
    }

    fn storage() -> io::Result<web_sys::Storage> {
        web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .ok_or_else(|| io::Error::other("no localStorage"))
    }
    fn key(path: &Path) -> String {
        format!("{PREFIX}{}", path.to_string_lossy().trim_end_matches('/'))
    }
    fn not_found() -> io::Error {
        io::Error::from(io::ErrorKind::NotFound)
    }
    /// Every stored path at or under `dir`.
    fn under(dir: &Path) -> io::Result<Vec<String>> {
        let store = storage()?;
        let base = key(dir);
        let len = store
            .length()
            .map_err(|_| io::Error::other("localStorage"))?;
        Ok((0..len)
            .filter_map(|i| store.key(i).ok().flatten())
            .filter(|k| k == &base || k.starts_with(&format!("{base}/")))
            .collect())
    }

    pub fn read(path: &Path) -> io::Result<Vec<u8>> {
        let name = path.to_string_lossy().into_owned();
        if let Some(bytes) = STASH.with(|s| s.borrow().get(&name).cloned()) {
            return Ok(bytes);
        }
        let text = storage()?
            .get_item(&key(path))
            .ok()
            .flatten()
            .ok_or_else(not_found)?;
        base64::engine::general_purpose::STANDARD
            .decode(text)
            .map_err(io::Error::other)
    }
    pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        storage()?
            .set_item(
                &key(path),
                &base64::engine::general_purpose::STANDARD.encode(bytes),
            )
            .map_err(|_| io::Error::other("localStorage is full"))
    }
    pub fn remove_file(path: &Path) -> io::Result<()> {
        storage()?
            .remove_item(&key(path))
            .map_err(|_| io::Error::other("localStorage"))
    }
    pub fn remove_dir_all(path: &Path) -> io::Result<()> {
        let store = storage()?;
        for k in under(path)? {
            let _ = store.remove_item(&k);
        }
        Ok(())
    }
    /// Folders are implied by the files in them.
    pub fn create_dir_all(_path: &Path) -> io::Result<()> {
        Ok(())
    }
    pub fn exists(path: &Path) -> bool {
        under(path).is_ok_and(|found| !found.is_empty())
    }
    pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
        let store = storage()?;
        let (old, new) = (key(from), key(to));
        for k in under(from)? {
            if let Ok(Some(value)) = store.get_item(&k) {
                let moved = format!("{new}{}", &k[old.len()..]);
                let _ = store.set_item(&moved, &value);
                let _ = store.remove_item(&k);
            }
        }
        Ok(())
    }
    pub fn list(dir: &Path) -> io::Result<Vec<String>> {
        let base = format!("{}/", key(dir));
        let mut names: Vec<String> = under(dir)?
            .iter()
            .filter_map(|k| k.strip_prefix(&base))
            .filter_map(|rest| rest.split('/').next())
            .map(str::to_owned)
            .collect();
        names.sort();
        names.dedup();
        Ok(names)
    }
}

pub fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    imp::read(path.as_ref())
}
pub fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> io::Result<()> {
    imp::write(path.as_ref(), bytes.as_ref())
}
pub fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
    imp::remove_file(path.as_ref())
}
pub fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    imp::remove_dir_all(path.as_ref())
}
pub fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    imp::create_dir_all(path.as_ref())
}
pub fn exists(path: impl AsRef<Path>) -> bool {
    imp::exists(path.as_ref())
}
pub fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
    imp::rename(from.as_ref(), to.as_ref())
}
/// Names of what is directly inside `dir`.
pub fn list(dir: impl AsRef<Path>) -> io::Result<Vec<String>> {
    imp::list(dir.as_ref())
}

/// Read a file as text.
pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
    String::from_utf8(read(path)?).map_err(io::Error::other)
}

/// Hold a picked file's bytes under a made-up path (a browser's files have
/// none), so the calls that take a path can take it.
#[cfg(target_arch = "wasm32")]
pub fn stash(name: &str, bytes: Vec<u8>) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let path = format!("/picked/{}/{name}", NEXT.fetch_add(1, Ordering::Relaxed));
    imp::STASH.with(|s| s.borrow_mut().insert(path.clone(), bytes));
    path.into()
}

/// The same, as futures that keep disk work off the async threads on
/// desktops and phones (`tokio::fs`); in a browser they are immediate.
pub mod nonblocking {
    use std::{io, path::Path};

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        tokio::fs::read(path.as_ref()).await
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> io::Result<()> {
        tokio::fs::write(path.as_ref(), bytes.as_ref()).await
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        tokio::fs::create_dir_all(path.as_ref()).await
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        tokio::fs::remove_dir_all(path.as_ref()).await
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn try_exists(path: impl AsRef<Path>) -> io::Result<bool> {
        tokio::fs::try_exists(path.as_ref()).await
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
        tokio::fs::rename(from.as_ref(), to.as_ref()).await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        super::read(path)
    }
    #[cfg(target_arch = "wasm32")]
    pub async fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> io::Result<()> {
        super::write(path, bytes)
    }
    #[cfg(target_arch = "wasm32")]
    pub async fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        super::create_dir_all(path)
    }
    #[cfg(target_arch = "wasm32")]
    pub async fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        super::remove_dir_all(path)
    }
    #[cfg(target_arch = "wasm32")]
    pub async fn try_exists(path: impl AsRef<Path>) -> io::Result<bool> {
        Ok(super::imp::exists(path.as_ref()))
    }
    #[cfg(target_arch = "wasm32")]
    pub async fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
        super::rename(from, to)
    }
}
