//! The title bar (`crates/ui/src/components/titlebar.rs`): back and forward
//! through the rooms visited, the scope's name with the logo, help, the inbox
//! with its count of rooms with something unread, and the window's buttons.
//! Anywhere on it that is not a button drags the window.

use gpui::{Context, FontWeight, IntoElement, div, prelude::*, px, rgb, rgba};

use crate::{
    chrome,
    dialogs::Dialog,
    e2e::E2eExt as _,
    icons::icon,
    model::{header_title, scope_for},
    shell::Shell,
    theme,
    view_model::invites::unread_count,
};

/// The bar's height.
pub const HEIGHT: f32 = chrome::BAR_HEIGHT;

impl Shell {
    /// Back (`-1`) or forward (`1`) through the rooms visited: the scope first,
    /// then the room, as the Dioxus bar does.
    pub fn nav_jump(&mut self, step: isize, cx: &mut Context<Self>) {
        let Some(room_id) = self.nav.step(step) else {
            return;
        };
        if let Some(room) = self.rooms.iter().find(|r| r.id == room_id) {
            let scope = scope_for(&self.rooms, room);
            if scope != self.scope {
                self.set_scope(scope, cx);
            }
        }
        self.open_room(room_id, cx);
    }

    /// The help button and Ctrl+/: the shortcuts dialog, open or closed.
    pub fn toggle_help(&mut self, cx: &mut Context<Self>) {
        if matches!(self.dialog, Some(Dialog::Help)) {
            self.dialog_close(cx);
        } else {
            self.dialog_open(Dialog::Help, cx);
        }
    }
}

/// A ghost button of the bar, a 28 px square: no drag from it, no hover
/// background (the glyph brightens, through `Shell::bar_hover`).
fn bar_button(shell: &Shell, cx: &mut Context<Shell>, id: &'static str) -> gpui::Stateful<gpui::Div> {
    let colour = crate::chat::bar_colour(shell, id, theme::text_muted());
    chrome::no_drag(
        div()
            .id(id)
            .relative()
            .size(px(28.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(theme::RADIUS_SM))
            .text_color(rgb(colour))
            .cursor_pointer()
            .on_hover(crate::chat::bar_hover(cx, id)),
    )
}

pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement {
    let unread = unread_count(&shell.rooms);
    let title = header_title(&shell.rooms, &shell.scope);
    let colour_of = |id: &'static str| format!("{:06x}", crate::chat::bar_colour(shell, id, theme::text_muted()));
    let nav_button = |cx: &mut Context<Shell>, id: &'static str, glyph: &'static str, label: &'static str, enabled: bool, step: isize| {
        let mut b = bar_button(shell, cx, id).child(icon(glyph, 18.0));
        if enabled {
            b = b.on_click(cx.listener(move |this, _, _, cx| this.nav_jump(step, cx)));
        } else {
            b = b.opacity(0.35).cursor_default();
        }
        let tagged = b
            .e2e("button", "ghost nav-button")
            .attr("title", label)
            .attr("data-color", colour_of(id))
            .attr("data-bg", "");
        if enabled { tagged } else { tagged.attr("disabled", "") }
    };
    let mut inbox = bar_button(shell, cx, "inbox-button")
        .child(icon("inbox", 16.0))
        .on_click(cx.listener(|this, _, _, cx| this.toggle_inbox(cx)));
    if unread > 0 {
        inbox = inbox.child(
            div()
                .absolute()
                .right(px(2.0))
                .bottom(px(2.0))
                .min_w(px(16.0))
                .h(px(16.0))
                .px(px(4.0))
                .rounded(px(8.0))
                .bg(rgb(theme::danger()))
                .text_color(rgb(0xffffff))
                .text_size(px(0.65 * theme::REM))
                .child(unread.to_string())
                .e2e("span", "rail-badge inbox-badge")
                .text(unread.to_string()),
        );
    }
    let help = bar_button(shell, cx, "help-button")
        .child(icon("circle-help", 16.0))
        .on_click(cx.listener(|this, _, _, cx| this.toggle_help(cx)));
    let at = shell.nav.index;
    let total = shell.nav.entries.len();
    chrome::draggable(
        div()
            .id("titlebar")
            .h(px(HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .px(px(8.0))
            .bg(rgb(theme::bg_rail()))
            .text_size(px(0.8 * theme::REM))
            .text_color(rgb(theme::text_muted()))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .gap(px(2.0))
                    .child(nav_button(
                        cx,
                        "nav-back",
                        "chevron-left",
                        "Back (Alt+←)",
                        shell.nav.can_go_back(),
                        -1,
                    ))
                    .child(nav_button(
                        cx,
                        "nav-forward",
                        "chevron-right",
                        "Forward (Alt+→)",
                        shell.nav.can_go_forward(),
                        1,
                    ))
                    .e2e("div", "titlebar-nav")
                    .attr("data-nav", format!("{at}/{total}")),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(theme::text()))
                    .child(
                        gpui::svg()
                            .path("logo.svg")
                            .size(px(16.0))
                            .flex_none()
                            .text_color(rgb(theme::accent())),
                    )
                    .child(title.clone())
                    .e2e("div", "titlebar-title")
                    .text(title),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .justify_end()
                    .items_center()
                    .gap(px(2.0))
                    .child(
                        inbox.e2e("button", "ghost titlebar-button inbox-button")
                            .attr("title", "Inbox (Ctrl+I)")
                            .attr("data-color", colour_of("inbox-button"))
                            .attr("data-bg", ""),
                    )
                    .child(
                        help.e2e("button", "ghost titlebar-button help-button")
                            .attr("title", "Help (Ctrl+/)")
                            .attr("data-color", colour_of("help-button"))
                            .attr("data-bg", ""),
                    )
                    .child(chrome::window_controls())
                    .e2e("div", "titlebar-actions"),
            ),
    )
    .e2e("header", "titlebar")
}
