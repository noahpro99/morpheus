//! The channel list as of the last run, kept in the app's folder so the next launch can draw it at
//! once, before the session is restored (#309). Pictures are not kept (the letter shows until the
//! live list arrives), nor who is in a call or the invites: those are live.

use serde::{Deserialize, Serialize};

use crate::sidebar::{RoomItem, ScopeItem, ScopeKind, SectionItem, Sidebar};

const FILE: &str = "sidebar-cache.json";

#[derive(Serialize, Deserialize)]
struct Room {
    id: String,
    name: String,
    topic: Option<String>,
    #[serde(default)]
    avatar_key: Option<String>,
    is_dm: bool,
    is_voice: bool,
    is_encrypted: bool,
    unread: u64,
    highlights: u64,
    muted: bool,
    pinned: bool,
}

#[derive(Serialize, Deserialize)]
struct Section {
    title: Option<String>,
    rooms: Vec<Room>,
}

#[derive(Serialize, Deserialize)]
struct Scope {
    key: String,
    kind: String,
    title: String,
    #[serde(default)]
    avatar_key: Option<String>,
    has_unread: bool,
    highlights: u64,
    sections: Vec<Section>,
}

fn kind_name(kind: ScopeKind) -> &'static str {
    match kind {
        ScopeKind::Dms => "dms",
        ScopeKind::Rooms => "rooms",
        ScopeKind::Favorites => "favorites",
        ScopeKind::Server => "server",
        ScopeKind::Network => "network",
    }
}

fn kind_of(name: &str) -> ScopeKind {
    match name {
        "dms" => ScopeKind::Dms,
        "favorites" => ScopeKind::Favorites,
        "server" => ScopeKind::Server,
        "network" => ScopeKind::Network,
        _ => ScopeKind::Rooms,
    }
}

/// The list as JSON text.
pub(crate) fn encode(sidebar: &Sidebar) -> String {
    let scopes: Vec<Scope> = sidebar
        .scopes
        .iter()
        .map(|scope| Scope {
            key: scope.key.clone(),
            kind: kind_name(scope.kind).to_owned(),
            title: scope.title.clone(),
            avatar_key: scope.avatar_key.clone(),
            has_unread: scope.has_unread,
            highlights: scope.highlights,
            sections: scope
                .sections
                .iter()
                .map(|section| Section {
                    title: section.title.clone(),
                    rooms: section
                        .rooms
                        .iter()
                        .map(|room| Room {
                            id: room.id.clone(),
                            name: room.name.clone(),
                            topic: room.topic.clone(),
                            avatar_key: room.avatar_key.clone(),
                            is_dm: room.is_dm,
                            is_voice: room.is_voice,
                            is_encrypted: room.is_encrypted,
                            unread: room.unread,
                            highlights: room.highlights,
                            muted: room.muted,
                            pinned: room.pinned,
                        })
                        .collect(),
                })
                .collect(),
        })
        .collect();
    serde_json::to_string(&scopes).unwrap_or_default()
}

pub(crate) fn decode(text: &str) -> Option<Sidebar> {
    let scopes: Vec<Scope> = serde_json::from_str(text).ok()?;
    Some(Sidebar {
        scopes: scopes
            .into_iter()
            .map(|scope| ScopeItem {
                key: scope.key,
                kind: kind_of(&scope.kind),
                title: scope.title,
                avatar: None,
                avatar_key: scope.avatar_key,
                has_unread: scope.has_unread,
                highlights: scope.highlights,
                sections: scope
                    .sections
                    .into_iter()
                    .map(|section| SectionItem {
                        title: section.title,
                        rooms: section
                            .rooms
                            .into_iter()
                            .map(|room| RoomItem {
                                id: room.id,
                                name: room.name,
                                topic: room.topic,
                                avatar: None,
                                avatar_key: room.avatar_key,
                                is_dm: room.is_dm,
                                is_voice: room.is_voice,
                                is_encrypted: room.is_encrypted,
                                unread: room.unread,
                                highlights: room.highlights,
                                muted: room.muted,
                                pinned: room.pinned,
                                call: Vec::new(),
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect(),
        invites: Vec::new(),
    })
}

pub(crate) fn save(dir: &std::path::Path, sidebar: &Sidebar) {
    let _ = std::fs::write(dir.join(FILE), encode(sidebar));
}

pub(crate) fn load(dir: &std::path::Path) -> Option<Sidebar> {
    decode(&std::fs::read_to_string(dir.join(FILE)).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_comes_back_as_it_was_saved() {
        let room = RoomItem {
            id: "!a:s".into(), name: "general".into(), topic: Some("t".into()), avatar: None, avatar_key: Some("mxc://s/a".into()), is_dm: false,
            is_voice: false, is_encrypted: true, unread: 3, highlights: 1, muted: false, pinned: true, call: Vec::new(),
        };
        let sidebar = Sidebar {
            scopes: vec![ScopeItem {
                key: "space:!x:s".into(), kind: ScopeKind::Server, title: "Eng".into(), avatar: None, avatar_key: None, has_unread: true, highlights: 1,
                sections: vec![SectionItem { title: Some("Text".into()), rooms: vec![room] }],
            }],
            invites: Vec::new(),
        };
        let back = decode(&encode(&sidebar)).expect("decodes");
        let (scope, room) = (&back.scopes[0], &back.scopes[0].sections[0].rooms[0]);
        assert_eq!((scope.key.as_str(), scope.kind, scope.title.as_str()), ("space:!x:s", ScopeKind::Server, "Eng"));
        assert_eq!((room.name.as_str(), room.unread, room.highlights, room.pinned, room.is_encrypted), ("general", 3, 1, true, true));
        assert!(decode("not json").is_none());
    }
}
