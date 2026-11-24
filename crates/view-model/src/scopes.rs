//! What a list of rooms shows, as the rail and the channel list decide it:
//! the scope a badge selects (direct messages, rooms outside a space,
//! favorites, one space), the rooms in it, a server's channels in its own
//! order under its categories, what is unread. Shared by the desktop app and
//! the phone apps (through `morpheus-ffi`).

use morpheus_core::{OwnedRoomId, RoomSummary, SpaceRoom};


/// What the sidebar lists: direct messages, ungrouped rooms, favorites, or
/// one space.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum Scope {
    Dms,
    #[default]
    Rooms,
    Favorites,
    /// The rooms bridged to one network (`discord`), whichever way they are otherwise filed.
    Network(String),
    /// A top-level space and everything nested under it.
    Space(OwnedRoomId),
}

impl Scope {
    /// The name of this entry in `last_rooms.json`.
    pub fn key(&self) -> String {
        match self {
            Self::Dms => "dms".into(),
            Self::Rooms => "rooms".into(),
            Self::Favorites => "favorites".into(),
            Self::Network(id) => format!("net:{id}"),
            Self::Space(id) => format!("space:{id}"),
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        if let Some(id) = key.strip_prefix("net:") {
            return (!id.is_empty()).then(|| Self::Network(id.to_owned()));
        }
        if let Some(id) = key.strip_prefix("space:") {
            return OwnedRoomId::try_from(id).ok().map(Self::Space);
        }
        match key {
            "dms" => Some(Self::Dms),
            "rooms" => Some(Self::Rooms),
            "favorites" => Some(Self::Favorites),
            _ => None,
        }
    }

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
    if let Some(network) = crate::bridges::network_id_of(room) {
        return Scope::Network(network);
    }
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

/// Not part of a bridged network: those are listed under their network.
fn not_bridged(room: &RoomSummary) -> bool {
    crate::bridges::network_id_of(room).is_none()
}

/// Rooms visible for a scope: DMs; rooms that are neither DMs nor in a space;
/// favorites; or a space's whole subtree.
pub fn rooms_in_scope(rooms: &[RoomSummary], scope: &Scope) -> Vec<RoomSummary> {
    let space_id = match scope {
        Scope::Dms => {
            return rooms
                .iter()
                .filter(|r| r.is_dm && !r.is_space && !r.invited && not_bridged(r))
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
                .filter(|r| !r.is_space && !r.is_dm && !r.invited && r.parent_spaces.is_empty() && not_bridged(r))
                .cloned()
                .collect();
        }
        Scope::Network(network) => {
            return rooms
                .iter()
                .filter(|r| !r.is_space && !r.invited && crate::bridges::network_id_of(r).as_deref() == Some(network.as_str()))
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

/// Whether a scope holds an unread room (muted ones aside), for the dot
/// beside its badge.
pub fn scope_has_unread(rooms: &[RoomSummary], scope: &Scope) -> bool {
    rooms_in_scope(rooms, scope).iter().any(|room| {
        room.invited
            || (!room.muted
                && (room.unread_messages > 0 || room.marked_unread || room.highlight_count > 0))
    })
}

/// What the badge on a rail icon shows about calls in its scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallBadge {
    /// No call going.
    None,
    /// Someone is in a voice channel: a speaker.
    Voice,
    /// Someone in it is sharing their screen: a screen, instead of the speaker.
    Screen,
}

impl CallBadge {
    /// The badge's glyph (`icons::glyph` names); None when there is no badge.
    pub fn icon(self) -> Option<&'static str> {
        match self {
            CallBadge::None => None,
            CallBadge::Voice => Some("volume-2"),
            CallBadge::Screen => Some("screen-share"),
        }
    }
}

/// The rooms of a scope that have a call to show (`has_call`).
fn rooms_with_calls<'a>(
    rooms: &'a [RoomSummary],
    scope: &Scope,
    has_call: impl Fn(&RoomSummary) -> bool,
) -> Vec<&'a RoomSummary> {
    let mut found: Vec<&RoomSummary> = Vec::new();
    if let Scope::Space(space_id) = scope
        && let Some(space_room) = rooms.iter().find(|r| &r.id == space_id)
    {
        found.extend(rooms.iter().filter(|r| space_room.space_children.contains(&r.id) && has_call(r)));
    }
    for room in rooms_in_scope(rooms, scope) {
        if has_call(&room) && !found.iter().any(|f| f.id == room.id) {
            if let Some(r) = rooms.iter().find(|r| r.id == room.id) {
                found.push(r);
            }
        }
    }
    found
}

/// The call badge of a scope: a screen when someone in one of its calls shares
/// theirs, else a speaker when anyone is in a call, else none (#360).
pub fn scope_call_badge(
    rooms: &[RoomSummary],
    scope: &Scope,
    has_call: impl Fn(&RoomSummary) -> bool,
) -> CallBadge {
    let with_calls = rooms_with_calls(rooms, scope, has_call);
    if with_calls.is_empty() {
        CallBadge::None
    } else if with_calls.iter().any(|r| {
        // Only people still in the call: a membership that ran out keeps its
        // last media state, and would hold the screen icon up (#376).
        r.call_media
            .iter()
            .any(|(user, media)| media.screen && r.call_participants.contains(user))
    }) {
        CallBadge::Screen
    } else {
        CallBadge::Voice
    }
}

/// Whether someone is in a call in a scope's voice channels (or DMs): the
/// speaker badge on its rail icon. `has_call` says whether a room has one
/// to show (`Shell::has_call_shown`: a call just hung up is gone at once).
pub fn scope_has_call(
    rooms: &[RoomSummary],
    scope: &Scope,
    has_call: impl Fn(&RoomSummary) -> bool,
) -> bool {
    match scope {
        Scope::Space(space_id) => {
            if let Some(space_room) = rooms.iter().find(|r| &r.id == space_id) {
                if rooms.iter().any(|r| {
                    space_room.space_children.contains(&r.id) && has_call(r)
                }) {
                    return true;
                }
            }
            rooms_in_scope(rooms, scope)
                .iter()
                .any(|room| has_call(room))
        }
        _ => rooms_in_scope(rooms, scope)
            .iter()
            .any(|room| has_call(room)),
    }
}

/// The sidebar header: what the rail selected.
pub fn header_title(rooms: &[RoomSummary], scope: &Scope) -> String {
    match scope {
        Scope::Dms => "Direct messages".into(),
        Scope::Rooms => "Rooms".into(),
        Scope::Favorites => "Favorites".into(),
        Scope::Network(id) => crate::bridges::network_name(rooms, id),
        Scope::Space(id) => rooms
            .iter()
            .find(|r| &r.id == id)
            .map_or_else(|| "Space".into(), |r| r.name.clone()),
    }
}

/// The channel list's rows, in the order they are drawn.
#[derive(Debug, Default)]
pub struct Sections {
    pub dms: Vec<RoomSummary>,
    pub channels: Vec<RoomSummary>,
    /// Rooms under a nested server, by its name, in the server's order.
    pub categories: Vec<(OwnedRoomId, String, Vec<RoomSummary>)>,
    /// A server's channels you are not in.
    pub unjoined: Vec<SpaceRoom>,
}

/// Sort the rooms of a scope (`dms` and `channels`, already filtered by the
/// caller) into the channel list: inside a server its channels follow the
/// server's own order, nested servers become headed categories, and the
/// channels you are not in (`directory`) come last.
pub fn categorise(
    rooms: &[RoomSummary],
    scope: &Scope,
    dms: Vec<RoomSummary>,
    channels: Vec<RoomSummary>,
    directory: &[SpaceRoom],
) -> Sections {
    let mut categories: Vec<(OwnedRoomId, String)> = match scope {
        Scope::Space(space_id) => rooms
            .iter()
            .filter(|r| r.is_space && r.parent_spaces.contains(space_id))
            .map(|r| (r.id.clone(), r.name.clone()))
            .collect(),
        _ => Vec::new(),
    };
    let (mut channels, categorised): (Vec<_>, Vec<_>) = channels.into_iter().partition(|room| {
        !categories
            .iter()
            .any(|(id, _)| room.parent_spaces.contains(id))
    });
    let children_of = |id: &OwnedRoomId| -> Vec<OwnedRoomId> {
        rooms
            .iter()
            .find(|r| &r.id == id)
            .map(|r| r.space_children.clone())
            .unwrap_or_default()
    };
    let place = |order: &[OwnedRoomId], id: &OwnedRoomId| {
        order.iter().position(|c| c == id).unwrap_or(usize::MAX)
    };
    if let Scope::Space(space_id) = scope {
        let order = children_of(space_id);
        channels.sort_by_key(|r| place(&order, &r.id));
        categories.sort_by_key(|(id, _)| place(&order, id));
    }
    let categories = categories
        .into_iter()
        .map(|(id, name)| {
            let order = children_of(&id);
            let mut members: Vec<RoomSummary> = categorised
                .iter()
                .filter(|r| r.parent_spaces.contains(&id))
                .cloned()
                .collect();
            members.sort_by_key(|r| place(&order, &r.id));
            (id, name, members)
        })
        .collect();
    let unjoined = match scope {
        Scope::Space(_) => directory
            .iter()
            .filter(|r| !r.is_space)
            .filter(|r| !rooms.iter().any(|known| known.id == r.id && !known.invited))
            .cloned()
            .collect(),
        _ => Vec::new(),
    };
    Sections {
        dms,
        channels,
        categories,
        unjoined,
    }
}

/// One rail entry of a phone (or any narrow) UI: a scope with its name, the
/// counts for its badge and the channel list it opens.
#[derive(Debug)]
pub struct ScopeView {
    pub scope: Scope,
    pub title: String,
    /// A server's picture (the other scopes have none).
    pub avatar: Option<morpheus_core::MediaRef>,
    /// Something unread (muted rooms aside), for the dot on its badge.
    pub has_unread: bool,
    /// Mentions across its rooms (muted ones aside).
    pub highlights: u64,
    pub sections: Sections,
}

/// Every scope worth showing, in order: direct messages, rooms outside a
/// server, favorites (when there are any), then the top-level servers in
/// the order they came. `rooms` is the core's room list as it is.
pub fn scope_views(rooms: &[RoomSummary]) -> Vec<ScopeView> {
    let mut rooms = rooms.to_vec();
    adopt_children(&mut rooms);
    let mut scopes = vec![Scope::Dms, Scope::Rooms];
    if rooms.iter().any(|room| room.favorite && !room.invited) {
        scopes.push(Scope::Favorites);
    }
    scopes.extend(crate::bridges::networks_in(&rooms).into_iter().map(Scope::Network));
    scopes.extend(top_spaces(&rooms).into_iter().map(|space| Scope::Space(space.id)));
    scopes
        .into_iter()
        .map(|scope| {
            let (dms, channels): (Vec<_>, Vec<_>) =
                rooms_in_scope(&rooms, &scope).into_iter().partition(|room| room.is_dm);
            let highlights = dms
                .iter()
                .chain(&channels)
                .filter(|room| !room.muted)
                .map(|room| room.highlight_count)
                .sum();
            let avatar = match &scope {
                Scope::Space(id) => rooms.iter().find(|room| &room.id == id).and_then(|room| room.avatar.clone()),
                _ => None,
            };
            ScopeView {
                title: header_title(&rooms, &scope),
                has_unread: scope_has_unread(&rooms, &scope),
                highlights,
                avatar,
                sections: categorise(&rooms, &scope, dms, channels, &[]),
                scope,
            }
        })
        .collect()
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn the_phone_lists_scopes_with_their_channels_and_counts() {
        let mut server = room("!s:x", "Engineering");
        server.is_space = true;
        let mut dev = room("!dev:x", "dev");
        dev.parent_spaces = vec![server.id.clone()];
        dev.unread_messages = 2;
        dev.highlight_count = 1;
        server.space_children = vec![dev.id.clone()];
        let mut dm = room("!dm:x", "bob");
        dm.is_dm = true;
        let mut invite = room("!i:x", "invite");
        invite.invited = true;
        let all = [server.clone(), dev, room("!g:x", "general"), dm, invite];

        let views = scope_views(&all);
        let keys: Vec<String> = views.iter().map(|v| v.scope.key()).collect();
        assert_eq!(keys, ["dms".to_owned(), "rooms".into(), format!("space:{}", server.id)]);
        let names = |view: &ScopeView| -> Vec<String> {
            view.sections.channels.iter().chain(&view.sections.dms).map(|r| r.name.clone()).collect()
        };
        assert_eq!(names(&views[0]), ["bob"]);
        assert_eq!(names(&views[1]), ["general"]);
        assert_eq!(names(&views[2]), ["dev"]);
        assert_eq!(views[2].title, "Engineering");
        assert!(views[2].has_unread);
        assert_eq!(views[2].highlights, 1);
        assert!(!views[1].has_unread);
    }

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
            bridge: None,
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

    #[test]
    fn scope_has_call_checks_participants_and_space_children() {
        let mut space = room("!s:x", "Server");
        space.is_space = true;
        let mut channel = room("!v:x", "Voice");
        channel.is_voice = true;
        space.space_children = vec![channel.id.clone()];
        let scope = Scope::Space(space.id.clone());

        let has_call = |r: &RoomSummary| !r.call_participants.is_empty();
        assert!(!scope_has_call(&[space.clone(), channel.clone()], &scope, has_call));
        channel
            .call_participants
            .push(morpheus_core::OwnedUserId::try_from("@bob:x").unwrap());
        assert!(scope_has_call(&[space, channel], &scope, has_call));
    }
}

#[cfg(test)]
mod call_badge_tests {
    use super::*;
    use crate::test_room::room;

