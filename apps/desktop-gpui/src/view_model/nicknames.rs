//! Your nicknames for people (`crates/ui/src/components/nicknames.rs`): kept
//! in private account data by the core, shown instead of display names in
//! messages, the member list, the profile card, DMs and the `@` finder. No
//! toolkit in here.

use std::collections::HashMap;

use morpheus_core::RoomSummary;

/// User ID to the nickname you gave them.
pub type NickMap = HashMap<String, String>;

/// The name to show for `user_id`: your nickname for them, else `fallback`.
pub fn shown_name(map: &NickMap, user_id: &str, fallback: &str) -> String {
    map.get(user_id)
        .cloned()
        .unwrap_or_else(|| fallback.to_owned())
}

/// A room's name as you see it: a DM goes by your nickname for the person.
pub fn room_title(map: &NickMap, room: &RoomSummary) -> String {
    room.dm_user
        .as_ref()
        .filter(|_| room.is_dm)
        .and_then(|user| map.get(user.as_str()).cloned())
        .unwrap_or_else(|| room.name.clone())
}

/// What the nickname dialog says under its title.
pub fn nickname_blurb(current: &str) -> String {
    format!("Only you see this, on all your devices. Leave it empty to use {current}'s own name.")
}

/// The map after saving `nickname` for `user_id` (empty clears it).
pub fn with_nickname(mut map: NickMap, user_id: &str, nickname: &str) -> NickMap {
    let nickname = nickname.trim();
    if nickname.is_empty() {
        map.remove(user_id);
    } else {
        map.insert(user_id.to_owned(), nickname.to_owned());
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nickname_replaces_the_name_and_clearing_brings_it_back() {
        let map = with_nickname(NickMap::new(), "@bob:x", "  Bobby ");
        assert_eq!(shown_name(&map, "@bob:x", "bob"), "Bobby");
        assert_eq!(shown_name(&map, "@carol:x", "carol"), "carol");
        let map = with_nickname(map, "@bob:x", "   ");
        assert_eq!(shown_name(&map, "@bob:x", "bob"), "bob");
        assert!(map.is_empty());
    }

    #[test]
    fn a_dm_goes_by_the_nickname_but_a_room_does_not() {
        let map = with_nickname(NickMap::new(), "@bob:x", "Bobby");
        let mut dm = crate::model::tests::room("!d:x", "bob");
        dm.is_dm = true;
        dm.dm_user = Some("@bob:x".try_into().unwrap());
        assert_eq!(room_title(&map, &dm), "Bobby");
        let mut group = dm.clone();
        group.is_dm = false;
        assert_eq!(room_title(&map, &group), "bob");
        assert!(nickname_blurb("bob").contains("bob's own name"));
    }
}
