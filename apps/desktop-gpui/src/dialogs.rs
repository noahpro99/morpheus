//! Dialogs over the chat (`crates/ui/src/components/{confirm,message_dialogs,
//! toast}.rs`): "Are you sure?", who reacted, report a message, the room's
//! pinned messages, and the toasts in the bottom-right corner. What a menu
//! item does (`Shell::run_menu_action`), the undo of a deletion and the
//! pinned list's upkeep are here too, since they are what these serve.

use std::time::Duration;

use gpui::{
    AnyElement, ClipboardItem, Context, Div, ElementId, Entity, EventEmitter, FocusHandle,
    FontWeight, IntoElement, MouseButton, Render, SharedString, Stateful, WeakEntity, Window,
    deferred, div, prelude::*, px, relative, rgb, rgba,
};
use morpheus_core::{
    Message, MessageId, OwnedEventId, OwnedRoomId, PinnedMessage, Session, TimelineHandle,
};

use crate::{
    e2e::E2eExt as _,
    input::{Submit, TextInput},
    menu::OpenMenu,
    message::RowAction,
    shell::Shell,
    theme,
    timeline::TimelineView,
    view_model::{
        menu::{
            Confirm, ConfirmAction, MenuAction, MenuTarget, confirm_delete, forward_targets,
            offers_quick_reactions, message_menu,
        },
        toast::{TOAST_SECS, UNDO_SECS, failure_text},
    },
};

/// What a dialog that is its own view tells the shell when it is done.
pub struct Closed;

/// Something that changed which messages are pinned.
pub struct PinsChanged;

/// The dialog over the chat, if any.
pub enum Dialog {
    Confirm(Confirm, ConfirmAction),
    Reactions(Box<Message>),
    Report(Entity<ReportDialog>),
    Pinned(Entity<PinnedDialog>),
    Room(Entity<crate::room_dialogs::RoomDialog>),
    ModView(Entity<crate::mod_view::ModView>),
    /// Ctrl+/: the list of shortcuts.
    Help,
    /// The tray's Acknowledgements.
    About,
    /// A room linked to that you are not in.
    Preview(Entity<crate::room_preview::RoomPreview>),
}

/// Something done at once that its Undo toast can still take back.
pub enum PendingUndo {
    /// A deleted message, hidden meanwhile.
    Delete {
        timeline: WeakEntity<TimelineView>,
        handle: TimelineHandle,
        message: Message,
    },
    /// A room left, out of the list meanwhile.
    Leave(OwnedRoomId),
}