    fn setup() -> (RoomSummary, RoomSummary) {
        let mut space = room("!s:x", "Server");
        space.is_space = true;
        let mut channel = room("!v:x", "Voice");
        channel.is_voice = true;
        space.space_children = vec![channel.id.clone()];
        (space, channel)
    }

    #[test]
    fn the_rail_badge_is_none_a_speaker_or_a_screen() {
        let (space, mut channel) = setup();
        let scope = Scope::Space(space.id.clone());
        let in_call = |r: &RoomSummary| !r.call_participants.is_empty();
        assert_eq!(scope_call_badge(&[space.clone(), channel.clone()], &scope, in_call), CallBadge::None);
        let bob = morpheus_core::OwnedUserId::try_from("@bob:x").expect("id");
        channel.call_participants.push(bob.clone());
        channel.call_media.push((bob.clone(), morpheus_core::CallMedia::default()));
        let badge = scope_call_badge(&[space.clone(), channel.clone()], &scope, in_call);
        assert_eq!((badge, badge.icon()), (CallBadge::Voice, Some("volume-2")));
        channel.call_media = vec![(bob, morpheus_core::CallMedia { screen: true, ..Default::default() })];
        let badge = scope_call_badge(&[space, channel], &scope, in_call);
        assert_eq!((badge, badge.icon()), (CallBadge::Screen, Some("screen-share")));
        assert_eq!(CallBadge::None.icon(), None);
    }

