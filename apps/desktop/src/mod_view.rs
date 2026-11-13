//! The mod view (`crates/ui/src/components/mod_view.rs`): one sheet for a
//! person, every room you share with where they stand there and the
//! moderation you may do in each. Which controls a row has is decided in
//! `view_model::mod_view`; this draws them and runs the calls.

use gpui::{
    AnyElement, Context, ElementId, Entity, EventEmitter, FocusHandle, FontWeight, IntoElement,
    Render, SharedString, Window, div, prelude::*, px, rgb, rgba,
};
use morpheus_core::{MemberRole, ModRoom, OwnedRoomId, OwnedUserId, Session};

use crate::{
    dialogs::{Closed, actions, dialog_box, overlay},
    e2e::E2eExt as _,
    icons::icon,
    input::TextInput,
    media::avatar,
    theme,
    view_model::mod_view::{LOOKING, NO_ROOMS, RowView, given_reason, row_view},
};

/// Somebody was moderated: member lists should be read again.
pub struct Moderated;

pub struct ModView {
    session: Session,
    user: OwnedUserId,
    /// The name to show them by (your nickname, else their own).
    shown: String,
    rooms: Option<Result<Vec<ModRoom>, String>>,
    reason: Entity<TextInput>,
    /// The room an action is running in.
    busy: Option<OwnedRoomId>,
    /// The room whose role list is open.
    open_roles: Option<OwnedRoomId>,
    focus: FocusHandle,
    take_focus: bool,
}

impl EventEmitter<Closed> for ModView {}
impl EventEmitter<Moderated> for ModView {}

