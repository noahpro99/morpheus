//! The low-delay noise suppression model (#37): DeepFilterNet 3 without
//! look-ahead, 36 MB, downloaded when it is first chosen and kept in the data
//! directory. A file is used only once its SHA-256 matches the pinned one; a
//! partial or corrupt file is never loaded.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use sha2::{Digest as _, Sha256};

/// Where the model is downloaded from: the pre-release `models-dfn3-ll-v1`
/// (the file is DeepFilterNet's own `DeepFilterNet3_ll_onnx.tar.gz`, MIT or
/// Apache-2.0, copied unchanged). `MORPHEUS_NOISE_MODEL_URL` names another
/// place (tests serve it locally).
pub const LOW_DELAY_URL: &str =
    "https://github.com/noahpro99/morpheus/releases/download/models-dfn3-ll-v1/DeepFilterNet3_ll_onnx.tar.gz";
/// The file's SHA-256, lower case hex.
pub const LOW_DELAY_SHA256: &str =
    "5998e58e8ba0e09bb76986ef97b84afa065a571ef282d4a1222f341e3251cf3a";
/// The file's size in bytes (for progress, and a quick check).
pub const LOW_DELAY_BYTES: u64 = 36_359_660;
/// Its name in the models directory.
pub const LOW_DELAY_FILE: &str = "DeepFilterNet3_ll_onnx.tar.gz";

static DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// The directory the downloaded models live in (set at startup by the app).
pub fn set_model_dir(dir: PathBuf) {
    if let Ok(mut now) = DIR.lock() {
        *now = Some(dir);
    }
}

/// Where the low-delay model is (or will be) kept.
pub fn low_delay_path() -> Option<PathBuf> {
    DIR.lock()
        .ok()
        .and_then(|dir| dir.clone())
        .map(|dir| dir.join(LOW_DELAY_FILE))
}

/// The URL to download from.
pub fn low_delay_url() -> String {
    std::env::var("MORPHEUS_NOISE_MODEL_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| LOW_DELAY_URL.to_owned())
}

/// Lower-case hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// `bytes` if they are the model (the pinned SHA-256), else why not.
pub fn check(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    if bytes.len() as u64 != LOW_DELAY_BYTES || sha256_hex(&bytes) != LOW_DELAY_SHA256 {
        return Err(format!(
            "the low-delay model file is not the expected one ({} bytes)",
            bytes.len()
        ));
    }
    Ok(bytes)
}

/// The low-delay model's bytes, read from its file and checked; `Err` says
/// what is wrong (not downloaded, partial, corrupt).
pub fn read_low_delay(path: &Path) -> Result<Vec<u8>, String> {
    match std::fs::read(path) {
        Ok(bytes) => check(bytes),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Err("the low-delay model is not downloaded".to_owned())
        }
        Err(err) => Err(format!("the low-delay model could not be read: {err}")),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn the_hash_is_sha256_in_lower_hex() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn only_the_pinned_file_passes() {
        let err = check(b"not the model".to_vec()).expect_err("wrong bytes");
        assert!(err.contains("not the expected"), "{err}");
        assert!(check(Vec::new()).is_err(), "an empty file");
    }

    #[test]
    fn a_missing_or_partial_file_is_not_loaded() {
        let dir = std::env::temp_dir().join(format!("morpheus-model-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join(LOW_DELAY_FILE);
        assert!(read_low_delay(&path).expect_err("missing").contains("not downloaded"));
        std::fs::write(&path, b"half of it").expect("write");
        assert!(read_low_delay(&path).is_err(), "a partial file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The real model, if this machine built the crate (its checkout has it):
    /// it matches the pinned hash and size.
    #[test]
    fn the_pinned_hash_is_the_models() {
        let Some(path) = checkout_model() else {
            eprintln!("DeepFilterNet checkout not found; skipping the real-file check");
            return;
        };
        let bytes = std::fs::read(path).expect("read");
        assert_eq!(bytes.len() as u64, LOW_DELAY_BYTES);
        assert_eq!(sha256_hex(&bytes), LOW_DELAY_SHA256);
        assert!(check(bytes).is_ok());
    }

    /// Tests that point the (process-wide) model directory somewhere take this.
    pub(crate) static DIR_LOCK: Mutex<()> = Mutex::new(());

    pub(crate) fn checkout_model() -> Option<PathBuf> {
        let home = std::env::var_os("CARGO_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))?;
        let checkouts = home.join("git/checkouts");
        for repo in std::fs::read_dir(checkouts).ok()?.flatten() {
            if !repo.file_name().to_string_lossy().starts_with("deepfilternet") {
                continue;
            }
            for rev in std::fs::read_dir(repo.path()).ok()?.flatten() {
                let file = rev.path().join("models").join(LOW_DELAY_FILE);
                if file.exists() {
                    return Some(file);
                }
            }
        }
        None
    }
}
