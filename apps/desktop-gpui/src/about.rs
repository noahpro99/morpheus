//! The Acknowledgements dialog (`crates/ui/src/components/about.rs`),
//! opened from the tray menu.

use gpui::{AnyElement, Context, FocusHandle, FontWeight, SharedString, Window, div, prelude::*, px, rgb};

use crate::{
    dialogs::{dialog_box, heading, muted, overlay},
    e2e::E2eExt as _,
    shell::Shell,
    theme,
    view_model::about::{HEADLINERS, VERSION, crates},
};

pub fn render(
    focus: &FocusHandle,
    window: &Window,
    cx: &mut Context<Shell>,
    close: impl Fn(&mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let list = crates();
    let header = format!("Open-source libraries ({})", list.len());
    let boxed = dialog_box()
        .id("about-dialog")
        .track_focus(focus)
        .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
            if event.keystroke.key == "escape" {
                this.dialog_close(cx);
            }
        }))
        .w(px(560.0))
        .child(heading("Acknowledgements"))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .min_h_0()
                .id("about-scroll")
                .overflow_y_scroll()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("Morpheus {VERSION}"))
                        .e2e("p", "about-version")
                        .text(format!("Morpheus {VERSION}")),
                )
                .child(muted(
                    "Morpheus stands on a great deal of open-source work. Thank you to everyone behind it.",
                    "muted",
                ))
                .children(HEADLINERS.iter().map(|(name, what)| {
                    div()
                        .flex()
                        .flex_col()
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(*name))
                        .child(div().text_color(rgb(theme::text_muted())).child(*what))
                        .e2e("li", "")
                        .text(format!("{name} {what}"))
                }))
                .child(
                    div()
                        .mt(px(8.0))
                        .font_weight(FontWeight::BOLD)
                        .child(header.clone())
                        .e2e("h3", "")
                        .text(header),
                )
                .children(list.into_iter().map(|(name, version)| {
                    div()
                        .flex()
                        .gap(px(8.0))
                        .child(div().child(name))
                        .child(div().text_color(rgb(theme::text_muted())).child(SharedString::from(version)))
                        .e2e("li", "")
                        .text(format!("{name} {version}"))
                }))
                .e2e("div", "about-body"),
        )
        .e2e("div", "message-dialog settings about")
        .text("Acknowledgements")
        .focusable(focus);
    overlay(window, boxed.into_any_element(), close)
}
