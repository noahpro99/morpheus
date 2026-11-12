//! The title bar's back and forward: the rooms visited, in order, with a
//! cursor (`Nav` in `crates/ui/src/components/titlebar.rs`).

use morpheus_core::OwnedRoomId;

/// Keep at most this many visited rooms.
pub const HISTORY_CAP: usize = 50;

/// Visited rooms in order, with a cursor for back and forward.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Nav {
    pub entries: Vec<OwnedRoomId>,
    pub index: usize,
}

impl Nav {
    /// A room was opened: it becomes the newest entry, dropping whatever was
    /// ahead of the cursor. Opening the room the cursor is on changes nothing.
    pub fn record(&mut self, room: OwnedRoomId) {
        if self.entries.get(self.index) == Some(&room) {
            return;
        }
        if !self.entries.is_empty() {
            self.entries.truncate(self.index + 1);
        }
        self.entries.push(room);
        if self.entries.len() > HISTORY_CAP {
            self.entries.remove(0);
        }
        self.index = self.entries.len() - 1;
    }

    pub fn can_go_back(&self) -> bool {
        self.index > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.index + 1 < self.entries.len()
    }

    /// Move the cursor by `step` (-1 back, 1 forward) and return the room it
    /// lands on; `None`, and no move, past either end.
    pub fn step(&mut self, step: isize) -> Option<OwnedRoomId> {
        let index = self.index.checked_add_signed(step)?;
        let room = self.entries.get(index)?.clone();
        self.index = index;
        Some(room)
    }

    /// A room left for good (or not joined any more): out of the history,
    /// with the cursor kept on the same entry where it can be.
    pub fn forget(&mut self, room: &OwnedRoomId) {
        let before = self.entries.iter().take(self.index).filter(|r| *r == room).count();
        let on_it = self.entries.get(self.index) == Some(room);
        self.entries.retain(|r| r != room);
        self.index = self.index.saturating_sub(before + usize::from(on_it));
        if self.index >= self.entries.len() {
            self.index = self.entries.len().saturating_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> OwnedRoomId {
        OwnedRoomId::try_from(format!("!{name}:x")).expect("room id")
    }

    #[test]
    fn walking_back_and_forward() {
        let mut nav = Nav::default();
        for room in ["a", "b", "c"] {
            nav.record(id(room));
        }
        assert!(nav.can_go_back() && !nav.can_go_forward());
        assert_eq!(nav.step(-1), Some(id("b")));
        assert_eq!(nav.step(-1), Some(id("a")));
        assert_eq!(nav.step(-1), None);
        assert!(!nav.can_go_back() && nav.can_go_forward());
        assert_eq!(nav.step(1), Some(id("b")));
        assert_eq!(nav.step(1), Some(id("c")));
        assert_eq!(nav.step(1), None);
    }

    #[test]
    fn a_new_room_drops_what_was_ahead() {
        let mut nav = Nav::default();
        for room in ["a", "b", "c"] {
            nav.record(id(room));
        }
        nav.step(-1);
        nav.record(id("d"));
        assert_eq!(nav.entries, [id("a"), id("b"), id("d")]);
        assert!(!nav.can_go_forward());
    }

    #[test]
    fn the_room_already_open_is_not_a_visit() {
        let mut nav = Nav::default();
        nav.record(id("a"));
        nav.record(id("a"));
        assert_eq!(nav.entries.len(), 1);
        assert!(!nav.can_go_back());
    }

    #[test]
    fn the_history_is_capped() {
        let mut nav = Nav::default();
        for i in 0..HISTORY_CAP + 10 {
            nav.record(id(&i.to_string()));
        }
        assert_eq!(nav.entries.len(), HISTORY_CAP);
        assert_eq!(nav.index, HISTORY_CAP - 1);
        assert_eq!(nav.entries[0], id("10"));
    }

    #[test]
    fn forgetting_a_room_keeps_the_cursor_on_its_entry() {
        let mut nav = Nav::default();
        for room in ["a", "b", "c"] {
            nav.record(id(room));
        }
        nav.step(-1);
        nav.forget(&id("a"));
        assert_eq!(nav.entries, [id("b"), id("c")]);
        assert_eq!(nav.entries[nav.index], id("b"));
    }
}
