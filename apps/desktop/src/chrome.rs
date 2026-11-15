//! The window's own chrome (the app draws it: no system title bar): the
//! minimise, maximise and close buttons, dragging the window by a bar,
//! resizing from the edges, and the bar over the screens before the shell
//! (`WindowBar` in `crates/ui/src/components/titlebar.rs`). On macOS the
//! system's title bar is transparent and the bar is the app's: the native
//! traffic lights sit at its left (`Layout::for_os`), and the app draws no
//! window buttons or resize edges of its own.

use gpui::{
    AnyElement, App, CursorStyle, FontWeight, IntoElement, MouseButton, ResizeEdge, Window, div,
    prelude::*, px, rgb, rgba,
};

use crate::{e2e::E2eExt as _, icons::icon, theme};

/// The bar's height.
pub const BAR_HEIGHT: f32 = 32.0;

/// The systems the window's chrome differs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Mac,
    Linux,
    Windows,
}

impl Os {
    pub const fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

/// Who draws what of the window's chrome on a system.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// The app draws minimise, maximise and close at the bar's right (else
    /// the system's own buttons are on the bar).
    pub app_buttons: bool,
    /// The app's resize strips along the edges (else the system resizes).
    pub app_resize_edges: bool,
    /// Where the system's buttons go, from the window's top left
    /// (`TitlebarOptions::traffic_light_position`); None: no transparent
    /// system title bar.
    pub traffic_lights: Option<(f32, f32)>,
    /// The bar's left padding: room for the traffic lights on macOS.
    pub left_padding: f32,
}

/// The bar's padding at each end where nothing else is there.
pub const BAR_PADDING: f32 = 8.0;

/// macOS's traffic lights: 12 px in, their 14 px circles centred in the bar.
const TRAFFIC_LIGHTS: (f32, f32) = (12.0, (BAR_HEIGHT - 14.0) / 2.0);
/// The width the traffic lights take with their margin: the bar's content
/// starts after it.
pub const TRAFFIC_LIGHTS_INSET: f32 = 78.0;

impl Layout {
    /// The chrome on `os`. `fullscreen`: macOS hides the traffic lights then
    /// (they come down with the menu bar), so the bar takes the space back.
    pub fn for_os(os: Os, fullscreen: bool) -> Layout {
        match os {
            Os::Mac => Layout {
                app_buttons: false,
                app_resize_edges: false,
                traffic_lights: Some(TRAFFIC_LIGHTS),
                left_padding: if fullscreen {
                    BAR_PADDING
                } else {
                    TRAFFIC_LIGHTS_INSET
                },
            },
            Os::Linux | Os::Windows => Layout {
                app_buttons: true,
                app_resize_edges: true,
                traffic_lights: None,
                left_padding: BAR_PADDING,
            },
        }
    }

    /// The layout of this build's system for `window`.
    pub fn current(window: &Window) -> Layout {
        Layout::for_os(Os::current(), window.is_fullscreen())
    }

    /// The window's title bar options: a transparent system bar with the
    /// traffic lights placed on macOS, the plain one elsewhere.
    pub fn titlebar_options(self, title: gpui::SharedString) -> gpui::TitlebarOptions {
        gpui::TitlebarOptions {
            title: Some(title),
            appears_transparent: self.traffic_lights.is_some(),
            traffic_light_position: self.traffic_lights.map(|(x, y)| gpui::point(px(x), px(y))),
        }
    }

    /// For the e2e tree and the start-up log: whose buttons are on the bar.
    pub fn buttons(self) -> &'static str {
        if self.app_buttons { "app" } else { "native" }
    }
}

/// How far in from the window's edge a resize starts, and a corner's reach.
const EDGE: f32 = 5.0;
const CORNER: f32 = 12.0;

/// Closing the window hides it when there is a tray to bring it back (the
/// app goes on syncing and notifying); without one the app ends.
pub fn close_window(window: &mut Window, cx: &mut App) {
    if crate::tray::hides_on_close() {
        crate::notify::set_looking(false, None);
        gpui::KEEP_RUNNING_WITHOUT_WINDOWS.store(true, std::sync::atomic::Ordering::Relaxed);
        window.remove_window();
    } else {
        crate::quit(cx);
    }
}

