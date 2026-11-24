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
//! (with a warning) if it does not answer. On macOS a dev build (a debug
//! build, or a binary outside a `.app`) uses the file, so `cargo run` never
//! prompts.
//!
//! Every account's session lives in ONE keyring item ([`Vault`]), read once
//! per process and kept in memory, written only on change. A keychain that
//! refuses (denied prompt, locked) is given up on for the rest of the process
//! and, on macOS, for later launches: it is never asked again in a loop.
//!
//! All operations here are blocking (the `keyring` crate and `std::fs` are
//! both synchronous); callers run them with `tokio::task::spawn_blocking`.

use std::path::{Path, PathBuf};

use base64::Engine;
use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce, aead::Aead};
#[cfg(not(target_arch = "wasm32"))]
use keyring::Entry;
use matrix_sdk::authentication::matrix::MatrixSession;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tracing::info;
#[cfg(not(target_arch = "wasm32"))]
use tracing::warn;

use crate::{Error, Result};

/// Service name under which keyring entries are grouped.
#[cfg(not(target_arch = "wasm32"))]
const SERVICE: &str = "dev.morpheus.Morpheus";
/// The one keyring item holding every account's session.
#[cfg(not(target_arch = "wasm32"))]
const VAULT_ACCOUNT: &str = "vault";
/// Written in the data dir's root when the keychain refused (macOS).
#[cfg(not(target_arch = "wasm32"))]
const DENIED_FILE: &str = "keychain-denied";
/// Written in an account folder once its old per-account keychain item was
/// looked for and was not there, so later launches do not ask again.
#[cfg(not(target_arch = "wasm32"))]
const LEGACY_CHECKED_FILE: &str = "keychain-old-entry-checked";

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

#[cfg(not(target_arch = "wasm32"))]
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
pub(crate) trait SessionStore: crate::rt::MaybeSend + crate::rt::MaybeSync {
    fn load(&self) -> Result<Option<StoredSession>>;
    fn save(&self, stored: &StoredSession) -> Result<()>;
    fn clear(&self) -> Result<()>;
}

/// Pick a backend for `data_dir`.
///
/// `forced` (an explicit `MORPHEUS_SESSION_STORE` env var or a
/// `SessionStoreKind` passed to `Core::with_session_store`) always wins. With
/// no explicit choice, the keyring is used, and the file backend takes over
/// (with a warning) if the keyring does not answer. Nothing is written to
/// the keyring to find out.
pub(crate) fn resolve(data_dir: &Path, forced: Option<SessionStoreKind>) -> Box<dyn SessionStore> {
    // A browser has no keyring: the session is kept, encrypted, in the
    // page's storage.
    #[cfg(target_arch = "wasm32")]
    {
        let _ = forced;
        Box::new(FileStore::new(data_dir))
    }
    #[cfg(not(target_arch = "wasm32"))]
    match forced.or_else(SessionStoreKind::from_env) {
        Some(SessionStoreKind::Keyring) => Box::new(KeyringStore::new(data_dir, true)),
        Some(SessionStoreKind::File) => Box::new(FileStore::new(data_dir)),
        None if dev_build_on_mac() => Box::new(FileStore::new(data_dir)),
        None => Box::new(KeyringStore::new(data_dir, false)),
    }
}

/// macOS asks for the login password whenever a program whose code
/// signature the item does not know reads it, which is every `cargo run`
/// build: those use the file unless the keychain is asked for.
#[cfg(not(target_arch = "wasm32"))]
fn dev_build_on_mac() -> bool {
    cfg!(target_os = "macos")
        && (cfg!(debug_assertions)
            || !std::env::current_exe()
                .is_ok_and(|exe| exe.to_string_lossy().contains(".app/Contents/MacOS/")))
}

/// What the OS keyring can do for [`Vault`]: items by account name. Errors
/// are messages: a missing item is `Ok(None)`, anything else means the
/// keyring cannot be used.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) trait Keychain: Send + Sync {
    fn get(&self, account: &str) -> std::result::Result<Option<String>, String>;
    fn set(&self, account: &str, value: &str) -> std::result::Result<(), String>;
    fn delete(&self, account: &str) -> std::result::Result<(), String>;
}

#[cfg(not(target_arch = "wasm32"))]
struct OsKeychain;

