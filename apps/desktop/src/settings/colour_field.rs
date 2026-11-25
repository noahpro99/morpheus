//! The colour menu of Settings > Appearance (#315, #354): the Custom theme tile opens it as a
//! popover under the tile. It holds the hex box, the picker and the recent colours, in that
//! order; Escape or a click outside closes it.

use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyElement, App, Corner, FocusHandle, IntoElement, KeyDownEvent, MouseButton, MouseUpEvent,
    Window, anchored, deferred, div, prelude::*, px, rgba,
};

use crate::{e2e::E2eExt as _, theme};

/// The tooltip of the Custom tile's palette icon.
pub fn tip() -> &'static str {
    crate::view_model::i18n::ts("Pick a colour")
}

/// The popover around `content`; `close` runs on Escape and `click_outside` once a click outside
/// it is done (pressed and released outside), and `focus` is the popover's, so Escape reaches it.
/// It closes on the release, not the press: closing at the press redraws the page before the
/// release, and the click that closed it, on another button, was lost (#359). `pressed_outside`
/// is the popover's memory of a press outside, across redraws.
pub fn popover(
    content: AnyElement,
    focus: &FocusHandle,
    pressed_outside: Rc<Cell<bool>>,
    close: impl Fn(&mut Window, &mut App) + 'static,
    click_outside: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let (outside, inside) = (pressed_outside.clone(), pressed_outside.clone());
    let panel = div()
        .id("colour-popover")
        .track_focus(focus)
        .on_key_down(move |event: &KeyDownEvent, window, cx| {
            if event.keystroke.key == "escape" {
                cx.stop_propagation();
                close(window, cx);
            }
        })
        .on_mouse_down_out(move |event, _, _| outside.set(event.button == MouseButton::Left))
        .on_mouse_up_out(MouseButton::Left, move |event, window, cx| {
            if pressed_outside.take() {
                click_outside(event, window, cx);
            }
        })
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            inside.set(false);
            cx.stop_propagation();
        })
        .mt(px(6.0))
        .p(px(12.0))
        .rounded(px(theme::RADIUS_LG))
        .bg(theme::solid_rail())
        .border_1()
        .border_color(rgba(theme::edge()))
        .shadow_lg()
        .child(content)
        .e2e("div", "colour-popover custom-colour")
        .focusable(focus);
    deferred(anchored().anchor(Corner::TopLeft).snap_to_window_with_margin(px(8.0)).child(panel))
        .with_priority(30)
        .into_any_element()
}
