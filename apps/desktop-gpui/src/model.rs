//! What the shell lists, as `crates/ui/src/components/{shell,rail,room_list}.rs`
//! decide it: the scope a rail badge selects, the rooms in it, the rail's
//! layout, what is unread.

use morpheus_core::{OwnedRoomId, RoomSummary};

use crate::prefs::{Prefs, RailEntry};

/// What the sidebar lists: direct messages, ungrouped rooms, favorites, or
/// one space.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum Scope {
    Dms,
    #[default]
    Rooms,
    Favorites,
    /// A top-level space and everything nested under it.
    Space(OwnedRoomId),
}

impl Scope {
    pub fn space_id(&self) -> Option<&OwnedRoomId> {
        match self {
            Self::Space(id) => Some(id),
            _ => None,
        }
    }
}

/// The rail scope a room belongs to: DMs, Rooms, or its top-level space.
/// Used when something other than the rail opens a room (the inbox), so the
/// sidebar follows instead of bouncing the selection back.
pub fn scope_for(rooms: &[RoomSummary], room: &RoomSummary) -> Scope {
    if room.is_dm {
        return Scope::Dms;
    }
    let mut current = room.parent_spaces.first().cloned();
    let mut hops = 0;
    while let Some(space_id) = current {
        let Some(space) = rooms.iter().find(|r| r.id == space_id) else {
            return Scope::Rooms;
        };
        match space.parent_spaces.first() {
            None => return Scope::Space(space_id),
            Some(parent) if hops < 16 => {
                hops += 1;
                current = Some(parent.clone());
            }
            Some(_) => return Scope::Rooms,
        }
    }
    Scope::Rooms
}

/// Rooms visible for a scope: DMs; rooms that are neither DMs nor in a space;
/// favorites; or a space's whole subtree.
pub fn rooms_in_scope(rooms: &[RoomSummary], scope: &Scope) -> Vec<RoomSummary> {
    let space_id = match scope {
        Scope::Dms => {
            return rooms
                .iter()
                .filter(|r| r.is_dm && !r.is_space && !r.invited)
                .cloned()
                .collect();
        }
        Scope::Favorites => {
            return rooms
                .iter()
                .filter(|r| r.favorite && !r.is_space && !r.invited)
                .cloned()
                .collect();
        }
        Scope::Rooms => {
            return rooms
                .iter()
                .filter(|r| !r.is_space && !r.is_dm && !r.invited && r.parent_spaces.is_empty())
                .cloned()
                .collect();
        }
        Scope::Space(id) => id,
    };
    // Every space reachable from the selected one through nested parents.
    let mut scope = std::collections::HashSet::new();
    scope.insert(space_id.clone());
    loop {
        let mut grew = false;
        for space in rooms.iter().filter(|r| r.is_space) {
            if !scope.contains(&space.id) && space.parent_spaces.iter().any(|p| scope.contains(p)) {
                scope.insert(space.id.clone());
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    rooms
        .iter()
        .filter(|r| !r.is_space && !r.invited && r.parent_spaces.iter().any(|p| scope.contains(p)))
        .cloned()
        .collect()
}

/// Rooms a joined space lists (its `m.space.child`) belong to it even when
/// they don't name it back (no `m.space.parent`), as other clients have it:
/// the space goes into their parents.
pub fn adopt_children(list: &mut [RoomSummary]) {
    let links: Vec<(OwnedRoomId, OwnedRoomId)> = list
        .iter()
        .filter(|r| r.is_space && !r.invited)
        .flat_map(|space| {
            space
                .space_children
                .iter()
                .map(|c| (c.clone(), space.id.clone()))
        })
        .collect();
    for (child, space) in links {
        if let Some(room) = list
            .iter_mut()
            .find(|r| r.id == child && !r.parent_spaces.contains(&space))
        {
            room.parent_spaces.push(space);
        }
    }
}

/// Top-level spaces, the ones with a badge.
pub fn top_spaces(rooms: &[RoomSummary]) -> Vec<RoomSummary> {
    rooms
        .iter()
        .filter(|r| r.is_space && r.parent_spaces.is_empty() && !r.invited)
        .cloned()
        .collect()
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

/// Whether a scope holds an unread room (muted ones aside), for the dot
/// beside its badge.
pub fn scope_has_unread(rooms: &[RoomSummary], scope: &Scope) -> bool {
    rooms_in_scope(rooms, scope).iter().any(|room| {
        room.invited
            || (!room.muted
                && (room.unread_messages > 0 || room.marked_unread || room.highlight_count > 0))
    })
}

/// The sidebar header: what the rail selected.
pub fn header_title(rooms: &[RoomSummary], scope: &Scope) -> String {
    match scope {
        Scope::Dms => "Direct messages".into(),
        Scope::Rooms => "Rooms".into(),
        Scope::Favorites => "Favorites".into(),
        Scope::Space(id) => rooms
            .iter()
            .find(|r| &r.id == id)
            .map_or_else(|| "Space".into(), |r| r.name.clone()),
    }
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
            favorite: false,
            is_voice: false,
            is_encrypted: false,
            latest_ms: 0,
            space_children: Vec::new(),
            can_edit_children: false,
        }
    }

    #[test]
    fn scopes_split_rooms_dms_and_spaces() {
        let mut space = room("!s:x", "Engineering");
        space.is_space = true;
        let mut inside = room("!dev:x", "dev");
        inside.parent_spaces = vec![space.id.clone()];
        let mut dm = room("!dm:x", "bob");
        dm.is_dm = true;
        let all = [space.clone(), inside, room("!g:x", "general"), dm];
        let names = |scope: &Scope| -> Vec<String> {
            rooms_in_scope(&all, scope)
                .into_iter()
                .map(|r| r.name)
                .collect()
        };
        assert_eq!(names(&Scope::Rooms), ["general"]);
        assert_eq!(names(&Scope::Dms), ["bob"]);
        assert_eq!(names(&Scope::Space(space.id.clone())), ["dev"]);
        assert_eq!(header_title(&all, &Scope::Space(space.id)), "Engineering");
    }

    #[test]
    fn nested_spaces_show_under_the_top_one() {
        let mut top = room("!top:x", "Top");
        top.is_space = true;
        let mut sub = room("!sub:x", "Sub");
        sub.is_space = true;
        sub.parent_spaces = vec![top.id.clone()];
        let mut deep = room("!deep:x", "deep");
        deep.parent_spaces = vec![sub.id.clone()];
        let all = [top.clone(), sub, deep];
        assert_eq!(rooms_in_scope(&all, &Scope::Space(top.id.clone())).len(), 1);
        assert_eq!(top_spaces(&all).len(), 1);
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

    #[test]
    fn a_space_adopts_the_rooms_it_lists() {
        let mut server = room("!s:x", "Projects");
        server.is_space = true;
        let child = room("!c:x", "projects-news");
        server.space_children = vec![child.id.clone()];
        let mut all = [server.clone(), child];
        assert_eq!(rooms_in_scope(&all, &Scope::Rooms).len(), 1);
        adopt_children(&mut all);
        assert!(rooms_in_scope(&all, &Scope::Rooms).is_empty());
        assert_eq!(rooms_in_scope(&all, &Scope::Space(server.id)).len(), 1);
    }

    #[test]
    fn muted_rooms_are_not_unread() {
        let mut r = room("!r:x", "r");
        r.unread_messages = 3;
        assert!(scope_has_unread(&[r.clone()], &Scope::Rooms));
        r.muted = true;
        assert!(!scope_has_unread(&[r], &Scope::Rooms));
    }
}