impl Shell {
    /// A toast with `text`; it goes by itself after a while.
    pub fn toast(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        let id = self.toasts.push(text, false);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(TOAST_SECS))
                .await;
            this.update(cx, |this, cx| {
                if this.toasts.dismiss(id) {
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// A failed action: a note in the log and a toast.
    pub fn fail(&mut self, what: &str, err: &str, cx: &mut Context<Self>) {
        eprintln!("morpheus-gpui: {what}: {err}");
        self.toast(failure_text(what, err), cx);
    }

    /// The pinned messages of the open room, read again.
    pub fn refresh_pinned(&mut self, cx: &mut Context<Self>) {
        let (Some(session), Some(room)) = (self.session.clone(), self.selected.clone()) else {
            self.pinned.clear();
            return;
        };
        let reading = session.pinned_event_ids(room.clone());
        cx.spawn(async move |this, cx| {
            let ids = reading.await;
            this.update(cx, |this, cx| {
                if this.selected.as_ref() == Some(&room)
                    && let Ok(ids) = ids
                    && ids != this.pinned
                {
                    this.pinned = ids;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// The menu of a message right-clicked in the open room.
    pub fn open_message_menu(
        &mut self,
        message: &Message,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        let room = self.selected.clone();
        let pinned = match &message.id {
            MessageId::Event(id) => self.pinned.contains(id),
            MessageId::Transaction(_) => false,
        };
        let targets = room
            .as_ref()
            .map(|room| forward_targets(&self.rooms, room))
            .unwrap_or_default();
        let items = message_menu(message, room.as_ref(), pinned, &targets, false);
        let react = offers_quick_reactions(message);
        let target = MenuTarget::Message {
            message: Box::new(message.clone()),
            room,
        };
        self.open_menu(OpenMenu::new(at, items, react, target), cx);
    }

    /// The timeline a message action acts on: the thread panel's replies if
    /// the menu came from there, else the room's.
    pub(crate) fn open_timeline(&self, cx: &gpui::App) -> Option<Entity<TimelineView>> {
        self.active_timeline(cx).map(|(timeline, _)| timeline)
    }

    /// That timeline and its handle.
    pub(crate) fn active_timeline(
        &self,
        cx: &gpui::App,
    ) -> Option<(Entity<TimelineView>, Option<morpheus_core::TimelineHandle>)> {
        if self.acting_in_thread
            && let Some((_, panel)) = &self.thread
        {
            return Some(panel.read(cx).timeline());
        }
        self.timeline
            .as_ref()
            .map(|(_, timeline)| (timeline.clone(), self.timeline_handle.clone()))
    }

    /// Runs what a menu item asked for.
    pub fn run_menu_action(&mut self, action: MenuAction, target: MenuTarget, cx: &mut Context<Self>) {
        match target {
            MenuTarget::Message { message, room } => self.run_message_action(action, message, room, cx),
            MenuTarget::Other => self.run_room_action(action, cx),
        }
    }

    fn run_message_action(
        &mut self,
        action: MenuAction,
        message: Box<Message>,
        room: Option<OwnedRoomId>,
        cx: &mut Context<Self>,
    ) {
        let timeline = self.open_timeline(cx);
        let unique_id = message.unique_id.clone();
        let session = self.session.clone();
        let event_id = match &message.id {
            MessageId::Event(id) => Some(id.clone()),
            MessageId::Transaction(_) => None,
        };
        match action {
            MenuAction::Reply | MenuAction::Edit => {
                let row = if action == MenuAction::Reply {
                    RowAction::Reply
                } else {
                    RowAction::Edit
                };
                if let Some(timeline) = timeline {
                    timeline.update(cx, |timeline, cx| timeline.row_action(row, &unique_id, cx));
                }
            }
            MenuAction::ReplyInThread => self.open_thread(*message, cx),
            MenuAction::React(key) => {
                if let Some(timeline) = timeline {
                    timeline.update(cx, |timeline, cx| timeline.react(message.id.clone(), key, cx));
                }
            }
            MenuAction::RemoveReactions(keys) => {
                if let Some(timeline) = timeline {
                    for key in keys {
                        timeline.update(cx, |timeline, cx| {
                            timeline.react(message.id.clone(), key, cx)
                        });
                    }
                }
            }
            MenuAction::ViewReactions => self.dialog_open(Dialog::Reactions(message), cx),
            MenuAction::Forward(to) => {
                if let (Some(session), Some(from), Some(event_id)) = (session, room, event_id) {
                    let forwarding = session.forward(from, event_id, to);
                    self.run_in_background("forward", forwarding, cx);
                }
            }
            MenuAction::Pin(pin) => {
                if let (Some(session), Some(room), Some(event_id)) = (session, room, event_id) {
                    let pinning = session.pin(room, event_id, pin);
                    cx.spawn(async move |this, cx| {
                        let result = pinning.await;
                        this.update(cx, |this, cx| {
                            if let Err(err) = result {
                                this.fail("pin", &err.to_string(), cx);
                            }
                            this.refresh_pinned(cx);
                        })
                        .ok();
                    })
                    .detach();
                }
            }
            MenuAction::MarkUnread => {
                if let (Some(session), Some(room)) = (session, room) {
                    if let Some(timeline) = timeline {
                        timeline.update(cx, |timeline, _| timeline.keep_unread());
                    }
                    let marking = session.mark_unread(room, true);
                    self.run_in_background("mark unread", marking, cx);
                }
            }
            MenuAction::CopyText(text) | MenuAction::CopyLink(text) | MenuAction::CopyId(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
            MenuAction::SelectText(_) => {}
            MenuAction::Report => {
                if let (Some(session), Some(room), Some(event_id)) = (session, room, event_id) {
                    let dialog = cx.new(|cx| ReportDialog::new(session, room, event_id, cx));
                    cx.subscribe(&dialog, |this, _, _: &Closed, cx| this.dialog_close(cx))
                        .detach();
                    self.dialog_open(Dialog::Report(dialog), cx);
                }
            }
            MenuAction::Delete => self.ask_to_delete(*message, cx),
            // The rest is for rooms and people.
            other => self.run_room_action(other, cx),
        }
    }

    /// Runs a core call that has nothing to hand back; a failure is a toast.
    pub(crate) fn run_in_background(
        &mut self,
        what: &'static str,
        future: morpheus_core::CoreFuture<()>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            if let Err(err) = future.await {
                this.update(cx, |this, cx| this.fail(what, &err.to_string(), cx))
                    .ok();
            }
        })
        .detach();
    }

    pub fn dialog_open(&mut self, dialog: Dialog, cx: &mut Context<Self>) {
        self.dialog = Some(dialog);
        self.take_dialog_focus = true;
        cx.notify();
    }

    pub fn dialog_close(&mut self, cx: &mut Context<Self>) {
        if self.dialog.take().is_some() {
            cx.notify();
        }
    }

    /// The header's pin button: the room's pinned messages.
    pub fn open_pinned(&mut self, cx: &mut Context<Self>) {
        let (Some(session), Some(room)) = (self.session.clone(), self.selected.clone()) else {
            return;
        };
        let dialog = cx.new(|cx| PinnedDialog::new(session, room, cx));
        cx.subscribe(&dialog, |this, _, _: &Closed, cx| this.dialog_close(cx))
            .detach();
        cx.subscribe(&dialog, |this, _, _: &PinsChanged, cx| this.refresh_pinned(cx))
            .detach();
        self.dialog_open(Dialog::Pinned(dialog), cx);
    }

    /// The hover bar's bin: asks first, unless Shift skipped the question.
    pub fn delete_message(&mut self, message: Message, skip_asking: bool, cx: &mut Context<Self>) {
        if skip_asking {
            self.delete_with_undo(message, cx);
        } else {
            self.ask_to_delete(message, cx);
        }
    }

    fn ask_to_delete(&mut self, message: Message, cx: &mut Context<Self>) {
        self.dialog_open(
            Dialog::Confirm(confirm_delete(), ConfirmAction::DeleteMessage(Box::new(message))),
            cx,
        );
    }

    /// Deletes a message undoably: it goes now, for good once its toast has.
    fn delete_with_undo(&mut self, message: Message, cx: &mut Context<Self>) {
        let Some((timeline, Some(handle))) = self.active_timeline(cx) else {
            return;
        };
        timeline.update(cx, |timeline, _| timeline.hide(&message.unique_id));
        self.undoable(
            "Message deleted",
            PendingUndo::Delete {
                timeline: timeline.downgrade(),
                handle,
                message,
            },
            cx,
        );
    }

    /// Shows `text` with an Undo button; once it has gone the action is
    /// done for good. The caller has already made it look done.
    pub fn undoable(&mut self, text: &str, pending: PendingUndo, cx: &mut Context<Self>) {
        let id = self.toasts.push(text, true);
        self.pending.insert(id, pending);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(UNDO_SECS))
                .await;
            this.update(cx, |this, cx| {
                this.commit(id, cx);
                if this.toasts.dismiss(id) {
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// The Undo button of a toast.
    pub fn undo_toast(&mut self, id: u32, cx: &mut Context<Self>) {
        match self.pending.remove(&id) {
            Some(PendingUndo::Delete {
                timeline, message, ..
            }) => {
                if let Some(timeline) = timeline.upgrade() {
                    timeline.update(cx, |timeline, cx| timeline.show(&message.unique_id, cx));
                }
            }
            Some(PendingUndo::Leave(room)) => {
                self.hidden_rooms.remove(room.as_str());
            }
            None => {}
        }
        if self.toasts.dismiss(id) {
            cx.notify();
        }
    }

    /// The toast ran out: the action is done for good.
    fn commit(&mut self, id: u32, cx: &mut Context<Self>) {
        match self.pending.remove(&id) {
            Some(PendingUndo::Delete {
                timeline,
                handle,
                message,
            }) => {
                let redacting = handle.redact(message.id.clone());
                cx.spawn(async move |this, cx| {
                    let result = redacting.await;
                    this.update(cx, |this, cx| {
                        if let Err(err) = result {
                            this.fail("delete", &err.to_string(), cx);
                        }
                        if let Some(timeline) = timeline.upgrade() {
                            timeline.update(cx, |timeline, cx| timeline.show(&message.unique_id, cx));
                        }
                    })
                    .ok();
                })
                .detach();
            }
            Some(PendingUndo::Leave(room)) => {
                if let Some(session) = self.session.clone() {
                    let leaving = session.leave_room(room.clone());
                    cx.spawn(async move |this, cx| {
                        let result = leaving.await;
                        this.update(cx, |this, cx| {
                            if let Err(err) = result {
                                this.fail("leave", &err.to_string(), cx);
                            }
                            this.hidden_rooms.remove(room.as_str());
                            cx.notify();
                        })
                        .ok();
                    })
                    .detach();
                }
            }
            None => {}
        }
    }

    pub fn dialog_focus(&mut self, cx: &mut Context<Self>) -> FocusHandle {
        self.dialog_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone()
    }

    pub fn menu_focus(&mut self, cx: &mut Context<Self>) -> FocusHandle {
        self.menu_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone()
    }
}

// ---- drawing --------------------------------------------------------------

/// `.overlay`: the dimmed window, the dialog near the top.
pub(crate) fn overlay(window: &Window, content: AnyElement, close: impl Fn(&mut Window, &mut gpui::App) + 'static) -> AnyElement {
    let top = 0.15 * f32::from(window.viewport_size().height);
    let layer = div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .justify_center()
        .items_start()
        .pt(px(top))
        .bg(rgba(0x0000008c))
        .occlude()
        .on_mouse_down(MouseButton::Left, move |_, window, cx| close(window, cx))
        .child(content)
        .e2e("div", "overlay");
    deferred(layer).with_priority(20).into_any_element()
}

/// `.message-dialog`: the box.
pub(crate) fn dialog_box() -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(12.0))
        .w(px(440.0))
        .max_w(relative(0.9))
        .max_h(relative(0.8))
        .p(px(20.0))
        .rounded(px(theme::RADIUS_XL))
        .bg(rgb(theme::BG_SIDEBAR))
        .shadow_lg()
        .text_color(rgb(theme::TEXT))
        .line_height(relative(theme::LINE_HEIGHT))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

pub(crate) fn heading(text: &str) -> impl IntoElement {
    div()
        .text_size(px(1.1 * theme::REM))
        .font_weight(FontWeight::BOLD)
        .child(text.to_owned())
        .e2e("h2", "")
        .text(text)
}

pub(crate) fn muted(text: &str, class: &str) -> impl IntoElement {
    div()
        .text_color(rgb(theme::TEXT_MUTED))
        .child(text.to_owned())
        .e2e("p", class)
        .text(text)
}

/// A button of a dialog: danger is a red block, others plain text.
pub(crate) fn dialog_button(id: impl Into<SharedString>, label: &str, danger: bool) -> Stateful<Div> {
    let id: SharedString = id.into();
    div()
        .id(ElementId::from(id))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .rounded(px(theme::RADIUS))
        .when(danger, |d| {
            d.bg(rgb(theme::DANGER))
                .text_color(rgb(0xffffff))
                .px(px(14.0))
                .py(px(8.0))
        })
        .when(!danger, |d| d.px(px(6.0)).py(px(1.0)))
        .child(label.to_owned())
}

pub(crate) fn actions(buttons: Vec<AnyElement>) -> impl IntoElement {
    div()
        .flex()
        .gap(px(8.0))
        .my(px(12.0))
        .children(buttons)
        .e2e("div", "settings-actions")
}

/// Whatever dialog is open, drawn over the window.
pub fn render_dialog(shell: &mut Shell, window: &mut Window, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let focus = shell.dialog_focus(cx);
    if std::mem::take(&mut shell.take_dialog_focus) {
        match &shell.dialog {
            Some(Dialog::Confirm(..) | Dialog::Reactions(_) | Dialog::Help | Dialog::About) => window.focus(&focus),
            _ => {}
        }
    }
    let dialog = shell.dialog.as_ref()?;
    let close = {
        let shell = cx.weak_entity();
        move |_: &mut Window, cx: &mut gpui::App| {
            shell.update(cx, |this, cx| this.dialog_close(cx)).ok();
        }
    };
    match dialog {
        Dialog::Confirm(confirm, action) => {
            let (confirm, action) = (confirm.clone(), action.clone());
            let go = {
                let action = action.clone();
                cx.listener(move |this, _: &gpui::ClickEvent, _, cx| this.confirmed(action.clone(), cx))
            };
            let boxed = dialog_box()
                .id("confirm-dialog")
                .track_focus(&focus)
                .on_key_down(cx.listener({
                    let action = action.clone();
                    move |this, event: &gpui::KeyDownEvent, _, cx| match event.keystroke.key.as_str() {
                        "enter" => this.confirmed(action.clone(), cx),
                        "escape" => this.dialog_close(cx),
                        _ => {}
                    }
                }))
                .child(heading(&confirm.title))
                .child(muted(&confirm.body, "muted"))
                .child(actions(vec![
                    dialog_button("confirm-cancel", "Cancel", false)
                        .text_color(rgb(theme::TEXT))
                        .on_click(cx.listener(|this, _, _, cx| this.dialog_close(cx)))
                        .e2e("button", "secondary")
                        .text("Cancel")
                        .into_any_element(),
                    dialog_button("confirm-go", &confirm.confirm, true)
                        .on_click(go)
                        .e2e("button", "danger")
                        .text(confirm.confirm.clone())
                        .into_any_element(),
                ]))
                .e2e("div", "message-dialog confirm-dialog")
                .attr("role", "alertdialog")
                .focusable(&focus);
            Some(overlay(window, boxed.into_any_element(), close))
        }
        Dialog::Reactions(message) => {
            let boxed = dialog_box()
                .id("reactions-dialog")
                .track_focus(&focus)
                .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                    if event.keystroke.key == "escape" {
                        this.dialog_close(cx);
                    }
                }))
                .child(heading("Reactions"))
                .when(message.reactions.is_empty(), |d| {
                    d.child(muted("No reactions yet.", "muted"))
                })
                .children(message.reactions.iter().map(|reaction| {
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(12.0))
                        .child(
                            div()
                                .text_size(px(1.2 * theme::REM))
                                .child(reaction.key.clone())
                                .e2e("span", "reaction-key")
                                .text(reaction.key.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .text_color(rgb(theme::TEXT_MUTED))
                                .text_size(px(0.9 * theme::REM))
                                .children(reaction.senders.iter().enumerate().map(|(i, sender)| {
                                    let name = sender.localpart().to_owned();
                                    let shown = if i > 0 { format!(", {name}") } else { name.clone() };
                                    div()
                                        .child(shown)
                                        .e2e("span", "")
                                        .attr("title", sender.to_string())
                                        .text(name)
                                }))
                                .e2e("span", "reaction-senders"),
                        )
                        .e2e("div", "reaction-row")
                }))
                .child(actions(vec![
                    dialog_button("reactions-close", "Close", false)
                        .text_color(rgb(theme::TEXT))
                        .on_click(cx.listener(|this, _, _, cx| this.dialog_close(cx)))
                        .e2e("button", "secondary")
                        .text("Close")
                        .into_any_element(),
                ]))
                .e2e("div", "message-dialog reactions-dialog")
                .focusable(&focus);
            Some(overlay(window, boxed.into_any_element(), close))
        }
        Dialog::Report(entity) => Some(entity.clone().into_any_element()),
        Dialog::Pinned(entity) => Some(entity.clone().into_any_element()),
        Dialog::Room(entity) => Some(entity.clone().into_any_element()),
        Dialog::ModView(entity) => Some(entity.clone().into_any_element()),
        Dialog::Help => {
            let boxed = dialog_box()
                .id("help-dialog")
                .track_focus(&focus)
                .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                    if event.keystroke.key == "escape" {
                        this.dialog_close(cx);
                    }
                }))
                .w(px(560.0))
                .max_h(px(520.0))
                .child(heading("Keyboard shortcuts"))
                .children(crate::view_model::hotkeys::GROUPS.iter().map(|(group, rows)| {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .mt(px(8.0))
                                .text_size(px(0.75 * theme::REM))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(theme::TEXT_FAINT))
                                .child(group.to_string())
                                .e2e("h3", "")
                                .text(group.to_string()),
                        )
                        .children(rows.iter().map(|(keys, what)| {
                            div()
                                .flex()
                                .gap(px(12.0))
                                .child(
                                    div()
                                        .w(px(200.0))
                                        .flex_none()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(keys.to_string())
                                        .e2e("kbd", "")
                                        .text(keys.to_string()),
                                )
                                .child(
                                    div()
                                        .text_color(rgb(theme::TEXT_MUTED))
                                        .child(what.to_string())
                                        .e2e("span", "")
                                        .text(what.to_string()),
                                )
                                .e2e("div", "help-row")
                        }))
                        .e2e("section", "help-group")
                }))
                .e2e("div", "message-dialog help-dialog")
                .text("Keyboard shortcuts")
                .focusable(&focus);
            Some(overlay(window, boxed.into_any_element(), close))
        }
        Dialog::Preview(entity) => Some(entity.clone().into_any_element()),
        Dialog::About => Some(crate::about::render(&focus, window, cx, close)),
    }
}

impl Shell {
    fn confirmed(&mut self, action: ConfirmAction, cx: &mut Context<Self>) {
        self.dialog_close(cx);
        match action {
            ConfirmAction::DeleteMessage(message) => self.delete_with_undo(*message, cx),
        }
    }
}

/// The corner's stack of toasts.
pub fn render_toasts(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    if shell.toasts.items.is_empty() {
        return None;
    }
    let stack = div()
        .absolute()
        .right(px(16.0))
        .bottom(px(theme::BOTTOM_H + 24.0))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .max_w(px(360.0))
        .children(shell.toasts.items.iter().map(|toast| {
            let id = toast.id;
            div()
                .id(ElementId::from(SharedString::from(format!("toast-{id}"))))
                .flex()
                .items_center()
                .gap(px(12.0))
                .px(px(14.0))
                .py(px(10.0))
                .rounded(px(theme::RADIUS))
                .bg(rgb(theme::BG_ELEVATED))
                .border_l_4()
                .border_color(rgb(theme::DANGER))
                .shadow_md()
                .text_size(px(0.9 * theme::REM))
                .text_color(rgb(theme::TEXT))
                .cursor_pointer()
                .occlude()
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.toasts.dismiss(id) {
                        cx.notify();
                    }
                }))
                .child(div().flex_1().child(toast.text.clone()).e2e("span", "toast-text").text(toast.text.clone()))
                .when(toast.undo, |d| {
                    d.child(
                        div()
                            .id(ElementId::from(SharedString::from(format!("undo-{id}"))))
                            .px(px(4.0))
                            .py(px(6.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(theme::ACCENT))
                            .cursor_pointer()
                            .hover(|s| s.underline())
                            .child("Undo")
                            .on_click(cx.listener(move |this, _, _, cx| this.undo_toast(id, cx)))
                            .e2e("button", "toast-undo")
                            .text("Undo"),
                    )
                })
                .e2e("div", "toast")
                .attr("role", "status")
        }))
        .e2e("div", "toast-layer")
        .attr("aria-live", "polite");
    Some(deferred(stack).with_priority(40).into_any_element())
}

