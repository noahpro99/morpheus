//! Password locks on rooms and direct messages, kept on this device only.
//! A lock is a privacy screen, not encryption: the room's messages stay
//! where they are, and the app does not show them (or announce them) until
//! the password is entered. Only a salted Argon2 hash of each password is
//! kept (`locks.json` in the data directory), never the password.
//!
//! The rules: a room with a lock starts locked every time the app starts.
//! Once unlocked it stays so while you look at it; when you are not (you are
//! in another room, or the window is not the one in front) a clock runs, and
//! at the timeout (a setting, 5 minutes unless changed) it locks again. Coming
//! back before that stops the clock. "Lock now" locks at once.

use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

/// How long an unlocked room that is not being looked at stays unlocked.
pub const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// What the setting offers: seconds, and its words.
pub const TIMEOUT_CHOICES: [(u64, &str); 5] = [
    (60, "1 minute"),
    (300, "5 minutes"),
    (900, "15 minutes"),
    (1800, "30 minutes"),
    (3600, "1 hour"),
];

/// The shortest password a lock accepts.
pub const MIN_PASSWORD: usize = 4;

const FILE: &str = "locks.json";

/// A salted Argon2id hash (in the PHC string form) of `password`.
pub fn hash_password(password: &str) -> Result<String, String> {
    use argon2::{
        Argon2,
        password_hash::{PasswordHasher, SaltString, rand_core::OsRng},
    };
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|err| err.to_string())
}

/// Whether `password` is the one `hash` was made from.
pub fn verify_password(hash: &str, password: &str) -> bool {
    use argon2::{
        Argon2,
        password_hash::{PasswordHash, PasswordVerifier},
    };
    PasswordHash::new(hash)
        .is_ok_and(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
}

/// What the "set a password" form says about what was typed, if it is not
/// acceptable.
pub fn password_problem(password: &str, again: &str) -> Option<&'static str> {
    if password.chars().count() < MIN_PASSWORD {
        Some("Use at least 4 characters.")
    } else if password != again {
        Some("The two passwords are different.")
    } else {
        None
    }
}

/// The locks of this device and which of them are open now.
#[derive(Debug, Default)]
pub struct Locks {
    /// Room ID to the hash of its password.
    hashes: BTreeMap<String, String>,
    timeout_secs: Option<u64>,
    /// A timeout of the tests' choosing (not kept).
    timeout_override: Option<Duration>,
    /// The rooms open now.
    unlocked: std::collections::BTreeSet<String>,
    /// Since when each open room has not been looked at.
    away: HashMap<String, Instant>,
}

impl Locks {
    /// Read from `dir` (no locks where missing or unreadable). Every lock
    /// starts locked.
    pub fn load(dir: &Path) -> Self {
        let kept: Value = std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or(Value::Null);
        let hashes = kept
            .get("rooms")
            .and_then(Value::as_object)
            .map(|rooms| {
                rooms
                    .iter()
                    .filter_map(|(room, hash)| Some((room.clone(), hash.as_str()?.to_owned())))
                    .collect()
            })
            .unwrap_or_default();
        let timeout_secs = kept.get("timeout_secs").and_then(Value::as_u64);
        Self { hashes, timeout_secs, ..Self::default() }
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        let json = json!({ "timeout_secs": self.timeout_secs, "rooms": self.hashes });
        std::fs::write(dir.join(FILE), json.to_string())
    }