    #[test]
    fn a_share_in_a_call_that_is_not_shown_does_not_count() {
        let (space, mut channel) = setup();
        let bob = morpheus_core::OwnedUserId::try_from("@bob:x").expect("id");
        channel.call_media = vec![(bob, morpheus_core::CallMedia { screen: true, ..Default::default() })];
        // No one in the call any more (just hung up): neither badge.
        let scope = Scope::Space(space.id.clone());
        assert_eq!(scope_call_badge(&[space, channel], &scope, |r| !r.call_participants.is_empty()), CallBadge::None);
    }

    #[test]
    fn a_share_from_someone_no_longer_in_the_call_does_not_count() {
        let (space, mut channel) = setup();
        let id = |s: &str| morpheus_core::OwnedUserId::try_from(s).expect("id");
        let (alice, bob) = (id("@alice:x"), id("@bob:x"));
        let scope = Scope::Space(space.id.clone());
        let in_call = |r: &RoomSummary| !r.call_participants.is_empty();
        // Bob's membership ran out (or he left) with a share on it; alice is
        // still in the call and shares nothing.
        channel.call_participants = vec![alice.clone()];
        channel.call_media = vec![
            (alice, morpheus_core::CallMedia::default()),
            (bob.clone(), morpheus_core::CallMedia { screen: true, ..Default::default() }),
        ];
        assert_eq!(scope_call_badge(&[space.clone(), channel.clone()], &scope, in_call), CallBadge::Voice);
        // Back in the call, his share counts.
        channel.call_participants.push(bob);
        assert_eq!(scope_call_badge(&[space, channel], &scope, in_call), CallBadge::Screen);
    }
}