impl ModView {
    pub fn new(session: Session, user: OwnedUserId, shown: String, cx: &mut Context<Self>) -> Self {
        let reason = cx.new(|cx| TextInput::new("reason", "Optional", false, "", cx).framed());
        let mut this = Self {
            session,
            user,
            shown,
            rooms: None,
            reason,
            busy: None,
            open_roles: None,
            focus: cx.focus_handle(),
            take_focus: true,
        };
        this.load(cx);
        this
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let reading = self.session.mod_overview(self.user.clone());
        cx.spawn(async move |this, cx| {
            let list = reading.await.map_err(|err| err.to_string());
            this.update(cx, |this, cx| {
                this.rooms = Some(list);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// One moderation action in one room, then a fresh look at every room.
    fn act(
        &mut self,
        room: OwnedRoomId,
        what: &'static str,
        action: morpheus_core::CoreFuture<()>,
        cx: &mut Context<Self>,
    ) {
        self.busy = Some(room);
        self.open_roles = None;
        cx.spawn(async move |this, cx| {
            let result = action.await;
            this.update(cx, |this, cx| {
                if let Err(err) = result {
                    eprintln!("morpheus-desktop: {what}: {err}");
                }
                this.busy = None;
                this.load(cx);
                cx.emit(Moderated);
                cx.notify();
            })
            .ok();
            // The change reaches the room state with the next sync: look again.
            for wait in [1u64, 3] {
                cx.background_executor().timer(std::time::Duration::from_secs(wait)).await;
                this.update(cx, |this, cx| this.load(cx)).ok();
            }
        })
        .detach();
        cx.notify();
    }

    fn reason(&self, cx: &Context<Self>) -> Option<String> {
        given_reason(self.reason.read(cx).value())
    }

    fn small_button(
        &self,
        id: String,
        glyph: &'static str,
        label: &'static str,
        class: &'static str,
        busy: bool,
        on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    ) -> AnyElement {
        let danger = class.contains("danger");
        div()
            .id(ElementId::from(SharedString::from(id)))
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(12.0))
            .py(px(6.0))
            .rounded(px(theme::RADIUS_SM))
            .cursor_pointer()
            .when(busy, |d| d.opacity(0.6))
            .when(danger, |d| d.bg(rgb(theme::danger())).text_color(rgb(0xffffff)))
            .when(!danger, |d| d.text_color(rgb(theme::text())))
            .child(icon(glyph, 16.0).text_color(rgb(if danger { 0xffffff } else { theme::text() })))
            .child(label)
            .on_click(on_click)
            .e2e("button", class)
            .text(label)
            .into_any_element()
    }

    fn role_select(&self, room: &ModRoom, roles: &[(MemberRole, &'static str)], current: MemberRole, cx: &mut Context<Self>) -> AnyElement {
        let id = room.room_id.clone();
        let open = self.open_roles.as_ref() == Some(&id);
        let shown = roles
            .iter()
            .find(|(role, _)| *role == current)
            .map_or("Member", |(_, name)| name);
        let head = div()
            .id(ElementId::from(SharedString::from(format!("role-{id}"))))
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(10.0))
            .py(px(4.0))
            .rounded(px(theme::RADIUS_SM))
            .border_1()
            .border_color(rgba(theme::bg_selected()))
            .bg(rgb(theme::bg_elevated()))
            .text_size(px(0.8 * theme::REM))
            .text_color(rgb(theme::text()))
            .cursor_pointer()
            .child(shown)
            .child(icon("chevron-down", 12.0).text_color(rgb(theme::text_muted())))
            .on_click({
                let id = id.clone();
                cx.listener(move |this, _, _, cx| {
                    this.open_roles = if this.open_roles.as_ref() == Some(&id) { None } else { Some(id.clone()) };
                    cx.notify();
                })
            });
        let list = open.then(|| {
            div()
                .flex()
                .flex_col()
                .mt(px(2.0))
                .rounded(px(theme::RADIUS_SM))
                .bg(rgb(theme::bg_elevated()))
                .text_size(px(0.8 * theme::REM))
                .text_color(rgb(theme::text()))
                .children(roles.iter().map(|(role, label)| {
                    let (role, room_id) = (*role, id.clone());
                    div()
                        .id(ElementId::from(SharedString::from(format!("role-{room_id}-{label}"))))
                        .px(px(10.0))
                        .py(px(6.0))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgba(theme::bg_hover())))
                        .child(*label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let (user, session) = (this.user.clone(), this.session.clone());
                            let setting = session.set_role(room_id.clone(), user, role);
                            this.act(room_id.clone(), "changing the role", setting, cx);
                        }))
                        .e2e("option", "")
                        .attr("value", *label)
                        .text(*label)
                }))
        });
        div()
            .flex()
            .flex_col()
            .child(head)
            .children(list)
            .e2e("select", "mod-role")
            .into_any_element()
    }

    fn room_row(&self, room: &ModRoom, cx: &mut Context<Self>) -> AnyElement {
        let view: RowView = row_view(room);
        let busy = self.busy.as_ref() == Some(&room.room_id);
        let info_tail = match &view.roles {
            Some((roles, current)) => self.role_select(room, roles, *current, cx),
            None => div()
                .text_size(px(0.8 * theme::REM))
                .text_color(rgb(if view.state == "Banned" { theme::danger() } else { theme::text_muted() }))
                .child(view.state)
                .e2e("span", "mod-state")
                .text(view.state)
                .into_any_element(),
        };
        let mut buttons: Vec<AnyElement> = Vec::new();
        let (id, user) = (room.room_id.clone(), self.user.clone());
        if view.kick {
            let (id, user) = (id.clone(), user.clone());
            buttons.push(self.small_button(format!("kick-{id}"), "user-x", "Kick", "secondary mod-kick", busy, cx.listener(move |this, _, _, cx| {
                let reason = this.reason(cx);
                let kicking = this.session.kick(id.clone(), user.clone(), reason);
                this.act(id.clone(), "kicking", kicking, cx);
            })));
        }
        if view.unban {
            let (id, user) = (id.clone(), user.clone());
            buttons.push(self.small_button(format!("unban-{id}"), "circle-minus", "Unban", "secondary mod-unban", busy, cx.listener(move |this, _, _, cx| {
                let unbanning = this.session.unban(id.clone(), user.clone());
                this.act(id.clone(), "unbanning", unbanning, cx);
            })));
        }
        if view.ban {
            let (id, user) = (id.clone(), user.clone());
            buttons.push(self.small_button(format!("ban-{id}"), "ban", "Ban", "danger mod-ban", busy, cx.listener(move |this, _, _, cx| {
                let reason = this.reason(cx);
                let banning = this.session.ban(id.clone(), user.clone(), reason);
                this.act(id.clone(), "banning", banning, cx);
            })));
        }
        let name = format!("# {}", room.name);
        div()
            .flex()
            .items_center()
            .gap(px(12.0))
            .px(px(12.0))
            .py(px(8.0))
            .rounded(px(theme::RADIUS))
            .bg(rgb(theme::bg_input()))
            .when(busy, |d| d.opacity(0.6))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .items_start()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(name.clone())
                            .e2e("span", "mod-room-name")
                            .text(name),
                    )
                    .child(info_tail),
            )
            .child(div().flex().flex_none().gap(px(6.0)).children(buttons).e2e("div", "mod-actions"))
            .e2e("div", if busy { "mod-room busy" } else { "mod-room" })
            .attr("data-name", room.name.clone())
            .attr("data-standing", view.state)
            .into_any_element()
    }
}

