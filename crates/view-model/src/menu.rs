//! What a right-click menu offers, as `crates/ui/src/components/context_menu.rs`
//! and the menus built in `timeline.rs`, `room_list.rs` and `members.rs`
//! decide it: which items show for which message (own or not, sent or still
//! sending, reacted to, pinned) and what each does. Items carry a
//! [`MenuAction`] (data), not a closure, so this is testable and the front
//! end only has to draw the menu and run the action it is handed.

use morpheus_core::{
    MemberRole, Message, MessageId, NotifyMode, OwnedRoomId, OwnedUserId, RoomSummary,
};

/// The reactions a message's hover bar and menu offer at once.
pub const QUICK_REACTIONS: [&str; 3] = ["👍", "❤️", "😂"];

/// The "Add reaction" grid of the message menu.
pub const EMOJI_PICKER: [&str; 24] = [
    "👍", "👎", "❤️", "😂", "😮", "😢", "😡", "🎉", "🔥", "👀", "🙏", "👏", "💯", "✅", "❌", "🤔",
    "😎", "🥳", "🚀", "⭐", "💀", "🫡", "🤝", "💡",
];

/// What choosing an item does. The front end holds what the menu was opened
/// on (a message, a room), so an action names only what it adds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuAction {
    // On a message.
    Reply,
    ReplyInThread,
    Edit,
    /// Toggles this reaction.
    React(String),
    ViewReactions,
    Forward(OwnedRoomId),
    /// Pin (true) or unpin the message.
    Pin(bool),
    MarkUnread,
    /// Touch screens: the text shown where holding selects it.
    SelectText(String),
    CopyText(String),
    CopyLink(String),
    CopyId(String),
    Report,
    Delete,
    /// Takes back each of these (our own) reactions.
    RemoveReactions(Vec<String>),

    // On rooms, servers and people (`room_menu`).
    /// Reads a room (or, for a server, each of its rooms).
    MarkRead(Vec<OwnedRoomId>),
    MarkRoomUnread(OwnedRoomId),
    Notify(Vec<OwnedRoomId>, NotifyMode),
    /// Leaves a room, undoably: its name for the toast.
    Leave(OwnedRoomId, String),
    Favorite(OwnedRoomId, bool),
    PinDm(OwnedRoomId, bool),
    CloseDm(OwnedRoomId),
    Ignore(OwnedUserId, bool),
    Block(OwnedUserId, bool),
    /// Mutes a room for this many milliseconds (0: until turned back on).
    Mute(OwnedRoomId, i64),
    Unmute(OwnedRoomId),
    Friend(OwnedUserId, bool),
    Invite(OwnedRoomId, OwnedUserId),
    /// Show or hide muted channels in the lists.
    ToggleHideMuted,
    /// Moves a server into a folder of the rail, a new one, or out of folders.
    MoveToFolder(OwnedRoomId, String),
    MoveToNewFolder(OwnedRoomId),
    MoveOutOfFolders(OwnedRoomId),
    Profile(OwnedUserId),

    // On a profile card (`profile`).
    /// Swaps the card for the full profile.
    FullProfile(OwnedUserId),
    /// Copies the profile's link and says so.
    ShareProfile(String),
    ReportUser(OwnedUserId),

    // On a member (`members`).
    /// Adds `@name ` to the message being written.
    Mention(String),
    /// Opens the direct message with them (making it if need be).
    Message(OwnedUserId),
    SetRole(OwnedRoomId, OwnedUserId, MemberRole),
    Kick(OwnedRoomId, OwnedUserId),
    Ban(OwnedRoomId, OwnedUserId),
    /// Opens a room dialog (invite, edit, create, privacy, leave a server).
    RoomDialog(super::room_dialogs::RoomDialogKind),
    /// "Create channel" on a server: lists that server, then asks for the room.
    CreateChannel(OwnedRoomId),

    // On the call devices' quick lists (`calls::device_menu`).
    /// Use this device (`None`: the system default) for calls.
    Device(DeviceKind, Option<String>),
    /// Opens Settings on Voice & Video.
    VoiceSettings,
}

/// Which device a call's quick list picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    Microphone,
    Speaker,
    Camera,
}