// ---- report -----------------------------------------------------------------

/// "Report message": a reason, sent to the homeserver's admins.
pub struct ReportDialog {
    session: Session,
    room: OwnedRoomId,
    event_id: OwnedEventId,
    input: Entity<TextInput>,
    focus: FocusHandle,
    busy: bool,
    result: Option<Result<&'static str, String>>,
    take_focus: bool,
}

impl EventEmitter<Closed> for ReportDialog {}

impl ReportDialog {
    fn new(session: Session, room: OwnedRoomId, event_id: OwnedEventId, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new("reason", "Spam, abuse, …", false, "", cx).framed());
        cx.subscribe(&input, |this, _, _: &Submit, cx| this.submit(cx)).detach();
        Self {
            session,
            room,
            event_id,
            input,
            focus: cx.focus_handle(),
            busy: false,
            result: None,
            take_focus: true,
        }
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.busy || matches!(self.result, Some(Ok(_))) {
            return;
        }
        self.busy = true;
        let reason = self.input.read(cx).value().to_owned();
        let reporting = self
            .session
            .report(self.room.clone(), self.event_id.clone(), reason);
        cx.spawn(async move |this, cx| {
            let result = reporting.await;
            this.update(cx, |this, cx| {
                this.busy = false;
                this.result = Some(match result {
                    Ok(()) => Ok("Reported to the server admins."),
                    Err(err) => Err(err.to_string()),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
}

impl Render for ReportDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.take_focus) {
            self.input.read(cx).focus(window);
        }
        let reported = matches!(self.result, Some(Ok(_)));
        let mut buttons = Vec::new();
        if !reported {
            buttons.push(
                dialog_button("report-send", "Report", true)
                    .when(self.busy, |d| d.opacity(0.6))
                    .on_click(cx.listener(|this, _, _, cx| this.submit(cx)))
                    .e2e("button", "danger")
                    .attr("type", "submit")
                    .text("Report")
                    .into_any_element(),
            );
        }
        buttons.push(
            dialog_button("report-close", "Close", false)
                .text_color(rgb(theme::TEXT))
                .on_click(cx.listener(|_, _, _, cx| cx.emit(Closed)))
                .e2e("button", "secondary")
                .attr("type", "button")
                .text("Close")
                .into_any_element(),
        );
        let boxed = dialog_box()
            .id("report-dialog")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(Closed);
                }
            }))
            .child(heading("Report message"))
            .child(muted(
                "Tell the homeserver admins what is wrong with this message.",
                "muted",
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .text_size(px(0.85 * theme::REM))
                    .text_color(rgb(theme::TEXT_MUTED))
                    .child("Reason")
                    .child(
                        div()
                            .text_size(px(theme::REM))
                            .text_color(rgb(theme::TEXT))
                            .child(self.input.clone()),
                    )
                    .e2e("label", "")
                    .text("Reason"),
            )
            .children(match &self.result {
                Some(Ok(text)) => Some(
                    div()
                        .text_color(rgb(0x6bd68a))
                        .child(*text)
                        .e2e("p", "ok")
                        .text(*text)
                        .into_any_element(),
                ),
                Some(Err(text)) => Some(
                    div()
                        .text_color(rgb(theme::DANGER))
                        .child(text.clone())
                        .e2e("p", "error")
                        .text(text.clone())
                        .into_any_element(),
                ),
                None => None,
            })
            .child(actions(buttons))
            .e2e("form", "message-dialog report-dialog")
            .focusable(&self.focus);
        overlay(
            window,
            boxed.into_any_element(),
            {
                let this = cx.weak_entity();
                move |_, cx| {
                    this.update(cx, |_, cx| cx.emit(Closed)).ok();
                }
            },
        )
    }
}

