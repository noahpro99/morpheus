//! Where a share can go (Android's share target): the rooms, channels and DMs
//! that match what is typed, best first, and the recent ones when nothing is.
//! The match is the palette's (`palette::fuzzy_score`), so a search finds the
//! same things on every app.
//!
//! Recency is the order the candidates come in: the room list is sorted by
//! recency (the server's own stamp), which is known for every room, where a
//! room's latest-message time is only worked out for the most recent ones
//! (172 of 194 rooms had none after a fresh sign-in).

use crate::palette::fuzzy_score;

/// A place the shared content can be sent to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub name: String,
    /// The server a channel lives in (empty for a DM or a room outside any server): words that find it too.
    pub place: String,
}

/// How many results at most (a scrolling list: more than a person goes through).
pub const LIMIT: usize = 200;

/// What typing the name itself earns over a fuzzy match: the whole name (far above), or its start. Recency never outweighs these.
fn literal_bonus(name: &str, needle: &str) -> i32 {
    let (name, needle) = (name.trim().to_lowercase(), needle.trim().to_lowercase());
    if name == needle {
        200
    } else if name.starts_with(&needle) {
        50
    } else {
        0
    }
}

/// The indexes of the `candidates` (given most recent first) to offer for
/// `needle`, best first. With nothing typed: the most recent first. Equal
/// matches go to the more recent.
pub fn rank(candidates: &[Candidate], needle: &str) -> Vec<usize> {
    let mut scored: Vec<(i32, usize)> = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, c)| {
            if needle.trim().is_empty() {
                return Some((0, index));
            }
            // The name counts for more than the place it is in (as the palette's label does over its words).
            let name = fuzzy_score(needle, &c.name).map(|s| s + 20 + literal_bonus(&c.name, needle));
            let place = if c.place.is_empty() { None } else { fuzzy_score(needle, &c.place) };
            name.max(place).map(|score| (score, index))
        })
        .collect();
    scored.sort_by_key(|(score, index)| (std::cmp::Reverse(*score), *index));
    scored.into_iter().take(LIMIT).map(|(_, index)| index).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(name: &str, place: &str) -> Candidate {
        Candidate { name: name.into(), place: place.into() }
    }

    fn names<'a>(list: &'a [Candidate], at: &[usize]) -> Vec<&'a str> {
        at.iter().map(|i| list[*i].name.as_str()).collect()
    }

    #[test]
    fn nothing_typed_lists_the_most_recent_first() {
        let list = [c("new", ""), c("mid", ""), c("old", "")];
        assert_eq!(names(&list, &rank(&list, "")), ["new", "mid", "old"]);
        assert_eq!(names(&list, &rank(&list, "  ")), ["new", "mid", "old"]);
    }

    #[test]
    fn typing_finds_by_name_best_match_first() {
        let list = [c("random", ""), c("rooms of bob", ""), c("bob", ""), c("alice", "")];
        let found = rank(&list, "bob");
        assert_eq!(names(&list, &found), ["bob", "rooms of bob"], "alice and random do not match, the exact name leads");
    }

    #[test]
    fn the_server_name_finds_its_channels_below_a_name_match() {
        let list = [c("general", "Work"), c("work", "")];
        assert_eq!(names(&list, &rank(&list, "work")), ["work", "general"]);
    }

    #[test]
    fn equal_matches_go_to_the_more_recent() {
        let list = [c("chat", ""), c("chat", "")];
        assert_eq!(rank(&list, "chat"), [0, 1]);
    }

    #[test]
    fn an_exact_name_beats_a_more_recent_longer_one() {
        let list = [c("bobby", ""), c("bobsled team", ""), c("bob", "")];
        assert_eq!(names(&list, &rank(&list, "bob")), ["bob", "bobby", "bobsled team"], "recency only breaks ties between equal matches");
        assert_eq!(names(&list, &rank(&list, "BOB ")), ["bob", "bobby", "bobsled team"], "case and spaces around it do not matter");
    }

    #[test]
    fn a_name_that_starts_with_it_beats_a_recent_one_that_only_has_it_inside() {
        let list = [c("my-bob-room", ""), c("bob room", "")];
        assert_eq!(names(&list, &rank(&list, "bob")), ["bob room", "my-bob-room"]);
    }

    #[test]
    fn the_list_is_capped() {
        let list: Vec<Candidate> = (0..300).map(|i| c(&format!("room {i}"), "")).collect();
        assert_eq!(rank(&list, "").len(), LIMIT);
        assert_eq!(rank(&list, "room").len(), LIMIT);
    }
}