/// One row of a menu.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuItem {
    pub label: String,
    /// `None` for a row that only opens a flyout.
    pub action: Option<MenuAction>,
    pub danger: bool,
    /// Shown with a check mark (the current choice in a submenu).
    pub checked: bool,
    /// A flyout opened by hovering the row.
    pub children: Vec<MenuItem>,
    /// Lay the flyout out as a grid (emoji).
    pub grid: bool,
    /// Lucide glyph name in front of the label; falls back to [`default_icon`].
    pub icon: Option<&'static str>,
    /// A picture in front of the label instead of a glyph (rooms, servers):
    /// the name its letter comes from.
    pub avatar: Option<String>,
}

impl MenuItem {
    pub fn new(label: impl Into<String>, action: MenuAction) -> Self {
        Self {
            label: label.into(),
            action: Some(action),
            danger: false,
            checked: false,
            children: Vec::new(),
            grid: false,
            icon: None,
            avatar: None,
        }
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// Shows the room's (or server's) picture, its letter for now.
    pub fn avatar(mut self, name: impl Into<String>) -> Self {
        self.avatar = Some(name.into());
        self
    }

    /// A row that opens `children` in a flyout.
    pub fn submenu(label: impl Into<String>, children: Vec<MenuItem>) -> Self {
        Self {
            action: None,
            children,
            ..Self::new(label, MenuAction::Reply)
        }
    }

    /// A submenu whose children are short labels in a grid.
    pub fn grid(label: impl Into<String>, children: Vec<MenuItem>) -> Self {
        Self {
            grid: true,
            ..Self::submenu(label, children)
        }
    }

    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    pub fn icon(mut self, glyph: &'static str) -> Self {
        self.icon = Some(glyph);
        self
    }

    /// The glyph drawn in front of the label, if any.
    pub fn glyph(&self) -> Option<&'static str> {
        if self.avatar.is_some() {
            return None;
        }
        self.icon.or_else(|| default_icon(&self.label))
    }

    pub fn has_submenu(&self) -> bool {
        !self.children.is_empty()
    }
}

/// What a menu was opened on.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuTarget {
    Message {
        message: Box<Message>,
        room: Option<OwnedRoomId>,
    },
    /// A room, server or person: the actions say which.
    Other,
}

/// A room a message can be forwarded to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForwardTarget {
    pub id: OwnedRoomId,
    /// `#name`, or `@name` for a direct message.
    pub label: String,
    pub dm: bool,
}

/// Every room but spaces and the one the message is in.
pub fn forward_targets(rooms: &[RoomSummary], current: &OwnedRoomId) -> Vec<ForwardTarget> {
    rooms
        .iter()
        .filter(|room| !room.is_space && &room.id != current)
        .map(|room| ForwardTarget {
            id: room.id.clone(),
            label: if room.is_dm {
                format!("@{}", room.name)
            } else {
                format!("#{}", room.name)
            },
            dm: room.is_dm,
        })
        .collect()
}

/// A message's link, as matrix.to makes it.
pub fn message_link(room: &OwnedRoomId, event_id: &str) -> String {
    format!("https://matrix.to/#/{room}/{event_id}")
}

/// Whether a menu on `message` has the quick-reaction row on top.
pub fn offers_quick_reactions(message: &Message) -> bool {
    !message.is_local_echo
}

