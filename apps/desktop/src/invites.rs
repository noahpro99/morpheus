//! Invites, the inbox and the join card (`crates/ui/src/components/invites.rs`
//! and the inbox of `titlebar.rs`): invite rows at the top of the sidebar, the
//! card in place of the chat for an invited room, the card to join a
//! server's channel you are not in, and the inbox panel from the title bar.
//! What they say is in `view_model::invites`.

use gpui::{
    AnyElement, Context, ElementId, FontWeight, IntoElement, MouseButton, SharedString, Window,
    div, prelude::*, px, rgb, rgba,
};
use morpheus_core::{OwnedRoomId, RoomSummary, SpaceRoom};

use crate::{
    e2e::E2eExt as _,
    icons::icon,
    media::avatar,
    shell::Shell,
    theme,
    view_model::invites::{self, ItemStatus},
};

impl Shell {
    /// Join an invited room; the invite goes away when the list updates.
    pub fn accept_invite(&mut self, id: OwnedRoomId, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let joining = session.join_room(id.clone());
        cx.spawn(async move |this, cx| {
            let result = joining.await;
            this.update(cx, |this, cx| match result {
                Ok(()) => {
                    // Joined: the list is told by the next sync, which can be
                    // a while coming; until then it is joined here.
                    this.joined_here.insert(id.clone());
                    if let Some(room) = this.rooms.iter_mut().find(|r| r.id == id) {
                        room.invited = false;
                    }
                    this.open_pending_dm(cx);
                    cx.notify();
                }
                Err(err) => this.fail("accepting an invite", &err.to_string(), cx),
            })
            .ok();
        })
        .detach();
    }
    /// Declining is leaving the invited room.
    pub fn decline_invite(&mut self, id: OwnedRoomId, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let leaving = session.leave_room(id.clone());
        cx.spawn(async move |this, cx| {
            let result = leaving.await;
            this.update(cx, |this, cx| match result {
                Ok(()) => {
                    if this.selected.as_ref() == Some(&id) {
                        this.selected = None;
                        this.timeline = None;
                        this.composer = None;
                    }
                    cx.notify();
                }
                Err(err) => this.fail("declining an invite", &err.to_string(), cx),
            })
            .ok();
        })
        .detach();
    }

    /// A channel of the server that you are not in: its join card.
    pub fn open_unjoined(&mut self, room: SpaceRoom, cx: &mut Context<Self>) {
        self.friends_open = false;
        self.stash_timeline(cx);
        self.composer = None;
        self.timeline_handle = None;
        self.members = None;
        self.thread = None;
        self.selected = Some(room.id.clone());
        self.preview = Some(room);
        cx.notify();
    }

