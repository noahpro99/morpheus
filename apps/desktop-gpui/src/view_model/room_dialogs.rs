//! The room dialogs' logic (`crates/ui/src/components/room_dialogs.rs`):
//! what each form accepts, what its wording says for the room or server it
//! is about, and which parts of the privacy dialog show. No toolkit in here.

use morpheus_core::{History, OwnedRoomId, OwnedUserId, RoomSummary};

/// A dialog a menu, a button or the rail asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoomDialogKind {
    Invite(OwnedRoomId),
    /// Name, topic and picture of a room or server.
    Edit(OwnedRoomId),
    /// A room in the scope that is open.
    Create,
    /// Join rule, history and encryption.
    Privacy(OwnedRoomId),
    /// Your name and picture inside a server.
    RoomProfile(OwnedRoomId),
    /// A category inside `Some(server)`, or a new server.
    CreateSpace(Option<OwnedRoomId>),
    LeaveSpace(OwnedRoomId),
}

/// The message of a form: a note that it worked, or why it did not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Note {
    Ok(String),
    Error(String),
}

impl Note {
    pub fn ok(text: impl Into<String>) -> Self {
        Self::Ok(text.into())
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self::Error(text.into())
    }

    #[allow(dead_code)]
    pub fn text(&self) -> &str {
        match self {
            Self::Ok(text) | Self::Error(text) => text,
        }
    }
}

// ---- invite -----------------------------------------------------------------

/// What the invite form's box holds as a user, or why it is not one.
pub fn parse_invitee(input: &str) -> Result<OwnedUserId, Note> {
    input
        .trim()
        .parse::<OwnedUserId>()
        .map_err(|_| Note::error("Enter a full Matrix ID like @name:server"))
}

/// What the form says once the invite went out.
pub fn invited_note(user: &OwnedUserId) -> Note {
    Note::ok(format!("Invited {user}."))
}

// ---- create a room ----------------------------------------------------------

/// One line under "Create room": where the room will be.
pub fn create_room_scope(space_name: Option<&str>) -> String {
    match space_name {
        Some(space) => format!("In the {space} space."),
        None => "Not in any space.".to_owned(),
    }
}

/// The categories of a server: spaces nested directly under it (id, name).
pub fn categories(rooms: &[RoomSummary], space: Option<&OwnedRoomId>) -> Vec<(OwnedRoomId, String)> {
    let Some(space) = space else {
        return Vec::new();
    };
    rooms
        .iter()
        .filter(|room| room.is_space && room.parent_spaces.contains(space))
        .map(|room| (room.id.clone(), room.name.clone()))
        .collect()
}

/// The name of a server (or space) in the list.
pub fn room_name(rooms: &[RoomSummary], id: &OwnedRoomId) -> Option<String> {
    rooms
        .iter()
        .find(|room| &room.id == id)
        .map(|room| room.name.clone())
}

/// What "Create room" is sent with: the name must have something in it.
pub fn can_create(name: &str) -> bool {
    !name.trim().is_empty()
}

/// Where a new room goes: the chosen category, else the server, else nowhere.
pub fn room_parent(category: Option<&OwnedRoomId>, space: Option<&OwnedRoomId>) -> Option<OwnedRoomId> {
    category.or(space).cloned()
}

// ---- create a server or category ---------------------------------------------

pub fn create_space_title(is_category: bool) -> &'static str {
    if is_category {
        "Create category"
    } else {
        "Create server"
    }
}

pub fn create_space_blurb(is_category: bool) -> &'static str {
    if is_category {
        "A category groups channels inside this server (a nested space)."
    } else {
        "A server is a Matrix space: a home for your channels."
    }
}

// ---- leave a server -----------------------------------------------------------

pub fn leave_space_title(name: Option<&str>) -> String {
    format!("Leave {}?", name.unwrap_or("this server"))
}

pub fn leave_space_body(rooms: usize) -> String {
    format!("You will leave the server and its {rooms} rooms. You can be invited back later.")
}

// ---- per-server profile ---------------------------------------------------------

pub fn room_profile_blurb(rooms: usize) -> String {
    format!(
        "How you appear in this server and its {rooms} rooms. Your global profile stays as it is."
    )
}

/// The name of a picked picture's file, shown beside "Choose avatar…".
pub fn picked_file(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

// ---- privacy --------------------------------------------------------------------

/// How encryption stands in the privacy dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encryption {
    /// Not asked yet.
    Checking,
    On,
    Off,
    /// Off, and the person pressed "Turn on encryption": the warning shows.
    Confirming,
}

impl Encryption {
    /// From what the server says, and whether the warning is up.
    pub fn from(on: Option<bool>, confirming: bool) -> Self {
        match (on, confirming) {
            (None, _) => Self::Checking,
            (Some(true), _) => Self::On,
            (Some(false), false) => Self::Off,
            (Some(false), true) => Self::Confirming,
        }
    }
}

pub const ENCRYPTION_ON: &str =
    "End-to-end encrypted: only the people in this room can read its messages.";
pub const ENCRYPTION_PERMANENT: &str = "Matrix rooms cannot turn encryption off again.";
pub const ENCRYPTION_OFF: &str =
    "Not encrypted: the servers of everyone in this room can read its messages.";
