//! Streamer mode's state, readable from anywhere that draws a name (and from
//! the notifier's thread): this device's choices (`view_model::streamer`) and
//! whether the screen is being shared. `Shell::sync_streamer` keeps the share
//! flag current and redraws when the mode flips.

use std::sync::{
    RwLock,
    atomic::{AtomicBool, Ordering},
};

use crate::view_model::streamer::{StreamerSettings, mask_id, mask_name};

static SETTINGS: RwLock<StreamerSettings> = RwLock::new(StreamerSettings {
    auto: true,
    now: false,
    names: true,
    notifications: true,
    ids: true,
    ask: true,
});
static SHARING: AtomicBool = AtomicBool::new(false);

/// Read this device's choices (at startup).
pub fn load(dir: &std::path::Path) {
    set_settings(StreamerSettings::load(dir));
}

pub fn settings() -> StreamerSettings {
    SETTINGS.read().map(|s| *s).unwrap_or_default()
}

pub fn set_settings(settings: StreamerSettings) {
    if let Ok(mut s) = SETTINGS.write() {
        *s = settings;
    }
}

/// Whether this device is sharing its screen in a call.
pub fn sharing() -> bool {
    SHARING.load(Ordering::Relaxed)
}

/// Note whether the screen is shared; true when that changed whether streamer
/// mode is on.
pub fn set_sharing(on: bool) -> bool {
    let was = active();
    SHARING.store(on, Ordering::Relaxed);
    was != active()
}

/// Whether streamer mode is on now.
pub fn active() -> bool {
    settings().active(sharing())
}

/// A person's name as it may be shown.
pub fn name(name: &str) -> String {
    if active() && settings().names { mask_name(name) } else { name.to_owned() }
}

/// An ID, e-mail address, phone number or link as it may be shown.
pub fn id(id: &str) -> String {
    if active() && settings().ids { mask_id(id) } else { id.to_owned() }
}

/// Whether notifications give nothing away.
pub fn notifications_hidden() -> bool {
    active() && settings().notifications
}

use gpui::Context;
use morpheus_core::OwnedRoomId;

use crate::{
    dialogs::Dialog,
    shell::Shell,
    view_model::{
        menu::{Confirm, ConfirmAction},
        streamer::open_question,
    },
};

impl Shell {
    /// Note whether this device shares its screen; draw everything again
    /// when that turned streamer mode on or off.
    pub fn sync_streamer(&mut self, cx: &mut Context<Self>) {
        let sharing = self.calls.state.as_ref().is_some_and(|call| call.screen);
        if !sharing {
            self.ask_skipped = false;
        }
        if set_sharing(sharing) {
            cx.defer(|cx| cx.refresh_windows());
        }
    }

    /// The streamer mode choices changed (in Settings).
    pub fn streamer_changed(&mut self, cx: &mut Context<Self>) {
        self.sync_streamer(cx);
        cx.refresh_windows();
        cx.notify();
    }

    /// Show a room the person asked for: while the screen is shared (and
    /// asking is on) the question comes first, as Discord's streamer mode.
    pub fn open_room(&mut self, id: OwnedRoomId, cx: &mut Context<Self>) {
        let asking = sharing() && settings().ask && !self.ask_skipped && self.selected.as_ref() != Some(&id);
        let room = self.rooms.iter().find(|room| room.id == id && !room.invited);
        let Some(room) = room.filter(|_| asking) else {
            return self.open_room_now(id, cx);
        };
        let shown = if room.is_dm {
            name(&crate::view_model::nicknames::room_title(&self.nicknames, room))
        } else {
            room.name.clone()
        };
        let body = open_question(&shown, room.is_dm);
        self.ask_tick = false;
        let confirm = Confirm { title: "Open channel".into(), body, confirm: "Open".into() };
        self.dialog_open(Dialog::Confirm(confirm, ConfirmAction::OpenRoom(id)), cx);
    }
}

/// A room's title as it may be shown: a direct message is named for a person.
pub fn room_title(nicknames: &crate::view_model::nicknames::NickMap, room: &morpheus_core::RoomSummary) -> String {
    let title = crate::view_model::nicknames::room_title(nicknames, room);
    if room.is_dm { name(&title) } else { title }
}