    /// The join card's button: join, and open the room once it is listed.
    pub fn join_preview(&mut self, room: SpaceRoom, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        self.joining = true;
        let id = room.id.clone();
        let joining = session.join_room_via(room.id, room.via);
        cx.spawn(async move |this, cx| {
            let result = joining.await;
            this.update(cx, |this, cx| {
                this.joining = false;
                match result {
                    Ok(()) => {
                        this.pending_dm = Some(id);
                        this.open_pending_dm(cx);
                    }
                    Err(err) => this.fail("join room", &err.to_string(), cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub fn toggle_inbox(&mut self, cx: &mut Context<Self>) {
        self.inbox_open = !self.inbox_open;
        self.inbox_take_focus = self.inbox_open;
        cx.notify();
    }

    fn close_inbox(&mut self, cx: &mut Context<Self>) {
        if self.inbox_open {
            self.inbox_open = false;
            cx.notify();
        }
    }

    /// Opens a room from the inbox, in the scope that lists it.
    fn open_from_inbox(&mut self, room: &RoomSummary, cx: &mut Context<Self>) {
        let scope = crate::model::scope_for(&self.rooms, room);
        self.set_scope(scope, cx);
        self.open_room(room.id.clone(), cx);
        self.close_inbox(cx);
    }
}

fn round_button(
    id: impl Into<SharedString>,
    _title: &'static str,
    glyph: &'static str,
    accept: bool,
) -> gpui::Stateful<gpui::Div> {
    let id: SharedString = id.into();
    div()
        .id(ElementId::from(id))
        .flex_none()
        .size(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(rgba(theme::bg_hover()))
        .text_color(rgb(theme::text_muted()))
        .cursor_pointer()
        .hover(move |s| {
            s.bg(rgb(if accept { theme::green() } else { theme::danger() }))
                .text_color(rgb(0xffffff))
        })
        .child(icon(glyph, 14.0))
}


/// An invite in the sidebar: the room, who asked, accept and decline.
pub fn invite_row(shell: &Shell, cx: &mut Context<Shell>, room: &RoomSummary) -> AnyElement {
    let selected = shell.selected.as_ref() == Some(&room.id);
    let (open, accept, decline) = (room.id.clone(), room.id.clone(), room.id.clone());
    let from = invites::from_line(room);
    div()
        .id(SharedString::from(format!("invite-{}", room.id)))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.0))
        .px(px(8.0))
        .py(px(6.0))
        .rounded(px(theme::RADIUS_SM))
        .cursor_pointer()
        .text_color(rgb(if selected { theme::text() } else { theme::text_muted() }))
        .when(selected, |d| d.bg(rgba(theme::bg_selected())))
        .when(!selected, |d| {
            d.hover(|s| s.bg(rgba(theme::bg_hover())).text_color(rgb(theme::text())))
        })
        .on_click(cx.listener(move |this, _, _, cx| this.open_room(open.clone(), cx)))
        .child(avatar(shell.picture(&room.avatar, 24), &room.name, 24.0, None))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .w_0()
                .min_w_0()
                .overflow_hidden()
                .line_height(gpui::relative(1.15))
                .child(
                    div()
                        .w_full().truncate()
                        .child(room.name.clone())
                        .e2e("span", "room-name")
                        .text(room.name.clone()),
                )
                .child(
                    div()
                        .w_full().truncate()
                        .text_size(px(0.72 * theme::REM))
                        .text_color(rgb(theme::text_muted()))
                        .child(from.clone())
                        .e2e("span", "invite-from")
                        .text(from),
                )
                .e2e("span", "invite-text"),
        )
        .child(
            round_button(format!("invite-accept-{}", room.id), "Accept", "check", true)
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.accept_invite(accept.clone(), cx)
                }))
                .e2e("button", "invite-accept")
                .attr("title", "Accept"),
        )
        .child(
            round_button(format!("invite-decline-{}", room.id), "Decline", "x", false)
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.decline_invite(decline.clone(), cx)
                }))
                .e2e("button", "invite-decline")
                .attr("title", "Decline"),
        )
        .e2e(
            "div",
            if selected {
                "room invite-row selected"
            } else {
                "room invite-row"
            },
        )
        .attr("role", "button")
        .into_any_element()
}

fn card_button(id: &'static str, label: &'static str, primary: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px(px(16.0))
        .py(px(8.0))
        .rounded(px(theme::RADIUS))
        .cursor_pointer()
        .font_weight(FontWeight::SEMIBOLD)
        .bg(if primary {
            gpui::Hsla::from(rgb(theme::accent_strong()))
        } else {
            gpui::Hsla::from(rgb(theme::bg_elevated()))
        })
        .text_color(rgb(if primary { 0xffffff } else { theme::text() }))
        .child(label)
}

fn card_shell(
    avatar_el: impl IntoElement,
    name: &str,
    line: String,
    topic: Option<&str>,
    buttons: Vec<AnyElement>,
    class: &str,
) -> AnyElement {
    let mut card = div()
        .m_auto()
        .max_w(px(420.0))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(10.0))
        .p(px(32.0))
        .text_center()
        .child(avatar_el)
        .child(
            div()
                .mt(px(4.0))
                .text_size(px(1.3 * theme::REM))
                .font_weight(FontWeight::BOLD)
                .child(name.to_owned())
                .e2e("h2", "")
                .text(name.to_owned()),
        )
        .child(
            div()
                .text_color(rgb(theme::text_muted()))
                .child(line.clone())
                .e2e("p", "muted")
                .text(line),
        );
    if let Some(topic) = topic {
        card = card.child(
            div()
                .text_color(rgb(theme::text_muted()))
                .child(topic.to_owned())
                .e2e("p", "topic")
                .text(topic.to_owned()),
        );
    }
    div()
        .flex_1()
        .flex()
        .child(
            card.child(
                div()
                    .flex()
                    .justify_center()
                    .gap(px(8.0))
                    .my(px(12.0))
                    .children(buttons)
                    .e2e("div", "settings-actions"),
            )
            .e2e("section", class),
        )
        .into_any_element()
}

/// Shown in the chat column for an invited room instead of a timeline.
pub fn invite_card(shell: &Shell, cx: &mut Context<Shell>, room: &RoomSummary) -> AnyElement {
    let (accept, decline) = (room.id.clone(), room.id.clone());
    card_shell(
        avatar(shell.picture(&room.avatar, 72), &room.name, 72.0, None),
        &room.name,
        invites::card_line(room),
        room.topic.as_deref(),
        vec![
            card_button("card-accept", "Accept", true)
                .on_click(cx.listener(move |this, _, _, cx| this.accept_invite(accept.clone(), cx)))
                .e2e("button", "primary invite-accept")
                .text("Accept")
                .into_any_element(),
            card_button("card-decline", "Decline", false)
                .on_click(cx.listener(move |this, _, _, cx| this.decline_invite(decline.clone(), cx)))
                .e2e("button", "secondary invite-decline")
                .text("Decline")
                .into_any_element(),
        ],
        "invite-card",
    )
}

