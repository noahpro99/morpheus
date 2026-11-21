//! The one colour field every colour choice in Settings uses (#315): the hex box with a small
//! artist's-palette button beside it. The button (tooltip "Pick a colour") opens the picker in a
//! popover under it; Escape or a click outside closes it. The hex box and the picker are the
//! caller's views of the same colour, so they stay in step.

use gpui::{
    AnyElement, App, ClickEvent, Corner, FocusHandle, IntoElement, KeyDownEvent, MouseButton,
    Window, anchored, deferred, div, prelude::*, px, rgba,
};

use crate::{e2e::E2eExt as _, icons::icon, theme, tooltip::TipExt as _};

/// The tooltip of the palette button.
pub const TIP: &str = "Pick a colour";

/// A colour field: `hex` (the text box) and the palette button; when `popover` is `Some`, the
/// picker is drawn under the button. `toggle` runs on a click of the button, `close` on Escape and
/// `click_outside` on a click outside the picker, and `focus` is the picker's, so Escape reaches it.
pub fn colour_field(
    id: &'static str,
    hex: impl IntoElement,
    popover: Option<AnyElement>,
    focus: &FocusHandle,
    toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    close: impl Fn(&mut Window, &mut App) + 'static,
    click_outside: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let open = popover.is_some();
    let button = div()
        .id(id)
        .size(px(32.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS_SM))
        .cursor_pointer()
        .hover(|s| s.bg(rgba(theme::bg_selected())))
        .when(open, |s| s.bg(rgba(theme::bg_selected())))
        .child(icon("palette", 18.0))
        .tip(TIP)
        .on_click(toggle)
        .e2e("button", "colour-field-button")
        .attr("data-tip", TIP);
    let popover = popover.map(|content| {
        let panel = div()
            .id("colour-popover")
            .track_focus(focus)
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    close(window, cx);
                }
            })
            .on_mouse_down_out(move |_, window, cx| click_outside(window, cx))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .mt(px(36.0))
            .p(px(12.0))
            .rounded(px(theme::RADIUS_LG))
            .bg(theme::solid_rail())
            .border_1()
            .border_color(rgba(theme::edge()))
            .shadow_lg()
            .child(content)
            .e2e("div", "colour-popover")
            .focusable(focus);
        deferred(anchored().anchor(Corner::TopLeft).snap_to_window_with_margin(px(8.0)).child(panel))
            .with_priority(30)
    });
    div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(hex)
        .child(div().relative().child(button).children(popover))
        .e2e("div", "colour-field")
        .into_any_element()
}
