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
    /// An image message: its picture itself onto the clipboard (desktop).
    CopyImage,
    CopyLink(String),
    CopyId(String),
    /// A message carries a theme code (`theme_share`): open it in Settings to preview.
    ApplyTheme(String),
    Report,
    Delete,
    /// Takes back each of these (our own) reactions.
    RemoveReactions(Vec<String>),

    // On rooms, servers and people (`room_menu`).
    /// Reads a room (or, for a server, each of its rooms).
    MarkRead(Vec<OwnedRoomId>),
    MarkRoomUnread(OwnedRoomId),
    /// Close an unlocked, password-locked room at once.
    LockNow(OwnedRoomId),
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
    /// The server's welcome screen again (its overview).
    ServerOverview(OwnedRoomId),
    /// Breaks a rail folder up: its servers stay, in its place.
    Ungroup(String),
    Profile(OwnedUserId),

    // On the footer chip's account menu (`account_menu`).
    /// Makes the stored account with this key the active one.
    SwitchAccount(String),
    /// Signs in another account without signing out of this one.
    AddAccount,
    /// Asks, then signs this account out.
    SignOut,

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
    /// Noise suppression mode, by its key (off, standard, strong).
    Noise(String),
    /// Opens Settings on Voice & Video.
    VoiceSettings,

    // On a person's card in a call (`calls::call_card_items`).
    /// Silences (true) or restores their voice for you alone.
    CallMute(OwnedUserId, bool),
    /// Silences (true) or restores the sound of their shared screen for you.
    CallMuteStream(OwnedUserId, bool),
    /// Stops watching their shared screen (it returns to a card to Watch).
    CallStopWatching(OwnedUserId),
    /// Focuses the card with this key, or (when focused already) puts it back.
    CallFocus(String),
    /// Hides (or shows again) this person's camera on their card.
    CallHideVideo(OwnedUserId),
    /// Starts (true) or stops sharing this device's screen in the call (`calls::call_menu`).
    CallShareScreen(bool),
    /// Join this voice channel's call (the palette's "Join voice: <channel>").
    JoinVoice(OwnedRoomId),
}

/// Which device a call's quick list picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    Microphone,
    Speaker,
    Camera,
}

/// An interactive slider in a menu row (e.g. input / output volume).
#[derive(Clone, Debug, PartialEq)]
pub enum MenuSlider {
    InputVolume(f32),
    OutputVolume(f32),
    /// How loud one person's voice plays for you.
    UserVolume(OwnedUserId, f32),
    /// How loud the sound of one person's shared screen plays for you.
    StreamVolume(OwnedUserId, f32),
}

