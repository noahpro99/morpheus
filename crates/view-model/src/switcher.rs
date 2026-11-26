//! What the quick switcher (the desktop's Ctrl+K, the phone's bottom search bar, #500, #455) offers for what is typed: the rooms, DMs and
//! servers whose name matches, the palette's actions and the settings. Best first; a phone draws them bottom-up, the best next to the thumb.
//!
//! Which actions a phone can do is [`phone_offers`].

use crate::palette::{PaletteAction, PaletteEntry, PaletteState};
use crate::settings::SettingEntry;

/// A place the switcher can go to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchRoom {
    pub id: String,
    pub name: String,
    /// The name without a nickname (a DM's person), also matched.
    pub real_name: String,
    /// Where it lives, shown beside it: its server, "Direct message", "Server".
    pub kind: String,
    pub is_server: bool,
}

/// One row of the switcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hit {
    /// An index into the rooms given.
    Room(usize),
    Action(PaletteEntry),
    Setting(&'static SettingEntry),
}

/// How many rooms show.
pub const ROOMS: usize = 8;

/// The rows for `needle`: with nothing typed the rooms opened last lead (see [`crate::recent_rooms::palette_order`]) and then the actions;
/// typed, the rooms whose name has it (servers first, as `rooms` is ordered), then the actions and the settings that match.
pub fn search(rooms: &[SwitchRoom], needle: &str, recent: &[String], current: Option<&str>, state: PaletteState, offered: impl Fn(PaletteAction) -> bool) -> Vec<Hit> {
    let needle = needle.trim().to_lowercase();
    let mut list: Vec<Hit> = if needle.is_empty() {
        let ids: Vec<&str> = rooms.iter().map(|r| r.id.as_str()).collect();
        crate::recent_rooms::palette_order(&ids, recent, current, ROOMS).into_iter().map(Hit::Room).collect()
    } else {
        rooms
            .iter()
            .enumerate()
            .filter(|(_, r)| r.name.to_lowercase().contains(&needle) || r.real_name.to_lowercase().contains(&needle))
            .take(ROOMS)
            .map(|(i, _)| Hit::Room(i))
            .collect()
    };
    let actions = crate::palette::actions(state).into_iter().filter(|a| offered(a.action)).collect();
    list.extend(crate::palette::search(actions, &needle).into_iter().map(Hit::Action));
    list.extend(crate::settings::find_settings(&needle, !state.no_window).into_iter().map(Hit::Setting));
    list
}

/// The actions a phone does: the ones that open a screen it has.
pub fn phone_offers(action: PaletteAction) -> bool {
    matches!(
        action,
        PaletteAction::CreateServer | PaletteAction::Discover | PaletteAction::StartDm | PaletteAction::Join | PaletteAction::Settings | PaletteAction::Inbox
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(id: &str, name: &str, kind: &str, is_server: bool) -> SwitchRoom {
        SwitchRoom { id: id.into(), name: name.into(), real_name: name.into(), kind: kind.into(), is_server }
    }

    fn rooms() -> Vec<SwitchRoom> {
        vec![
            room("!eng", "Engineering", "Server", true),
            room("!gen", "general", "Engineering", false),
            room("!bob", "Bob Builder", "Direct message", false),
            room("!eng2", "engine-room", "Engineering", false),
        ]
    }

    #[test]
    fn typing_finds_servers_rooms_and_dms_by_name_with_the_given_order_kept() {
        let state = PaletteState { no_window: true, ..Default::default() };
        let hits = search(&rooms(), "eng", &[], None, state, phone_offers);
        assert_eq!(&hits[..2], &[Hit::Room(0), Hit::Room(3)]);
        let hits = search(&rooms(), "BOB", &[], None, state, phone_offers);
        assert_eq!(hits[0], Hit::Room(2));
    }

    #[test]
    fn nothing_typed_leads_with_the_recent_rooms_and_then_the_phones_actions() {
        let state = PaletteState { no_window: true, ..Default::default() };
        let hits = search(&rooms(), "", &["!bob".into()], Some("!gen"), state, phone_offers);
        assert_eq!(hits[0], Hit::Room(2), "the room opened last leads");
        assert!(!hits.iter().any(|h| matches!(h, Hit::Room(1))) || hits.iter().position(|h| *h == Hit::Room(1)) > Some(0), "the open room is not the first");
        let actions: Vec<PaletteAction> = hits.iter().filter_map(|h| if let Hit::Action(a) = h { Some(a.action) } else { None }).collect();
        assert!(actions.contains(&PaletteAction::Settings) && actions.contains(&PaletteAction::Inbox) && actions.contains(&PaletteAction::StartDm));
        assert!(!actions.contains(&PaletteAction::Quit) && !actions.contains(&PaletteAction::ToggleMute), "a phone has no Quit or call hotkeys: {actions:?}");
    }

    #[test]
    fn actions_and_settings_are_found_by_what_is_typed() {
        let state = PaletteState { no_window: true, ..Default::default() };
        let hits = search(&rooms(), "inbox", &[], None, state, phone_offers);
        assert!(hits.iter().any(|h| matches!(h, Hit::Action(a) if a.action == PaletteAction::Inbox)));
        let hits = search(&rooms(), "language", &[], None, state, phone_offers);
        assert!(hits.iter().any(|h| matches!(h, Hit::Setting(_))), "a setting by its name");
        assert!(search(&rooms(), "zzzzqq", &[], None, state, phone_offers).is_empty());
    }
}
