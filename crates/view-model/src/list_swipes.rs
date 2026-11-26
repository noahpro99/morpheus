//! The phone's swipes between its two lists (Servers and DMs) and the last chat of each (#389, directions fixed in #394).
//!
//! "Swipe right" is the finger moving from the left of the screen to the right, "swipe left" the other way.
//!
//! - Servers, swipe right: show the DMs. Servers, swipe left: open the last server channel's chat.
//! - DMs, swipe right: open the last DM's chat. DMs, swipe left: go back to Servers.
//!
//! A swipe that would open a chat when none was kept does nothing.
//!
//! In an open chat a swipe either way leaves it for its list, the chat following the finger (#469; it was only the opposite of the one that
//! opened it, #458).
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


/// The screen edge a chat sits at: it comes in from there when it opens and goes back out to it when it closes (#464). The rule is that
/// whatever a swipe opens or closes moves the way the finger moves: a DM opens with a swipe right, so it comes in from the left edge and
/// a swipe left sends it back there; a server channel opens with a swipe left, so it sits at the right edge.
pub fn chat_edge(is_dm: bool) -> Finger {
    // The chat travels with the finger that opens it, from the side the finger came from.
    opposite(if is_dm { TOWARDS_DMS } else { TOWARDS_SERVERS })
}

/// The screen edge a list sits at, beside the other: the DMs on the left, Servers on the right (the bottom bar's order). A swipe
/// towards the DMs brings them in from the left edge.
pub fn list_edge(list: List) -> Finger {
    match list {
        List::Dms => opposite(TOWARDS_DMS),
        List::Servers => opposite(TOWARDS_SERVERS),
    }
}

fn opposite(finger: Finger) -> Finger {
    match finger {
        Finger::Left => Finger::Right,
        Finger::Right => Finger::Left,
    }
}

/// Does a drag of `distance` pixels (either way) across a pane `width` pixels wide go through, or spring back? A tenth of the width (#486).
pub fn swipe_commits(distance: f32, width: f32) -> bool {
    distance.abs() > width / 10.0
}

/// [`swipe_commits`], or a quick flick: the finger left going `velocity` pixels a second the same way as the drag, faster than two fifths of a
/// pane width in a second, goes through after any drag past the touch slop (a fortieth of the width here).
pub fn swipe_commits_with_velocity(distance: f32, velocity: f32, width: f32) -> bool {
    swipe_commits(distance, width) || (distance.abs() > width / 40.0 && velocity * distance > 0.0 && velocity.abs() > width * 0.4)
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
    fn a_chat_or_a_list_moves_the_way_the_finger_moves() {
        // A DM opens with a swipe right and so sits at the left edge; a channel opens with a swipe left and sits at the right.
        assert_eq!(chat_edge(true), Finger::Left);
        assert_eq!(chat_edge(false), Finger::Right);
        for is_dm in [true, false] {
            let edge = chat_edge(is_dm);
            // The swipe that sends a chat away moves it towards its own edge.
            // The one that opens it moves away from there.
            let opening = if is_dm { TOWARDS_DMS } else { TOWARDS_SERVERS };
            assert_ne!(opening, edge);
            assert!(matches!(kept().swipe(list_of_chat(is_dm), opening), Step::Open(_)));
        }
        // The lists: a swipe towards the DMs brings them in from the left, one towards Servers brings Servers in from the right.
        assert_eq!(list_edge(List::Dms), Finger::Left);
        assert_eq!(list_edge(List::Servers), Finger::Right);
    }

    #[test]
    fn a_drag_goes_through_past_a_tenth_of_the_width_or_on_a_quick_flick() {
        assert!(swipe_commits(120.0, 1080.0));
        assert!(swipe_commits(-120.0, 1080.0));
        assert!(!swipe_commits(100.0, 1080.0));
        // A flick commits a short drag; a slow one, a tiny one, or one lifted going the other way does not.
        assert!(swipe_commits_with_velocity(60.0, 2000.0, 1080.0));
        assert!(swipe_commits_with_velocity(-60.0, -2000.0, 1080.0));
        assert!(!swipe_commits_with_velocity(60.0, 300.0, 1080.0));
        assert!(!swipe_commits_with_velocity(10.0, 2000.0, 1080.0));
        assert!(!swipe_commits_with_velocity(60.0, -2000.0, 1080.0));
    }

    #[test]
    fn a_swipe_in_a_chat_either_way_goes_back_to_its_list() {
        // Either swipe leaves a chat (#469); it goes back to the list it was opened from.
        assert_eq!(list_of_chat(true), List::Dms);
        assert_eq!(list_of_chat(false), List::Servers);
        // Each list's own swipe still opens its last chat.
        let last = kept();
        assert_eq!(last.swipe(List::Dms, Finger::Right), Step::Open("!dm:x".into()));
        assert_eq!(last.swipe(List::Servers, Finger::Left), Step::Open("!chan:x".into()));
    }
}
