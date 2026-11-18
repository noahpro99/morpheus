//! Who is watching whose shared screen. Nobody receives a share they did
//! not ask for (the SFU forwards only what is subscribed, and the sharer's
//! encoder pauses when nobody is: dynacast), so each watcher says which
//! shares they watch, in a small data message, every few seconds and at
//! once when it changes; a sharer counts the watchers it heard from lately.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// The topic the messages go under.
pub(crate) const TOPIC: &str = "morpheus.watch";

/// A watcher is forgotten this long after their last message.
pub(crate) const STALE: Duration = Duration::from_secs(12);

/// How often the watchers say again what they watch.
pub(crate) const EVERY: Duration = Duration::from_secs(4);

/// The message: the identities of the sharers watched, comma separated.
pub(crate) fn encode(watching: &HashSet<String>) -> Vec<u8> {
    let mut names: Vec<&str> = watching.iter().map(String::as_str).collect();
    names.sort_unstable();
    names.join("\n").into_bytes()
}

pub(crate) fn decode(payload: &[u8]) -> HashSet<String> {
    String::from_utf8_lossy(payload)
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// What each watcher last said they watch.
#[derive(Default)]
pub(crate) struct Tally {
    heard: HashMap<String, (HashSet<String>, Instant)>,
}

impl Tally {
    pub(crate) fn hear(&mut self, watcher: &str, watching: HashSet<String>, now: Instant) {
        self.heard.insert(watcher.to_owned(), (watching, now));
    }

    pub(crate) fn forget(&mut self, watcher: &str) {
        self.heard.remove(watcher);
    }

    /// How many people watch `sharer`'s screen (the sharer's own preview is
    /// not counted: it says nothing).
    pub(crate) fn watching(&self, sharer: &str, now: Instant) -> usize {
        self.heard
            .iter()
            .filter(|(watcher, (set, at))| {
                watcher.as_str() != sharer
                    && set.contains(sharer)
                    && now.duration_since(*at) < STALE
            })
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(names: &[&str]) -> HashSet<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    #[test]
    fn messages_round_trip() {
        assert_eq!(decode(&encode(&set(&["@a:x", "@b:x"]))), set(&["@a:x", "@b:x"]));
        assert!(decode(&encode(&set(&[]))).is_empty());
    }

    #[test]
    fn a_sharer_counts_the_watchers_it_heard_from_lately() {
        let t0 = Instant::now();
        let mut tally = Tally::default();
        tally.hear("b", set(&["a"]), t0);
        tally.hear("c", set(&["a", "d"]), t0);
        tally.hear("d", set(&["c"]), t0);
        assert_eq!(tally.watching("a", t0), 2);
        assert_eq!(tally.watching("d", t0), 1);
        // Stopped watching: said so.
        tally.hear("b", set(&[]), t0);
        assert_eq!(tally.watching("a", t0), 1);
        // Gone silent for too long, or left.
        assert_eq!(tally.watching("a", t0 + STALE), 0);
        tally.forget("c");
        assert_eq!(tally.watching("d", t0), 0);
    }
}