#[cfg(not(target_arch = "wasm32"))]
impl OsKeychain {
    fn entry(account: &str) -> std::result::Result<Entry, String> {
        // Debug builds say who asks (#159): any keychain access is a bug there.
        #[cfg(debug_assertions)]
        tracing::warn!(account, "keychain access from:\n{}", std::backtrace::Backtrace::force_capture());
        Entry::new(SERVICE, account).map_err(|err| err.to_string())
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Keychain for OsKeychain {
    fn get(&self, account: &str) -> std::result::Result<Option<String>, String> {
        match Self::entry(account)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(err.to_string()),
        }
    }

    fn set(&self, account: &str, value: &str) -> std::result::Result<(), String> {
        Self::entry(account)?
            .set_password(value)
            .map_err(|err| err.to_string())
    }

    fn delete(&self, account: &str) -> std::result::Result<(), String> {
        match Self::entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err.to_string()),
        }
    }
}

/// The shape of the one keyring item.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default, Serialize, Deserialize)]
struct VaultData {
    #[serde(default)]
    accounts: std::collections::BTreeMap<String, StoredSession>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct VaultState {
    data: Option<VaultData>,
    /// Accounts whose old per-account item was looked for (once each).
    legacy_checked: std::collections::HashSet<String>,
    /// Why the keyring is given up on, for the rest of the process.
    failed: Option<String>,
}

/// How many times the keyring was touched, for the tests and the
/// self-test (each is also logged at debug level).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub(crate) struct Counts {
    pub reads: std::sync::atomic::AtomicUsize,
    pub writes: std::sync::atomic::AtomicUsize,
    pub deletes: std::sync::atomic::AtomicUsize,
}

#[cfg(not(target_arch = "wasm32"))]
impl Counts {
    pub(crate) fn get(&self) -> (usize, usize, usize) {
        use std::sync::atomic::Ordering::Relaxed;
        (
            self.reads.load(Relaxed),
            self.writes.load(Relaxed),
            self.deletes.load(Relaxed),
        )
    }
}

/// Every account's session in ONE keyring item, so the OS asks for access at
/// most once per launch (not once per account and per look). Read at first
/// use and kept in memory; written only when something changes. The
/// per-account items older builds wrote are moved in on first sight and
/// deleted. If the keyring fails in any way (denied, cancelled, locked,
/// absent) it is not touched again by this process.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct Vault {
    keychain: Box<dyn Keychain>,
    /// Where to remember a refusal for later launches; `None` to forget it
    /// when the process ends.
    marker: Option<PathBuf>,
    state: std::sync::Mutex<VaultState>,
    pub(crate) counts: Counts,
}

#[cfg(not(target_arch = "wasm32"))]
type VaultResult<T> = std::result::Result<T, String>;

#[cfg(not(target_arch = "wasm32"))]
impl Vault {
    pub(crate) fn new(keychain: Box<dyn Keychain>, marker: Option<PathBuf>) -> Self {
        Self {
            keychain,
            marker,
            state: std::sync::Mutex::new(VaultState::default()),
            counts: Counts::default(),
        }
    }

    fn count(what: &str, counter: &std::sync::atomic::AtomicUsize) {
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        tracing::debug!("keychain access: {what}");
    }

    /// Give up on the keyring; remember it if a marker is set.
    fn fail(&self, state: &mut VaultState, why: String) -> String {
        warn!(error = %why, "OS keyring unusable, using encrypted file storage from now on");
        if let Some(marker) = &self.marker {
            let _ = std::fs::write(marker, &why);
        }
        set_notice(&why);
        state.failed = Some(why.clone());
        why
    }

    /// The vault's contents, read once; `Err` when the keyring must not be
    /// used (now or earlier).
    fn ready(&self, state: &mut VaultState, ignore_marker: bool) -> VaultResult<()> {
        if let Some(why) = &state.failed {
            return Err(why.clone());
        }
        if !ignore_marker
            && let Some(marker) = &self.marker
            && marker.exists()
        {
            let why = format!(
                "refused earlier: {}",
                std::fs::read_to_string(marker).unwrap_or_default()
            );
            state.failed = Some(why.clone());
            return Err(why);
        }
        if state.data.is_none() {
            Self::count("read vault", &self.counts.reads);
            let loaded = match self.keychain.get(VAULT_ACCOUNT) {
                Ok(None) => VaultData::default(),
                Ok(Some(json)) => match serde_json::from_str(&json) {
                    Ok(data) => data,
                    Err(err) => return Err(self.fail(state, format!("unreadable vault: {err}"))),
                },
                Err(why) => return Err(self.fail(state, why)),
            };
            state.data = Some(loaded);
        }
        Ok(())
    }

