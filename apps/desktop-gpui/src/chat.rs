//! The chat area (`crates/ui/src/components/shell.rs`, `ChatRoom`): a header
//! with the open room's name, its encryption state and topic; the timeline
//! (task 5) and composer (task 6) come later. With no room open, the
//! "Pick a room" placeholder.

use gpui::{Context, ExternalPaths, FontWeight, IntoElement, div, prelude::*, px, rgb};

use crate::{e2e::E2eExt as _, icons::icon, media::avatar, shell::Shell, theme};

pub fn render(shell: &Shell, _cx: &mut Context<Shell>) -> impl IntoElement {
    let body = match shell.current() {
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
            header = if room.is_dm {
                header.child(avatar(
                    shell.picture(&room.avatar, 24),
                    &room.name,
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
                    .child(room.name.clone())
                    .e2e("h2", "")
                    .text(room.name.clone()),
            );
            header = header.child(
                div()
                    .id("e2e-badge")
                    .px(px(8.0))
                    .py(px(6.0))
                    .rounded(px(theme::RADIUS_SM))
                    .text_color(rgb(theme::TEXT_MUTED))
                    .cursor_pointer()
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
            let mut column = div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .child(header.e2e("header", "chat-header"));
            if let Some((id, timeline)) = &shell.timeline
                && id == &room.id
            {
                column = column.child(timeline.clone());
            }
            if let Some((id, composer)) = &shell.composer
                && id == &room.id
            {
                // The line where who is typing shows: room for it, empty here.
                column = column
                    .child(div().h(px(8.0)).flex_none().e2e("div", "typing empty"))
                    .child(composer.clone());
            }
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