impl Render for ModView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.take_focus) {
            window.focus(&self.focus);
        }
        let rows: Vec<AnyElement> = match &self.rooms {
            None => vec![div().text_color(rgb(theme::text_muted())).child(LOOKING).e2e("p", "muted").text(LOOKING).into_any_element()],
            Some(Err(err)) => vec![div().text_color(rgb(theme::danger())).child(err.clone()).e2e("p", "error").text(err.clone()).into_any_element()],
            Some(Ok(list)) if list.is_empty() => vec![div().text_color(rgb(theme::text_muted())).child(NO_ROOMS).e2e("p", "muted").text(NO_ROOMS).into_any_element()],
            Some(Ok(list)) => list.iter().map(|room| self.room_row(room, cx)).collect(),
        };
        let header = div()
            .flex()
            .items_center()
            .gap(px(12.0))
            .child(avatar(None, &self.shown, 48.0, None))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(1.1 * theme::REM))
                            .font_weight(FontWeight::BOLD)
                            .child(self.shown.clone())
                            .e2e("h2", "")
                            .text(self.shown.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(0.8 * theme::REM))
                            .text_color(rgb(theme::text_muted()))
                            .child(self.user.to_string())
                            .e2e("span", "mod-view-id")
                            .text(self.user.to_string()),
                    ),
            )
            .e2e("header", "mod-view-header");
        let reason = div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .text_size(px(0.85 * theme::REM))
            .text_color(rgb(theme::text_muted()))
            .child("Reason (for kicks and bans)")
            .child(
                div()
                    .text_size(px(theme::REM))
                    .text_color(rgb(theme::text()))
                    .child(self.reason.clone()),
            )
            .e2e("label", "mod-view-reason");
        let boxed = dialog_box()
            .id("mod-view")
            .w(px(560.0))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(Closed);
                }
            }))
            .child(header)
            .child(reason)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .children(rows)
                    .e2e("div", "mod-rooms"),
            )
            .child(actions(vec![
                div()
                    .id("mod-view-close")
                    .px(px(6.0))
                    .py(px(1.0))
                    .cursor_pointer()
                    .child("Close")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(Closed)))
                    .e2e("button", "secondary")
                    .text("Close")
                    .into_any_element(),
            ]))
            .e2e("div", "message-dialog mod-view")
            .focusable(&self.focus);
        let this = cx.weak_entity();
        overlay(window, boxed.into_any_element(), move |_, cx| {
            this.update(cx, |_, cx| cx.emit(Closed)).ok();
        })
    }
}
