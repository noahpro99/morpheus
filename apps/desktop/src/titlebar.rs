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
        self.open_room_in_scope(room_id, cx);
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
fn bar_button(
    shell: &Shell,
    cx: &mut Context<Shell>,
    id: &'static str,
) -> gpui::Stateful<gpui::Div> {
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

/// The streamer mode control: shown while the auto-on-share setting is on or
/// the mode is on. The pill when the mode is on (click turns it off); an
/// eye-off icon when off (click turns it on, clearing a share's override).
fn streamer_button(shell: &Shell, cx: &mut Context<Shell>) -> Option<impl IntoElement> {
    let on = crate::streamer::active();
    if !on && !crate::streamer::settings().auto {
        return None;
    }
    let toggle = cx.listener(move |this, _, _, cx| {
        let settings = if on {
            crate::streamer::turn_off()
        } else {
            crate::streamer::turn_on()
        };
        if let Err(err) = settings.save(&crate::data_dir()) {
            eprintln!("morpheus-desktop: saving the streamer mode setting: {err}");
        }
        crate::streamer::set_settings(settings);
        this.streamer_changed(cx);
    });
    if !on {
        return Some(
            bar_button(shell, cx, "streamer-button")
                .child(icon("eye-off", 18.0))
                .on_click(toggle)
                .e2e("button", "ghost streamer-button")
                .attr("title", "Turn on streamer mode"),
        );
    }
    Some(
        bar_button(shell, cx, "streamer-button")
            .w_auto()
            .px(px(8.0))
            .h(px(20.0))
            .mr(px(6.0))
            .gap(px(4.0))
            .rounded(px(10.0))
            .bg(rgba(theme::accent_tint()))
            .text_color(rgb(theme::accent()))
            .child(icon("eye-off", 12.0))
            .child("Streamer mode")
            .on_click(toggle)
            .e2e("button", "streamer-badge streamer-button")
            .attr(
                "title",
                "Streamer mode is on: names and IDs are hidden. Click to turn off",
            )
            .text("Streamer mode"),
    )
}

pub fn render(shell: &Shell, window: &gpui::Window, cx: &mut Context<Shell>) -> impl IntoElement {
    let layout = chrome::Layout::current(window);
    let unread = unread_count(&crate::locks::unlocked(&shell.rooms));
    let title = header_title(&shell.rooms, &shell.scope);
    let colour_of = |id: &'static str| {
        format!(
            "{:06x}",
            crate::chat::bar_colour(shell, id, theme::text_muted())
        )
    };
    let nav_button = |cx: &mut Context<Shell>,
                      id: &'static str,
                      glyph: &'static str,
                      label: &'static str,
                      enabled: bool,
                      step: isize| {
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
        if enabled {
            tagged
        } else {
            tagged.attr("disabled", "")
        }
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
                .px(px(3.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(rgb(theme::danger()))
                .text_color(rgb(0xffffff))
                .text_size(px(0.65 * theme::REM))
                .font_weight(FontWeight::BOLD)
                .text_align(gpui::TextAlign::Center)
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
            .pl(px(layout.left_padding))
            .pr(px(chrome::BAR_PADDING))
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
                    .children(streamer_button(shell, cx))
                    .children(crate::update::topbar_button(shell, cx))
                    .child(
                        inbox
                            .e2e("button", "ghost titlebar-button inbox-button")
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
                    .child(chrome::window_controls(window))
                    .e2e("div", "titlebar-actions"),
            ),
    )
    .e2e("header", "titlebar")
    .attr("data-window-buttons", layout.buttons())
    .attr("data-inset", layout.left_padding.to_string())
}

#[cfg(all(test, feature = "e2e"))]
mod badge_tests {
    use super::*;
    use crate::model::tests::room;
    use gpui::{Entity, TestAppContext, point};

    struct Harness(Entity<Shell>);
    impl gpui::Render for Harness {
        fn render(&mut self, _: &mut gpui::Window, _: &mut Context<Self>) -> impl IntoElement {
            crate::e2e::frame(div().size_full().child(self.0.clone()))
        }
    }

    #[gpui::test]
    fn inspect_inbox_badge(cx: &mut TestAppContext) {
        let (harness, cx) = cx.add_window_view(|_, cx| Harness(cx.new(|_| Shell::bare(None))));
        let shell = harness.read_with(cx, |h, _| h.0.clone());
        shell.update(cx, |s, cx| {
            let mut r = room("!r:x", "test");
            r.unread_messages = 5;
            s.rooms = vec![r];
            cx.notify();
        });
        cx.run_until_parked();
        cx.draw(
            point(px(0.0), px(0.0)),
            gpui::size(px(800.0), px(600.0)),
            |_, _| div(),
        );
        cx.run_until_parked();

        let badge = crate::e2e::query_bounds(".inbox-badge").expect("badge");
        let texts = crate::e2e::query(".inbox-badge");
        assert_eq!(texts.len(), 1);
        let (_, (tx, ty)) = &texts[0];
        assert_eq!(*tx, badge.x + badge.w / 2.0);
        assert_eq!(*ty, badge.y + badge.h / 2.0);
    }
}
