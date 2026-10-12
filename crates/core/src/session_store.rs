//! Secure storage for session credentials.
//!
//! Two backends implement [`SessionStore`]:
//!
//! - [`KeyringStore`]: the OS keyring (Secret Service on Linux via D-Bus,
//!   Keychain on macOS, Credential Manager on Windows) through the `keyring`
//!   crate.
//! - [`FileStore`]: an encrypted-at-rest file, for headless environments
//!   (tests, CI, containers) that have no keyring daemon.
//!
//! [`resolve`] picks a backend: `MORPHEUS_SESSION_STORE=keyring|file` or a
//! `SessionStoreKind` passed to [`crate::Core::with_session_store`] forces
//! one; otherwise the keyring is tried first and the file backend is used
//! (with a warning) if it is unavailable.
//!
//! All operations here are blocking (the `keyring` crate and `std::fs` are
//! both synchronous); callers run them with `tokio::task::spawn_blocking`.

use std::path::{Path, PathBuf};

use base64::Engine;
use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce, aead::Aead};
use keyring::Entry;
use matrix_sdk::authentication::matrix::MatrixSession;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::{Error, Result};

/// Service name under which keyring entries are grouped.
const SERVICE: &str = "dev.morpheus.Morpheus";
/// A fixed, data-dir-independent entry used only to test keyring availability.
const PROBE_ACCOUNT: &str = "keyring-probe";

/// Directory holding the local sqlite caches, relative to the data dir.
pub(crate) const STORE_DIR: &str = "store";

const KEY_FILE: &str = "session.key";
const ENC_FILE: &str = "session.enc";
const LEGACY_SESSION_FILE: &str = "session.json";

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

/// Everything persisted across launches: where to reconnect, who we are, and
/// the passphrase protecting the local sqlite store.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct StoredSession {
    pub homeserver: String,
    pub session: MatrixSession,
    pub store_passphrase: String,
}

/// Shape of the legacy, unencrypted `session.json`, kept only for migration.
#[derive(Deserialize)]
struct LegacyStoredSession {
    homeserver: String,
    session: MatrixSession,
}

/// Which secure storage backend to use for session credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStoreKind {
    /// The OS keyring (Secret Service / Keychain / Credential Manager).
    Keyring,
    /// An encrypted-at-rest file under the data directory.
    File,
}

impl SessionStoreKind {
    fn from_env() -> Option<Self> {
        match std::env::var("MORPHEUS_SESSION_STORE").ok()?.as_str() {
            "keyring" => Some(Self::Keyring),
            "file" => Some(Self::File),
            other => {
                warn!(value = %other, "unknown MORPHEUS_SESSION_STORE value, ignoring");
                None
            }
        }
    }
}

/// Backend-agnostic storage for the one [`StoredSession`] a data directory holds.
pub(crate) trait SessionStore: Send + Sync {
    fn load(&self) -> Result<Option<StoredSession>>;
    fn save(&self, stored: &StoredSession) -> Result<()>;
    fn clear(&self) -> Result<()>;
}

/// Pick a backend for `data_dir`.
///
/// `forced` (an explicit `MORPHEUS_SESSION_STORE` env var or a
/// `SessionStoreKind` passed to `Core::with_session_store`) always wins. With
/// no explicit choice, the keyring is probed and used if available, else the
/// file backend is used and a warning is logged.
pub(crate) fn resolve(data_dir: &Path, forced: Option<SessionStoreKind>) -> Box<dyn SessionStore> {
    match forced.or_else(SessionStoreKind::from_env) {
        Some(SessionStoreKind::Keyring) => Box::new(KeyringStore::new(data_dir)),
        Some(SessionStoreKind::File) => Box::new(FileStore::new(data_dir)),
        None => match keyring_available() {
            Ok(()) => Box::new(KeyringStore::new(data_dir)),
            Err(err) => {
                warn!(
                    error = %err,
                    "OS keyring unavailable, falling back to encrypted file storage"
                );
                Box::new(FileStore::new(data_dir))
            }
        },
    }
}

/// Round-trip a canary secret to check that a keyring backend actually
/// works (an `Entry` can be constructed even with no Secret Service running;
/// the real failure only shows up on first use).
fn keyring_available() -> Result<()> {
    let entry = Entry::new(SERVICE, PROBE_ACCOUNT).map_err(Error::Keyring)?;
    entry.set_password("probe").map_err(Error::Keyring)?;
    let _ = entry.delete_credential();
    Ok(())
}

/// Generate a random passphrase for the SDK's sqlite store.
pub(crate) fn generate_passphrase() -> String {
    let mut bytes = [0u8; KEY_LEN];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::STANDARD_NO_PAD.encode(bytes)
}

