//! The timeline's logic with no UI toolkit in it (no gpui, no dioxus): the
//! core's entries as rows (grouping, day dividers, NEW divider), message
//! bodies as a neutral tree of styled text, link finding and formatting of
//! times and sizes. The GPUI side (`message.rs`, `timeline.rs`) only maps
//! these to elements. Kept apart so it can move to a crate shared with other
//! front ends.

pub mod about;
pub mod permissions;
pub mod spellcheck;
pub mod release_notes;
pub mod avatar_color;
pub mod calls;
pub mod composer;
pub mod crop;
pub mod diffs;
pub mod emoji;
pub mod emoji_picker;
pub mod format;
pub mod friends;
pub mod diagnostics;
pub mod gpu;
pub mod history;
pub mod hotkeys;
pub mod capture;
pub mod instance;
pub mod sidebar;
pub mod url_scheme;
pub mod invites;
pub mod keys;
pub mod last_place;
pub mod discover;
pub mod link_previews;
pub mod links;
pub mod locks;
pub mod lru;
pub mod linkify;
pub mod marks;
pub mod members;
pub mod profile;
pub mod menu;
pub mod mini_call;
pub mod mod_view;
pub mod nav;
pub mod notices;
pub mod nicknames;
pub mod ringing;
pub mod room_dialogs;
pub mod scopes;
pub mod room_menu;
pub mod settings;
pub mod state_lines;
pub mod streamer;
pub mod rail_drag;
pub mod scrollbar;
pub mod reorder;
pub mod rich;
pub mod rows;
pub mod search;
pub mod text_buffer;
pub mod theme;
pub mod theme_history;
pub mod threads;
pub mod toast;
pub mod typing;
pub mod updates;
pub mod vim;
pub mod verification;

#[cfg(test)]
mod test_room {
    use morpheus_core::{OwnedRoomId, RoomSummary};

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
            call_started_ms: None,
            favorite: false,
            is_voice: false,
            is_encrypted: false,
            latest_ms: 0,
            space_children: Vec::new(),
            can_edit_children: false,
        }
    }
}