/// Shown for a server's channel you aren't in: what it is, and a button to
/// join it.
pub fn join_card(shell: &Shell, cx: &mut Context<Shell>, room: &SpaceRoom) -> AnyElement {
    let joining = shell.joining;
    let target = room.clone();
    let label = if joining { "Joining…" } else { "Join" };
    let mut button = card_button("card-join", label, true);
    button = if joining {
        button.opacity(0.5)
    } else {
        button.on_click(cx.listener(move |this, _, _, cx| this.join_preview(target.clone(), cx)))
    };
    let mut button = button.e2e("button", "primary join-button").text(label);
    if joining {
        button = button.attr("disabled", "");
    }
    card_shell(
        avatar(shell.picture(&room.avatar, 72), &room.name, 72.0, None),
        &room.name,
        invites::members_label(room.member_count),
        room.topic.as_deref(),
        vec![button.into_any_element()],
        "invite-card join-card",
    )
}

/// The inbox, over the window from the title bar's button.
pub fn inbox(shell: &mut Shell, window: &mut Window, cx: &mut Context<Shell>) -> Option<AnyElement> {
    if !shell.inbox_open {
        return None;
    }
    let focus = shell
        .inbox_focus
        .get_or_insert_with(|| cx.focus_handle())
        .clone();
    if std::mem::take(&mut shell.inbox_take_focus) {
        window.focus(&focus);
    }
    let pending = invites::pending_invites(&shell.rooms);
    let items = invites::unread_rooms(&shell.rooms);
    let mut panel = div()
        .id("inbox-panel")
        .track_focus(&focus)
        .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
            if event.keystroke.key == "escape" {
                this.close_inbox(cx);
            }
        }))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .w(px(320.0))
        .max_h(px(0.7 * f32::from(window.viewport_size().height)))
        .overflow_y_scroll()
        .pb(px(6.0))
        .rounded(px(10.0))
        .bg(rgb(theme::bg_sidebar()))
        .border_1()
        .border_color(rgba(theme::edge()))
        .shadow_lg()
        .line_height(gpui::relative(theme::LINE_HEIGHT));
    let mut header = div()
        .flex()
        .items_center()
        .justify_between()
        .px(px(14.0))
        .pt(px(12.0))
        .pb(px(6.0))
        .child(
            div()
                .text_size(px(0.9 * theme::REM))
                .font_weight(FontWeight::BOLD)
                .child(invites::INBOX_TITLE)
                .e2e("h3", "")
                .text(invites::INBOX_TITLE),
        );
    if !items.is_empty() {
        let ids: Vec<OwnedRoomId> = items.iter().map(|r| r.id.clone()).collect();
        header = header.child(
            div()
                .id("inbox-mark-all")
                .px(px(8.0))
                .py(px(4.0))
                .rounded(px(theme::RADIUS_SM))
                .text_size(px(0.85 * theme::REM))
                .text_color(rgb(theme::text_muted()))
                .cursor_pointer()
                .hover(|s| s.text_color(rgb(theme::text())))
                .child(invites::MARK_ALL)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(session) = this.session.clone() {
                        for id in ids.clone() {
                            let reading = session.mark_room_read(id);
                            this.run_in_background("mark read", reading, cx);
                        }
                    }
                }))
                .e2e("button", "ghost")
                .text(invites::MARK_ALL),
        );
    }
    panel = panel.child(header.e2e("header", "inbox-header"));
    if !pending.is_empty() {
        panel = panel.child(
            div()
                .mx(px(8.0))
                .mt(px(8.0))
                .mb(px(4.0))
                .text_size(px(0.7 * theme::REM))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(theme::text_faint()))
                .child("INVITES")
                .e2e("h4", "section")
                .text("Invites"),
        );
        for room in &pending {
            let (open, accept, decline) = (room.clone(), room.id.clone(), room.id.clone());
            let name_line = format!("{} {}", room.name, invites::from_line(room));
            panel = panel.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .mx(px(6.0))
                    .mb(px(4.0))
                    .px(px(10.0))
                    .py(px(8.0))
                    .rounded(px(theme::RADIUS_SM))
                    .hover(|s| s.bg(rgba(theme::bg_hover())))
                    .child(
                        div()
                            .id(SharedString::from(format!("inbox-open-{}", room.id)))
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .cursor_pointer()
                            .child(
                                div()
                                    .flex()
                                    .gap(px(4.0))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(room.name.clone())
                                            .e2e("span", "inbox-name")
                                            .text(room.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_color(rgb(theme::text_muted()))
                                            .child(format!("from {}", invites::inviter(room)))
                                            .e2e("span", "muted")
                                            .text(format!("from {}", invites::inviter(room))),
                                    ),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_from_inbox(&open, cx)
                            }))
                            .e2e("button", "inbox-open")
                            .text(name_line),
                    )
                    .child(
                        round_button(format!("inbox-accept-{}", room.id), "Accept", "check", true)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.accept_invite(accept.clone(), cx)
                            }))
                            .e2e("button", "invite-accept")
                            .attr("title", "Accept"),
                    )
                    .child(
                        round_button(format!("inbox-decline-{}", room.id), "Decline", "x", false)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.decline_invite(decline.clone(), cx)
                            }))
                            .e2e("button", "invite-decline")
                            .attr("title", "Decline"),
                    )
                    .e2e("div", "inbox-item inbox-invite"),
            );
        }
    }
    let mentions: Vec<_> = crate::view_model::notices::recent_mentions(&shell.notices)
        .into_iter()
        .cloned()
        .collect();
    if !mentions.is_empty() {
        panel = panel.child(
            div()
                .mx(px(8.0))
                .mt(px(8.0))
                .mb(px(4.0))
                .text_size(px(0.7 * theme::REM))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(theme::text_faint()))
                .child("MENTIONS")
                .e2e("h4", "section")
                .text("Mentions"),
        );
        for (index, notice) in mentions.into_iter().enumerate() {
            let room = shell.rooms.iter().find(|r| r.id == notice.room_id).cloned();
            let who = format!("{} in {}", notice.sender_name, notice.room_name);
            let mut item = div()
                .id(SharedString::from(format!("inbox-mention-{index}")))
                .flex()
                .flex_col()
                .mx(px(6.0))
                .mb(px(4.0))
                .px(px(10.0))
                .py(px(8.0))
                .rounded(px(theme::RADIUS_SM))
                .cursor_pointer()
                .hover(|s| s.bg(rgba(theme::bg_hover())))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(who.clone())
                        .e2e("span", "inbox-name")
                        .text(who),
                )
                .child(
                    div()
                        .text_size(px(0.85 * theme::REM))
                        .text_color(rgb(theme::text_muted()))
                        .truncate()
                        .child(notice.body_summary.clone())
                        .e2e("span", "inbox-body")
                        .text(notice.body_summary.clone()),
                );
            if let Some(room) = room {
                item = item.on_click(cx.listener(move |this, _, _, cx| this.open_from_inbox(&room, cx)));
            }
            panel = panel.child(item.e2e("button", "inbox-item inbox-mention"));
        }
    }
    if items.is_empty() {
        panel = panel.child(
            div()
                .px(px(16.0))
                .py(px(12.0))
                .text_color(rgb(theme::text_muted()))
                .child(invites::CAUGHT_UP)
                .e2e("p", "muted pad")
                .text(invites::CAUGHT_UP),
        );
    }
    for room in items {
        let status = ItemStatus::of(&room);
        let text = status.text();
        let name = invites::item_name(&room);
        let open = room.clone();
        let tail = match status {
            ItemStatus::Mentions(_) => div()
                .min_w(px(18.0))
                .px(px(6.0))
                .rounded(px(9.0))
                .bg(rgb(theme::danger()))
                .text_color(rgb(0xffffff))
                .text_size(px(0.75 * theme::REM))
                .font_weight(FontWeight::BOLD)
                .child(text.clone())
                .e2e("span", "badge")
                .text(text),
            _ => div()
                .text_color(rgb(theme::text_muted()))
                .child(text.clone())
                .e2e("span", "muted")
                .text(text),
        };
        panel = panel.child(
            div()
                .id(SharedString::from(format!("inbox-item-{}", room.id)))
                .flex()
                .items_center()
                .justify_between()
                .mx(px(6.0))
                .mb(px(4.0))
                .px(px(10.0))
                .py(px(8.0))
                .rounded(px(theme::RADIUS_SM))
                .cursor_pointer()
                .hover(|s| s.bg(rgba(theme::bg_hover())))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(name.clone())
                        .e2e("span", "inbox-name")
                        .text(name),
                )
                .child(tail)
                .on_click(cx.listener(move |this, _, _, cx| this.open_from_inbox(&open, cx)))
                .e2e("button", "inbox-item"),
        );
    }
    let panel = panel.e2e("div", "inbox-panel").attr("role", "dialog");
    Some(
        div()
            .absolute()
            .inset_0()
            .flex()
            .justify_end()
            .items_start()
            .pt(px(36.0))
            .pr(px(12.0))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.close_inbox(cx)),
            )
            .child(panel)
            .e2e("div", "overlay panel-overlay")
            .into_any_element(),
    )
}
