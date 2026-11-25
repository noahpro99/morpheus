//! The phone's swipes between its two lists (Servers and DMs) and the last chat of each (#389, directions fixed in #394).
//!
//! "Swipe right" is the finger moving from the left of the screen to the right, "swipe left" the other way.
//!
//! - Servers, swipe right: show the DMs. Servers, swipe left: open the last server channel's chat.
//! - DMs, swipe right: open the last DM's chat. DMs, swipe left: go back to Servers.
//!
//! A swipe that would open a chat when none was kept does nothing.
//!
//! In an open chat the swipe that leaves it is the opposite of the one that opened it (#458): a DM opens with a swipe right from the
//! DMs, so a swipe left goes back to the DMs; a server channel opens with a swipe left from Servers, so a swipe right goes back to Servers.
//!
//! Which way is which is the one pair of constants below.

/// The way the finger moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finger {
    Left,
    Right,
}

/// The finger direction that leads towards the DMs (and, from the DMs, on to their last chat): a swipe right.
pub const TOWARDS_DMS: Finger = Finger::Right;
/// The finger direction that leads towards the servers (and, from Servers, on to their last channel): a swipe left.
pub const TOWARDS_SERVERS: Finger = Finger::Left;

/// The list on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum List {
    Servers,
    Dms,
}

/// What a swipe does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Show the other list.
    Show(List),
    /// Open this room's chat.
    Open(String),
    /// Nothing to go to.
    Nothing,
}

/// The chats opened last: one DM and one server channel (a room that is not a DM), kept across launches.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LastChats {
    pub dm: Option<String>,
    pub channel: Option<String>,
}

impl LastChats {
    /// From what [`Self::encode`] made (anything else: nothing kept).
    pub fn parse(text: &str) -> Self {
        let mut lines = text.lines().map(|l| Some(l.to_owned()).filter(|l| !l.is_empty()));
        Self { dm: lines.next().flatten(), channel: lines.next().flatten() }
    }

    /// One line for the DM, one for the channel.
    pub fn encode(&self) -> String {
        format!("{}\n{}", self.dm.as_deref().unwrap_or(""), self.channel.as_deref().unwrap_or(""))
    }

    /// A chat was opened: it is the last of its kind.
    pub fn opened(&mut self, room: &str, is_dm: bool) {
        let slot = if is_dm { &mut self.dm } else { &mut self.channel };
        *slot = Some(room.to_owned());
    }

    /// What a swipe on `list` does.
    pub fn swipe(&self, list: List, finger: Finger) -> Step {
        let chat = |room: &Option<String>| room.clone().map_or(Step::Nothing, Step::Open);
        match (list, finger == TOWARDS_DMS) {
            (List::Servers, true) => Step::Show(List::Dms),
            (List::Dms, true) => chat(&self.dm),
            (List::Dms, false) => Step::Show(List::Servers),
            (List::Servers, false) => chat(&self.channel),
        }
    }
}

/// In an open chat, does this swipe leave it for its list? A DM goes back on the swipe that leads to the servers, a channel on the one
/// that leads to the DMs, each the opposite of the swipe that opened it.
pub fn leaves_chat(is_dm: bool, finger: Finger) -> bool {
    if is_dm { finger == TOWARDS_SERVERS } else { finger == TOWARDS_DMS }
}

/// The list a chat goes back to.
pub fn list_of_chat(is_dm: bool) -> List {
    if is_dm { List::Dms } else { List::Servers }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept() -> LastChats {
        let mut last = LastChats::default();
        last.opened("!dm:x", true);
        last.opened("!chan:x", false);
        last
    }

    #[test]
    fn the_four_swipes_go_where_the_finger_moves() {
        let last = kept();
        // Swipe right: the finger moves from the left to the right.
        assert_eq!(last.swipe(List::Servers, Finger::Right), Step::Show(List::Dms));
        assert_eq!(last.swipe(List::Dms, Finger::Right), Step::Open("!dm:x".into()));
        // Swipe left: the other way.
        assert_eq!(last.swipe(List::Dms, Finger::Left), Step::Show(List::Servers));
        assert_eq!(last.swipe(List::Servers, Finger::Left), Step::Open("!chan:x".into()));
    }

    #[test]
    fn with_no_last_chat_the_chat_swipes_do_nothing_but_the_list_swipes_still_work() {
        let none = LastChats::default();
        assert_eq!(none.swipe(List::Dms, Finger::Right), Step::Nothing);
        assert_eq!(none.swipe(List::Servers, Finger::Left), Step::Nothing);
        assert_eq!(none.swipe(List::Servers, Finger::Right), Step::Show(List::Dms));
        assert_eq!(none.swipe(List::Dms, Finger::Left), Step::Show(List::Servers));
        let mut dm_only = LastChats::default();
        dm_only.opened("!dm:x", true);
        assert_eq!(dm_only.swipe(List::Servers, Finger::Left), Step::Nothing, "a DM is not a server channel");
    }

    #[test]
    fn dms_and_channels_are_tracked_apart_and_survive_a_restart() {
        let mut last = kept();
        last.opened("!dm2:x", true);
        assert_eq!((last.dm.as_deref(), last.channel.as_deref()), (Some("!dm2:x"), Some("!chan:x")));
        assert_eq!(LastChats::parse(&last.encode()), last);
        let only_channel = LastChats { dm: None, channel: Some("!c:x".into()) };
        assert_eq!(LastChats::parse(&only_channel.encode()), only_channel);
        assert_eq!(LastChats::parse(""), LastChats::default());
        assert_eq!(LastChats::parse("garbage"), LastChats { dm: Some("garbage".into()), channel: None });
    }

    #[test]
    fn a_swipe_in_a_chat_goes_back_to_its_list_the_way_opposite_to_the_one_that_opened_it() {
        assert!(leaves_chat(true, Finger::Left));
        assert!(!leaves_chat(true, Finger::Right));
        assert_eq!(list_of_chat(true), List::Dms);
        assert!(leaves_chat(false, Finger::Right));
        assert!(!leaves_chat(false, Finger::Left));
        assert_eq!(list_of_chat(false), List::Servers);
        // Each is the reverse of the swipe that opens it from its list.
        let last = kept();
        assert_eq!(last.swipe(List::Dms, Finger::Right), Step::Open("!dm:x".into()));
        assert_eq!(last.swipe(List::Servers, Finger::Left), Step::Open("!chan:x".into()));
    }
}
