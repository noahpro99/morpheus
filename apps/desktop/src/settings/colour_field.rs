//! The colour menu of Settings > Appearance (#315, #354): the Custom theme tile opens it as a
//! popover under the tile. It holds the hex box, the picker and the recent colours, in that
//! order; Escape or a click outside closes it.

use gpui::{
    AnyElement, App, Corner, FocusHandle, IntoElement, KeyDownEvent, MouseButton, Window, anchored,
    deferred, div, prelude::*, px, rgba,
};

use crate::{e2e::E2eExt as _, theme};

/// The tooltip of the Custom tile's palette icon.
pub const TIP: &str = "Pick a colour";

/// The popover around `content`; `close` runs on Escape and `click_outside` on a click outside it,
/// and `focus` is the popover's, so Escape reaches it.
pub fn popover(
    content: AnyElement,
    focus: &FocusHandle,
    close: impl Fn(&mut Window, &mut App) + 'static,
    click_outside: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
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
