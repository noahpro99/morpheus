//! The chat area (`crates/ui/src/components/shell.rs`, `ChatRoom`): a header
//! with the open room's name, its encryption state and topic; the timeline
//! (task 5) and composer (task 6) come later. With no room open, the
//! "Pick a room" placeholder.

use gpui::{Context, ExternalPaths, FontWeight, IntoElement, div, prelude::*, px, rgb};

use crate::{
    e2e::E2eExt as _, icons::icon, media::avatar, shell::Shell, theme,
    view_model::{nicknames::room_title, room_dialogs::RoomDialogKind},
};

pub fn render(shell: &Shell, window: &gpui::Window, cx: &mut Context<Shell>) -> impl IntoElement {
    let friends = shell
        .friends_page
        .clone()
        .filter(|_| shell.friends_open);
    let body = match shell.current().filter(|_| friends.is_none()) {
        _ if friends.is_some() => friends.into_iter().next().map_or_else(
            || div().into_any_element(),
            |page| page.into_any_element(),
        ),
        Some(room) if room.invited => crate::invites::invite_card(shell, cx, room),
        None if shell.preview.as_ref().is_some_and(|p| Some(&p.id) == shell.selected.as_ref()) => {
            match &shell.preview {
                Some(room) => crate::invites::join_card(shell, cx, room),
                None => div().into_any_element(),
            }
        }
        // A server's voice channel is all call: no top bar, only its chat
        // button in the corner.
        Some(room) if room.is_voice && !room.is_dm => {
            let mut chat = div().flex().flex_col().flex_1().min_h_0();
            if let Some((id, timeline)) = &shell.timeline
                && id == &room.id
            {
                chat = chat.child(timeline.clone());
            }
            if let Some((id, composer)) = &shell.composer
                && id == &room.id
            {
                chat = chat.child(composer.clone());
            }
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .child(crate::call_ui::voice_channel(
                    shell,
                    cx,
                    room,
                    Some(chat.into_any_element()),
                    window,
                ))
                .into_any_element()
        }
        Some(room) => {
            let mut header = div()
                .h(px(48.0))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(8.0))
                .pl(px(6.0)).pr(px(16.0))
                .border_b_1()
                .border_color(gpui::rgba(0x0000004d));
            let title = room_title(&shell.nicknames, room);
            // The icon (a DM's picture, a channel's #) and the name are one
            // button, as tall as the bar and with a hover background, as in
            // Discord: any of it opens the room's menu, under the name.
            let icon_el = if room.is_dm {
                avatar(shell.picture(&room.avatar, 24), &title, 24.0, None).into_any_element()
            } else {
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("#")
                    .e2e("span", "hash")
                    .text("#")
                    .into_any_element()
            };
            let head_colour = bar_colour(shell, "room-head", theme::text_muted());
            let anchor = shell.title_bounds.clone();
            let menu_room = room.clone();
            header = header.child(
                div()
                    .id("room-head")
                    .h_full()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(10.0))
                    .rounded(px(theme::RADIUS_SM))
                    .cursor_pointer()
                    .text_color(rgb(head_colour))
                    .on_hover(bar_hover(cx, "room-head"))
                    .child(icon_el)
                    .child(
                        div()
                            .relative()
                            .font_weight(FontWeight::BOLD)
                            .child(title.clone())
                            .child(
                                gpui::canvas(
                                    {
                                        let anchor = anchor.clone();
                                        move |bounds, _, _| anchor.set(bounds)
                                    },
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .size_full(),
                            )
                            .e2e("h2", "room-title")
                            .text(title.clone()),
                    )
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        let b = anchor.get();
                        let at = gpui::point(b.origin.x, b.origin.y + b.size.height + px(2.0));
                        this.open_room_menu(menu_room.clone(), at, cx)
                    }))
                    .e2e("button", "ghost room-head")
                    .attr("data-color", format!("{head_colour:06x}"))
                    .attr("data-bg", ""),
            );
            let lock_colour = bar_colour(shell, "e2e-badge", theme::text_muted());
            header = header.child(
                div()
                    .id("e2e-badge")
                    .px(px(8.0))
                    .h_full().flex().items_center()
                    .rounded(px(theme::RADIUS_SM))
                    .cursor_pointer()
                    .on_hover(bar_hover(cx, "e2e-badge"))
                    .on_click({
                        let id = room.id.clone();
                        cx.listener(move |this, _, _, cx| {
                            this.open_room_dialog(RoomDialogKind::Privacy(id.clone()), cx)
                        })
                    })
                    .child(icon(
                        if room.is_encrypted {
                            "lock"
                        } else {
                            "lock-open"
                        },
                        14.0,
                    )
                    .text_color(rgb(lock_colour)))
                    .e2e(
                        "button",
                        if room.is_encrypted {
                            "ghost e2e-badge on"
                        } else {
                            "ghost e2e-badge off"
                        },
                    )
                    .attr(
                        "title",
                        if room.is_encrypted {
                            "End-to-end encrypted"
                        } else {
                            "Not encrypted"
                        },
                    )
                    .attr("data-color", format!("{lock_colour:06x}"))
                    .attr("data-bg", ""),
            );
            if let Some(topic) = room.topic.clone() {
                header = header.child(
                    div()
                        .ml(px(8.0))
                        .pl(px(12.0))
                        .min_w_0()
                        .truncate()
                        .border_l_1()
                        .border_color(rgb(theme::bg_elevated()))
                        .text_color(rgb(theme::text_muted()))
                        .text_size(px(0.85 * theme::REM))
                        .child(topic.clone())
                        .e2e("span", "topic")
                        .text(topic),
                );
            }
            // The rest of the header sits at the right: the room's menu, the
            // member list's toggle, the pinned messages.
            header = header.child(div().flex_1());
            let count = room.member_count.to_string();
            let open = shell.members_open;
            let members_colour = if open { theme::text() } else { bar_colour(shell, "member-toggle", theme::text_muted()) };
            header = header.child(
                div()
                    .id("member-toggle")
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .px(px(8.0))
                    .h_full()
                    .rounded(px(theme::RADIUS_SM))
                    .text_size(px(0.85 * theme::REM))
                    .text_color(rgb(members_colour))
                    .cursor_pointer()
                    .on_hover(bar_hover(cx, "member-toggle"))
                    .child(icon("users", 16.0).text_color(rgb(members_colour)))
                    .child(count.clone())
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_members(cx)))
                    .e2e(
                        "button",
                        if open {
                            "ghost member-toggle active"
                        } else {
                            "ghost member-toggle"
                        },
                    )
                    .attr("title", "Member list (Ctrl+U)")
                    .attr("data-color", format!("{members_colour:06x}"))
                    .attr("data-bg", "")
                    .text(count),
            );
            // Calls belong to DMs and voice channels, as in Discord.
            if room.is_dm {
                header = header.child(crate::call_ui::header_call_button(shell, cx, room));
            }
            let pins = shell.pinned.len();
            let pins_colour = bar_colour(shell, "pin-toggle", theme::text_muted());
            header = header.child(
                div()
                    .id("pin-toggle")
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .px(px(8.0))
                    .h_full()
                    .rounded(px(theme::RADIUS_SM))
                    .text_size(px(0.85 * theme::REM))
                    .text_color(rgb(pins_colour))
                    .cursor_pointer()
                    .on_hover(bar_hover(cx, "pin-toggle"))
                    .child(icon("pin", 16.0).text_color(rgb(pins_colour)))
                    .child(pins.to_string())
                    .on_click(cx.listener(|this, _, _, cx| this.open_pinned(cx)))
                    .e2e("button", "ghost pin-toggle")
                    .attr("title", "Pinned messages (Ctrl+P)")
                    .attr("data-color", format!("{pins_colour:06x}"))
                    .attr("data-bg", "")
                    .text(pins.to_string()),
            );
            // The search box at the right end: it widens in place while it
            // has the caret or text; Enter shows the results in the side
            // column.
            if let Some(input) = &shell.search_input {
                const REST_W: f32 = 144.0;
                const WIDE_W: f32 = 240.0;
                let (focused, text) = {
                    let input = input.read(cx);
                    (input.is_focused(window), input.value().to_owned())
                };
                let wide = focused || !text.is_empty();
                let was_wide = shell.search_was_wide.replace(wide);
                let target = if wide { WIDE_W } else { REST_W };
                let from = if wide { REST_W } else { WIDE_W };
                let target_input = input.clone();
                let clear_colour = bar_colour(shell, "search-clear", theme::text_muted());
                let mut bar = div()
                    .id(if wide { "search-bar-wide" } else { "search-bar" })
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(6.0))
                    .h(px(28.0))
                    .pl(px(8.0))
                    .when(text.is_empty(), |d| d.pr(px(8.0)))
                    .rounded(px(theme::RADIUS_SM))
                    .bg(rgb(theme::bg_input()))
                    .text_size(px(0.85 * theme::REM))
                    .text_color(rgb(theme::text()))
                    .overflow_hidden()
                    .cursor_text()
                    .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                        target_input.read(cx).focus(window);
                    })
                    .child(div().flex_1().min_w_0().child(input.clone()))
                    .child(if text.is_empty() {
                        icon("search", 16.0).text_color(rgb(theme::text_muted())).into_any_element()
                    } else {
                        div()
                            .id("search-clear")
                            .flex()
                            .items_center()
                            .h_full()
                            .px(px(8.0))
                            .cursor_pointer()
                            .text_color(rgb(clear_colour))
                            .on_hover(bar_hover(cx, "search-clear"))
                            .child(icon("x", 16.0).text_color(rgb(clear_colour)))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.clear_search(cx);
                                this.focus_search(window, cx);
                            }))
                            .e2e("button", "ghost search-clear")
                            .attr("title", "Clear search")
                            .attr("data-color", format!("{clear_colour:06x}"))
                            .attr("data-bg", "")
                            .into_any_element()
                    });
                header = header.child(if was_wide == wide && !wide {
                    bar.w(px(REST_W)).e2e("div", "search-bar").into_any_element()
                } else if was_wide == wide {
                    bar.w(px(WIDE_W)).e2e("div", "search-bar").into_any_element()
                } else {
                    use gpui::{Animation, AnimationExt as _};
                    bar = bar.w(px(from));
                    bar.with_animation(
                        if wide { "widen" } else { "narrow" },
                        Animation::new(std::time::Duration::from_millis(150))
                            .with_easing(gpui::ease_out_quint()),
                        move |el, delta| el.w(px(from + (target - from) * delta)),
                    )
                    .e2e("div", "search-bar")
                    .into_any_element()
                });
            }
            // The messages and the message box; the member list beside them,
            // under the header, which runs over both.
            let mut content = div().flex().flex_col().flex_1().min_w_0().min_h_0();
            content = content.children(crate::call_ui::call_stage(shell, cx, &room.id, window));
            if let Some((id, timeline)) = &shell.timeline
                && id == &room.id
            {
                content = content.child(timeline.clone());
            }
            if let Some((id, composer)) = &shell.composer
                && id == &room.id
            {
                // The line where who is typing shows: room for it, empty here.
                if let Some((typing_room, typing)) = &shell.typing
                    && typing_room == &room.id
                {
                    content = content.child(typing.clone());
                } else {
                    content = content.child(div().h(px(8.0)).flex_none().e2e("div", "typing empty"));
                }
                content = content.child(composer.clone());
            }
            let mut body = div().flex().flex_1().min_h_0().child(content);
            // The side column: an open thread, else the member list.
            let searching = shell.thread.is_none() && shell.search.as_ref().is_some_and(|(id, _)| id == &room.id);
            let side = if let Some((id, thread)) = &shell.thread
                && id == &room.id
            {
                Some(thread.clone().into_any_element())
            } else if let Some((id, search)) = &shell.search
                && id == &room.id
            {
                Some(search.clone().into_any_element())
            } else if shell.members_open
                && let Some((id, members)) = &shell.members
                && id == &room.id
            {
                Some(members.clone().into_any_element())
            } else {
                None
            };
            if let Some(side) = side {
                body = body.child(
                    div()
                        .w(px(if searching { theme::SEARCH_W } else { theme::MEMBERS_W }))
                        .flex_none()
                        .flex()
                        .flex_col()
                        .pt(px(8.0))
                        .pr(px(8.0))
                        .pb(px(8.0))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_h_0()
                                .rounded(px(theme::RADIUS_LG))
                                .overflow_hidden()
                                .bg(rgb(theme::bg_sidebar()))
                                .child(side)
                                .e2e("div", "side-pane")
                                .attr("data-radius", theme::RADIUS_LG.to_string()),
                        ),
                );
            }
            let column = div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .child(header.e2e("header", "chat-header"))
                .child(body);
            column.into_any_element()
        }
        None => div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .child("Pick a room")
                    .e2e("h2", "")
                    .text("Pick a room"),
            )
            .child(
                div()
                    .text_color(rgb(theme::text_muted()))
                    .child("Your conversations will show up here.")
                    .e2e("p", "muted")
                    .text("Your conversations will show up here."),
            )
            .e2e("div", "empty-state")
            .into_any_element(),
    };
    let composer = shell.composer.as_ref().map(|(_, c)| c.clone());
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .h_full()
        .bg(rgb(theme::bg_chat()))
        // Files dropped anywhere on the chat wait on the message being written.
        .on_drop::<ExternalPaths>(move |paths, _, cx| {
            if let Some(composer) = &composer {
                crate::composer::dropped(composer, paths, cx);
            }
        })
        .child(body)
        .e2e("main", "chat")
}

/// A top-bar button's colour: the normal text colour while the pointer is
/// over it, else `rest`. Top-bar buttons never paint a background.
pub fn bar_colour(shell: &Shell, key: &'static str, rest: u32) -> u32 {
    if shell.bar_hover == Some(key) { theme::text() } else { rest }
}

/// An `on_hover` handler that remembers which top-bar button the pointer is over.
pub fn bar_hover(
    cx: &mut Context<Shell>,
    key: &'static str,
) -> impl Fn(&bool, &mut gpui::Window, &mut gpui::App) + 'static {
    let view = cx.entity();
    move |over, _, cx| {
        view.update(cx, |s, cx| {
            let now = if *over {
                Some(key)
            } else if s.bar_hover == Some(key) {
                None
            } else {
                return;
            };
            if s.bar_hover != now {
                s.bar_hover = now;
                cx.notify();
            }
        })
    }
}
