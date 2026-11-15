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
/// Streamer mode was turned off by hand during this share: it stays off until
/// the share ends, whatever the auto-on-share setting says.
static OVERRIDDEN: AtomicBool = AtomicBool::new(false);

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
    if !on {
        OVERRIDDEN.store(false, Ordering::Relaxed);
    }
    SHARING.store(on, Ordering::Relaxed);
    was != active()
}

/// Whether streamer mode is on now.
pub fn active() -> bool {
    settings().active(sharing()) && !(sharing() && OVERRIDDEN.load(Ordering::Relaxed))
}

/// Turn streamer mode off (the pill, the Settings switch): the saved choice
/// goes off, and during a share it stays off until the share ends; the
/// auto-on-share setting is kept. Returns the settings to save.
pub fn turn_off() -> StreamerSettings {
    let mut s = settings();
    s.now = false;
    if sharing() {
        OVERRIDDEN.store(true, Ordering::Relaxed);
    }
    s
}

/// Turn streamer mode on by hand; returns the settings to save.
pub fn turn_on() -> StreamerSettings {
    OVERRIDDEN.store(false, Ordering::Relaxed);
    let mut s = settings();
    s.now = true;
    s
}

/// A person's name as it may be shown.
pub fn name(name: &str) -> String {
    if active() && settings().names {
        mask_name(name)
    } else {
        name.to_owned()
    }
}

/// An ID, e-mail address, phone number or link as it may be shown.
pub fn id(id: &str) -> String {
    if active() && settings().ids {
        mask_id(id)
    } else {
        id.to_owned()
    }
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
        streamer::{open_question, open_server_question},
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
        self.go_to(None, id, cx)
    }

    /// [`Self::open_room`] in `scope` (the rail entry that lists the room): the
    /// one gate every way to another room goes through, so the question comes
    /// before anything changes, the list included.
    pub fn open_room_in(
        &mut self,
        scope: crate::model::Scope,
        id: OwnedRoomId,
        cx: &mut Context<Self>,
    ) {
        self.go_to(Some(scope), id, cx)
    }

    /// Show a server's welcome screen, asking first while sharing (it lists
    /// the server's channels).
    pub fn open_welcome(&mut self, space: OwnedRoomId, cx: &mut Context<Self>) {
        let asking = sharing()
            && active()
            && settings().ask
            && !self.ask_skipped
            && self.welcome.as_ref() != Some(&space);
        let server = self.rooms.iter().find(|room| room.id == space);
        let Some(server) = server.filter(|_| asking) else {
            return self.show_welcome(space, cx);
        };
        let body = open_server_question(&server.name);
        self.ask_tick = false;
        self.ask_scope = None;
        let confirm = Confirm {
            title: "Open server".into(),
            body,
            confirm: "Open".into(),
        };
        self.dialog_open(
            Dialog::Confirm(confirm, ConfirmAction::OpenWelcome(space)),
            cx,
        );
    }

    pub(crate) fn go_to(
        &mut self,
        scope: Option<crate::model::Scope>,
        id: OwnedRoomId,
        cx: &mut Context<Self>,
    ) {
        let asking =
            sharing() && active() && settings().ask && !self.ask_skipped
            && (self.selected.as_ref() != Some(&id) || self.welcome.is_some());
        let room = self
            .rooms
            .iter()
            .find(|room| room.id == id && !room.invited);
        let Some(room) = room.filter(|_| asking) else {
            if let Some(scope) = scope.filter(|scope| *scope != self.scope) {
                self.set_scope(scope, cx);
            }
            return self.open_room_now(id, cx);
        };
        let shown = if room.is_dm {
            name(&crate::view_model::nicknames::room_title(
                &self.nicknames,
                room,
            ))
        } else {
            room.name.clone()
        };
        let body = open_question(&shown, room.is_dm);
        self.ask_tick = false;
        self.ask_scope = scope;
        let confirm = Confirm {
            title: "Open channel".into(),
            body,
            confirm: "Open".into(),
        };
        self.dialog_open(Dialog::Confirm(confirm, ConfirmAction::OpenRoom(id)), cx);
    }
}

/// A room's title as it may be shown: a direct message is named for a person.
pub fn room_title(
    nicknames: &crate::view_model::nicknames::NickMap,
    room: &morpheus_core::RoomSummary,
) -> String {
    let title = crate::view_model::nicknames::room_title(nicknames, room);
    if room.is_dm { name(&title) } else { title }
}

/// A state line ("bob joined the room") as it may be shown: `who`, the name
/// it starts with (an ID when the person has no name), swapped for its mask.
pub fn state_line(text: &str, who: Option<&str>) -> String {
    let Some(who) = who else {
        return text.to_owned();
    };
    let shown = if who.starts_with('@') && who.contains(':') {
        id_or_name(who)
    } else {
        name(who)
    };
    crate::view_model::streamer::swap_who(text, who, &shown)
}

/// An ID standing where a name would be: it is both an ID and a person.
fn id_or_name(user: &str) -> String {
    if active() && settings().ids {
        mask_id(user)
    } else {
        name(user)
    }
}

/// A message's blocks as they may be shown: the person pills in them masked.
pub fn pills(
    blocks: &[crate::view_model::rich::Block],
) -> std::borrow::Cow<'_, [crate::view_model::rich::Block]> {
    if active() && settings().names {
        std::borrow::Cow::Owned(crate::view_model::streamer::mask_pills(blocks, &|n| {
            mask_name(n.trim_start_matches('@'))
        }))
    } else {
        std::borrow::Cow::Borrowed(blocks)
    }
}

/// An invited room as it may be shown: the person who invited you, and a
/// direct message's name (it is the other person's), masked.
pub fn invite(room: &morpheus_core::RoomSummary) -> morpheus_core::RoomSummary {
    let mut room = room.clone();
    // A direct message is named for the other person, and an invite to one
    // that is not marked as such yet is named for whoever asked.
    if room.is_dm || room.inviter.as_deref() == Some(room.name.as_str()) {
        room.name = name(&room.name);
    }
    room.inviter = room.inviter.map(|who| name(&who));
    room
}
