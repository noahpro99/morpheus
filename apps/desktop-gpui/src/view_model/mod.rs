//! The timeline's logic with no UI toolkit in it (no gpui, no dioxus): the
//! core's entries as rows (grouping, day dividers, NEW divider), message
//! bodies as a neutral tree of styled text, link finding and formatting of
//! times and sizes. The GPUI side (`message.rs`, `timeline.rs`) only maps
//! these to elements. Kept apart so it can move to a crate shared with other
//! front ends.

pub mod composer;
pub mod emoji;
pub mod emoji_picker;
pub mod format;
pub mod keys;
pub mod linkify;
pub mod marks;
pub mod members;
pub mod profile;
pub mod menu;
pub mod room_dialogs;
pub mod room_menu;
pub mod rich;
pub mod rows;
pub mod text_buffer;
pub mod toast;
pub mod verification;
