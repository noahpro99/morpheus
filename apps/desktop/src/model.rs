//! What the shell lists, as `crates/ui/src/components/{shell,rail,room_list}.rs`
//! decide it: the scope a rail badge selects, the rooms in it, the rail's
//! layout, what is unread. The scopes themselves are in `view_model::scopes`
//! (the phone apps use them too); the rail's saved layout is the desktop's.

use morpheus_core::{OwnedRoomId, RoomSummary};

use crate::prefs::{Prefs, RailEntry};
pub use morpheus_view_model::scopes::{
    Scope, adopt_children, header_title, rooms_in_scope, scope_for, scope_has_call,
    scope_has_unread, top_spaces,
};

/// Every rail entry in order (DMs, Rooms, Favorites if any, then the servers as
/// drawn, folders opened out): what Ctrl+Alt+Up and Down step through.
pub fn rail_scopes(prefs: &Prefs, rooms: &[RoomSummary]) -> Vec<Scope> {
    let mut all = vec![Scope::Dms, Scope::Rooms];
    if rooms.iter().any(|room| room.favorite && !room.invited) {
        all.push(Scope::Favorites);
    }
    all.extend(crate::view_model::bridges::networks_in(rooms).into_iter().map(Scope::Network));
    for entry in layout(prefs, &top_spaces(rooms)) {
        match entry {
            RailEntry::Space(id) => all.push(Scope::Space(id)),
            RailEntry::Folder { spaces, .. } => all.extend(spaces.into_iter().map(Scope::Space)),
        }
    }
    all
}

/// The rail as drawn: the saved layout, then spaces it does not know.
pub fn layout(prefs: &Prefs, spaces: &[RoomSummary]) -> Vec<RailEntry> {
    let known = |id: &OwnedRoomId| {
        prefs.rail.iter().any(|e| match e {
            RailEntry::Space(s) => s == id,
            RailEntry::Folder { spaces, .. } => spaces.contains(id),
        })
    };
    let mut entries: Vec<RailEntry> = prefs
        .rail
        .iter()
        .filter_map(|entry| match entry {
            RailEntry::Space(id) => spaces.iter().any(|s| &s.id == id).then(|| entry.clone()),
            RailEntry::Folder {
                id,
                name,
                spaces: members,
            } => {
                let present: Vec<OwnedRoomId> = members
                    .iter()
                    .filter(|m| spaces.iter().any(|s| &s.id == *m))
                    .cloned()
                    .collect();
                (!present.is_empty()).then(|| RailEntry::Folder {
                    id: id.clone(),
                    name: name.clone(),
                    spaces: present,
                })
            }
        })
        .collect();
    entries.extend(
        spaces
            .iter()
            .filter(|s| !known(&s.id))
            .map(|s| RailEntry::Space(s.id.clone())),
    );
    entries
}

#[cfg(test)]
pub mod tests {
    use super::*;
    /// A joined room with nothing special; tests set what they need.
    pub fn room(id: &str, name: &str) -> RoomSummary {
        RoomSummary {
            id: OwnedRoomId::try_from(id).expect("room id"),
            name: name.into(),
            topic: None,
            avatar: None,
            is_space: false,
            is_dm: false,
            dm_user: None,
            member_count: 0,
            unread_messages: 0,
            highlight_count: 0,
            marked_unread: false,
            muted: false,
            parent_spaces: Vec::new(),
            invited: false,
            inviter: None,
            call_participants: Vec::new(),
            call_media: Vec::new(),
            call_memberships: Vec::new(),
            favorite: false,
            is_voice: false,
            is_encrypted: false,
            latest_ms: 0,
            space_children: Vec::new(),
            can_edit_children: false,
        }
    }

    #[test]
    fn unknown_spaces_come_after_the_saved_layout() {
        let mut a = room("!a:x", "A");
        a.is_space = true;
        let mut b = room("!b:x", "B");
        b.is_space = true;
        let prefs = Prefs {
            hide_muted: false,
            rail: vec![RailEntry::Space(b.id.clone())],
            ..Prefs::default()
        };
        let entries = layout(&prefs, &[a.clone(), b.clone()]);
        assert_eq!(entries, [RailEntry::Space(b.id), RailEntry::Space(a.id)]);
    }
}
