//! Private marks on rooms and people (`crates/ui/src/components/marks.rs`,
//! kept in account data, see `morpheus_core::marks`): DMs pinned to the top
//! or closed until a new message, people ignored, and rooms muted for a
//! while. This is what they do to the DM list; loading and saving them is
//! the front end's.

use std::collections::HashMap;

use morpheus_core::{OwnedRoomId, RoomSummary, marks};

/// Key (room or user ID) to the time it was marked, in milliseconds since
/// the epoch (for mutes: when the mute ends, 0 for good).
pub type Map = HashMap<String, i64>;

/// The mute lengths a DM's menu offers: label and milliseconds (0: for good).
pub const MUTE_CHOICES: [(&str, i64); 6] = [
    ("For 15 minutes", 15 * 60_000),
    ("For 1 hour", 60 * 60_000),
    ("For 3 hours", 3 * 60 * 60_000),
    ("For 8 hours", 8 * 60 * 60_000),
    ("For 24 hours", 24 * 60 * 60_000),
    ("Until I turn it back on", 0),
];

#[derive(Clone, Debug, Default)]
pub struct Marks {
    pub pinned: Map,
    pub closed: Map,
    pub ignored: Map,
    pub muted_until: Map,
    /// Mutes changed here, which the room list only learns of with the
    /// room's next change.
    pub muted_now: HashMap<String, bool>,
}

impl Marks {
    /// The map of one kind of mark.
    pub fn map_mut(&mut self, kind: &str) -> &mut Map {
        match kind {
            marks::PINNED => &mut self.pinned,
            marks::CLOSED => &mut self.closed,
            marks::IGNORED => &mut self.ignored,
            _ => &mut self.muted_until,
        }
    }

    /// Sets (`Some`) or clears one mark.
    pub fn set(&mut self, kind: &str, key: &str, at: Option<i64>) {
        let map = self.map_mut(kind);
        match at {
            Some(at) => {
                map.insert(key.to_owned(), at);
            }
            None => {
                map.remove(key);
            }
        }
    }

    /// Whether a room is muted, counting changes made here.
    pub fn is_muted(&self, room: &RoomSummary) -> bool {
        let key = room.id.as_str();
        self.muted_now.get(key).copied().unwrap_or(room.muted)
            || self.muted_until.contains_key(key)
    }

    pub fn is_pinned(&self, room: &RoomSummary) -> bool {
        room.is_dm && self.pinned.contains_key(room.id.as_str())
    }

    /// The DMs the list shows: closed ones stay out until a new message
    /// comes (or they are the open one), pinned ones come first, in the
    /// order they were pinned; the rest keep their order.
    pub fn visible_dms(
        &self,
        dms: Vec<RoomSummary>,
        open: Option<&OwnedRoomId>,
        friends_page_open: bool,
    ) -> Vec<RoomSummary> {
        let mut shown: Vec<RoomSummary> = dms
            .into_iter()
            .filter(|room| {
                self.closed
                    .get(room.id.as_str())
                    .is_none_or(|at| i64::try_from(room.latest_ms).unwrap_or(i64::MAX) > *at)
                    || (Some(&room.id) == open && !friends_page_open)
            })
            .collect();
        shown.sort_by_key(|room| {
            self.pinned
                .get(room.id.as_str())
                .copied()
                .unwrap_or(i64::MAX)
        });
        shown
    }

    /// Closed DMs that are in `shown` again (a message came): their closed
    /// mark is to be cleared.
    pub fn reopened(&self, shown: &[RoomSummary]) -> Vec<String> {
        shown
            .iter()
            .map(|room| room.id.to_string())
            .filter(|id| self.closed.contains_key(id))
            .collect()
    }

    /// Rooms whose timed mute has run out at `now`.
    pub fn ended_mutes(&self, now: i64) -> Vec<String> {
        self.muted_until
            .iter()
            .filter(|(_, until)| **until > 0 && **until <= now)
            .map(|(room, _)| room.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::room;

    fn dm(id: &str, name: &str, latest: u64) -> RoomSummary {
        let mut r = room(id, name);
        r.is_dm = true;
        r.latest_ms = latest;
        r
    }

    fn names(rooms: &[RoomSummary]) -> Vec<&str> {
        rooms.iter().map(|r| r.name.as_str()).collect()
    }

    #[test]
    fn pinned_dms_come_first_in_the_order_pinned() {
        let mut marks = Marks::default();
        marks.set(marks::PINNED, "!c:x", Some(10));
        marks.set(marks::PINNED, "!b:x", Some(20));
        let all = vec![dm("!a:x", "a", 0), dm("!b:x", "b", 0), dm("!c:x", "c", 0), dm("!d:x", "d", 0)];
        let shown = marks.visible_dms(all, None, false);
        assert_eq!(names(&shown), ["c", "b", "a", "d"]);
    }

    #[test]
    fn a_closed_dm_stays_out_until_a_newer_message_or_it_is_open() {
        let mut marks = Marks::default();
        marks.set(marks::CLOSED, "!a:x", Some(100));
        let old = dm("!a:x", "a", 50);
        assert!(marks.visible_dms(vec![old.clone()], None, false).is_empty());
        let newer = dm("!a:x", "a", 150);
        assert_eq!(marks.visible_dms(vec![newer.clone()], None, false).len(), 1);
        assert_eq!(marks.reopened(&[newer]), ["!a:x"]);
        // Open (and not on the Friends page): stays listed.
        assert_eq!(marks.visible_dms(vec![old.clone()], Some(&old.id), false).len(), 1);
        assert!(marks.visible_dms(vec![old.clone()], Some(&old.id), true).is_empty());
    }

    #[test]
    fn mutes_count_what_was_changed_here_and_timed_ones_end() {
        let mut marks = Marks::default();
        let mut r = dm("!a:x", "a", 0);
        assert!(!marks.is_muted(&r));
        marks.muted_now.insert("!a:x".into(), true);
        assert!(marks.is_muted(&r));
        marks.muted_now.insert("!a:x".into(), false);
        r.muted = true;
        assert!(!marks.is_muted(&r), "unmuted here beats the list's old state");
        marks.set(marks::MUTED_UNTIL, "!a:x", Some(1_000));
        assert!(marks.is_muted(&r));
        assert!(marks.ended_mutes(500).is_empty());
        assert_eq!(marks.ended_mutes(1_000), ["!a:x"]);
        marks.set(marks::MUTED_UNTIL, "!b:x", Some(0));
        assert_eq!(marks.ended_mutes(9_999_999), ["!a:x"], "0 is for good");
    }
}
