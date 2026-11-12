//! Matrix links opened in the app (`open_matrix_link` in
//! `crates/ui/src/components/shell.rs`): a person's profile; a room you are
//! in (at the message, for a message's link); a room you are not in,
//! previewed with Join. Also the tray's and the Help page's update check.

use std::time::Duration;

use gpui::{AppContext as _, Context};
use morpheus_core::{MatrixLink, OwnedRoomId, UpdateStatus};

use crate::{
    dialogs::{Closed, Dialog},
    model::scope_for,
    room_preview::{Joined, RoomPreview},
    shell::Shell,
};

impl Shell {
    /// A Matrix link from a message or from outside the app.
    pub fn open_link(&mut self, url: &str, cx: &mut Context<Self>) {
        let Some(link) = morpheus_core::parse_matrix_link(url) else {
            return;
        };
        match link {
            MatrixLink::User(user) => {
                self.open_profile(user, None, None, cx);
                self.show_full_profile(cx);
            }
            MatrixLink::Room { room, event, via } => {
                let Some(session) = self.session.clone() else {
                    return;
                };
                let linked = session.linked_room(room, via.clone());
                cx.spawn(async move |this, cx| {
                    let linked = match linked.await {
                        Ok(linked) => linked,
                        Err(err) => {
                            this.update(cx, |this, cx| this.fail("opening the link", &err.to_string(), cx)).ok();
                            return;
                        }
                    };
                    if !linked.joined {
                        this.update(cx, |this, cx| this.preview_room(linked, via, cx)).ok();
                        return;
                    }
                    let room_id = linked.room_id;
                    this.update(cx, |this, cx| this.open_room_in_scope(room_id, cx)).ok();
                    if let Some(event) = event {
                        // Once the room's own timeline is up (it takes the jump).
                        cx.background_executor().timer(Duration::from_millis(350)).await;
                        this.update(cx, |this, cx| {
                            if let Some((_, timeline)) = &this.timeline {
                                timeline.update(cx, |timeline, cx| timeline.jump_to(event.to_string(), cx));
                            }
                        })
                        .ok();
                    }
                })
                .detach();
            }
        }
    }

    /// Open `room_id` where it lives: its server (or the DMs, Rooms), then the room.
    pub fn open_room_in_scope(&mut self, room_id: OwnedRoomId, cx: &mut Context<Self>) {
        if let Some(room) = self.rooms.iter().find(|r| r.id == room_id) {
            let scope = scope_for(&self.rooms, room);
            if scope != self.scope {
                self.set_scope(scope, cx);
            }
        }
        self.open_room(room_id, cx);
    }

    fn preview_room(&mut self, room: morpheus_core::LinkedRoom, via: Vec<String>, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let preview = cx.new(|cx| RoomPreview::new(session, room, via, cx));
        cx.subscribe(&preview, |this: &mut Self, _, _: &Closed, cx| this.dialog_close(cx))
            .detach();
        cx.subscribe(&preview, |this: &mut Self, _, joined: &Joined, cx| {
            this.dialog_close(cx);
            // Opened once the room list has it.
            this.pending_dm = Some(joined.0.clone());
            this.open_pending_dm(cx);
        })
        .detach();
        self.dialog_open(Dialog::Preview(preview), cx);
    }

    /// The tray's Acknowledgements.
    pub fn open_about(&mut self, cx: &mut Context<Self>) {
        self.dialog_open(Dialog::About, cx);
    }

    /// Ask the release feed and say what it found in a toast.
    pub fn check_for_updates(&mut self, _asked: bool, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let version = crate::view_model::about::VERSION;
        let checking = session.check_for_update(version);
        cx.spawn(async move |this, cx| {
            let result = checking.await;
            this.update(cx, |this, cx| match result {
                Ok(UpdateStatus::UpToDate) => this.toast(format!("Morpheus {version} is up to date"), cx),
                Ok(UpdateStatus::NoFeed) => this.toast(
                    format!("Morpheus {version}: this build has no release feed to check"),
                    cx,
                ),
                Ok(UpdateStatus::Available { version, url, .. }) => this.toast(
                    match &url {
                        Some(url) => format!("Morpheus {version} is available: {url}"),
                        None => format!("Morpheus {version} is available"),
                    },
                    cx,
                ),
                Err(err) => this.fail("update check", &err.to_string(), cx),
            })
            .ok();
        })
        .detach();
    }
}