/// The items of the menu on `message` in `room`: the hover bar's actions,
/// plus reactions, forwarding, pinning, copying, reporting and deleting.
/// `pinned` is whether the message is pinned now; `touch` adds "Select
/// text" (holding opens this menu on a phone).
pub fn message_menu(
    message: &Message,
    room: Option<&OwnedRoomId>,
    pinned: bool,
    targets: &[ForwardTarget],
    touch: bool,
) -> Vec<MenuItem> {
    let can_reply = matches!(message.id, MessageId::Event(_)) && !message.is_local_echo;
    let mut items = Vec::new();
    if can_reply {
        items.push(MenuItem::new("Reply", MenuAction::Reply));
        items.push(MenuItem::new("Reply in thread", MenuAction::ReplyInThread));
    }
    if message.can_edit {
        items.push(MenuItem::new("Edit message", MenuAction::Edit));
    }
    // Everything below needs a sent event in a known room.
    let sent = match (&message.id, room) {
        (MessageId::Event(event_id), Some(room)) if !message.is_local_echo => {
            Some((event_id.to_string(), room))
        }
        _ => None,
    };
    if sent.is_some() {
        let grid = EMOJI_PICKER
            .iter()
            .map(|key| MenuItem::new(*key, MenuAction::React((*key).to_owned())))
            .collect();
        items.push(MenuItem::grid("Add reaction", grid));
        if !message.reactions.is_empty() {
            items.push(MenuItem::new("View reactions", MenuAction::ViewReactions));
        }
    }
    if sent.is_some() {
        let forward: Vec<MenuItem> = targets
            .iter()
            .map(|target| {
                MenuItem::new(target.label.clone(), MenuAction::Forward(target.id.clone()))
                    .icon(if target.dm { "at-sign" } else { "hash" })
            })
            .collect();
        if !forward.is_empty() {
            items.push(MenuItem::submenu("Forward to", forward));
        }
        items.push(MenuItem::new(
            if pinned {
                "Unpin message"
            } else {
                "Pin message"
            },
            MenuAction::Pin(!pinned),
        ));
        items.push(MenuItem::new("Mark unread", MenuAction::MarkUnread));
    }
    if let Some(text) = message.kind.plain_body().filter(|t| !t.is_empty()) {
        // Holding opens this menu on a phone, so selecting needs a way in.
        if touch {
            items.push(MenuItem::new(
                "Select text",
                MenuAction::SelectText(text.to_owned()),
            ));
        }
        items.push(MenuItem::new(
            "Copy text",
            MenuAction::CopyText(text.to_owned()),
        ));
    }
    if let Some((event_id, room)) = &sent {
        items.push(MenuItem::new(
            "Copy message link",
            MenuAction::CopyLink(message_link(room, event_id)),
        ));
    }
    // Only our own reactions can be taken back.
    let mine: Vec<String> = message
        .reactions
        .iter()
        .filter(|r| r.mine)
        .map(|r| r.key.clone())
        .collect();
    if !mine.is_empty() {
        let each = mine
            .iter()
            .map(|key| MenuItem::new(key.clone(), MenuAction::React(key.clone())))
            .collect();
        items.push(MenuItem::submenu("Remove reactions", each));
        items.push(MenuItem::new(
            "Remove all my reactions",
            MenuAction::RemoveReactions(mine),
        ));
    }
    if sent.is_some() && !message.is_own {
        items.push(MenuItem::new("Report message", MenuAction::Report));
    }
    if message.is_own && can_reply {
        items.push(MenuItem::new("Delete message", MenuAction::Delete).danger());
    }
    if let Some((event_id, _)) = sent {
        items.push(MenuItem::new(
            "Copy message ID",
            MenuAction::CopyId(event_id),
        ));
    }
    items
}

