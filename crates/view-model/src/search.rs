//! The room search panel without a toolkit: what it asks, how long it waits
//! after typing, and what its results area shows for each outcome.

use morpheus_core::SearchHit;

use crate::format::{calendar_date, clock_time};

/// Quiet time after typing before searching, in milliseconds.
pub const DEBOUNCE_MS: u64 = 250;
/// Most results shown.
pub const LIMIT: usize = 50;
/// The input's placeholder.
pub const PLACEHOLDER: &str = "Search messages";
/// The note under it.
pub const NOTE: &str = "Searches the messages this device has loaded.";
/// What is shown when nothing matched.
pub const NONE_FOUND: &str = "No messages found.";

/// One result as listed: who, when, what.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HitRow {
    pub event_id: String,
    pub sender: String,
    /// `YYYY-MM-DD HH:MM`.
    pub when: String,
    pub body: String,
}

impl HitRow {
    pub fn new(hit: &SearchHit) -> Self {
        Self {
            event_id: hit.event_id.clone(),
            sender: hit.sender_name.clone(),
            when: format!(
                "{} {}",
                calendar_date(hit.timestamp_ms),
                clock_time(hit.timestamp_ms)
            ),
            body: hit.body.clone(),
        }
    }
}

/// What the area under the box shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Results {
    /// Nothing asked yet (or the box is empty).
    Idle,
    Error(String),
    /// Asked, and nothing matched.
    Empty,
    Hits(Vec<HitRow>),
}

/// The results area for the query last searched (`settled`) and what the
/// search gave back (`None` while it runs).
pub fn results(settled: &str, outcome: Option<&Result<Vec<SearchHit>, String>>) -> Results {
    match outcome {
        None => Results::Idle,
        Some(Err(err)) => Results::Error(err.clone()),
        Some(Ok(hits)) if hits.is_empty() => {
            if settled.trim().is_empty() {
                Results::Idle
            } else {
                Results::Empty
            }
        }
        Some(Ok(hits)) => Results::Hits(hits.iter().map(HitRow::new).collect()),
    }
}

/// Whether a query is worth asking the server or index about.
pub fn is_query(text: &str) -> bool {
    !text.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(id: &str) -> SearchHit {
        SearchHit {
            event_id: id.into(),
            sender_name: "alice".into(),
            body: "an okapi in the forest".into(),
            timestamp_ms: 0,
        }
    }

    #[test]
    fn nothing_is_shown_until_a_search_comes_back() {
        assert_eq!(results("okapi", None), Results::Idle);
        assert_eq!(results("", Some(&Ok(Vec::new()))), Results::Idle);
        assert_eq!(results("  ", Some(&Ok(Vec::new()))), Results::Idle);
    }

    #[test]
    fn no_hits_for_a_real_query_says_so() {
        assert_eq!(results("okapi", Some(&Ok(Vec::new()))), Results::Empty);
    }

    #[test]
    fn errors_are_passed_on() {
        assert_eq!(
            results("okapi", Some(&Err("offline".into()))),
            Results::Error("offline".into())
        );
    }

    #[test]
    fn hits_list_sender_time_and_text() {
        let Results::Hits(rows) = results("okapi", Some(&Ok(vec![hit("$a"), hit("$b")]))) else {
            panic!("hits expected");
        };
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].event_id, "$a");
        assert_eq!(rows[0].sender, "alice");
        assert_eq!(rows[0].body, "an okapi in the forest");
        assert_eq!(rows[0].when.len(), "1970-01-01 00:00".len());
        assert!(rows[0].when.contains(' '));
    }

    #[test]
    fn blank_queries_are_not_asked() {
        assert!(!is_query("   "));
        assert!(is_query(" a "));
    }
}