    fn write(&self, state: &mut VaultState) -> VaultResult<()> {
        let Some(data) = &state.data else {
            return Ok(());
        };
        let result = if data.accounts.is_empty() {
            Self::count("delete vault", &self.counts.deletes);
            self.keychain.delete(VAULT_ACCOUNT)
        } else {
            Self::count("write vault", &self.counts.writes);
            match serde_json::to_string(data) {
                Ok(json) => self.keychain.set(VAULT_ACCOUNT, &json),
                Err(err) => Err(err.to_string()),
            }
        };
        result.map_err(|why| self.fail(state, why))
    }

    fn accounts(state: &mut VaultState) -> &mut std::collections::BTreeMap<String, StoredSession> {
        &mut state.data.get_or_insert_with(VaultData::default).accounts
    }

    fn load_locked(
        &self,
        state: &mut VaultState,
        account: &str,
        ignore_marker: bool,
    ) -> VaultResult<Option<StoredSession>> {
        self.ready(state, ignore_marker)?;
        if let Some(found) = Self::accounts(state).get(account) {
            return Ok(Some(found.clone()));
        }
        if !state.legacy_checked.insert(account.to_owned()) {
            return Ok(None);
        }
        // Looked for in an earlier launch and not there: no keychain call.
        let checked_file = Path::new(account).join(LEGACY_CHECKED_FILE);
        if checked_file.exists() {
            return Ok(None);
        }
        Self::count("read old entry", &self.counts.reads);
        let old = match self.keychain.get(account) {
            Ok(old) => old,
            Err(why) => return Err(self.fail(state, why)),
        };
        let absent = old.is_none();
        let Some(stored) = old.and_then(|json| serde_json::from_str::<StoredSession>(&json).ok())
        else {
            // Only a clean "nothing there" is remembered (an error returned
            // above, and an unreadable item is looked at again).
            if absent {
                let _ = std::fs::write(&checked_file, "");
            }
            return Ok(None);
        };
        Self::accounts(state).insert(account.to_owned(), stored.clone());
        self.write(state)?;
        Self::count("delete old entry", &self.counts.deletes);
        match self.keychain.delete(account) {
            Ok(()) => {
                let _ = std::fs::write(&checked_file, "");
            }
            Err(err) => warn!(error = %err, "could not delete an old keyring entry"),
        }
        info!("moved an account's session into the single keyring item");
        Ok(Some(stored))
    }

    /// The session of `account`, moving in its old per-account item first
    /// if there is one.
    pub(crate) fn load(
        &self,
        account: &str,
        ignore_marker: bool,
    ) -> VaultResult<Option<StoredSession>> {
        let mut guard = self.state.lock().map_err(|_| "poisoned".to_owned())?;
        self.load_locked(&mut guard, account, ignore_marker)
    }

    pub(crate) fn save(
        &self,
        account: &str,
        stored: &StoredSession,
        ignore_marker: bool,
    ) -> VaultResult<()> {
        let mut guard = self.state.lock().map_err(|_| "poisoned".to_owned())?;
        let state = &mut *guard;
        self.ready(state, ignore_marker)?;
        if Self::accounts(state).get(account) == Some(stored) {
            return Ok(());
        }
        Self::accounts(state).insert(account.to_owned(), stored.clone());
        state.legacy_checked.insert(account.to_owned());
        self.write(state)
    }

    pub(crate) fn clear(&self, account: &str, ignore_marker: bool) -> VaultResult<()> {
        let mut guard = self.state.lock().map_err(|_| "poisoned".to_owned())?;
        let state = &mut *guard;
        // An old item is moved in first, so that removing it here is real.
        self.load_locked(state, account, ignore_marker)?;
        if Self::accounts(state).remove(account).is_none() {
            return Ok(());
        }
        self.write(state)
    }
}