/// A bar's mouse handling: a press anywhere that is not a button drags the
/// window; a double click maximises it (or puts it back), on macOS doing
/// what the system's setting for a title bar's double click says.
pub fn draggable<E: gpui::InteractiveElement + gpui::StatefulInteractiveElement>(bar: E) -> E {
    bar.on_mouse_down(MouseButton::Left, |event, window, _| {
        if event.click_count == 2 {
            if Os::current() == Os::Mac {
                window.titlebar_double_click();
            } else {
                window.zoom_window();
            }
        } else {
            window.start_window_move();
        }
    })
}

/// A button that does not start a drag when it is pressed.
pub fn no_drag<E: gpui::InteractiveElement>(button: E) -> E {
    button.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

/// Minimise, maximise and close: nothing where the system's own buttons are
/// on the bar (macOS).
pub fn window_controls(window: &Window) -> AnyElement {
    if !Layout::current(window).app_buttons {
        return div().into_any_element();
    }
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
            .e2e(
                "button",
                if title == "Close" {
                    "ghost close"
                } else {
                    "ghost"
                },
            )
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
pub fn window_bar(window: &Window) -> AnyElement {
    let layout = Layout::current(window);
    draggable(
        div()
            .id("window-bar")
            .h(px(BAR_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .pl(px(layout.left_padding))
            .pr(px(BAR_PADDING))
            .bg(rgb(theme::bg_rail()))
            .text_size(px(0.8 * theme::REM))
            .text_color(rgb(theme::text_muted()))
            .child(div().flex_1())
            .child(brand(13.0))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_end()
                    .child(window_controls(window)),
            ),
    )
    .e2e("header", "titlebar window-bar")
    .attr("data-window-buttons", layout.buttons())
    .attr("data-inset", layout.left_padding.to_string())
    .into_any_element()
}

/// Invisible strips along the window's edges and corners: pressing one hands
/// the resize to the compositor. Nothing while the window is maximised.
pub fn resize_edges(window: &Window) -> AnyElement {
    if !Layout::current(window).app_resize_edges || window.is_maximized() || window.is_fullscreen()
    {
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
        .child(strip(
            ResizeEdge::TopLeft,
            CursorStyle::ResizeUpLeftDownRight,
        ))
        .child(strip(
            ResizeEdge::BottomRight,
            CursorStyle::ResizeUpLeftDownRight,
        ))
        .child(strip(
            ResizeEdge::TopRight,
            CursorStyle::ResizeUpRightDownLeft,
        ))
        .child(strip(
            ResizeEdge::BottomLeft,
            CursorStyle::ResizeUpRightDownLeft,
        ))
        .into_any_element()
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn macos_has_the_system_buttons_on_a_transparent_bar_and_the_content_after_them() {
        let mac = Layout::for_os(Os::Mac, false);
        assert!(
            !mac.app_buttons,
            "no app-drawn buttons beside the traffic lights"
        );
        assert!(!mac.app_resize_edges);
        let (x, y) = mac.traffic_lights.expect("traffic lights placed");
        // The 14 px circles centred in the 32 px bar, at its left.
        assert_eq!(y + 7.0, BAR_HEIGHT / 2.0);
        assert!(x > 0.0 && x < 20.0);
        // Three buttons 20 px apart: the bar's content starts clear of them.
        assert!(mac.left_padding >= x + 2.0 * 20.0 + 14.0 + 8.0);
        assert!(mac.left_padding <= 80.0);
        let options = mac.titlebar_options("Morpheus".into());
        assert!(options.appears_transparent);
        assert_eq!(
            options.traffic_light_position,
            Some(gpui::point(px(x), px(y)))
        );
        assert_eq!(mac.buttons(), "native");
    }

    #[test]
    fn macos_full_screen_gives_the_traffic_lights_space_back() {
        // macOS hides the traffic lights in full screen.
        assert_eq!(Layout::for_os(Os::Mac, true).left_padding, BAR_PADDING);
    }

    #[test]
    fn linux_and_windows_draw_their_own_buttons_and_edges_as_before() {
        for os in [Os::Linux, Os::Windows] {
            for fullscreen in [false, true] {
                let layout = Layout::for_os(os, fullscreen);
                assert!(layout.app_buttons && layout.app_resize_edges, "{os:?}");
                assert_eq!(layout.traffic_lights, None);
                assert_eq!(layout.left_padding, BAR_PADDING);
                let options = layout.titlebar_options("Morpheus".into());
                assert!(!options.appears_transparent);
                assert_eq!(options.traffic_light_position, None);
                assert_eq!(layout.buttons(), "app");
            }
        }
    }
}
