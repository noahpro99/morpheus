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
    let mut numbered: Vec<u32> = crate::fs::list(root.join(ACCOUNTS_DIR))
        .map(|names| names.iter().filter_map(|name| name.parse().ok()).collect())
        .unwrap_or_default();
    numbered.sort_unstable();
    std::iter::once(String::new())
        .chain(numbered.into_iter().map(|n| format!("{ACCOUNTS_DIR}/{n}")))
        .collect()
}

fn read_active(root: &Path) -> String {
    crate::fs::read_to_string(root.join(ACTIVE_FILE))
        .map(|text| text.trim().to_owned())
        .unwrap_or_default()
}

pub(crate) fn write_active(root: &Path, key: &str) -> Result<()> {
    crate::fs::write(root.join(ACTIVE_FILE), key)?;
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

    /// Blocking: where a new sign-in goes: the first folder holding neither an
    /// account nor a local store (a sign-in starts with a fresh store, so a
    /// folder with one is never reused: if its credentials were only out of
    /// reach for a moment, its data would be lost), else a new one.
    pub(crate) fn free_account_dir(&self) -> Result<PathBuf> {
        let root = self.data_dir();
        let existing = slots(root);
        if let Some(free) = existing.iter().find(|key| {
            matches!(self.stored(key), Ok(None))
                && !crate::fs::exists(slot_dir(root, key).join(session_store::STORE_DIR))
        }) {
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
        crate::fs::create_dir_all(&dir)?;
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
            crate::rt::spawn_blocking(move || {
                core.merge_duplicates();
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
            crate::rt::spawn_blocking(move || write_active(&root, &key)).await??;
            core.restore_session().await
        })
    }
}

/// A homeserver as written by whoever typed it, for comparing: no scheme, no
/// trailing slash, lower case (`matrix.org` is `https://Matrix.org/`).
fn homeserver_key(homeserver: &str) -> String {
    let lower = homeserver.trim().to_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or(&lower);
    rest.trim_end_matches('/').to_owned()
}

/// Which stored accounts to drop so each user on each homeserver is listed once:
/// of those that are the same account, the `keep` one stays, else the active one,
/// else the newest (the highest folder: the latest sign-in).
fn duplicates_to_drop(
    accounts: &[(String, StoredSession)],
    keep: Option<&str>,
    active: &str,
) -> Vec<String> {
    let id = |stored: &StoredSession| {
        (
            stored.session.meta.user_id.clone(),
            homeserver_key(&stored.homeserver),
        )
    };
    let mut drop = Vec::new();
    for (key, stored) in accounts {
        let same: Vec<&String> = accounts
            .iter()
            .filter(|(_, other)| id(other) == id(stored))
            .map(|(k, _)| k)
            .collect();
        if same.len() < 2 {
            continue;
        }
        let winner = keep
            .and_then(|k| same.iter().find(|s| s.as_str() == k))
            .or_else(|| same.iter().find(|s| s.as_str() == active))
            .or_else(|| same.last())
            .copied();
        if winner != Some(key) {
            drop.push(key.clone());
        }
    }
    drop
}

impl Core {
    /// Blocking: forgets an account folder's credentials and local store (the first
    /// account's folder is the data directory itself, so only its contents go).
    fn remove_slot(&self, key: &str) {
        let dir = slot_dir(self.data_dir(), key);
        let _ = session_store::resolve(&dir, self.session_store_kind()).clear();
        let _ = crate::fs::remove_dir_all(dir.join(session_store::STORE_DIR));
        if !key.is_empty() {
            let _ = crate::fs::remove_dir_all(&dir);
        }
    }

    fn drop_accounts(&self, drop: &[String], keep: &str) {
        if drop.is_empty() {
            return;
        }
        let active = read_active(self.data_dir());
        for key in drop {
            self.remove_slot(key);
        }
        if drop.contains(&active) {
            let _ = write_active(self.data_dir(), keep);
        }
    }

    /// Blocking: after `dir` was signed in, no other folder keeps the same account
    /// (user on homeserver): a second sign-in replaces the first, never adds a
    /// second entry (#337).
    pub(crate) fn drop_duplicates_of(&self, dir: &Path) {
        let keep = self.account_key(dir);
        let accounts = self.stored_accounts();
        let drop = duplicates_to_drop(&accounts, Some(&keep), &read_active(self.data_dir()));
        self.drop_accounts(&drop, &keep);
    }

    /// Blocking: merges accounts stored twice before this fix, keeping the active
    /// one (the one in use), else the newest sign-in.
    pub(crate) fn merge_duplicates(&self) {
        let accounts = self.stored_accounts();
        let active = read_active(self.data_dir());
        let drop = duplicates_to_drop(&accounts, None, &active);
        let keep = accounts
            .iter()
            .map(|(k, _)| k)
            .find(|k| !drop.contains(k))
            .cloned()
            .unwrap_or_default();
        self.drop_accounts(&drop, &keep);
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use matrix_sdk::{
        SessionMeta, SessionTokens,
        authentication::matrix::MatrixSession,
        ruma::{owned_device_id, owned_user_id},
    };

    use super::*;
    use crate::SessionStoreKind;

    fn account(user: &str, homeserver: &str, token: &str) -> StoredSession {
        StoredSession {
            homeserver: homeserver.into(),
            session: MatrixSession {
                meta: SessionMeta {
                    user_id: user.try_into().unwrap_or_else(|_| owned_user_id!("@x:y.z")),
                    device_id: owned_device_id!("DEV"),
                },
                tokens: SessionTokens { access_token: token.into(), refresh_token: None },
            },
            store_passphrase: format!("pass-{token}"),
        }
    }

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("morpheus-accounts-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    fn core_with(dir: &Path, accounts: &[(&str, StoredSession)]) -> Core {
        let core = Core::with_session_store(dir, SessionStoreKind::File).expect("core");
        for (key, stored) in accounts {
            let slot = slot_dir(dir, key);
            std::fs::create_dir_all(slot.join(session_store::STORE_DIR)).expect("slot");
            session_store::resolve(&slot, Some(SessionStoreKind::File))
                .save(stored)
                .expect("save");
        }
        core
    }

    fn keys(core: &Core) -> Vec<(String, String)> {
        core.stored_accounts()
            .into_iter()
            .map(|(key, s)| (key, s.session.tokens.access_token))
            .collect()
    }

    #[test]
    fn homeservers_are_compared_as_people_write_them() {
        assert_eq!(homeserver_key("https://Matrix.org/"), homeserver_key("matrix.org"));
        assert_ne!(homeserver_key("matrix.org"), homeserver_key("example.org"));
    }

    #[test]
    fn signing_in_again_replaces_the_listed_account() {
        let dir = temp("again");
        let alice = "@alice:example.org";
        let core = core_with(
            &dir,
            &[
                ("", account(alice, "https://example.org", "old")),
                ("accounts/1", account("@bob:example.org", "https://example.org", "bob")),
                ("accounts/2", account(alice, "example.org", "new")),
            ],
        );
        core.drop_duplicates_of(&dir.join("accounts/2"));
        assert_eq!(
            keys(&core),
            vec![("accounts/1".to_owned(), "bob".to_owned()), ("accounts/2".to_owned(), "new".to_owned())]
        );
        assert!(!dir.join(session_store::STORE_DIR).exists(), "the old store went too");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn accounts_stored_twice_are_merged_keeping_the_active_one() {
        let dir = temp("merge");
        let alice = "@alice:example.org";
        let core = core_with(
            &dir,
            &[
                ("", account(alice, "https://example.org", "first")),
                ("accounts/1", account(alice, "https://example.org", "second")),
                ("accounts/2", account("@bob:example.org", "https://example.org", "bob")),
            ],
        );
        write_active(&dir, "").expect("active");
        core.merge_duplicates();
        assert_eq!(
            keys(&core),
            vec![("".to_owned(), "first".to_owned()), ("accounts/2".to_owned(), "bob".to_owned())]
        );
        // Without an active one among them, the newest sign-in stays; same account
        // on another homeserver is another account.
        let dir2 = temp("merge2");
        let core = core_with(
            &dir2,
            &[
                ("", account(alice, "https://example.org", "a")),
                ("accounts/1", account(alice, "https://example.org", "b")),
                ("accounts/2", account(alice, "https://other.org", "c")),
            ],
        );
        write_active(&dir2, "accounts/1").expect("active");
        core.merge_duplicates();
        assert_eq!(
            keys(&core),
            vec![("accounts/1".to_owned(), "b".to_owned()), ("accounts/2".to_owned(), "c".to_owned())]
        );
        assert_eq!(read_active(&dir2), "accounts/1");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&dir2).ok();
    }
}