/// Migrate a legacy plaintext `session.json` into `store`, if there is one
/// and nothing has been migrated yet, then delete it.
///
/// The old sqlite store under `STORE_DIR` was created without a passphrase,
/// so it is wiped as part of migration: the client rebuilds it encrypted on
/// the next connection, at the cost of one resync (no messages are lost, the
/// homeserver stays the source of truth).
pub(crate) fn migrate_legacy_session(data_dir: &Path, store: &dyn SessionStore) -> Result<()> {
    let legacy_path = data_dir.join(LEGACY_SESSION_FILE);
    if !legacy_path.exists() {
        return Ok(());
    }
    if store.load()?.is_some() {
        let _ = std::fs::remove_file(&legacy_path);
        return Ok(());
    }

    let raw = std::fs::read(&legacy_path)?;
    let legacy: LegacyStoredSession = serde_json::from_slice(&raw)?;
    let stored = StoredSession {
        homeserver: legacy.homeserver,
        session: legacy.session,
        store_passphrase: generate_passphrase(),
    };
    store.save(&stored)?;
    std::fs::remove_file(&legacy_path)?;
    let _ = std::fs::remove_dir_all(data_dir.join(STORE_DIR));

    info!("migrated plaintext session.json into secure storage");
    Ok(())
}

/// Stable identifier for a data directory, so several profiles running side
/// by side (`MORPHEUS_DATA_DIR`) each get their own keyring entry.
fn account_id(data_dir: &Path) -> String {
    data_dir
        .canonicalize()
        .unwrap_or_else(|_| data_dir.to_path_buf())
        .display()
        .to_string()
}

struct KeyringStore {
    account: String,
}

impl KeyringStore {
    fn new(data_dir: &Path) -> Self {
        Self {
            account: account_id(data_dir),
        }
    }

    fn entry(&self) -> Result<Entry> {
        Entry::new(SERVICE, &self.account).map_err(Error::Keyring)
    }
}

impl SessionStore for KeyringStore {
    fn load(&self) -> Result<Option<StoredSession>> {
        let entry = self.entry()?;
        match entry.get_password() {
            Ok(json) => Ok(Some(serde_json::from_str(&json)?)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(Error::Keyring(err)),
        }
    }

    fn save(&self, stored: &StoredSession) -> Result<()> {
        let entry = self.entry()?;
        entry
            .set_password(&serde_json::to_string(stored)?)
            .map_err(Error::Keyring)
    }

    fn clear(&self) -> Result<()> {
        let entry = self.entry()?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(Error::Keyring(err)),
        }
    }
}

/// Encrypted-at-rest file backend: `ChaCha20Poly1305` with a random key kept
/// in a sibling, owner-only-readable file.
///
/// This protects the session file from casual inspection (e.g. being swept
/// up in a backup or a `cat` of the data dir) but, since the key lives next
/// to the data it encrypts, it is not a substitute for the OS keyring or for
/// full-disk encryption. It exists for environments with no keyring daemon:
/// tests, CI, containers.
struct FileStore {
    key_path: PathBuf,
    data_path: PathBuf,
}

impl FileStore {
    fn new(data_dir: &Path) -> Self {
        Self {
            key_path: data_dir.join(KEY_FILE),
            data_path: data_dir.join(ENC_FILE),
        }
    }

    fn cipher(&self) -> Result<ChaCha20Poly1305> {
        let key_bytes = match std::fs::read(&self.key_path) {
            Ok(bytes) if bytes.len() == KEY_LEN => bytes,
            _ => {
                let mut bytes = vec![0u8; KEY_LEN];
                rand::rng().fill_bytes(&mut bytes);
                write_private_file(&self.key_path, &bytes)?;
                bytes
            }
        };
        let key = Key::try_from(key_bytes.as_slice()).map_err(|_| Error::SessionCrypto)?;
        Ok(ChaCha20Poly1305::new(&key))
    }
}

impl SessionStore for FileStore {
    fn load(&self) -> Result<Option<StoredSession>> {
        let Ok(raw) = std::fs::read(&self.data_path) else {
            return Ok(None);
        };
        if raw.len() < NONCE_LEN {
            return Err(Error::SessionCrypto);
        }
        let (nonce_bytes, ciphertext) = raw.split_at(NONCE_LEN);
        let nonce = Nonce::try_from(nonce_bytes).map_err(|_| Error::SessionCrypto)?;
        let cipher = self.cipher()?;
        let plaintext = cipher
            .decrypt(&nonce, ciphertext)
            .map_err(|_| Error::SessionCrypto)?;
        Ok(Some(serde_json::from_slice(&plaintext)?))
    }

