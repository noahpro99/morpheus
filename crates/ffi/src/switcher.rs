//! The quick switcher on the phone (#500, #455): the bottom search bar of each list opens it, and it searches every room, DM, server and
//! command. The matching and the order are view-model's (`switcher::search`), shared with the desktop's Ctrl+K.

use std::sync::Arc;

use morpheus_view_model::palette::{PaletteAction, PaletteState};
use morpheus_view_model::switcher::{phone_offers, search, Hit, SwitchRoom};

use crate::{MediaHandle, Morpheus};

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SwitcherKind {
    Server,
    Channel,
    Dm,
    /// A command; its `id` is the action's name (`Settings`, `StartDm`, `Join`, `CreateServer`, `Discover`, `Inbox`; `Settings…` for a settings page).
    Action,
    /// A setting, found by name; its `id` is its label.
    Setting,
}

/// One row of the switcher.
#[derive(uniffi::Record)]
pub struct SwitcherHit {
    pub kind: SwitcherKind,
    /// The room's id for a place, the action's name for a command.
    pub id: String,
    pub label: String,
    /// Where it lives ("Server", "Direct message", its server) or what it belongs to ("Settings · Appearance").
    pub detail: String,
    /// The desktop's icon name for a command or setting; empty for a place.
    pub icon: String,
    pub avatar: Option<Arc<MediaHandle>>,
    pub avatar_key: Option<String>,
}

#[uniffi::export]
impl Morpheus {
    /// What the switcher shows for `query`, best first (the app draws them bottom-up). `recent` is the rooms opened last, newest first; `current`
    /// the one open now; `light` is whether the light theme is on.
    pub fn switcher_search(&self, query: String, recent: Vec<String>, current: Option<String>, light: bool) -> Vec<SwitcherHit> {
        let Ok(session) = self.session() else { return Vec::new() };
        let all = self.rooms_now.lock().map(|r| r.clone()).unwrap_or_default();
        let mut listed: Vec<&morpheus_core::RoomSummary> = all.iter().filter(|r| !r.invited).collect();
        // Servers lead (the sort keeps each group in the order given).
        listed.sort_by_key(|r| !r.is_space);
        let place = |room: &morpheus_core::RoomSummary| -> Option<String> {
            room.parent_spaces
                .iter()
                .chain(all.iter().filter(|s| s.is_space && s.space_children.contains(&room.id)).map(|s| &s.id))
                .find_map(|id| all.iter().find(|s| &s.id == id && s.is_space))
                .map(|s| s.name.clone())
        };
        let rooms: Vec<SwitchRoom> = listed
            .iter()
            .map(|r| SwitchRoom {
                id: r.id.to_string(),
                name: r.name.clone(),
                real_name: r.name.clone(),
                kind: if r.is_space {
                    "Server".to_owned()
                } else if r.is_dm {
                    "Direct message".to_owned()
                } else {
                    place(r).unwrap_or_else(|| "Rooms".to_owned())
                },
                is_server: r.is_space,
            })
            .collect();
        let state = PaletteState { no_window: true, light, ..Default::default() };
        search(&rooms, &query, &recent, current.as_deref(), state, phone_offers)
            .into_iter()
            .map(|hit| match hit {
                Hit::Room(i) => {
                    let (room, item) = (listed[i], &rooms[i]);
                    SwitcherHit {
                        kind: if room.is_space { SwitcherKind::Server } else if room.is_dm { SwitcherKind::Dm } else { SwitcherKind::Channel },
                        id: item.id.clone(),
                        label: item.name.clone(),
                        detail: item.kind.clone(),
                        icon: String::new(),
                        avatar: room.avatar.clone().map(|m| MediaHandle::new(m, &session)),
                        avatar_key: room.avatar.as_ref().map(|m| m.key().to_owned()),
                    }
                }
                Hit::Action(a) => SwitcherHit {
                    kind: SwitcherKind::Action,
                    id: match a.action {
                        PaletteAction::SettingsPage(_) => "SettingsPage".to_owned(),
                        other => format!("{other:?}"),
                    },
                    label: a.label,
                    detail: String::new(),
                    icon: a.icon.to_owned(),
                    avatar: None,
                    avatar_key: None,
                },
                Hit::Setting(s) => SwitcherHit {
                    kind: SwitcherKind::Setting,
                    id: s.label.to_owned(),
                    label: s.label.to_owned(),
                    detail: morpheus_view_model::i18n::tf("Settings · %{page}", &[("page", &morpheus_view_model::i18n::t(s.page.label()))]),
                    icon: s.page.glyph().to_owned(),
                    avatar: None,
                    avatar_key: None,
                },
            })
            .collect()
    }
}
