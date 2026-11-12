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
                .px(px(16.0))
                .border_b_1()
                .border_color(gpui::rgba(0x0000004d));
            let title = room_title(&shell.nicknames, room);
            header = if room.is_dm {
                header.child(avatar(
                    shell.picture(&room.avatar, 24),
                    &title,
                    24.0,
                    None,
                ))
            } else {
                header.child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(theme::TEXT_FAINT))
                        .child("#")
                        .e2e("span", "hash")
                        .text("#"),
                )
            };
            header = header.child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .child(title.clone())
                    .e2e("h2", "")
                    .text(title.clone()),
            );
            header = header.child(
                div()
                    .id("e2e-badge")
                    .px(px(8.0))
                    .py(px(6.0))
                    .rounded(px(theme::RADIUS_SM))
                    .text_color(rgb(theme::TEXT_MUTED))
                    .cursor_pointer()
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
                    .text_color(rgb(theme::TEXT_MUTED)))
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
                    ),
            );
            if let Some(topic) = room.topic.clone() {
                header = header.child(
                    div()
                        .ml(px(8.0))
                        .pl(px(12.0))
                        .min_w_0()
                        .truncate()
                        .border_l_1()
                        .border_color(rgb(theme::BG_ELEVATED))
                        .text_color(rgb(theme::TEXT_MUTED))
                        .text_size(px(0.85 * theme::REM))
                        .child(topic.clone())
                        .e2e("span", "topic")
                        .text(topic),
                );
            }
            // The rest of the header sits at the right: the room's menu, the
            // member list's toggle, the pinned messages.
            header = header.child(div().flex_1());
            let menu_room = room.clone();
            header = header.child(
                div()
                    .id("room-menu-button")
                    .px(px(8.0))
                    .py(px(6.0))
                    .rounded(px(theme::RADIUS_SM))
                    .text_color(rgb(theme::TEXT_MUTED))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(theme::TEXT)))
                    .child(icon("ellipsis", 16.0).text_color(rgb(theme::TEXT_MUTED)))
                    .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                        this.open_room_menu(menu_room.clone(), event.position(), cx)
                    }))
                    .e2e("button", "ghost room-menu-button")
                    .attr("title", "Room options"),
            );
            let count = room.member_count.to_string();
            let open = shell.members_open;
            header = header.child(
                div()
                    .id("member-toggle")
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .px(px(8.0))
                    .py(px(6.0))
                    .rounded(px(theme::RADIUS_SM))
                    .text_size(px(0.85 * theme::REM))
                    .text_color(rgb(if open { theme::TEXT } else { theme::TEXT_MUTED }))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(theme::TEXT)))
                    .child(icon("users", 16.0).text_color(rgb(if open {
                        theme::TEXT
                    } else {
                        theme::TEXT_MUTED
                    })))
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
                    .text(count),
            );
            // Calls belong to DMs and voice channels, as in Discord.
            if room.is_dm {
                header = header.child(crate::call_ui::header_call_button(shell, cx, room));
            }
            let pins = shell.pinned.len();
            header = header.child(
                div()
                    .id("pin-toggle")
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .px(px(8.0))
                    .py(px(6.0))
                    .rounded(px(theme::RADIUS_SM))
                    .text_size(px(0.85 * theme::REM))
                    .text_color(rgb(theme::TEXT_MUTED))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(theme::TEXT)))
                    .child(icon("pin", 16.0).text_color(rgb(theme::TEXT_MUTED)))
                    .child(pins.to_string())
                    .on_click(cx.listener(|this, _, _, cx| this.open_pinned(cx)))
                    .e2e("button", "ghost pin-toggle")
                    .attr("title", "Pinned messages (Ctrl+P)")
                    .text(pins.to_string()),
            );
            // A search bar at the right end; opens the search panel.
            let searching = shell.search.is_some();
            header = header.child(
                div()
                    .id("search-toggle")
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.0))
                    .w(px(180.0))
                    .h(px(28.0))
                    .px(px(8.0))
                    .rounded(px(theme::RADIUS_SM))
                    .bg(if searching {
                        gpui::Hsla::from(gpui::rgba(theme::BG_SELECTED))
                    } else {
                        gpui::Hsla::from(rgb(theme::BG_INPUT))
                    })
                    .text_size(px(0.85 * theme::REM))
                    .text_color(rgb(if searching { theme::TEXT } else { theme::TEXT_MUTED }))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(theme::TEXT)))
                    .child("Search")
                    .child(icon("search", 16.0).text_color(rgb(if searching {
                        theme::TEXT
                    } else {
                        theme::TEXT_MUTED
                    })))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_search(cx)))
                    .e2e(
                        "button",
                        if searching {
                            "ghost search-toggle active"
                        } else {
                            "ghost search-toggle"
                        },
                    )
                    .attr("title", "Search this room")
                    .text("Search"),
            );
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
                content = content
                    .child(div().h(px(8.0)).flex_none().e2e("div", "typing empty"))
                    .child(composer.clone());
            }
            let mut body = div().flex().flex_1().min_h_0().child(content);
            // The side column: an open thread, else the member list.
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
                        .w(px(theme::MEMBERS_W))
                        .flex_none()
                        .pt(px(8.0))
                        .pr(px(8.0))
                        .pb(px(8.0))
                        .child(
                            div()
                                .size_full()
                                .rounded(px(theme::RADIUS_LG))
                                .overflow_hidden()
                                .bg(rgb(theme::BG_SIDEBAR))
                                .child(side)
                                .e2e("div", "side-pane"),
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
                    .text_color(rgb(theme::TEXT_MUTED))
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
        .bg(rgb(theme::BG_CHAT))
        // Files dropped anywhere on the chat wait on the message being written.
        .on_drop::<ExternalPaths>(move |paths, _, cx| {
            if let Some(composer) = &composer {
                crate::composer::dropped(composer, paths, cx);
            }
        })
        .child(body)
        .e2e("main", "chat")
}