impl MenuSlider {
    pub fn value(&self) -> f32 {
        match self {
            Self::InputVolume(v) | Self::OutputVolume(v) | Self::UserVolume(_, v) | Self::StreamVolume(_, v) => *v,
        }
    }
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
    /// A background for the row (an account's own colour in the account switcher); its
    /// text follows the contrast rule.
    pub fill: Option<u32>,
    /// An interactive slider in the row (e.g. volume).
    pub slider: Option<MenuSlider>,
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
            fill: None,
            slider: None,
        }
    }

    pub fn slider(label: impl Into<String>, slider: MenuSlider) -> Self {
        Self {
            label: label.into(),
            action: None,
            danger: false,
            checked: false,
            children: Vec::new(),
            grid: false,
            icon: None,
            avatar: None,
            fill: None,
            slider: Some(slider),
        }
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// Draws the row on this colour.
    pub fn fill(mut self, colour: u32) -> Self {
        self.fill = Some(colour);
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
        if crate::theme_share::find_in_message(text).is_some() {
            items.push(MenuItem::new("Apply this theme", MenuAction::ApplyTheme(text.to_owned())));
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
        ("Apply this theme", "palette"),
        ("Copy text", "copy"),
        ("Copy image", "copy"),
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
        ("Add an account", "user-plus"),
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

pub fn default_icon(label: &str) -> Option<&'static str> {
    TABLE
        .iter()
        .find(|(prefix, _)| label.starts_with(prefix))
        .map(|(_, glyph)| *glyph)
}

/// The menu labels that have a translation of their own (the table above).
pub fn translatable_labels() -> Vec<&'static str> {
    TABLE.iter().map(|(label, _)| *label).collect()
}

/// A menu label in the language in use: a known label whole ("Reply"), or one that starts
/// with a known label and goes on with a name ("Kick carol" is "Expulsar carol"), else as is.
pub fn translated_label(label: &str) -> String {
    translated_label_in(crate::i18n::language(), label)
}

pub fn translated_label_in(code: &str, label: &str) -> String {
    if let Some((prefix, _)) = TABLE.iter().find(|(prefix, _)| label.starts_with(prefix)) {
        let rest = &label[prefix.len()..];
        // A prefix only counts at a word boundary ("Message" is not the start of "Messages").
        if rest.is_empty() || rest.starts_with(' ') {
            return format!("{}{rest}", crate::i18n::t_in(code, prefix));
        }
    }
    crate::i18n::t_in(code, label)
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
    SignOut,
    /// Open a room while the screen is shared (streamer mode asked).
    OpenRoom(OwnedRoomId),
    /// Open a server's welcome screen while the screen is shared (it lists
    /// its channels).
    OpenWelcome(OwnedRoomId),
}

/// "Are you sure?" before deleting a message.
pub fn confirm_delete() -> Confirm {
    Confirm {
        title: "Delete message".into(),
        body: "Are you sure you want to delete this message? Tip: hold Shift when you click the bin to skip this.".into(),
        confirm: "Delete".into(),
    }
}

/// What the footer chip's menu offers: your profile, its editors, the
/// accounts stored on this device (the signed-in one ticked) and adding
/// another, and signing out.
pub fn account_menu(own: &OwnedUserId, accounts: &[morpheus_core::SavedAccount]) -> Vec<MenuItem> {
    let mut switch: Vec<MenuItem> = accounts
        .iter()
        .map(|account| {
            let label = account.user_id.to_string();
            MenuItem::new(label.clone(), MenuAction::SwitchAccount(account.key.clone()))
                .avatar(label.trim_start_matches('@').to_owned())
                .checked(&account.user_id == own)
        })
        .collect();
    switch.push(MenuItem::new("Add an account", MenuAction::AddAccount).icon("user-plus"));
    vec![
        MenuItem::new("View profile", MenuAction::Profile(own.clone())),
        MenuItem::new("Edit profile", MenuAction::RoomDialog(super::room_dialogs::RoomDialogKind::EditProfile)),
        MenuItem::new("Set status", MenuAction::RoomDialog(super::room_dialogs::RoomDialogKind::SetStatus)),
        MenuItem::submenu("Switch account", switch),
        MenuItem::new("Sign out", MenuAction::SignOut).danger(),
    ]
}

/// "Are you sure?" before signing out.
pub fn confirm_sign_out() -> Confirm {
    Confirm {
        title: "Sign out".into(),
        body: "Are you sure you want to sign out? You will need your password (or single sign-on) to sign back in here.".into(),
        confirm: "Sign out".into(),
    }
}

/// A box in window pixels: where it is and how big.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Frame {
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
}

/// Where a flyout of `size` (w, h) goes beside its `row`, inside a window of
/// `window` (w, h) kept `margin` from the edges. It opens to the right of the
/// row, on its left when the right has no room, and lines its top with the
/// row's; when it would run off the bottom it moves up (its bottom lines with
/// the row's), and is clamped inside the window as a last resort. Returns the
/// flyout's top-left.
pub fn place_flyout(row: Frame, size: (f32, f32), window: (f32, f32), margin: f32) -> (f32, f32) {
    const GAP: f32 = 2.0;
    const LIFT: f32 = 6.0;
    let (w, h) = size;
    let right = row.right() + GAP;
    let left = row.x - GAP - w;
    let x = if right + w + margin <= window.0 || left < margin {
        right
    } else {
        left
    };
    let mut y = row.y - LIFT;
    if y + h + margin > window.1 {
        y = row.bottom() + LIFT - h;
    }
    (clamp_into(x, w, window.0, margin), clamp_into(y, h, window.1, margin))
}

/// Where a menu of `size` goes when opened at the pointer `at`: right of and
/// below it, flipped to its left and above when there is no room, then
/// clamped inside the window.
pub fn place_menu(at: (f32, f32), size: (f32, f32), window: (f32, f32), margin: f32) -> (f32, f32) {
    let x = if at.0 + size.0 + margin > window.0 { at.0 - size.0 } else { at.0 };
    let y = if at.1 + size.1 + margin > window.1 { at.1 - size.1 } else { at.1 };
    (clamp_into(x, size.0, window.0, margin), clamp_into(y, size.1, window.1, margin))
}

