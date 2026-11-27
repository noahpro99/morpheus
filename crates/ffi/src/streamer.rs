//! Streamer mode (the desktop's `streamer.rs`, #540): while the screen is shared, or when turned on by hand, the app hides what could give
//! people away: names show as their first letter and dots, user, room and device IDs, e-mail addresses, phone numbers and links as dots, and
//! notifications say nothing of who or what. The rules are `view_model::streamer`.

use morpheus_view_model::streamer::{self, StreamerSettings};

use crate::Morpheus;

/// What streamer mode does, as the settings page shows it.
#[derive(Clone, uniffi::Record)]
pub struct StreamerChoices {
    /// Turns on by itself while this phone shares its screen.
    pub auto: bool,
    /// Turned on by hand.
    pub now: bool,
    pub names: bool,
    pub notifications: bool,
    pub ids: bool,
}

#[uniffi::export]
impl Morpheus {
    pub fn streamer_choices(&self) -> StreamerChoices {
        let s = StreamerSettings::load(&self.dir);
        StreamerChoices { auto: s.auto, now: s.now, names: s.names, notifications: s.notifications, ids: s.ids }
    }

    /// Change one choice (`auto`, `now`, `names`, `notifications`, `ids`); kept.
    pub fn set_streamer_choice(&self, key: String, on: bool) {
        let mut s = StreamerSettings::load(&self.dir);
        match key.as_str() {
            "auto" => s.auto = on,
            "now" => s.now = on,
            "names" => s.names = on,
            "notifications" => s.notifications = on,
            "ids" => s.ids = on,
            _ => return,
        }
        let _ = s.save(&self.dir);
    }

    /// Whether streamer mode is on now (`sharing`: this phone shares its screen).
    pub fn streamer_active(&self, sharing: bool) -> bool {
        StreamerSettings::load(&self.dir).active(sharing)
    }
}

/// A name as streamer mode shows it ("Alice" is "A•••").
#[uniffi::export]
pub fn streamer_mask_name(name: String) -> String {
    streamer::mask_name(&name)
}

/// An ID, address, number or link as streamer mode shows it (dots).
#[uniffi::export]
pub fn streamer_mask_id(id: String) -> String {
    streamer::mask_id(&id)
}
