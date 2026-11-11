//! The title bar (`crates/ui/src/components/titlebar.rs`): the room or
//! scope's name in the middle and the inbox button at the right, with its
//! count of rooms with something unread. No back/forward, help or window
//! buttons yet.

use gpui::{Context, FontWeight, IntoElement, div, prelude::*, px, rgb};

use crate::{
    e2e::E2eExt as _, icons::icon, shell::Shell, theme,
    view_model::invites::unread_count,
};

/// The bar's height.
pub const HEIGHT: f32 = 32.0;

pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement {
    let unread = unread_count(&shell.rooms);
    let title = if shell.friends_open {
        "Friends".to_owned()
    } else {
        shell
            .current()
            .map(|room| room.name.clone())
            .unwrap_or_else(|| crate::model::header_title(&shell.rooms, &shell.scope))
    };
    let mut inbox = div()
        .id("inbox-button")
        .relative()
        .px(px(8.0))
        .py(px(2.0))
        .rounded(px(theme::RADIUS_SM))
        .text_color(rgb(theme::TEXT_MUTED))
        .cursor_pointer()
        .hover(|s| s.text_color(rgb(theme::TEXT)))
        .child(icon("inbox", 16.0))
        .on_click(cx.listener(|this, _, _, cx| this.toggle_inbox(cx)));
    if unread > 0 {
        inbox = inbox.child(
            div()
                .absolute()
                .right(px(-2.0))
                .bottom(px(-4.0))
                .min_w(px(16.0))
                .h(px(16.0))
                .px(px(4.0))
                .rounded(px(8.0))
                .bg(rgb(theme::DANGER))
                .text_color(rgb(0xffffff))
                .text_size(px(0.65 * theme::REM))
                .child(unread.to_string())
                .e2e("span", "rail-badge inbox-badge")
                .text(unread.to_string()),
        );
    }
    div()
        .h(px(HEIGHT))
        .flex_none()
        .flex()
        .items_center()
        .px(px(8.0))
        .bg(rgb(theme::BG_RAIL))
        .text_size(px(0.8 * theme::REM))
        .text_color(rgb(theme::TEXT_MUTED))
        .child(div().flex_1())
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(theme::TEXT))
                .child(title.clone())
                .e2e("div", "titlebar-title")
                .text(title),
        )
        .child(
            div()
                .flex_1()
                .flex()
                .justify_end()
                .child(inbox.e2e("button", "ghost titlebar-button inbox-button").attr("title", "Inbox (Ctrl+I)"))
                .e2e("div", "titlebar-actions"),
        )
        .e2e("header", "titlebar")
}
