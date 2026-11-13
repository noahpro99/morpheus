//! "Invite to voice channel": the modal opened from the call view's invite
//! card when you are the only one in the call. Your friends (the shared
//! `FriendPicker`); picking one sends them a message with a matrix.to link
//! to the channel, which opens it for them. In a DM's call the room is a
//! conversation of two, so the invite is the room's own Matrix invite.

use gpui::{
    Context, Entity, EventEmitter, FocusHandle, IntoElement, Render, Window, div, prelude::*, px,
};
use morpheus_core::{OwnedRoomId, OwnedUserId, Session};

use crate::{
    dialogs::{Closed, dialog_box, heading, muted, overlay},
    e2e::E2eExt as _,
    friend_picker::{FriendPicker, Picked, RowState},
    view_model::nicknames::NickMap,
};

pub struct CallInvite {
    session: Session,
    /// The call's room and its name, and whether it is a DM.
    room: OwnedRoomId,
    name: String,
    direct: bool,
    picker: Entity<FriendPicker>,
    focus: FocusHandle,
    take_focus: bool,
}

impl EventEmitter<Closed> for CallInvite {}

/// The link a voice channel is opened with: matrix.to, via its own server.
pub fn channel_link(room: &OwnedRoomId, home: Option<&str>) -> String {
    match room.server_name().map(|s| s.to_string()).or(home.map(str::to_owned)) {
        Some(server) => format!("https://matrix.to/#/{room}?via={server}"),
        None => format!("https://matrix.to/#/{room}"),
    }
}

impl CallInvite {
    pub fn new(
        session: Session,
        room: OwnedRoomId,
        name: String,
        direct: bool,
        friends: Vec<OwnedUserId>,
        nicknames: NickMap,
        cx: &mut Context<Self>,
    ) -> Self {
        let picker = cx.new(|cx| {
            FriendPicker::new(
                Some(session.clone()),
                friends,
                nicknames,
                "friend-filter",
                "Search friends or enter @friend:matrix.org",
                if direct { "Invite" } else { "Send invite" },
                if direct { "Invited" } else { "Sent" },
                cx,
            )
        });
        cx.subscribe(&picker, |this, _, picked: &Picked, cx| this.send(picked.0.clone(), cx))
            .detach();
        // Enter on a typed ID invites it.
        let input = picker.read(cx).input().clone();
        cx.subscribe(&input, |this, _, _: &crate::input::Submit, cx| {
            let typed = this.picker.read(cx).input().read(cx).value().trim().to_owned();
            if let Ok(user) = typed.parse::<OwnedUserId>() {
                this.picker
                    .update(cx, |picker, cx| picker.set_state(&user, Some(RowState::Working), cx));
                this.send(user, cx);
            }
        })
        .detach();
        Self {
            session,
            room,
            name,
            direct,
            picker,
            focus: cx.focus_handle(),
            take_focus: true,
        }
    }

    fn send(&mut self, user: OwnedUserId, cx: &mut Context<Self>) {
        let session = self.session.clone();
        let (room, name, direct) = (self.room.clone(), self.name.clone(), self.direct);
        let who = user.clone();
        let home = self.session.user_id().map(|u| u.server_name().to_string());
        cx.spawn(async move |this, cx| {
            let sent = async {
                if direct {
                    session.invite(room, who).await.map_err(|err| err.to_string())
                } else {
                    let dm = session.dm_with(who).await.map_err(|err| err.to_string())?;
                    let body = format!("Join me in [{name}]({})", channel_link(&room, home.as_deref()));
                    session.say(dm, body).await.map_err(|err| err.to_string())
                }
            }
            .await;
            this.update(cx, |this, cx| {
                let state = match sent {
                    Ok(()) => RowState::Done,
                    Err(why) => RowState::Failed(why),
                };
                this.picker
                    .update(cx, |picker, cx| picker.set_state(&user, Some(state), cx));
            })
            .ok();
        })
        .detach();
    }
}

impl Render for CallInvite {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.take_focus) {
            self.picker.read(cx).input().read(cx).focus(window);
        }
        let title = if self.direct { "Invite to call" } else { "Invite to voice channel" };
        let hint = if self.direct {
            "They are invited to this conversation and can join the call."
        } else {
            "They get a message with a link that opens the channel."
        };
        let boxed = dialog_box()
            .id("call-invite-dialog")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(Closed);
                }
            }))
            .w(px(440.0))
            .child(heading(title))
            .child(muted(hint, "muted"))
            .child(div().child(self.picker.clone()))
            .e2e("div", "message-dialog call-invite-dialog")
            .focusable(&self.focus);
        let this = cx.weak_entity();
        overlay(window, boxed.into_any_element(), move |_, cx| {
            this.update(cx, |_, cx| cx.emit(Closed)).ok();
        })
    }
}

impl crate::shell::Shell {
    /// The call invite card was clicked: the friends to ask in.
    pub fn open_call_invite(&mut self, room: OwnedRoomId, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let (name, direct) = self
            .rooms
            .iter()
            .find(|r| r.id == room)
            .map(|r| (r.name.clone(), r.is_dm))
            .unwrap_or_default();
        let (friends, nicknames) = (self.friends.clone(), self.nicknames.clone());
        let dialog = cx.new(|cx| CallInvite::new(session, room, name, direct, friends, nicknames, cx));
        cx.subscribe(&dialog, |this, _, _: &Closed, cx| this.dialog_close(cx))
            .detach();
        self.dialog_open(crate::dialogs::Dialog::CallInvite(dialog), cx);
    }
}
