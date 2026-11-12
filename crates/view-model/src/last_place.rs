//! Where each account on this device was last (`last_place.json`, as
//! `crates/ui/src/device_prefs.rs` keeps it), so starting the app again, even
//! after quitting it, opens there: the room, in its server (or the DMs).

use std::path::Path;

use morpheus_core::RoomSummary;

/// The file in the data directory: `{user: {room, chat}}`.
pub const LAST_PLACE: &str = "last_place.json";

/// How long a start waits for the room it was last in to reach the list.
pub const WAIT_SECS: u64 = 20;

/// The room last open, and on a phone whether its chat was in front.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastPlace {
    pub room: String,
    pub chat: bool,
}

fn places(data_dir: &Path) -> serde_json::Map<String, serde_json::Value> {
    std::fs::read(data_dir.join(LAST_PLACE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn read(data_dir: &Path, user: &str) -> Option<LastPlace> {
    let places = places(data_dir);
    let place = places.get(user)?;
    Some(LastPlace {
        room: place.get("room")?.as_str()?.to_owned(),
        chat: place.get("chat").and_then(|c| c.as_bool()).unwrap_or(false),
    })
}

pub fn write(data_dir: &Path, user: &str, place: &LastPlace) -> std::io::Result<()> {
    let mut places = places(data_dir);
    places.insert(
        user.to_owned(),
        serde_json::json!({ "room": place.room, "chat": place.chat }),
    );
    std::fs::write(
        data_dir.join(LAST_PLACE),
        serde_json::Value::Object(places).to_string(),
    )
}

/// Whether the room of `place` can be reopened now: it is in the list (and
/// joined), and so are its servers (a channel can reach the list before its
/// server, and would be filed under Rooms and moved off again).
pub fn ready<'a>(place: &LastPlace, rooms: &'a [RoomSummary]) -> Option<&'a RoomSummary> {
    let room = rooms
        .iter()
        .find(|room| room.id.as_str() == place.room && !room.invited)?;
    room.parent_spaces
        .iter()
        .all(|space| rooms.iter().any(|r| &r.id == space))
        .then_some(room)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_room::room;

    fn place(room: &str) -> LastPlace {
        LastPlace {
            room: room.to_owned(),
            chat: false,
        }
    }

    #[test]
    fn a_place_is_kept_per_account() {
        let dir = std::env::temp_dir().join(format!("morpheus-last-place-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        assert_eq!(read(&dir, "@a:x"), None);
        write(&dir, "@a:x", &place("!one:x")).expect("write");
        write(&dir, "@b:x", &place("!two:x")).expect("write");
        assert_eq!(read(&dir, "@a:x"), Some(place("!one:x")));
        assert_eq!(read(&dir, "@b:x"), Some(place("!two:x")));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_room_waits_for_the_list_and_its_server() {
        let mut channel = room("!c:x", "general");
        let space = room("!s:x", "Engineering");
        channel.parent_spaces = vec![space.id.clone()];
        assert!(ready(&place("!c:x"), &[]).is_none());
        assert!(ready(&place("!c:x"), std::slice::from_ref(&channel)).is_none());
        let both = [channel.clone(), space];
        assert_eq!(ready(&place("!c:x"), &both).map(|r| r.name.as_str()), Some("general"));
        let mut invited = channel;
        invited.invited = true;
        assert!(ready(&place("!c:x"), &[invited]).is_none());
    }
}
