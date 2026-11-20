//! Join by address (#217): type or paste a room alias, a room ID or a
//! matrix.to / matrix: link; Enter or Preview looks the room up and shows it
//! in the room preview card, which joins it (or asks to, when it takes knocks).

use crate::view_model::i18n::t;
use gpui::{
    Context, Entity, EventEmitter, FocusHandle, FontWeight, IntoElement, Render, Window, div,
    prelude::*, px, rgb, rgba,
};
use morpheus_core::{LinkedRoom, MatrixLink, Session};

use crate::{
    dialogs::{Closed, actions, dialog_box, overlay},
    e2e::E2eExt as _,
    input::{Changed, Submit, TextInput},
    room_dialogs::button,
    theme,
    view_model::join_address::parse_room_address,
};

/// The room was found: show it (the servers to ask come with it).
pub struct Found(pub LinkedRoom, pub Vec<String>);

pub struct JoinAddress {
    session: Session,
    input: Entity<TextInput>,
    busy: bool,
    error: Option<String>,
    focus: FocusHandle,
    take_focus: bool,
}

impl EventEmitter<Closed> for JoinAddress {}
impl EventEmitter<Found> for JoinAddress {}

impl JoinAddress {
    pub fn new(session: Session, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextInput::new(
                "join-address",
                "#room:server, !room:server or a matrix.to link",
                false,
                "",
                cx,
            )
            .bare("input", "")
        });
        cx.subscribe(&input, |this, _, _: &Submit, cx| this.look_up(cx))
            .detach();
        cx.subscribe(&input, |this, _, _: &Changed, cx| {
            if this.error.take().is_some() {
                cx.notify();
            }
        })
        .detach();
        Self {
            session,
            input,
            busy: false,
            error: None,
            focus: cx.focus_handle(),
            take_focus: true,
        }
    }

    fn look_up(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let text = self.input.read(cx).value().to_owned();
        let (room, via) = match parse_room_address(&text) {
            Ok(MatrixLink::Room { room, via, .. }) => (room, via),
            Ok(_) => return,
            Err(err) => {
                self.error = Some(err.message().to_owned());
                cx.notify();
                return;
            }
        };
        self.busy = true;
        self.error = None;
        cx.notify();
        let looking = self.session.linked_room(room, via.clone());
        cx.spawn(async move |this, cx| {
            let result = looking.await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(room) => cx.emit(Found(room, via)),
                    Err(err) => {
                        this.error = Some(crate::view_model::i18n::tf("Could not find that room: %{err}", &[("err", &err.to_string())]));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for JoinAddress {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.take_focus) {
            self.input.read(cx).focus(window);
        }
        let mut boxed = dialog_box()
            .id("join-address")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(Closed);
                }
            }))
            .child(
                div()
                    .text_size(px(1.1 * theme::REM))
                    .font_weight(FontWeight::BOLD)
                    .child(t("Join by address"))
                    .e2e("h2", "")
                    .text(t("Join by address")),
            )
            .child(
                div()
                    .text_color(rgb(theme::text_muted()))
                    .child(t("A room's alias, its room ID, or a matrix.to or matrix: link."))
                    .e2e("p", "muted"),
            )
            .child(
                div()
                    .px(px(12.0))
                    .py(px(4.0))
                    .rounded(px(theme::RADIUS))
                    .bg(theme::fill_input())
                    .border_1()
                    .border_color(rgba(theme::bg_selected()))
                    .child(self.input.clone()),
            );
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
                button(
                    "join-address-preview",
                    Some("search"),
                    "Preview",
                    "primary join-address-preview",
                    false,
                    self.busy,
                    cx.listener(|this, _, _, cx| this.look_up(cx)),
                ),
                button(
                    "join-address-cancel",
                    None,
                    "Cancel",
                    "secondary",
                    false,
                    false,
                    cx.listener(|_, _, _, cx| cx.emit(Closed)),
                ),
            ]))
            .e2e("div", "message-dialog room-dialog join-address-dialog")
            .focusable(&self.focus);
        let this = cx.weak_entity();
        overlay(window, cx, boxed.into_any_element(), move |_, cx| {
            this.update(cx, |_, cx| cx.emit(Closed)).ok();
        })
    }
}

impl crate::shell::Shell {
    /// The Join by address dialog.
    pub fn open_join_address(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let dialog = cx.new(|cx| JoinAddress::new(session, cx));
        cx.subscribe(&dialog, |this: &mut Self, _, _: &Closed, cx| {
            this.dialog_close(cx)
        })
        .detach();
        cx.subscribe(&dialog, |this: &mut Self, _, found: &Found, cx| {
            // The preview card takes the dialog's place.
            this.dialog_close(cx);
            if found.0.joined {
                this.open_room_in_scope(found.0.room_id.clone(), cx);
            } else {
                this.preview_room(found.0.clone(), found.1.clone(), cx);
            }
        })
        .detach();
        self.dialog_open(crate::dialogs::Dialog::JoinAddress(dialog), cx);
    }
}
