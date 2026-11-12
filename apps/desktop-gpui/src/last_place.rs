//! Opening where this account was when the app last ran (even if it was
//! quit, or its window closed to the tray): `view_model::last_place`.

use std::time::Duration;

use gpui::Context;

use crate::{
    model::scope_for,
    shell::Shell,
    view_model::last_place::{self, LastPlace},
};

impl Shell {
    /// Read the account's last place and wait for its room to reach the list.
    pub fn wait_for_last_place(&mut self, cx: &mut Context<Self>) {
        let Some(user) = self.session.as_ref().and_then(|s| s.user_id()) else {
            return;
        };
        self.place_user = user.to_string();
        // The room last open in each server, kept since the last run.
        for (key, room) in last_place::read_rooms(&crate::data_dir(), &self.place_user) {
            if let (Some(scope), Ok(room)) = (
                crate::model::Scope::from_key(&key),
                morpheus_core::OwnedRoomId::try_from(room),
            ) {
                self.last_room_in.entry(scope).or_insert(room);
            }
        }
        self.pending_place = last_place::read(&crate::data_dir(), &self.place_user);
        if self.pending_place.is_none() {
            return;
        }
        // A room that is gone (left elsewhere) is not waited for forever.
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(last_place::WAIT_SECS))
                .await;
            this.update(cx, |this, _| this.pending_place = None).ok();
        })
        .detach();
    }

    /// Reopen the last room once the list has it (and its server).
    pub fn restore_place(&mut self, cx: &mut Context<Self>) {
        let Some(place) = self.pending_place.clone() else {
            return;
        };
        let Some(room) = last_place::ready(&place, &self.rooms).cloned() else {
            return;
        };
        // Someone opened a room meanwhile: that stays.
        self.pending_place = None;
        if self.selected.is_some() {
            return;
        }
        eprintln!("morpheus-gpui: reopening the last room {}", room.id);
        let scope = scope_for(&self.rooms, &room);
        if scope != self.scope {
            self.set_scope(scope, cx);
        }
        self.open_room(room.id, cx);
    }

    /// The room last open in each rail entry, for the next run.
    pub fn save_last_rooms(&self) {
        if self.place_user.is_empty() {
            return;
        }
        let rooms = self
            .last_room_in
            .iter()
            .map(|(scope, room)| (scope.key(), room.to_string()))
            .collect();
        if let Err(err) = last_place::write_rooms(&crate::data_dir(), &self.place_user, &rooms) {
            eprintln!("morpheus-gpui: last rooms not kept: {err}");
        }
    }

    /// Where this account is, for the next start. Nothing is kept until the
    /// last place has been reopened (or given up on), so a room picked
    /// meanwhile does not overwrite it.
    pub fn save_place(&self) {
        if self.pending_place.is_some() || self.place_user.is_empty() {
            return;
        }
        let Some(room) = &self.selected else {
            return;
        };
        let place = LastPlace {
            room: room.to_string(),
            chat: false,
        };
        if let Err(err) = last_place::write(&crate::data_dir(), &self.place_user, &place) {
            eprintln!("morpheus-gpui: last place not kept: {err}");
        }
    }
}
