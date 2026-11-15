//! Least-recently-used keeps of things worth having ready (the timelines of
//! rooms just left), bounded by how many there are and by their combined
//! weight (a measure of memory the caller chooses).

/// Most items kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub max_items: usize,
    pub max_weight: usize,
}

/// Items by recency, most recent first.
#[derive(Debug)]
pub struct Lru<K, V> {
    budget: Budget,
    items: Vec<(K, V, usize)>,
}

impl<K: PartialEq, V> Lru<K, V> {
    pub fn new(budget: Budget) -> Self {
        Self {
            budget,
            items: Vec::new(),
        }
    }

    /// Makes `key` the most recent, replacing what it held, and returns what
    /// had to go to stay in budget, least recently used first. The item just
    /// put in always stays, however heavy: only older ones are evicted.
    pub fn insert(&mut self, key: K, value: V, weight: usize) -> Vec<V> {
        let mut gone = Vec::new();
        if let Some(at) = self.items.iter().position(|(k, ..)| *k == key) {
            gone.push(self.items.remove(at).1);
        }
        self.items.insert(0, (key, value, weight));
        while self.items.len() > 1
            && (self.items.len() > self.budget.max_items || self.weight() > self.budget.max_weight)
        {
            if let Some((_, value, _)) = self.items.pop() {
                gone.push(value);
            }
        }
        gone
    }

    /// Takes `key` out (to use it: put it back with [`Self::insert`] when
    /// done).
    pub fn take(&mut self, key: &K) -> Option<V> {
        let at = self.items.iter().position(|(k, ..)| k == key)?;
        Some(self.items.remove(at).1)
    }

    pub fn contains(&self, key: &K) -> bool {
        self.items.iter().any(|(k, ..)| k == key)
    }

    /// What `key` holds, without making it more recent.
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: std::borrow::Borrow<Q>,
        Q: PartialEq + ?Sized,
    {
        self.items
            .iter()
            .find(|(k, ..)| k.borrow() == key)
            .map(|(_, v, _)| v)
    }

    /// Takes the items that fail `keep` out.
    pub fn retain(&mut self, mut keep: impl FnMut(&K) -> bool) -> Vec<V> {
        let mut gone = Vec::new();
        let mut kept = Vec::new();
        for item in self.items.drain(..) {
            if keep(&item.0) {
                kept.push(item);
            } else {
                gone.push(item.1);
            }
        }
        self.items = kept;
        gone
    }

    /// The combined weight of what is kept.
    pub fn weight(&self) -> usize {
        self.items.iter().map(|(.., w)| w).sum()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Every item, most recent first.
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.items.iter().map(|(_, v, _)| v)
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.items.iter().map(|(k, ..)| k)
    }
}

/// Things in use lately within a budget of bytes, for what costs memory only
/// while it is kept (decoded pictures): each is [`Self::touch`]ed when used,
/// and [`Self::evict`] names the least recently used ones to let go of until
/// the total is back within the budget. What was used in the last `keep`
/// rounds (frames) is never named, so what is on screen is not let go of
/// only to be made again at once; the budget can then be over for as long
/// as one screen needs more.
#[derive(Debug)]
pub struct ByteLru<K> {
    max_bytes: usize,
    items: std::collections::HashMap<K, (usize, u64)>,
    bytes: usize,
    round: u64,
}