/// The note for the person, once: set when the keyring is given up on.
#[cfg(not(target_arch = "wasm32"))]
static NOTICE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[cfg(not(target_arch = "wasm32"))]
fn set_notice(why: &str) {
    // Only where a refusal is a person's choice: on Linux a missing Secret
    // Service is the usual case and not worth a note.
    if cfg!(any(target_os = "macos", target_os = "windows"))
        && let Ok(mut notice) = NOTICE.lock()
        && notice.is_none()
    {
        *notice = Some(format!(
            "Morpheus could not use the system keychain ({why}), so it keeps your sign-in in an encrypted file instead. You may need to sign in again."
        ));
    }
}

/// The one-time note that the keychain was given up on, if it was.
#[cfg(not(target_arch = "wasm32"))]
pub fn take_keychain_notice() -> Option<String> {
    NOTICE.lock().ok()?.take()
}

/// This process's vault in the real keyring. A refusal is remembered in
/// `root` on macOS.
#[cfg(not(target_arch = "wasm32"))]
fn global_vault(root: &Path) -> &'static Vault {
    static VAULT: std::sync::OnceLock<Vault> = std::sync::OnceLock::new();
    VAULT.get_or_init(|| {
        let marker = cfg!(target_os = "macos").then(|| root.join(DENIED_FILE));
        Vault::new(Box::new(OsKeychain), marker)
    })
}

/// The data dir's root for an account folder (`<root>/accounts/<n>`).
#[cfg(not(target_arch = "wasm32"))]
fn root_of(dir: &Path) -> &Path {
    match dir.parent() {
        Some(parent) if parent.file_name().is_some_and(|name| name == "accounts") => {
            parent.parent().unwrap_or(dir)
        }
        _ => dir,
    }
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
    if !crate::fs::exists(&legacy_path) {
        return Ok(());
    }
    if store.load()?.is_some() {
        let _ = crate::fs::remove_file(&legacy_path);
        return Ok(());
    }

    let raw = crate::fs::read(&legacy_path)?;
    let legacy: LegacyStoredSession = serde_json::from_slice(&raw)?;
    let stored = StoredSession {
        homeserver: legacy.homeserver,
        session: legacy.session,
        store_passphrase: generate_passphrase(),
    };
    store.save(&stored)?;
    crate::fs::remove_file(&legacy_path)?;
    let _ = crate::fs::remove_dir_all(data_dir.join(STORE_DIR));

    info!("migrated plaintext session.json into secure storage");
    Ok(())
}

/// Stable identifier for a data directory, so several profiles running side
/// by side (`MORPHEUS_DATA_DIR`) each get their own keyring entry.
#[cfg(not(target_arch = "wasm32"))]
fn account_id(data_dir: &Path) -> String {
    data_dir
        .canonicalize()
        .unwrap_or_else(|_| data_dir.to_path_buf())
        .display()
        .to_string()
}

/// The keyring through [`Vault`], with the file taking over when the keyring
/// refuses (unless the keyring was asked for by name: then the error shows).
#[cfg(not(target_arch = "wasm32"))]
struct KeyringStore {
    account: String,
    vault: &'static Vault,
    /// Asked for by name: no fallback, and an earlier refusal is not honoured.
    forced: bool,
    file: FileStore,
}

#[cfg(not(target_arch = "wasm32"))]
impl KeyringStore {
    fn new(data_dir: &Path, forced: bool) -> Self {
        Self {
            account: account_id(data_dir),
            vault: global_vault(root_of(data_dir)),
            forced,
            file: FileStore::new(data_dir),
        }
    }

