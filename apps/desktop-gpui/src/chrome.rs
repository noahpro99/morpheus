//! The window's own chrome (the app draws it: no system title bar): the
//! minimise, maximise and close buttons, dragging the window by a bar,
//! resizing from the edges, and the bar over the screens before the shell
//! (`WindowBar` in `crates/ui/src/components/titlebar.rs`).

use gpui::{rgba, 
    AnyElement, App, CursorStyle, FontWeight, IntoElement, MouseButton, ResizeEdge, Window, div,
    prelude::*, px, rgb,
};

use crate::{e2e::E2eExt as _, icons::icon, theme};

/// The bar's height.
pub const BAR_HEIGHT: f32 = 32.0;

/// How far in from the window's edge a resize starts, and a corner's reach.
const EDGE: f32 = 5.0;
const CORNER: f32 = 12.0;

/// Closing the window hides it when there is a tray to bring it back (the
/// app goes on syncing and notifying); without one the app ends.
pub fn close_window(window: &mut Window, cx: &mut App) {
    if crate::tray::hides_on_close() {
        crate::notify::set_looking(false, None);
        crate::HIDING.store(true, std::sync::atomic::Ordering::Relaxed);
        window.remove_window();
    } else {
        cx.quit();
    }
}

/// A bar's mouse handling: a press anywhere that is not a button drags the
/// window; a double click maximises it (or puts it back).
pub fn draggable<E: gpui::InteractiveElement + gpui::StatefulInteractiveElement>(bar: E) -> E {
    bar.on_mouse_down(MouseButton::Left, |event, window, _| {
        if event.click_count == 2 {
            window.zoom_window();
        } else {
            window.start_window_move();
        }
    })
}

/// A button that does not start a drag when it is pressed.
pub fn no_drag<E: gpui::InteractiveElement>(button: E) -> E {
    button.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

/// Minimise, maximise and close.
pub fn window_controls() -> AnyElement {
    let button = |id: &'static str, title: &'static str, glyph: &'static str, size: f32| {
        let mut b = div()
            .id(id)
            .px(px(9.0))
            .py(px(2.0))
            .rounded(px(theme::RADIUS_SM))
            .flex()
            .items_center()
            .justify_center()
            .text_color(rgb(theme::text_muted()))
            .cursor_pointer()
            .child(icon(glyph, size));
        b = if title == "Close" {
            b.hover(|s| s.bg(rgb(theme::danger())).text_color(rgb(0xffffff)))
        } else {
            b.hover(|s| s.bg(rgba(theme::bg_hover())).text_color(rgb(theme::text())))
        };
        no_drag(b)
            .on_click(move |_, window, cx| match title {
                "Minimize" => window.minimize_window(),
                "Maximize" => window.zoom_window(),
                _ => close_window(window, cx),
            })
            .e2e("button", if title == "Close" { "ghost close" } else { "ghost" })
            .attr("title", title)
    };
    div()
        .flex()
        .gap(px(2.0))
        .ml(px(10.0))
        .child(button("window-minimize", "Minimize", "minus", 14.0))
        .child(button("window-maximize", "Maximize", "square", 13.0))
        .child(button("window-close", "Close", "x", 14.0))
        .e2e("div", "window-controls")
        .into_any_element()
}

