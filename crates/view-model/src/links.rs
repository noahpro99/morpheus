//! Matrix links opened in the app (`open_matrix_link` in
//! `crates/ui/src/components/shell.rs`, `RoomPreviewModal`): what the preview
//! of a room you are not in says.

use morpheus_core::LinkedRoom;

/// The room's name, or its id when it has none.
pub fn preview_name(room: &LinkedRoom) -> String {
    room.name.clone().unwrap_or_else(|| room.room_id.to_string())
}

/// "1 member", "12 members".
pub fn members_line(members: u64) -> String {
    match members {
        1 => "1 member".to_owned(),
        n => format!("{n} members"),
    }
}

/// The topic, unless it is blank.
pub fn preview_topic(room: &LinkedRoom) -> Option<String> {
    room.topic.clone().filter(|t| !t.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(name: Option<&str>, topic: Option<&str>) -> LinkedRoom {
        LinkedRoom {
            room_id: "!r:x".try_into().expect("id"),
            name: name.map(str::to_owned),
            topic: topic.map(str::to_owned),
            avatar: None,
            members: 3,
            joined: false,
        }
    }

    #[test]
    fn the_name_falls_back_to_the_id() {
        assert_eq!(preview_name(&room(Some("Linked"), None)), "Linked");
        assert_eq!(preview_name(&room(None, None)), "!r:x");
    }

    #[test]
    fn counts_and_topics() {
        assert_eq!(members_line(1), "1 member");
        assert_eq!(members_line(12), "12 members");
        assert_eq!(preview_topic(&room(None, Some("  "))), None);
        assert_eq!(preview_topic(&room(None, Some("come on in"))).as_deref(), Some("come on in"));
    }
}