impl<K: std::hash::Hash + Eq + Clone> ByteLru<K> {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            max_bytes,
            items: std::collections::HashMap::new(),
            bytes: 0,
            round: 0,
        }
    }

    /// `key` was used now; `bytes` is what it costs (read only when it is
    /// new). True when it was not kept before.
    pub fn touch(&mut self, key: K, bytes: impl FnOnce() -> usize) -> bool {
        if let Some((_, used)) = self.items.get_mut(&key) {
            *used = self.round;
            return false;
        }
        let bytes = bytes();
        self.bytes += bytes;
        self.items.insert(key, (bytes, self.round));
        true
    }

    /// The next round (frame) begins: what is touched from now on is newer.
    pub fn next_round(&mut self) {
        self.round += 1;
    }

    /// Takes out the least recently used items until the total is within
    /// the budget, never one used in the last `keep` rounds (the current
    /// round counts as one), and returns them, oldest first.
    pub fn evict(&mut self, keep: u64) -> Vec<K> {
        if self.bytes <= self.max_bytes {
            return Vec::new();
        }
        let newest_evictable = self.round.checked_sub(keep);
        let mut candidates: Vec<(u64, K)> = self
            .items
            .iter()
            .filter(|(_, (_, used))| newest_evictable.is_some_and(|newest| *used <= newest))
            .map(|(key, (_, used))| (*used, key.clone()))
            .collect();
        candidates.sort_by_key(|(used, _)| *used);
        let mut gone = Vec::new();
        for (_, key) in candidates {
            if self.bytes <= self.max_bytes {
                break;
            }
            self.remove(&key);
            gone.push(key);
        }
        gone
    }

    /// Takes `key` out (it went some other way); true when it was kept.
    pub fn remove(&mut self, key: &K) -> bool {
        match self.items.remove(key) {
            Some((bytes, _)) => {
                self.bytes -= bytes;
                true
            }
            None => false,
        }
    }

    pub fn contains(&self, key: &K) -> bool {
        self.items.contains_key(key)
    }

    /// What the items kept cost together.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lru(max_items: usize, max_weight: usize) -> Lru<&'static str, &'static str> {
        Lru::new(Budget {
            max_items,
            max_weight,
        })
    }

    #[test]
    fn the_least_recently_used_goes_first() {
        let mut keep = lru(3, 1000);
        assert!(keep.insert("a", "A", 1).is_empty());
        assert!(keep.insert("b", "B", 1).is_empty());
        assert!(keep.insert("c", "C", 1).is_empty());
        assert_eq!(keep.insert("d", "D", 1), vec!["A"]);
        assert_eq!(keep.keys().copied().collect::<Vec<_>>(), ["d", "c", "b"]);
        // Using b (taking it out and putting it back) makes it the newest.
        let b = keep.take(&"b").expect("b is kept");
        assert!(keep.insert("b", b, 1).is_empty());
        assert_eq!(keep.insert("e", "E", 1), vec!["C"]);
        assert_eq!(keep.keys().copied().collect::<Vec<_>>(), ["e", "b", "d"]);
    }

    #[test]
    fn weight_is_a_budget_too_and_evicts_oldest_first() {
        let mut keep = lru(10, 100);
        keep.insert("a", "A", 40);
        keep.insert("b", "B", 40);
        // 120 > 100: a, the oldest, goes; b and c stay.
        assert_eq!(keep.insert("c", "C", 40), vec!["A"]);
        assert_eq!(keep.weight(), 80);
        // A heavy one pushes out as many as it must.
        assert_eq!(keep.insert("d", "D", 90), vec!["B", "C"]);
        assert_eq!(keep.len(), 1);
    }

    #[test]
    fn the_newest_stays_even_when_it_alone_is_over_budget() {
        let mut keep = lru(5, 10);
        keep.insert("a", "A", 5);
        assert_eq!(keep.insert("big", "BIG", 500), vec!["A"]);
        assert!(keep.contains(&"big"));
        assert_eq!(keep.len(), 1);
    }

    #[test]
    fn inserting_a_key_again_replaces_it() {
        let mut keep = lru(5, 100);
        keep.insert("a", "old", 10);
        assert_eq!(keep.insert("a", "new", 20), vec!["old"]);
        assert_eq!(keep.len(), 1);
        assert_eq!(keep.weight(), 20);
        assert_eq!(keep.take(&"a"), Some("new"));
        assert!(keep.is_empty());
    }

    #[test]
    fn retain_drops_what_is_no_longer_wanted() {
        let mut keep = lru(5, 100);
        keep.insert("a", "A", 1);
        keep.insert("b", "B", 1);
        keep.insert("c", "C", 1);
        assert_eq!(keep.retain(|k| *k != "b"), vec!["B"]);
        assert_eq!(keep.keys().copied().collect::<Vec<_>>(), ["c", "a"]);
    }

    #[test]
    fn get_finds_without_reordering() {
        let mut keep = lru(3, 100);
        keep.insert("a", "A", 1);
        keep.insert("b", "B", 1);
        assert_eq!(keep.get(&"a"), Some(&"A"));
        assert_eq!(keep.get(&"z"), None);
        assert_eq!(keep.keys().copied().collect::<Vec<_>>(), ["b", "a"]);
    }

    const MB: usize = 1024 * 1024;

    #[test]
    fn bytes_over_the_budget_evict_the_least_recently_used() {
        let mut drawn = ByteLru::new(10 * MB);
        for (round, key) in ["a", "b", "c", "d"].into_iter().enumerate() {
            assert!(drawn.touch(key, || 3 * MB), "{key} is new");
            if round == 2 {
                // a is used again: b is now the oldest.
                assert!(!drawn.touch("a", || unreachable!("known already")));
            }
            drawn.next_round();
        }
        assert_eq!(drawn.bytes(), 12 * MB);
        assert_eq!(drawn.evict(1), vec!["b"]);
        assert_eq!(drawn.bytes(), 9 * MB);
        assert_eq!(drawn.len(), 3);
        // Within budget: nothing more goes.
        assert!(drawn.evict(1).is_empty());
    }

    #[test]
    fn what_was_used_in_the_last_rounds_is_never_evicted() {
        let mut drawn = ByteLru::new(4 * MB);
        drawn.touch("old", || 2 * MB);
        drawn.next_round();
        // One screen needs more than the budget: all used in this round.
        for key in ["x", "y", "z"] {
            drawn.touch(key, || 2 * MB);
        }
        // Only what was not on screen goes, and the budget stays over.
        assert_eq!(drawn.evict(1), vec!["old"]);
        assert_eq!(drawn.bytes(), 6 * MB);
        // Kept for two rounds: the next round still keeps them.
        drawn.next_round();
        assert!(drawn.evict(2).is_empty());
        // Once they are off screen, the oldest go until it fits.
        drawn.touch("z", || 0);
        drawn.next_round();
        drawn.next_round();
        let gone = drawn.evict(1);
        assert_eq!(gone.len(), 1);
        assert!(gone[0] == "x" || gone[0] == "y");
        assert!(drawn.contains(&"z"));
        assert_eq!(drawn.bytes(), 4 * MB);
    }

    #[test]
    fn many_pictures_stay_within_the_budget() {
        let mut drawn = ByteLru::new(64 * MB);
        for picture in 0..1000u32 {
            drawn.touch(picture, || 2 * MB);
            drawn.next_round();
            drawn.evict(1);
            assert!(
                drawn.bytes() <= 64 * MB,
                "{} MB at {picture}",
                drawn.bytes() / MB
            );
        }
        assert_eq!(drawn.len(), 32);
        // The newest are the ones kept.
        assert!(drawn.contains(&999) && !drawn.contains(&0));
        assert!(drawn.remove(&999));
        assert!(!drawn.remove(&999));
        assert_eq!(drawn.bytes(), 62 * MB);
    }
}
