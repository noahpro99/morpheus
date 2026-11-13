//! A least-recently-used keep of things worth having ready (the timelines of
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
        Self { budget, items: Vec::new() }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn lru(max_items: usize, max_weight: usize) -> Lru<&'static str, &'static str> {
        Lru::new(Budget { max_items, max_weight })
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
}