    /// Run `op` on the keyring, or on the file when it refuses.
    fn with<T>(
        &self,
        op: impl FnOnce(&Vault) -> VaultResult<T>,
        on_file: impl FnOnce(&FileStore) -> Result<T>,
    ) -> Result<T> {
        match op(self.vault) {
            Ok(value) => Ok(value),
            Err(why) if self.forced => Err(Error::Other(format!("OS keyring: {why}"))),
            Err(_) => on_file(&self.file),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl SessionStore for KeyringStore {
    fn load(&self) -> Result<Option<StoredSession>> {
        self.with(
            |vault| vault.load(&self.account, self.forced),
            |file| file.load(),
        )
    }

    fn save(&self, stored: &StoredSession) -> Result<()> {
        self.with(
            |vault| vault.save(&self.account, stored, self.forced),
            |file| file.save(stored),
        )
    }

    fn clear(&self) -> Result<()> {
        self.with(
            |vault| vault.clear(&self.account, self.forced),
            |file| file.clear(),
        )
    }
}

/// Headless check of the keyring's access count (`MORPHEUS_KEYCHAIN_SELF_TEST`,
/// run by apple.yml against a CI keychain): with `seed`, three accounts are
/// saved; then a launch is acted out (each account loaded several times as
/// the app does). The line says how often the keyring was touched.
#[cfg(not(target_arch = "wasm32"))]
pub fn keychain_self_test(root: &Path, seed: bool) -> Result<String> {
    let dirs = [
        root.to_path_buf(),
        root.join("accounts").join("1"),
        root.join("accounts").join("2"),
    ];
    let stores: Vec<KeyringStore> = dirs
        .iter()
        .map(|dir| {
            crate::fs::create_dir_all(dir)?;
            Ok(KeyringStore::new(dir, true))
        })
        .collect::<Result<_>>()?;
    if seed {
        for (n, store) in stores.iter().enumerate() {
            store.save(&self_test_session(n))?;
        }
    }
    for _ in 0..4 {
        for (n, store) in stores.iter().enumerate() {
            let found = store
                .load()?
                .is_some_and(|s| s.homeserver == self_test_session(n).homeserver);
            if !found {
                return Err(Error::Other(format!("account {n} not found in the keychain")));
            }
        }
    }
    let (reads, writes, deletes) = stores[0].vault.counts.get();
    Ok(format!(
        "keychain self-test: ok: seeded={seed} reads={reads} writes={writes} deletes={deletes} accounts=3"
    ))
}

#[cfg(not(target_arch = "wasm32"))]
fn self_test_session(n: usize) -> StoredSession {
    use matrix_sdk::{
        SessionMeta, SessionTokens,
        ruma::{OwnedDeviceId, owned_user_id},
    };
    StoredSession {
        homeserver: format!("https://self-test-{n}.example.org"),
        session: MatrixSession {
            meta: SessionMeta {
                user_id: owned_user_id!("@selftest:example.org"),
                device_id: OwnedDeviceId::from(format!("SELFTEST{n}")),
            },
            tokens: SessionTokens {
                access_token: "self-test-token".into(),
                refresh_token: None,
            },
        },
        store_passphrase: "self-test-passphrase".into(),
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
        let key_bytes = match crate::fs::read(&self.key_path) {
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
        let Ok(raw) = crate::fs::read(&self.data_path) else {
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
        let _ = crate::fs::remove_file(&self.data_path);
        let _ = crate::fs::remove_file(&self.key_path);
        Ok(())
    }
}

/// Write `bytes` to `path`, restricted to owner read/write on Unix.
fn write_private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    crate::fs::write(path, bytes)?;
    #[cfg(all(unix, not(target_arch = "wasm32")))]
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

    // --- The keyring item: how often the OS is asked ---

    use std::collections::HashMap;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering::Relaxed},
    };

    /// A keyring that counts what it is asked, and can refuse everything.
    #[derive(Default)]
    struct FakeKeychain {
        items: Mutex<HashMap<String, String>>,
        calls: AtomicUsize,
        deny: std::sync::atomic::AtomicBool,
    }

    impl FakeKeychain {
        fn touch(&self) -> std::result::Result<(), String> {
            self.calls.fetch_add(1, Relaxed);
            if self.deny.load(Relaxed) {
                Err("User canceled the operation (errSecUserCanceled)".into())
            } else {
                Ok(())
            }
        }
        fn put(&self, account: &str, value: String) {
            self.items.lock().expect("items").insert(account.into(), value);
        }
        fn has(&self, account: &str) -> bool {
            self.items.lock().expect("items").contains_key(account)
        }
    }

    impl Keychain for Arc<FakeKeychain> {
        fn get(&self, account: &str) -> std::result::Result<Option<String>, String> {
            self.touch()?;
            Ok(self.items.lock().expect("items").get(account).cloned())
        }
        fn set(&self, account: &str, value: &str) -> std::result::Result<(), String> {
            self.touch()?;
            self.put(account, value.into());
            Ok(())
        }
        fn delete(&self, account: &str) -> std::result::Result<(), String> {
            self.touch()?;
            self.items.lock().expect("items").remove(account);
            Ok(())
        }
    }

    fn session_for(n: usize) -> StoredSession {
        let mut stored = sample();
        stored.homeserver = format!("https://example{n}.org");
        stored.store_passphrase = format!("passphrase-{n}");
        stored
    }

    const ACCOUNTS: [&str; 3] = ["/data", "/data/accounts/1", "/data/accounts/2"];

    #[test]
    fn a_launch_with_three_accounts_reads_the_keychain_once_and_writes_nothing() {
        let fake = Arc::new(FakeKeychain::default());
        let first = Vault::new(Box::new(fake.clone()), None);
        for (n, account) in ACCOUNTS.iter().enumerate() {
            first.save(account, &session_for(n), true).expect("save");
        }
        // A new launch, nothing changed: the accounts are looked at over and over.
        let before = fake.calls.load(Relaxed);
        let launch = Vault::new(Box::new(fake.clone()), None);
        for _ in 0..5 {
            for (n, account) in ACCOUNTS.iter().enumerate() {
                assert_eq!(launch.load(account, true).expect("load"), Some(session_for(n)));
                // Saving what is already there is not a write.
                launch.save(account, &session_for(n), true).expect("save");
            }
        }
        assert_eq!(fake.calls.load(Relaxed) - before, 1, "one read, no writes");
        assert_eq!(launch.counts.get(), (1, 0, 0));
    }

    #[test]
    fn a_refused_keychain_is_not_asked_again_by_this_or_the_next_launch() {
        let dir = tempfile::tempdir().expect("dir");
        let marker = dir.path().join(DENIED_FILE);
        let fake = Arc::new(FakeKeychain::default());
        fake.deny.store(true, Relaxed);

        let vault = Vault::new(Box::new(fake.clone()), Some(marker.clone()));
        assert!(vault.load("/data", false).is_err());
        for _ in 0..5 {
            assert!(vault.load("/data", false).is_err());
            assert!(vault.save("/data", &sample(), false).is_err());
            assert!(vault.clear("/data", false).is_err());
        }
        assert_eq!(fake.calls.load(Relaxed), 1, "one refused call, then none");
        assert!(marker.exists(), "the refusal is remembered on disk");

        // The next launch, even with a keychain that would now answer.
        fake.deny.store(false, Relaxed);
        fake.calls.store(0, Relaxed);
        let next = Vault::new(Box::new(fake.clone()), Some(marker));
        assert!(next.load("/data", false).is_err());
        assert_eq!(fake.calls.load(Relaxed), 0);
    }

    #[test]
    fn a_refusal_falls_back_to_the_file_backend() {
        let dir = tempfile::tempdir().expect("dir");
        let fake = Arc::new(FakeKeychain::default());
        fake.deny.store(true, Relaxed);
        let vault: &'static Vault = Box::leak(Box::new(Vault::new(
            Box::new(fake.clone()),
            Some(dir.path().join(DENIED_FILE)),
        )));
        let store = KeyringStore {
            account: account_id(dir.path()),
            vault,
            forced: false,
            file: FileStore::new(dir.path()),
        };
        assert_eq!(store.load().expect("load"), None);
        store.save(&sample()).expect("save to the file");
        assert_eq!(store.load().expect("load").map(|s| s.homeserver), Some("https://example.org".into()));
        assert!(dir.path().join(ENC_FILE).exists());
        assert_eq!(fake.calls.load(Relaxed), 1);
    }

    #[test]
    fn old_per_account_entries_are_moved_into_the_one_item_and_deleted() {
        let fake = Arc::new(FakeKeychain::default());
        for (n, account) in ACCOUNTS.iter().enumerate() {
            fake.put(account, serde_json::to_string(&session_for(n)).expect("json"));
        }
        let vault = Vault::new(Box::new(fake.clone()), None);
        for (n, account) in ACCOUNTS.iter().enumerate() {
            assert_eq!(vault.load(account, true).expect("load"), Some(session_for(n)));
        }
        for account in ACCOUNTS {
            assert!(!fake.has(account), "{account}: old entry deleted");
        }
        assert!(fake.has(VAULT_ACCOUNT));

        // After that, a launch is one read.
        let launch = Vault::new(Box::new(fake.clone()), None);
        for (n, account) in ACCOUNTS.iter().enumerate() {
            assert_eq!(launch.load(account, true).expect("load"), Some(session_for(n)));
        }
        assert_eq!(launch.counts.get(), (1, 0, 0));
    }

    /// Three real folders (the account string is the folder's path).
    fn folders() -> (tempfile::TempDir, Vec<String>) {
        let dir = tempfile::tempdir().expect("dir");
        let paths = (0..3)
            .map(|n| {
                let path = dir.path().join(format!("slot{n}"));
                std::fs::create_dir_all(&path).expect("slot");
                path.display().to_string()
            })
            .collect();
        (dir, paths)
    }

    #[test]
    fn a_folder_with_no_old_item_is_looked_for_once_ever() {
        let (_dir, folders) = folders();
        let fake = Arc::new(FakeKeychain::default());
        // An unrelated session keeps the vault item alive.
        let setup = Vault::new(Box::new(fake.clone()), None);
        setup.save("/elsewhere", &session_for(9), true).expect("save");

        let first = Vault::new(Box::new(fake.clone()), None);
        for folder in &folders {
            assert_eq!(first.load(folder, true).expect("load"), None);
        }
        assert_eq!(first.counts.get(), (4, 0, 0), "first launch: the vault and each old item");

        let second = Vault::new(Box::new(fake.clone()), None);
        for _ in 0..3 {
            for folder in &folders {
                assert_eq!(second.load(folder, true).expect("load"), None);
            }
        }
        assert_eq!(second.counts.get(), (1, 0, 0), "second launch: the vault only");
    }

    #[test]
    fn a_refused_look_for_the_old_item_is_not_remembered_as_nothing_there() {
        let (_dir, folders) = folders();
        let fake = Arc::new(FakeKeychain::default());
        fake.put(&folders[0], serde_json::to_string(&session_for(0)).expect("json"));
        let setup = Vault::new(Box::new(fake.clone()), None);
        setup.save("/elsewhere", &session_for(9), true).expect("save");

        // The vault reads fine, then the keychain refuses the old item.
        let vault = Vault::new(Box::new(fake.clone()), None);
        assert_eq!(vault.load("/elsewhere", true).expect("load"), Some(session_for(9)));
        fake.deny.store(true, Relaxed);
        assert!(vault.load(&folders[0], true).is_err());
        fake.deny.store(false, Relaxed);

        let next = Vault::new(Box::new(fake.clone()), None);
        assert_eq!(next.load(&folders[0], true).expect("load"), Some(session_for(0)));
        assert!(!fake.has(&folders[0]), "moved in and deleted");
    }

    #[test]
    fn signing_in_to_a_checked_folder_still_works() {
        let (_dir, folders) = folders();
        let fake = Arc::new(FakeKeychain::default());
        let first = Vault::new(Box::new(fake.clone()), None);
        assert_eq!(first.load(&folders[0], true).expect("load"), None);
        first.save(&folders[0], &session_for(1), true).expect("save");
        let next = Vault::new(Box::new(fake.clone()), None);
        assert_eq!(next.load(&folders[0], true).expect("load"), Some(session_for(1)));
        next.clear(&folders[0], true).expect("clear");
        let last = Vault::new(Box::new(fake.clone()), None);
        assert_eq!(last.load(&folders[0], true).expect("load"), None);
    }

    #[test]
    fn clearing_an_account_removes_it_and_the_last_one_removes_the_item() {
        let fake = Arc::new(FakeKeychain::default());
        let vault = Vault::new(Box::new(fake.clone()), None);
        vault.save("/a", &session_for(1), true).expect("save");
        vault.save("/b", &session_for(2), true).expect("save");
        vault.clear("/a", true).expect("clear");
        assert_eq!(vault.load("/a", true).expect("load"), None);
        assert!(vault.load("/b", true).expect("load").is_some());
        vault.clear("/b", true).expect("clear");
        assert!(!fake.has(VAULT_ACCOUNT));
    }
}
