//! Invites, the inbox and the join card without a toolkit
//! (`crates/ui/src/components/{invites,titlebar}.rs`): which rooms are
//! pending invites, what an invite says, what the inbox lists and how it
//! labels each item.

use morpheus_core::RoomSummary;

/// Rooms the user has been invited to, in the order the list reports them.
pub fn pending_invites(rooms: &[RoomSummary]) -> Vec<RoomSummary> {
    rooms.iter().filter(|room| room.invited).cloned().collect()
}

/// Who asked, as the invite shows them.
pub fn inviter(room: &RoomSummary) -> String {
    room.inviter.clone().unwrap_or_else(|| "Someone".into())
}

/// The sidebar row's second line.
pub fn from_line(room: &RoomSummary) -> String {
    format!("from {}", inviter(room))
}

/// What the invite is to.
pub fn kind(room: &RoomSummary) -> &'static str {
    if room.is_dm {
        "a direct message"
    } else if room.is_space {
        "a server"
    } else {
        "a room"
    }
}

/// The line under the room's name on its invite card.
pub fn card_line(room: &RoomSummary) -> String {
    format!("{} invited you to {}.", inviter(room), kind(room))
}

/// "1 member" or "N members": the join card's line.
pub fn members_label(count: u64) -> String {
    if count == 1 {
        "1 member".to_owned()
    } else {
        format!("{count} members")
    }
}

/// Whether a room has something the inbox lists: unread messages, mentions
/// or a mark.
fn is_unread(room: &RoomSummary) -> bool {
    !room.is_space
        && !room.invited
        && (room.unread_messages > 0 || room.highlight_count > 0 || room.marked_unread)
}

/// The rooms the inbox lists, mentions first, then by name.
pub fn unread_rooms(rooms: &[RoomSummary]) -> Vec<RoomSummary> {
    let mut items: Vec<RoomSummary> = rooms.iter().filter(|r| is_unread(r)).cloned().collect();
    items.sort_by(|a, b| {
        b.highlight_count
            .cmp(&a.highlight_count)
            .then_with(|| a.name.cmp(&b.name))
    });
    items
}

/// How many rooms have something unread: the inbox button's badge.
pub fn unread_count(rooms: &[RoomSummary]) -> usize {
    rooms.iter().filter(|r| is_unread(r)).count()
}

/// An inbox item's name: `@` for a DM, `#` for a room.
pub fn item_name(room: &RoomSummary) -> String {
    format!("{}{}", if room.is_dm { "@" } else { "#" }, room.name)
}

/// What an inbox item says on its right.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemStatus {
    /// Mentions: a red badge.
    Mentions(u64),
    /// "N new".
    New(u64),
    MarkedUnread,
}

impl ItemStatus {
    pub fn of(room: &RoomSummary) -> Self {
        if room.highlight_count > 0 {
            Self::Mentions(room.highlight_count.into())
        } else if room.unread_messages > 0 {
            Self::New(room.unread_messages.into())
        } else {
            Self::MarkedUnread
        }
    }

    pub fn text(&self) -> String {
        match self {
            Self::Mentions(n) => n.to_string(),
            Self::New(n) => format!("{n} new"),
            Self::MarkedUnread => "marked unread".to_owned(),
        }
    }
}

pub const INBOX_TITLE: &str = "Inbox";
pub const CAUGHT_UP: &str = "You're all caught up.";
pub const MARK_ALL: &str = "Mark all as read";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_room::room;

    #[test]
    fn invites_are_the_invited_rooms() {
        let mut a = room("!a:x", "a");
        a.invited = true;
        let b = room("!b:x", "b");
        assert_eq!(pending_invites(&[a.clone(), b]), vec![a]);
    }

    #[test]
    fn the_card_says_who_and_to_what() {
        let mut r = room("!a:x", "a");
        assert_eq!(card_line(&r), "Someone invited you to a room.");
        r.inviter = Some("alice".into());
        assert_eq!(card_line(&r), "alice invited you to a room.");
        assert_eq!(from_line(&r), "from alice");
        r.is_dm = true;
        assert_eq!(card_line(&r), "alice invited you to a direct message.");
        r.is_dm = false;
        r.is_space = true;
        assert_eq!(card_line(&r), "alice invited you to a server.");
    }

    #[test]
    fn member_counts_are_singular_and_plural() {
        assert_eq!(members_label(1), "1 member");
        assert_eq!(members_label(0), "0 members");
        assert_eq!(members_label(12), "12 members");
    }

    #[test]
    fn the_inbox_lists_unread_rooms_mentions_first() {
        let mut quiet = room("!q:x", "quiet");
        quiet.unread_messages = 3;
        let mut loud = room("!l:x", "zloud");
        loud.highlight_count = 1;
        let mut marked = room("!m:x", "marked");
        marked.marked_unread = true;
        let mut space = room("!s:x", "space");
        space.is_space = true;
        space.unread_messages = 9;
        let mut invited = room("!i:x", "invited");
        invited.invited = true;
        invited.unread_messages = 1;
        let read = room("!r:x", "read");
        let all = [quiet, loud, marked, space, invited, read];
        let names: Vec<_> = unread_rooms(&all).into_iter().map(|r| r.name).collect();
        assert_eq!(names, ["zloud", "marked", "quiet"]);
        assert_eq!(unread_count(&all), 3);
    }

    #[test]
    fn items_are_named_and_described() {
        let mut r = room("!a:x", "alice");
        r.is_dm = true;
        assert_eq!(item_name(&r), "@alice");
        r.is_dm = false;
        assert_eq!(item_name(&r), "#alice");
        r.marked_unread = true;
        assert_eq!(ItemStatus::of(&r).text(), "marked unread");
        r.unread_messages = 4;
        assert_eq!(ItemStatus::of(&r).text(), "4 new");
        r.highlight_count = 2;
        assert_eq!(ItemStatus::of(&r), ItemStatus::Mentions(2));
        assert_eq!(ItemStatus::of(&r).text(), "2");
    }
}
