//! Several accounts side by side. Each account has a folder of its own
//! holding its credentials (keyring entries are keyed by folder) and its
//! local store: the data directory itself for the first account (so older
//! installs keep theirs where it is), `accounts/<n>` for any added later.
//! `active_account` names the one to restore at start.

use std::path::{Path, PathBuf};

use matrix_sdk::ruma::OwnedUserId;

use crate::{
    Core, CoreFuture, Result, Session,
    session_store::{self, StoredSession},
};

const ACCOUNTS_DIR: &str = "accounts";
const ACTIVE_FILE: &str = "active_account";

/// An account with a stored session on this device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedAccount {
    /// Which folder it lives in, for [`Core::switch_account`].
    pub key: String,
    pub user_id: OwnedUserId,
    pub homeserver: String,
    /// The one restored at start (and normally the one signed in).
    pub active: bool,
}

/// `""` is the data directory itself; anything else is relative to it.
fn slot_dir(root: &Path, key: &str) -> PathBuf {
    if key.is_empty() {
        root.to_path_buf()
    } else {
        root.join(key)
    }
}

/// Every account folder, the first account's first, then by number.
fn slots(root: &Path) -> Vec<String> {
    let mut numbered: Vec<u32> = std::fs::read_dir(root.join(ACCOUNTS_DIR))
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().is_dir())
                .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    numbered.sort_unstable();
    std::iter::once(String::new())
        .chain(numbered.into_iter().map(|n| format!("{ACCOUNTS_DIR}/{n}")))
        .collect()
}

fn read_active(root: &Path) -> String {
    std::fs::read_to_string(root.join(ACTIVE_FILE))
        .map(|text| text.trim().to_owned())
        .unwrap_or_default()
}

pub(crate) fn write_active(root: &Path, key: &str) -> Result<()> {
    std::fs::write(root.join(ACTIVE_FILE), key)?;
    Ok(())
}

impl Core {
    fn stored(&self, key: &str) -> Result<Option<StoredSession>> {
        let dir = slot_dir(self.data_dir(), key);
        session_store::resolve(&dir, self.session_store_kind()).load()
    }

    /// Blocking: the folders with a stored session, as `(key, session)`.
    fn stored_accounts(&self) -> Vec<(String, StoredSession)> {
        slots(self.data_dir())
            .into_iter()
            .filter_map(|key| match self.stored(&key) {
                Ok(Some(stored)) => Some((key, stored)),
                _ => None,
            })
            .collect()
    }

    /// Blocking: the folder to restore at start: the active one if it holds
    /// an account, else the first that does.
    pub(crate) fn active_account_dir(&self) -> PathBuf {
        let active = read_active(self.data_dir());
        let key = if matches!(self.stored(&active), Ok(Some(_))) {
            active
        } else {
            self.stored_accounts()
                .into_iter()
                .next()
                .map(|(key, _)| key)
                .unwrap_or_default()
        };
        slot_dir(self.data_dir(), &key)
    }

    /// Blocking: where a new sign-in goes: the first folder holding no
    /// account (keeping the others), else a new one.
    pub(crate) fn free_account_dir(&self) -> Result<PathBuf> {
        let root = self.data_dir();
        let existing = slots(root);
        if let Some(free) = existing
            .iter()
            .find(|key| matches!(self.stored(key), Ok(None)))
        {
            return Ok(slot_dir(root, free));
        }
        let next = existing
            .iter()
            .filter_map(|key| {
                key.strip_prefix(&format!("{ACCOUNTS_DIR}/"))?
                    .parse::<u32>()
                    .ok()
            })
            .max()
            .map_or(1, |n| n + 1);
        let dir = root.join(ACCOUNTS_DIR).join(next.to_string());
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// The key (`""` or `accounts/<n>`) of an account folder.
    pub(crate) fn account_key(&self, dir: &Path) -> String {
        dir.strip_prefix(self.data_dir())
            .map(|rel| rel.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// The accounts stored on this device.
    pub fn accounts(&self) -> CoreFuture<Vec<SavedAccount>> {
        let core = self.clone();
        self.run(async move {
            tokio::task::spawn_blocking(move || {
                let active = core.account_key(&core.active_account_dir());
                core.stored_accounts()
                    .into_iter()
                    .map(|(key, stored)| SavedAccount {
                        active: key == active,
                        user_id: stored.session.meta.user_id,
                        homeserver: stored.homeserver,
                        key,
                    })
                    .collect()
            })
            .await
            .map_err(Into::into)
        })
    }

    /// Make `key` the active account and restore it. The current session
    /// should be [`Session::suspend`]ed first.
    pub fn switch_account(&self, key: String) -> CoreFuture<Option<Session>> {
        let core = self.clone();
        self.run(async move {
            let root = core.data_dir().to_path_buf();
            tokio::task::spawn_blocking(move || write_active(&root, &key)).await??;
            core.restore_session().await
        })
    }
}