/// Glyphs by label, so every menu gets icons without each builder naming
/// them. A label starting with one of these gets its glyph ("Kick carol").
pub fn default_icon(label: &str) -> Option<&'static str> {
    const TABLE: &[(&str, &str)] = &[
        ("Add reaction", "smile-plus"),
        ("View reactions", "eye"),
        ("Remove reactions", "circle-minus"),
        ("Remove all my reactions", "circle-minus"),
        ("Reply in thread", "messages-square"),
        ("Reply", "reply"),
        ("Edit message", "pencil"),
        ("Edit room", "pencil"),
        ("Privacy & encryption", "lock"),
        ("Edit profile", "pencil"),
        ("Copy text", "copy"),
        ("Copy message link", "link"),
        ("Copy link", "link"),
        ("Copy message ID", "hash"),
        ("Copy room ID", "hash"),
        ("Copy user ID", "hash"),
        ("Forward to", "forward"),
        ("Unpin message", "pin-off"),
        ("Pin message", "pin"),
        ("Mark unread", "mail"),
        ("Mark as read", "check-check"),
        ("Mark as unread", "mail"),
        ("Unpin", "pin-off"),
        ("Pin", "pin"),
        ("Add to favorites", "star"),
        ("Remove from favorites", "star"),
        ("Change friend nickname", "pencil"),
        ("Close DM", "x"),
        ("Unignore", "eye"),
        ("Ignore", "eye-off"),
        ("Unmute", "bell"),
        ("Voice & Video settings", "settings"),
        ("Report message", "flag"),
        ("Delete message", "trash-2"),
        ("View profile", "user"),
        ("Profile", "user"),
        ("Mention", "at-sign"),
        ("Set nickname…", "pencil"),
        ("Add friend", "user-plus"),
        ("Remove friend", "user-x"),
        ("Message", "message-circle"),
        ("Invite to room", "user-plus"),
        ("Invite people", "user-plus"),
        ("Unblock", "ban"),
        ("Block", "ban"),
        ("Roles", "shield"),
        ("Kick", "user-x"),
        ("Ban", "gavel"),
        ("Notifications", "bell"),
        ("Create room", "plus"),
        ("Leave room", "log-out"),
        ("Leave server", "log-out"),
        ("Set status", "smile"),
        ("Switch account", "users"),
        ("Sign out", "log-out"),
        ("All messages", "bell"),
        ("Only mentions", "at-sign"),
        ("Mute", "bell-off"),
        ("Member", "user"),
        ("Moderator", "shield"),
        ("Admin", "shield-check"),
    ];
    TABLE
        .iter()
        .find(|(prefix, _)| label.starts_with(prefix))
        .map(|(_, glyph)| *glyph)
}

/// What a confirmation dialog says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirm {
    pub title: String,
    pub body: String,
    /// The button that goes ahead.
    pub confirm: String,
}

/// What a confirmed dialog goes ahead with.
#[derive(Clone, Debug, PartialEq)]
pub enum ConfirmAction {
    DeleteMessage(Box<Message>),
}