// ---- pinned ----------------------------------------------------------------

/// The room's pinned messages, with unpin for each.
pub struct PinnedDialog {
    session: Session,
    room: OwnedRoomId,
    list: Option<Result<Vec<PinnedMessage>, String>>,
    focus: FocusHandle,
    take_focus: bool,
}

impl EventEmitter<Closed> for PinnedDialog {}
impl EventEmitter<PinsChanged> for PinnedDialog {}

impl PinnedDialog {
    fn new(session: Session, room: OwnedRoomId, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            session,
            room,
            list: None,
            focus: cx.focus_handle(),
            take_focus: true,
        };
        this.load(cx);
        this
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let reading = self.session.pinned_messages(self.room.clone());
        cx.spawn(async move |this, cx| {
            let list = reading.await.map_err(|err| err.to_string());
            this.update(cx, |this, cx| {
                this.list = Some(list);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn unpin(&mut self, event_id: OwnedEventId, cx: &mut Context<Self>) {
        let unpinning = self.session.pin(self.room.clone(), event_id, false);
        cx.spawn(async move |this, cx| {
            if let Err(err) = unpinning.await {
                eprintln!("morpheus-gpui: unpin: {err}");
            }
            this.update(cx, |this, cx| {
                this.load(cx);
                cx.emit(PinsChanged);
            })
            .ok();
        })
        .detach();
    }
}

impl Render for PinnedDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.take_focus) {
            window.focus(&self.focus);
        }
        let mut boxed = dialog_box()
            .id("pinned-dialog")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(Closed);
                }
            }))
            .child(heading("Pinned messages"));
        match &self.list {
            None => {}
            Some(Err(err)) => {
                boxed = boxed.child(
                    div()
                        .text_color(rgb(theme::DANGER))
                        .child(err.clone())
                        .e2e("p", "error")
                        .text(err.clone()),
                );
            }
            Some(Ok(list)) if list.is_empty() => {
                boxed = boxed.child(muted(
                    "Nothing pinned yet. Right-click a message to pin it.",
                    "muted",
                ));
            }
            Some(Ok(list)) => {
                for item in list.iter().rev() {
                    let event_id = item.event_id.clone();
                    boxed = boxed.child(
                        div()
                            .flex()
                            .items_start()
                            .gap(px(8.0))
                            .px(px(10.0))
                            .py(px(8.0))
                            .rounded(px(theme::RADIUS))
                            .bg(rgb(theme::BG_ELEVATED))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(item.sender.localpart().to_owned())
                                            .e2e("span", "sender")
                                            .text(item.sender.localpart().to_owned()),
                                    )
                                    .child(
                                        div()
                                            .child(item.body.clone())
                                            .e2e("p", "body")
                                            .text(item.body.clone()),
                                    )
                                    .e2e("div", "pinned-main"),
                            )
                            .child(
                                div()
                                    .id(ElementId::from(SharedString::from(format!(
                                        "unpin-{}",
                                        item.event_id
                                    ))))
                                    .cursor_pointer()
                                    .text_color(rgb(theme::TEXT_MUTED))
                                    .child("✕")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.unpin(event_id.clone(), cx)
                                    }))
                                    .e2e("button", "ghost")
                                    .attr("title", "Unpin")
                                    .text("✕"),
                            )
                            .e2e("div", "pinned-item"),
                    );
                }
            }
        }
        let boxed = boxed
            .child(actions(vec![
                dialog_button("pinned-close", "Close", false)
                    .text_color(rgb(theme::TEXT))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(Closed)))
                    .e2e("button", "secondary")
                    .text("Close")
                    .into_any_element(),
            ]))
            .e2e("div", "message-dialog pinned-dialog")
            .focusable(&self.focus);
        let this = cx.weak_entity();
        overlay(window, boxed.into_any_element(), move |_, cx| {
            this.update(cx, |_, cx| cx.emit(Closed)).ok();
        })
    }
}