    fn save(&self, stored: &StoredSession) -> Result<()> {
        let cipher = self.cipher()?;
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from(nonce_bytes);
        let plaintext = serde_json::to_vec(stored)?;
        let ciphertext = cipher
            .encrypt(&nonce, plaintext.as_slice())
            .map_err(|_| Error::SessionCrypto)?;
        let mut out = nonce_bytes.to_vec();
        out.extend(ciphertext);
        write_private_file(&self.data_path, &out)
    }

    fn clear(&self) -> Result<()> {
        let _ = std::fs::remove_file(&self.data_path);
        let _ = std::fs::remove_file(&self.key_path);
        Ok(())
    }
}

/// Write `bytes` to `path`, restricted to owner read/write on Unix.
fn write_private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn sample() -> StoredSession {
        use matrix_sdk::{
            SessionMeta, SessionTokens,
            ruma::{owned_device_id, owned_user_id},
        };

        StoredSession {
            homeserver: "https://example.org".into(),
            session: MatrixSession {
                meta: SessionMeta {
                    user_id: owned_user_id!("@alice:example.org"),
                    device_id: owned_device_id!("DEVICEID"),
                },
                tokens: SessionTokens {
                    access_token: "access-token-value".into(),
                    refresh_token: Some("refresh-token-value".into()),
                },
            },
            store_passphrase: generate_passphrase(),
        }
    }

    #[test]
    fn file_store_round_trips() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = FileStore::new(dir.path());

        assert_eq!(store.load().expect("load empty"), None);

        let stored = sample();
        store.save(&stored).expect("save");
        assert_eq!(store.load().expect("load"), Some(stored));
    }

    #[test]
    fn file_store_clear_removes_data_and_key() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = FileStore::new(dir.path());
        store.save(&sample()).expect("save");

        store.clear().expect("clear");

        assert_eq!(store.load().expect("load after clear"), None);
        assert!(!dir.path().join(ENC_FILE).exists());
        assert!(!dir.path().join(KEY_FILE).exists());
    }

    #[test]
    fn file_store_rejects_tampered_ciphertext() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = FileStore::new(dir.path());
        store.save(&sample()).expect("save");

        let mut raw = std::fs::read(dir.path().join(ENC_FILE)).expect("read ciphertext");
        let last = raw.len() - 1;
        raw[last] ^= 0xFF;
        std::fs::write(dir.path().join(ENC_FILE), raw).expect("write tampered");

        assert!(matches!(store.load(), Err(Error::SessionCrypto)));
    }

    #[cfg(unix)]
    #[test]
    fn file_store_key_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("temp dir");
        let store = FileStore::new(dir.path());
        store.save(&sample()).expect("save");

        let mode = std::fs::metadata(dir.path().join(KEY_FILE))
            .expect("key metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn migrate_legacy_session_moves_plaintext_into_store() {
        let dir = tempfile::tempdir().expect("temp dir");
        let legacy = serde_json::json!({
            "homeserver": "https://example.org",
            "session": {
                "user_id": "@alice:example.org",
                "device_id": "DEVICEID",
                "access_token": "access-token-value",
                "refresh_token": "refresh-token-value",
            },
        });
        std::fs::write(
            dir.path().join(LEGACY_SESSION_FILE),
            serde_json::to_vec(&legacy).expect("serialise legacy session"),
        )
        .expect("write legacy session.json");
        std::fs::create_dir_all(dir.path().join(STORE_DIR)).expect("fake old store dir");
        std::fs::write(
            dir.path().join(STORE_DIR).join("state.sqlite3"),
            b"old-unencrypted",
        )
        .expect("fake old store file");

        let store = FileStore::new(dir.path());
        migrate_legacy_session(dir.path(), &store).expect("migrate");

        assert!(!dir.path().join(LEGACY_SESSION_FILE).exists());
        assert!(!dir.path().join(STORE_DIR).exists());
        let migrated = store.load().expect("load migrated").expect("some session");
        assert_eq!(migrated.homeserver, "https://example.org");
        assert_eq!(migrated.session.meta.user_id.as_str(), "@alice:example.org");
        assert!(!migrated.store_passphrase.is_empty());
    }

    #[test]
    fn migrate_legacy_session_is_a_noop_without_a_legacy_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = FileStore::new(dir.path());
        migrate_legacy_session(dir.path(), &store).expect("migrate");
        assert_eq!(store.load().expect("load"), None);
    }

    #[test]
    fn migrate_legacy_session_does_not_overwrite_an_existing_migrated_session() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = FileStore::new(dir.path());
        let already_migrated = sample();
        store.save(&already_migrated).expect("save");
        std::fs::write(dir.path().join(LEGACY_SESSION_FILE), b"stale leftover")
            .expect("write stale legacy file");

        migrate_legacy_session(dir.path(), &store).expect("migrate");

        assert!(!dir.path().join(LEGACY_SESSION_FILE).exists());
        assert_eq!(store.load().expect("load"), Some(already_migrated));
    }
}