/// The Morpheus lockup as morpheuschat.org's header has it: the glasses
/// cropped tight in the moon colour, the name in Unbounded, a small ™.
pub fn brand(size: f32) -> AnyElement {
    let width = 2.083 * size;
    div()
        .flex()
        .items_center()
        .gap(px(0.5 * size))
        .font_family("Unbounded")
        .font_weight(FontWeight::BOLD)
        .text_size(px(size))
        .line_height(px(size))
        .text_color(rgb(theme::text()))
        .child(
            gpui::svg()
                .path("logo-tight.svg")
                .w(px(width))
                .h(px(width * 154.0 / 416.0))
                .flex_none()
                .text_color(rgb(theme::own()))
                .e2e("svg", "mark")
                .attr("viewBox", "48 182 416 154")
                .attr("data-color", format!("{:06x}", theme::own())),
        )
        .child(
            div()
                .flex()
                .child(
                    div()
                        .child("Morpheus")
                        .e2e("span", "brand-name")
                        .attr("data-font", "Unbounded")
                        .attr("data-weight", "700")
                        .attr("data-size", size.to_string())
                        .text("Morpheus"),
                )
                .child(
                    div()
                        .ml(px(2.0))
                        .mb(px(0.9 * 0.5 * size))
                        .font_family("Inter")
                        .font_weight(FontWeight::MEDIUM)
                        .text_size(px(0.5 * size))
                        .text_color(rgb(theme::text_muted()))
                        .child("™")
                        .e2e("span", "tm")
                        .attr("data-font", "Inter")
                        .text("™"),
                ),
        )
        .e2e("span", "brand")
        .into_any_element()
}

/// The bar over the splash and the sign-in screen: the lockup in the middle,
/// the window's buttons at the right, the rest to drag by.
pub fn window_bar() -> AnyElement {
    draggable(
        div()
            .id("window-bar")
            .h(px(BAR_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .px(px(8.0))
            .bg(rgb(theme::bg_rail()))
            .text_size(px(0.8 * theme::REM))
            .text_color(rgb(theme::text_muted()))
            .child(div().flex_1())
            .child(brand(13.0))
            .child(div().flex_1().flex().justify_end().child(window_controls())),
    )
    .e2e("header", "titlebar window-bar")
    .into_any_element()
}

/// Invisible strips along the window's edges and corners: pressing one hands
/// the resize to the compositor. Nothing while the window is maximised.
pub fn resize_edges(window: &Window) -> AnyElement {
    if window.is_maximized() || window.is_fullscreen() {
        return div().into_any_element();
    }
    let strip = |edge: ResizeEdge, cursor: CursorStyle| {
        let (name, mut d) = (format!("{edge:?}"), div().absolute());
        d = match edge {
            ResizeEdge::Top => d.top_0().left(px(CORNER)).right(px(CORNER)).h(px(EDGE)),
            ResizeEdge::Bottom => d.bottom_0().left(px(CORNER)).right(px(CORNER)).h(px(EDGE)),
            ResizeEdge::Left => d.left_0().top(px(CORNER)).bottom(px(CORNER)).w(px(EDGE)),
            ResizeEdge::Right => d.right_0().top(px(CORNER)).bottom(px(CORNER)).w(px(EDGE)),
            ResizeEdge::TopLeft => d.top_0().left_0().size(px(CORNER)),
            ResizeEdge::TopRight => d.top_0().right_0().size(px(CORNER)),
            ResizeEdge::BottomLeft => d.bottom_0().left_0().size(px(CORNER)),
            ResizeEdge::BottomRight => d.bottom_0().right_0().size(px(CORNER)),
        };
        d.cursor(cursor)
            .occlude()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.start_window_resize(edge);
                cx.stop_propagation();
            })
            .e2e("div", "window-edge")
            .attr("data-edge", name)
    };
    div()
        .absolute()
        .size_full()
        .child(strip(ResizeEdge::Top, CursorStyle::ResizeUpDown))
        .child(strip(ResizeEdge::Bottom, CursorStyle::ResizeUpDown))
        .child(strip(ResizeEdge::Left, CursorStyle::ResizeLeftRight))
        .child(strip(ResizeEdge::Right, CursorStyle::ResizeLeftRight))
        .child(strip(ResizeEdge::TopLeft, CursorStyle::ResizeUpLeftDownRight))
        .child(strip(ResizeEdge::BottomRight, CursorStyle::ResizeUpLeftDownRight))
        .child(strip(ResizeEdge::TopRight, CursorStyle::ResizeUpRightDownLeft))
        .child(strip(ResizeEdge::BottomLeft, CursorStyle::ResizeUpRightDownLeft))
        .into_any_element()
}
