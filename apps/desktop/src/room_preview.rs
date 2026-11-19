//! A room someone linked to that you are not in
//! (`RoomPreviewModal` in `crates/ui/src/components/room_dialogs.rs`): its
//! name, topic and size, and Join (which opens it).

use gpui::{
    Context, EventEmitter, FocusHandle, FontWeight, IntoElement, Render, Window, div, prelude::*,
    px, rgb,
};
use morpheus_core::{LinkedRoom, OwnedRoomId, Session};

use crate::{
    dialogs::{Closed, actions, dialog_box, overlay},
    e2e::E2eExt as _,
    media::avatar,
    room_dialogs::button,
    theme,
};

/// The room was joined; the shell opens it once its list has it.
pub struct Joined(pub OwnedRoomId);

pub struct RoomPreview {
    session: Session,
    room: LinkedRoom,
    via: Vec<String>,
    busy: bool,
    error: Option<String>,
    /// A knock was sent.
    asked: bool,
    focus: FocusHandle,
    take_focus: bool,
}

impl EventEmitter<Closed> for RoomPreview {}
impl EventEmitter<Joined> for RoomPreview {}

impl RoomPreview {
    pub fn new(
        session: Session,
        room: LinkedRoom,
        via: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            session,
            room,
            via,
            busy: false,
            error: None,
            asked: false,
            focus: cx.focus_handle(),
            take_focus: true,
        }
    }

    /// Ask to join a room that takes knocks.
    fn knock(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.asked {
            return;
        }
        self.busy = true;
        cx.notify();
        let room: morpheus_core::OwnedRoomOrAliasId = self.room.room_id.clone().into();
        let knocking = self.session.knock_room(room, self.via.clone());
        cx.spawn(async move |this, cx| {
            let result = knocking.await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(()) => this.asked = true,
                    Err(err) => this.error = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn join(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        cx.notify();
        let room_id = self.room.room_id.clone();
        let joining = self
            .session
            .join_room_via(room_id.clone(), self.via.clone());
        cx.spawn(async move |this, cx| {
            let result = joining.await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(()) => cx.emit(Joined(room_id)),
                    Err(err) => this.error = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for RoomPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.take_focus) {
            window.focus(&self.focus);
        }
        let name = crate::view_model::links::preview_name(&self.room);
        let members = crate::view_model::links::members_line(self.room.members);
        let mut boxed = dialog_box()
            .id("room-preview")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(Closed);
                }
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(avatar(None, &name, 56.0, None))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(1.1 * theme::REM))
                                    .font_weight(FontWeight::BOLD)
                                    .child(name.clone())
                                    .e2e("h2", "")
                                    .text(name.clone()),
                            )
                            .child(
                                div()
                                    .text_color(rgb(theme::text_muted()))
                                    .child(members.clone())
                                    .e2e("p", "muted")
                                    .text(members),
                            ),
                    )
                    .e2e("div", "room-preview-head"),
            );
        if let Some(topic) = crate::view_model::links::preview_topic(&self.room) {
            boxed = boxed.child(
                div()
                    .child(topic.clone())
                    .e2e("p", "room-preview-topic")
                    .text(topic),
            );
        }
        if self.asked {
            boxed = boxed.child(
                div()
                    .text_color(rgb(theme::ok()))
                    .child("Request sent. You can join once someone lets you in.")
                    .e2e("p", "room-preview-note")
                    .text("Request sent. You can join once someone lets you in."),
            );
        }
        if let Some(error) = &self.error {
            boxed = boxed.child(
                div()
                    .text_color(rgb(theme::danger()))
                    .child(error.clone())
                    .e2e("p", "error")
                    .text(error.clone()),
            );
        }
        let boxed = boxed
            .child(actions(vec![
                if self.room.knockable {
                    button(
                        "room-preview-knock",
                        Some("log-in"),
                        "Ask to join",
                        "primary room-preview-knock",
                        false,
                        self.busy || self.asked,
                        cx.listener(|this, _, _, cx| this.knock(cx)),
                    )
                } else {
                    button(
                        "room-preview-join",
                        Some("log-in"),
                        "Join",
                        "primary room-preview-join",
                        false,
                        self.busy,
                        cx.listener(|this, _, _, cx| this.join(cx)),
                    )
                },
                button(
                    "room-preview-cancel",
                    None,
                    "Cancel",
                    "secondary",
                    false,
                    false,
                    cx.listener(|_, _, _, cx| cx.emit(Closed)),
                ),
            ]))
            .e2e("div", "message-dialog room-dialog room-preview")
            .focusable(&self.focus);
        let this = cx.weak_entity();
        overlay(window, cx, boxed.into_any_element(), move |_, cx| {
            this.update(cx, |_, cx| cx.emit(Closed)).ok();
        })
    }
}