/// `pos` moved so a span of `len` fits in `0..limit` with `margin` to spare
/// (the start edge wins when it cannot).
fn clamp_into(pos: f32, len: f32, limit: f32, margin: f32) -> f32 {
    pos.min(limit - margin - len).max(margin)
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
            mentions_me: false,
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
            read_by: Vec::new(),
            state_kind: None,
            state_who: None,
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
    fn a_message_with_a_theme_code_offers_to_apply_it_and_others_do_not() {
        let offered = |m: &Message| {
            labels(&message_menu(m, Some(&room()), false, &[], false)).contains(&"Apply this theme")
        };
        assert!(!offered(&message(false)));
        let mut shared = message(false);
        shared.kind = MessageKind::Text { body: "mine: morpheus-theme/1;theme=light".into(), html: None };
        assert!(offered(&shared));
        let items = message_menu(&shared, Some(&room()), false, &[], false);
        let item = items.iter().find(|i| i.label == "Apply this theme").expect("offered");
        assert_eq!(item.action, Some(MenuAction::ApplyTheme("mine: morpheus-theme/1;theme=light".into())));
        let mut mention = message(false);
        mention.kind = MessageKind::Text { body: "codes start with morpheus-theme/1;".into(), html: None };
        assert!(!offered(&mention));
    }

    #[test]
    fn menu_labels_translate_whole_or_by_their_known_start() {
        assert_eq!(translated_label_in("es", "Reply"), "Responder");
        assert_eq!(translated_label_in("es", "Reply in thread"), "Responder en un hilo");
        assert_eq!(translated_label_in("es", "Kick carol"), "Expulsar carol");
        assert_eq!(translated_label_in("en", "Kick carol"), "Kick carol");
        // Only at a word boundary, and a label with no translation stays as it is.
        assert_eq!(translated_label_in("es", "Messages from carol"), "Messages from carol");
        assert_eq!(translated_label_in("es", "Something new"), "Something new");
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

    #[test]
    fn the_account_menu_ticks_the_signed_in_account_and_ends_with_adding_one() {
        let alice = OwnedUserId::try_from("@alice:x").expect("user id");
        let bob = OwnedUserId::try_from("@bob:x").expect("user id");
        let saved = |user: &OwnedUserId, active| morpheus_core::SavedAccount {
            key: user.localpart().to_owned(),
            user_id: user.clone(),
            homeserver: "x".into(),
            active,
        };
        let items = account_menu(&bob, &[saved(&alice, false), saved(&bob, true)]);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["View profile", "Edit profile", "Set status", "Switch account", "Sign out"]);
        let switch = &items[3].children;
        assert_eq!(switch.len(), 3);
        assert!(!switch[0].checked && switch[1].checked);
        assert_eq!(switch[2].label, "Add an account");
        assert!(items[4].danger);
    }

    const WIN: (f32, f32) = (800.0, 600.0);

    fn inside(at: (f32, f32), size: (f32, f32)) -> bool {
        at.0 >= 8.0 && at.1 >= 8.0 && at.0 + size.0 <= WIN.0 - 8.0 && at.1 + size.1 <= WIN.1 - 8.0
    }

    #[test]
    fn a_flyout_opens_right_of_its_row_when_there_is_room() {
        let row = Frame {
            x: 100.0,
            y: 100.0,
            w: 188.0,
            h: 33.0,
        };
        assert_eq!(place_flyout(row, (188.0, 100.0), WIN, 8.0), (290.0, 94.0));
    }

    #[test]
    fn a_flyout_flips_left_and_up_at_the_corner() {
        let row = Frame {
            x: 600.0,
            y: 540.0,
            w: 188.0,
            h: 33.0,
        };
        let (x, y) = place_flyout(row, (236.0, 200.0), WIN, 8.0);
        assert!(x + 236.0 <= row.x, "left of the row: {x}");
        assert!(
            y + 200.0 <= row.bottom() + 6.0 + 0.1 && y < row.y,
            "above: {y}"
        );
        assert!(inside((x, y), (236.0, 200.0)));
    }

    #[test]
    fn a_flyout_with_no_room_either_side_is_clamped_inside() {
        let row = Frame {
            x: 5.0,
            y: 590.0,
            w: 780.0,
            h: 33.0,
        };
        let at = place_flyout(row, (300.0, 590.0 - 8.0), (800.0, 600.0), 8.0);
        assert!(inside(at, (300.0, 582.0)), "{at:?}");
    }

    #[test]
    fn a_menu_flips_to_the_pointers_left_and_above_and_stays_inside() {
        assert_eq!(
            place_menu((100.0, 100.0), (188.0, 200.0), WIN, 8.0),
            (100.0, 100.0)
        );
        let at = place_menu((790.0, 590.0), (188.0, 200.0), WIN, 8.0);
        assert_eq!(at, (602.0, 390.0));
        let at = place_menu((5.0, 5.0), (188.0, 700.0), WIN, 8.0);
        assert_eq!(at.0, 8.0);
        assert_eq!(at.1, 8.0);
    }
}
