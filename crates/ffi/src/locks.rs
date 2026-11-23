//! Password locks on rooms and DMs, on this device (`view_model::locks` has the rules, the desktop the same ones): a
//! locked room asks for its password before it opens, and closes again after the time the lock allows away from it.

use morpheus_view_model::{i18n, locks as rules};

use crate::{FfiError, Morpheus};

fn failed(reason: impl Into<String>) -> FfiError {
    FfiError::Failed { reason: reason.into() }
}

/// Whether a room has a lock, and whether it is closed now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LockState {
    pub has_lock: bool,
    pub locked: bool,
}

/// What the lock pages say.
#[derive(uniffi::Record)]
pub struct LockWords {
    pub blurb: String,
    pub forgot: String,
    pub locked_title: String,
}

impl Morpheus {
    fn with_locks<R>(&self, f: impl FnOnce(&mut rules::Locks) -> R) -> R {
        match self.locks.lock() {
            Ok(mut locks) => f(&mut locks),
            Err(poisoned) => f(&mut poisoned.into_inner()),
        }
    }

    fn keep_locks(&self) {
        let _ = self.with_locks(|locks| locks.save(&self.dir));
    }

    /// Whether `room` has a lock and is closed (used by the menus too).
    pub(crate) fn lock_state_of(&self, room: &str) -> LockState {
        self.with_locks(|locks| LockState { has_lock: locks.has_lock(room), locked: locks.is_locked(room) })
    }

    pub(crate) fn clear_locks(&self) {
        self.with_locks(rules::Locks::clear);
        self.keep_locks();
    }
}

#[uniffi::export]
impl Morpheus {
    pub fn lock_state(&self, room_id: String) -> LockState {
        self.lock_state_of(&room_id)
    }

    pub fn lock_words(&self) -> LockWords {
        LockWords { blurb: i18n::t(rules::LOCK_BLURB), forgot: i18n::t(rules::FORGOT), locked_title: i18n::t(rules::LOCKED_TITLE) }
    }

    /// Put a lock on a room: the password twice. Closed at once.
    pub async fn set_lock(&self, room_id: String, password: String, again: String) -> Result<(), FfiError> {
        if let Some(problem) = rules::password_problem(&password, &again) {
            return Err(failed(i18n::t(problem)));
        }
        let hash = crate::runtime().spawn_blocking(move || rules::hash_password(&password))
            .await
            .map_err(|e| failed(e.to_string()))?
            .map_err(failed)?;
        self.with_locks(|locks| locks.add(&room_id, hash));
        self.keep_locks();
        Ok(())
    }

    /// Take a lock off (the password is asked).
    pub async fn remove_lock(&self, room_id: String, password: String) -> Result<(), FfiError> {
        let Some(hash) = self.with_locks(|locks| locks.hash_of(&room_id).map(str::to_owned)) else { return Ok(()) };
        let right = crate::runtime().spawn_blocking(move || rules::verify_password(&hash, &password)).await.map_err(|e| failed(e.to_string()))?;
        if !right {
            return Err(failed(i18n::t("That is not the password.")));
        }
        self.with_locks(|locks| locks.remove(&room_id));
        self.keep_locks();
        Ok(())
    }

    /// Open a locked room with its password; whether it was right.
    pub async fn unlock_room(&self, room_id: String, password: String) -> Result<bool, FfiError> {
        let Some(hash) = self.with_locks(|locks| locks.hash_of(&room_id).map(str::to_owned)) else { return Ok(true) };
        let right = crate::runtime().spawn_blocking(move || rules::verify_password(&hash, &password)).await.map_err(|e| failed(e.to_string()))?;
        if right {
            self.with_locks(|locks| locks.unlock(&room_id));
        }
        Ok(right)
    }

    /// Close an open lock at once ("Lock now").
    pub fn lock_room(&self, room_id: String) {
        self.with_locks(|locks| locks.lock(&room_id));
    }

    /// Time passes: the locks whose room has been left for long enough close. `watching` is the room on screen.
    /// Gives the rooms that closed now.
    pub fn lock_tick(&self, watching: Option<String>) -> Vec<String> {
        self.with_locks(|locks| locks.tick(std::time::Instant::now(), watching.as_deref()))
    }
}