/// "Are you sure?" before deleting a message.
pub fn confirm_delete() -> Confirm {
    Confirm {
        title: "Delete message".into(),
        body: "Are you sure you want to delete this message? Tip: hold Shift when you click the bin to skip this.".into(),
        confirm: "Delete".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use morpheus_core::{MessageKind, OwnedEventId, OwnedUserId, Reaction};

    fn message(own: bool) -> Message {
        Message {
            unique_id: "u1".into(),
            id: MessageId::Event(OwnedEventId::try_from("$e:x").expect("event id")),
            sender: OwnedUserId::try_from("@bob:x").expect("user id"),
            sender_name: "bob".into(),
            sender_avatar: None,
            timestamp_ms: 0,
            is_own: own,
            is_local_echo: false,
            is_edited: false,
            can_edit: own,
            reactions: Vec::new(),
            in_reply_to: None,
            thread_root: None,
            thread: None,
            kind: MessageKind::Text {
                body: "hello".into(),
                html: None,
            },
            shield: None,
        }
    }

    fn room() -> OwnedRoomId {
        OwnedRoomId::try_from("!r:x").expect("room id")
    }

    fn labels(items: &[MenuItem]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_str()).collect()
    }

    fn target(name: &str, dm: bool) -> ForwardTarget {
        ForwardTarget {
            id: OwnedRoomId::try_from(format!("!{name}:x")).expect("room id"),
            label: format!("{}{name}", if dm { "@" } else { "#" }),
            dm,
        }
    }

    #[test]
    fn an_own_message_offers_edit_and_delete_but_not_report() {
        let items = message_menu(&message(true), Some(&room()), false, &[target("random", false)], false);
        assert_eq!(
            labels(&items),
            [
                "Reply",
                "Reply in thread",
                "Edit message",
                "Add reaction",
                "Forward to",
                "Pin message",
                "Mark unread",
                "Copy text",
                "Copy message link",
                "Delete message",
                "Copy message ID",
            ]
        );
        assert!(items.iter().find(|i| i.label == "Delete message").is_some_and(|i| i.danger));
    }

    #[test]
    fn someone_elses_message_offers_report_not_edit_or_delete() {
        let items = message_menu(&message(false), Some(&room()), false, &[], false);
        let labels = labels(&items);
        assert!(labels.contains(&"Report message"));
        assert!(!labels.contains(&"Edit message"));
        assert!(!labels.contains(&"Delete message"));
        assert!(!labels.contains(&"Forward to"), "no rooms to forward to");
    }

    #[test]
    fn a_message_still_sending_can_only_be_edited_or_copied() {
        let mut m = message(true);
        m.is_local_echo = true;
        m.id = MessageId::Transaction("t".try_into().expect("txn"));
        m.can_edit = false;
        let items = message_menu(&m, Some(&room()), false, &[], false);
        assert_eq!(labels(&items), ["Copy text"]);
        assert!(!offers_quick_reactions(&m));
    }

    #[test]
    fn pinned_messages_offer_unpin_and_the_action_flips() {
        let items = message_menu(&message(false), Some(&room()), true, &[], false);
        let pin = items.iter().find(|i| i.label == "Unpin message").expect("unpin");
        assert_eq!(pin.action, Some(MenuAction::Pin(false)));
        let items = message_menu(&message(false), Some(&room()), false, &[], false);
        let pin = items.iter().find(|i| i.label == "Pin message").expect("pin");
        assert_eq!(pin.action, Some(MenuAction::Pin(true)));
    }

    #[test]
    fn reactions_add_viewing_and_taking_back_only_our_own() {
        let mut m = message(false);
        m.reactions = vec![
            Reaction {
                key: "🎉".into(),
                count: 2,
                mine: true,
                senders: Vec::new(),
            },
            Reaction {
                key: "👍".into(),
                count: 1,
                mine: false,
                senders: Vec::new(),
            },
        ];
        let items = message_menu(&m, Some(&room()), false, &[], false);
        let names = labels(&items);
        assert!(names.contains(&"View reactions"));
        let all = items
            .iter()
            .find(|i| i.label == "Remove all my reactions")
            .expect("remove all");
        assert_eq!(all.action, Some(MenuAction::RemoveReactions(vec!["🎉".into()])));
        let each = items
            .iter()
            .find(|i| i.label == "Remove reactions")
            .expect("remove one");
        assert_eq!(labels(&each.children), ["🎉"]);
    }

    #[test]
    fn the_grid_offers_the_common_emoji_and_the_link_names_the_message() {
        let items = message_menu(&message(false), Some(&room()), false, &[], false);
        let add = items.iter().find(|i| i.label == "Add reaction").expect("add");
        assert!(add.grid && add.has_submenu());
        assert_eq!(add.children.len(), EMOJI_PICKER.len());
        let link = items
            .iter()
            .find(|i| i.label == "Copy message link")
            .expect("link");
        assert_eq!(
            link.action,
            Some(MenuAction::CopyLink("https://matrix.to/#/!r:x/$e:x".into()))
        );
    }

    #[test]
    fn without_a_known_room_nothing_needs_one() {
        let items = message_menu(&message(false), None, false, &[], false);
        assert_eq!(labels(&items), ["Reply", "Reply in thread", "Copy text"]);
    }

    #[test]
    fn touch_adds_select_text_before_copy() {
        let items = message_menu(&message(false), None, false, &[], true);
        assert_eq!(
            labels(&items),
            ["Reply", "Reply in thread", "Select text", "Copy text"]
        );
    }

    #[test]
    fn forward_targets_skip_spaces_and_the_current_room() {
        let mk = |id: &str, name: &str| crate::test_room::room(id, name);
        let mut space = mk("!s:x", "Space");
        space.is_space = true;
        let mut dm = mk("!d:x", "carol");
        dm.is_dm = true;
        let rooms = [mk("!r:x", "here"), mk("!o:x", "other"), space, dm];
        let targets = forward_targets(&rooms, &room());
        let names: Vec<_> = targets.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(names, ["#other", "@carol"]);
    }

    #[test]
    fn labels_get_icons_by_prefix() {
        assert_eq!(default_icon("Kick carol"), Some("user-x"));
        assert_eq!(default_icon("Reply in thread"), Some("messages-square"));
        assert_eq!(default_icon("Reply"), Some("reply"));
        assert_eq!(default_icon("🎉"), None);
    }
}