    pub fn timeout(&self) -> Duration {
        self.timeout_override
            .unwrap_or_else(|| Duration::from_secs(self.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS)))
    }

    /// The timeout in seconds, as the setting shows it.
    pub fn timeout_secs(&self) -> u64 {
        self.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS)
    }

    pub fn set_timeout_secs(&mut self, secs: u64) {
        self.timeout_secs = Some(secs);
    }

    /// For tests: a timeout that is not kept.
    pub fn override_timeout(&mut self, timeout: Duration) {
        self.timeout_override = Some(timeout);
    }

    /// Whether the room has a lock at all.
    pub fn has_lock(&self, room: &str) -> bool {
        self.hashes.contains_key(room)
    }

    /// Whether the room has a lock that is closed now.
    pub fn is_locked(&self, room: &str) -> bool {
        self.has_lock(room) && !self.unlocked.contains(room)
    }

    pub fn hash_of(&self, room: &str) -> Option<&str> {
        self.hashes.get(room).map(String::as_str)
    }

    /// Put a lock on the room (closed at once), made with `hash`.
    pub fn add(&mut self, room: &str, hash: String) {
        self.hashes.insert(room.to_owned(), hash);
        self.unlocked.remove(room);
        self.away.remove(room);
    }

    /// Take the lock off.
    pub fn remove(&mut self, room: &str) {
        self.hashes.remove(room);
        self.unlocked.remove(room);
        self.away.remove(room);
    }

    /// Drop every lock (signing out).
    pub fn clear(&mut self) {
        self.hashes.clear();
        self.unlocked.clear();
        self.away.clear();
    }

    /// Open the room (its password was right).
    pub fn unlock(&mut self, room: &str) {
        if self.has_lock(room) {
            self.unlocked.insert(room.to_owned());
            self.away.remove(room);
        }
    }

    /// Close the room again.
    pub fn lock(&mut self, room: &str) {
        self.unlocked.remove(room);
        self.away.remove(room);
    }

    /// Time passes. `watching` is the room being looked at (its window in
    /// front), if any. Returns the rooms that locked now.
    pub fn tick(&mut self, now: Instant, watching: Option<&str>) -> Vec<String> {
        let timeout = self.timeout();
        let mut closed = Vec::new();
        for room in &self.unlocked {
            if Some(room.as_str()) == watching {
                self.away.remove(room);
                continue;
            }
            let since = *self.away.entry(room.clone()).or_insert(now);
            if now.duration_since(since) >= timeout {
                closed.push(room.clone());
            }
        }
        for room in &closed {
            self.lock(room);
        }
        closed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("morpheus-locks-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn locks_json_holds_a_hash_and_never_the_password() {
        let dir = temp_dir("file");
        let mut locks = Locks::default();
        let hash = hash_password("correct horse").expect("hash");
        locks.add("!dm:server", hash);
        locks.save(&dir).expect("save");
        let text = std::fs::read_to_string(dir.join("locks.json")).expect("read");
        assert!(!text.contains("correct horse"), "the password is not in the file: {text}");
        assert!(text.contains("$argon2"), "an Argon2 hash is: {text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_same_password_gets_different_hashes_and_both_verify() {
        let (a, b) = (hash_password("same").expect("a"), hash_password("same").expect("b"));
        assert_ne!(a, b, "random salt");
        assert!(verify_password(&a, "same") && verify_password(&b, "same"));
        assert!(!verify_password(&a, "other"));
        assert!(!verify_password("not a hash", "same"));
    }

    #[test]
    fn a_lock_survives_a_restart_closed() {
        let dir = temp_dir("restart");
        let mut locks = Locks::default();
        locks.add("!r:s", hash_password("secret").expect("hash"));
        locks.unlock("!r:s");
        assert!(!locks.is_locked("!r:s"));
        locks.set_timeout_secs(900);
        locks.save(&dir).expect("save");
        let again = Locks::load(&dir);
        assert!(again.has_lock("!r:s") && again.is_locked("!r:s"), "closed after a restart");
        assert_eq!(again.timeout_secs(), 900);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_open_room_locks_after_the_timeout_when_not_looked_at() {
        let mut locks = Locks::default();
        locks.add("!r:s", "hash".into());
        assert!(locks.is_locked("!r:s"), "closed when put on");
        locks.unlock("!r:s");
        let t0 = Instant::now();
        assert!(locks.tick(t0, Some("!r:s")).is_empty(), "looked at: stays");
        assert!(locks.tick(t0 + Duration::from_secs(1000), Some("!r:s")).is_empty(), "looked at for long: stays");
        // Away from t0+1000.
        let away = t0 + Duration::from_secs(1000);
        assert!(locks.tick(away, None).is_empty());
        assert!(locks.tick(away + Duration::from_secs(299), None).is_empty(), "inside the timeout");
        assert!(!locks.is_locked("!r:s"));
        assert_eq!(locks.tick(away + Duration::from_secs(300), None), vec!["!r:s".to_owned()]);
        assert!(locks.is_locked("!r:s"));
    }

    #[test]
    fn coming_back_in_time_stops_the_clock() {
        let mut locks = Locks::default();
        locks.add("!r:s", "hash".into());
        locks.unlock("!r:s");
        let t0 = Instant::now();
        locks.tick(t0, None);
        locks.tick(t0 + Duration::from_secs(200), Some("!r:s"));
        // Gone again: the clock starts over.
        locks.tick(t0 + Duration::from_secs(210), None);
        assert!(locks.tick(t0 + Duration::from_secs(400), None).is_empty());
        assert_eq!(locks.tick(t0 + Duration::from_secs(510), None).len(), 1);
    }

    #[test]
    fn the_timeout_can_be_shortened_for_tests_and_chosen() {
        let mut locks = Locks::default();
        assert_eq!(locks.timeout(), Duration::from_secs(300));
        locks.set_timeout_secs(60);
        assert_eq!(locks.timeout(), Duration::from_secs(60));
        locks.override_timeout(Duration::from_millis(500));
        assert_eq!(locks.timeout(), Duration::from_millis(500));
    }

    #[test]
    fn removing_and_clearing_take_locks_away() {
        let mut locks = Locks::default();
        locks.add("!a:s", "h".into());
        locks.add("!b:s", "h".into());
        locks.remove("!a:s");
        assert!(!locks.has_lock("!a:s") && !locks.is_locked("!a:s"));
        locks.clear();
        assert!(!locks.has_lock("!b:s"));
    }

    #[test]
    fn a_new_password_must_be_long_enough_and_typed_twice() {
        assert!(password_problem("abc", "abc").is_some());
        assert!(password_problem("abcd", "abce").is_some());
        assert!(password_problem("abcd", "abcd").is_none());
    }
}