pub const ENCRYPTION_WARNING: &str = "Once on, encryption cannot be turned off. Bots and bridges that cannot decrypt will stop seeing new messages, and people joining later will not read what came before.";

/// Which parts of the privacy dialog a room has: a space has no encryption,
/// a DM has no join rule or history to choose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrivacyParts {
    pub encryption: bool,
    pub join_and_history: bool,
}

pub fn privacy_parts(rooms: &[RoomSummary], id: &OwnedRoomId) -> PrivacyParts {
    let room = rooms.iter().find(|room| &room.id == id);
    let is_dm = room.is_some_and(|room| room.is_dm);
    let is_space = room.is_some_and(|room| room.is_space);
    PrivacyParts {
        encryption: !is_space,
        join_and_history: !is_dm,
    }
}

/// The history choices, as the form words them.
pub const HISTORY_CHOICES: [(History, &str); 3] = [
    (History::Shared, "Everything"),
    (History::Invited, "From their invite"),
    (History::Joined, "From when they joined"),
];

/// "Who can join": `true` is public.
pub const JOIN_CHOICES: [(bool, &str); 2] = [(true, "Anyone with the link"), (false, "Invite only")];

#[cfg(test)]
mod tests {
    use super::*;

    fn room(id: &str, name: &str, space: bool, dm: bool, parents: &[&str]) -> RoomSummary {
        let mut room = crate::model::tests::room(id, name);
        room.is_space = space;
        room.is_dm = dm;
        room.parent_spaces = parents.iter().map(|p| (*p).try_into().unwrap()).collect();
        room
    }

    #[test]
    fn invitees_must_be_full_matrix_ids() {
        assert!(parse_invitee(" @bob:localhost ").is_ok());
        assert_eq!(
            parse_invitee("bob"),
            Err(Note::error("Enter a full Matrix ID like @name:server"))
        );
        assert!(parse_invitee("").is_err());
        let bob = parse_invitee("@bob:localhost").unwrap();
        assert_eq!(invited_note(&bob).text(), "Invited @bob:localhost.");
    }

    #[test]
    fn the_scope_line_names_the_server_or_none() {
        assert_eq!(create_room_scope(None), "Not in any space.");
        assert_eq!(create_room_scope(Some("Engineering")), "In the Engineering space.");
    }

    #[test]
    fn categories_are_the_spaces_directly_under_a_server() {
        let rooms = vec![
            room("!srv:x", "Server", true, false, &[]),
            room("!cat:x", "Text", true, false, &["!srv:x"]),
            room("!room:x", "chat", false, false, &["!srv:x"]),
            room("!other:x", "Elsewhere", true, false, &["!nope:x"]),
        ];
        let server: OwnedRoomId = "!srv:x".try_into().unwrap();
        assert_eq!(
            categories(&rooms, Some(&server)),
            vec![("!cat:x".try_into().unwrap(), "Text".to_owned())]
        );
        assert!(categories(&rooms, None).is_empty());
        assert_eq!(room_name(&rooms, &server).as_deref(), Some("Server"));
    }

    #[test]
    fn a_room_needs_a_name_and_goes_under_its_category_first() {
        assert!(!can_create("   "));
        assert!(can_create(" x "));
        let (a, b): (OwnedRoomId, OwnedRoomId) = ("!a:x".try_into().unwrap(), "!b:x".try_into().unwrap());
        assert_eq!(room_parent(Some(&a), Some(&b)), Some(a.clone()));
        assert_eq!(room_parent(None, Some(&b)), Some(b));
        assert_eq!(room_parent(None, None), None);
    }

    #[test]
    fn wording_for_servers_and_categories() {
        assert_eq!(create_space_title(true), "Create category");
        assert_eq!(create_space_title(false), "Create server");
        assert!(create_space_blurb(true).contains("nested space"));
        assert_eq!(leave_space_title(Some("Dev")), "Leave Dev?");
        assert_eq!(leave_space_title(None), "Leave this server?");
        assert!(leave_space_body(3).contains("its 3 rooms"));
        assert!(room_profile_blurb(2).contains("its 2 rooms"));
        assert_eq!(picked_file(std::path::Path::new("/tmp/me.png")), "me.png");
    }

    #[test]
    fn encryption_follows_the_server_and_the_warning() {
        assert_eq!(Encryption::from(None, true), Encryption::Checking);
        assert_eq!(Encryption::from(Some(true), true), Encryption::On);
        assert_eq!(Encryption::from(Some(false), false), Encryption::Off);
        assert_eq!(Encryption::from(Some(false), true), Encryption::Confirming);
    }

    #[test]
    fn privacy_shows_only_what_a_room_can_have() {
        let rooms = vec![
            room("!space:x", "S", true, false, &[]),
            room("!dm:x", "bob", false, true, &[]),
            room("!room:x", "r", false, false, &[]),
        ];
        let parts = |id: &str| privacy_parts(&rooms, &id.try_into().unwrap());
        assert_eq!(parts("!space:x"), PrivacyParts { encryption: false, join_and_history: true });
        assert_eq!(parts("!dm:x"), PrivacyParts { encryption: true, join_and_history: false });
        assert_eq!(parts("!room:x"), PrivacyParts { encryption: true, join_and_history: true });
    }
}
