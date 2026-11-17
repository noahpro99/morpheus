//! The window's own chrome (the app draws it: no system title bar): the
//! minimise, maximise and close buttons, dragging the window by a bar,
//! resizing from the edges, and the bar over the screens before the shell
//! (`WindowBar` in `crates/ui/src/components/titlebar.rs`). On macOS the
//! system's title bar is transparent and the bar is the app's: the native
//! traffic lights sit at its left (`Layout::for_os`), and the app draws no
//! window buttons or resize edges of its own. On Windows the system's caption
//! is hidden too (GPUI's `appears_transparent`: WM_NCCALCSIZE gives the
//! caption to the client area, keeping the resize borders) and the app's bar
//! and buttons answer WM_NCHITTEST as the caption and its buttons, so the
//! system still drags, snaps (Aero snap, Win+arrows, the snap layouts over
//! the maximise button), maximises on a double click and resizes.

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
    /// No system caption above the app's bar: macOS's is transparent under
    /// it, Windows's is taken into the client area
    /// (`TitlebarOptions::appears_transparent`).
    pub hide_system_caption: bool,
    /// Windows: the bar tells the system it is the caption (WM_NCHITTEST
    /// HTCAPTION, `WindowControlArea::Drag`) and the buttons are its
    /// minimise, maximise and close (HTMINBUTTON...), so the system drags,
    /// snaps and maximises; elsewhere the app starts the drag itself.
    pub system_hit_test: bool,
}

/// The app's window buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowButton {
    Minimize,
    Maximize,
    Close,
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
                hide_system_caption: true,
                system_hit_test: false,
            },
            Os::Linux => Layout {
                app_buttons: true,
                app_resize_edges: true,
                traffic_lights: None,
                left_padding: BAR_PADDING,
                hide_system_caption: false,
                system_hit_test: false,
            },
            // The system keeps its (invisible) resize borders: no app edges.
            Os::Windows => Layout {
                app_buttons: true,
                app_resize_edges: false,
                traffic_lights: None,
                left_padding: BAR_PADDING,
                hide_system_caption: true,
                system_hit_test: true,
            },
        }
    }

    /// This build's system, not in full screen: for what does not depend on it.
    pub fn this_os() -> Layout {
        Layout::for_os(Os::current(), false)
    }

    /// What the bar is to the system's hit test: the caption on Windows.
    pub fn bar_area(self) -> Option<gpui::WindowControlArea> {
        self.system_hit_test
            .then_some(gpui::WindowControlArea::Drag)
    }

    /// What a window button is to the system's hit test: on Windows its own
    /// button (HTMAXBUTTON over maximise brings Windows 11's snap layouts).
    pub fn button_area(self, button: WindowButton) -> Option<gpui::WindowControlArea> {
        self.system_hit_test.then_some(match button {
            WindowButton::Minimize => gpui::WindowControlArea::Min,
            WindowButton::Maximize => gpui::WindowControlArea::Max,
            WindowButton::Close => gpui::WindowControlArea::Close,
        })
    }

    /// Whether the app acts on a click of a window button. On Windows the
    /// system minimises, maximises and restores from its own button areas;
    /// closing stays the app's (it hides the window when there is a tray).
    pub fn app_clicks(self, button: WindowButton) -> bool {
        !self.system_hit_test || button == WindowButton::Close
    }

    /// The layout of this build's system for `window`.
    pub fn current(window: &Window) -> Layout {
        Layout::for_os(Os::current(), window.is_fullscreen())
    }

    /// The window's title bar options: a transparent system bar with the
    /// traffic lights placed on macOS, no system caption on Windows, the
    /// plain one on Linux (client decorations instead).
    pub fn titlebar_options(self, title: gpui::SharedString) -> gpui::TitlebarOptions {
        gpui::TitlebarOptions {
            title: Some(title),
            appears_transparent: self.hide_system_caption,
            traffic_light_position: self.traffic_lights.map(|(x, y)| gpui::point(px(x), px(y))),
        }
    }

    /// The app's window buttons as Windows draws its own (Linux too), (width,
    /// height): the bar's full height, side by side at its right end, so a
    /// push into the top right corner is Close. macOS has its own (`None`).
    pub fn caption_buttons(self) -> Option<(f32, f32)> {
        self.app_buttons.then_some((46.0, BAR_HEIGHT))
    }

    /// The bar's right padding: none where the caption buttons reach its end.
    pub fn right_padding(self) -> f32 {
        if self.caption_buttons().is_some() { 0.0 } else { BAR_PADDING }
    }

    /// For the e2e tree and the start-up log: whose buttons are on the bar.
    pub fn buttons(self) -> &'static str {
        if self.app_buttons { "app" } else { "native" }
    }
}

/// How far in from the window's edge a resize starts, and a corner's reach.
const EDGE: f32 = 5.0;
const CORNER: f32 = 12.0;
/// Where the app's caption buttons are, the corner strip is this small (the
/// buttons reach the window's corner) and the top and right strips keep off them.
const CORNER_OVER_BUTTONS: f32 = 2.0;
/// The three buttons' width together.
const BUTTONS_W: f32 = 3.0 * 46.0;

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
/// what the system's setting for a title bar's double click says. On
/// Windows the bar is the system's caption, which does all that itself.
pub fn draggable<E: gpui::InteractiveElement + gpui::StatefulInteractiveElement>(bar: E) -> E {
    if let Some(area) = Layout::this_os().bar_area() {
        return bar.window_control_area(area);
    }
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

/// A button that does not start a drag when it is pressed. On Windows it
/// also hides the bar behind it from the system's hit test (it is client
/// area, not caption).
pub fn no_drag<E: gpui::InteractiveElement>(button: E) -> E {
    let button = if Layout::this_os().system_hit_test {
        button.occlude()
    } else {
        button
    };
    button.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

/// Minimise, maximise and close: nothing where the system's own buttons are
/// on the bar (macOS).
pub fn window_controls(window: &Window) -> AnyElement {
    let layout = Layout::current(window);
    if !layout.app_buttons {
        return div().into_any_element();
    }
    let button = |id: &'static str, title: &'static str, glyph: &'static str, size: f32| {
        let which = match title {
            "Minimize" => WindowButton::Minimize,
            "Maximize" => WindowButton::Maximize,
            _ => WindowButton::Close,
        };
        let mut b = div().id(id);
        b = match layout.caption_buttons() {
            Some((w, h)) => b.w(px(w)).h(px(h)),
            None => b.px(px(9.0)).py(px(2.0)).rounded(px(theme::RADIUS_SM)),
        };
        b = b
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
        // Windows: the system's own button there (hovering maximise shows
        // the snap layouts); the system minimises, maximises and restores.
        if let Some(area) = layout.button_area(which) {
            b = b.window_control_area(area).occlude();
        }
        if layout.app_clicks(which) {
            b = no_drag(b).on_click(move |_, window, cx| match which {
                WindowButton::Minimize => window.minimize_window(),
                WindowButton::Maximize => window.zoom_window(),
                WindowButton::Close => close_window(window, cx),
            });
        }
        b.e2e(
            "button",
            if title == "Close" {
                "ghost close"
            } else {
                "ghost"
            },
        )
        .attr("title", title)
    };
    let edge = layout.caption_buttons().is_some();
    div()
        .flex()
        .when(!edge, |d| d.gap(px(2.0)))
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
            .pr(px(layout.right_padding()))
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
    let buttons = Layout::current(window).caption_buttons().is_some();
    let (top_right, top_gap, right_gap) = if buttons {
        (CORNER_OVER_BUTTONS, BUTTONS_W, BAR_HEIGHT)
    } else {
        (CORNER, CORNER, CORNER)
    };
    let strip = |edge: ResizeEdge, cursor: CursorStyle| {
        let (name, mut d) = (format!("{edge:?}"), div().absolute());
        d = match edge {
            ResizeEdge::Top => d.top_0().left(px(CORNER)).right(px(top_gap)).h(px(EDGE)),
            ResizeEdge::Bottom => d.bottom_0().left(px(CORNER)).right(px(CORNER)).h(px(EDGE)),
            ResizeEdge::Left => d.left_0().top(px(CORNER)).bottom(px(CORNER)).w(px(EDGE)),
            ResizeEdge::Right => d.right_0().top(px(right_gap)).bottom(px(CORNER)).w(px(EDGE)),
            ResizeEdge::TopLeft => d.top_0().left_0().size(px(CORNER)),
            ResizeEdge::TopRight => d.top_0().right_0().size(px(top_right)),
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

    const BUTTONS: [WindowButton; 3] = [
        WindowButton::Minimize,
        WindowButton::Maximize,
        WindowButton::Close,
    ];

    #[test]
    fn linux_draws_its_own_buttons_and_edges_as_before() {
        for fullscreen in [false, true] {
            let layout = Layout::for_os(Os::Linux, fullscreen);
            assert!(layout.app_buttons && layout.app_resize_edges);
            assert_eq!(layout.traffic_lights, None);
            assert_eq!(layout.left_padding, BAR_PADDING);
            let options = layout.titlebar_options("Morpheus".into());
            assert!(!options.appears_transparent);
            assert_eq!(options.traffic_light_position, None);
            assert_eq!(layout.buttons(), "app");
            // The app drags, maximises and minimises itself.
            assert_eq!(layout.bar_area(), None);
            for b in BUTTONS {
                assert_eq!(layout.button_area(b), None);
                assert!(layout.app_clicks(b));
            }
        }
    }

    #[test]
    fn macos_leaves_the_bar_to_the_app() {
        let mac = Layout::for_os(Os::Mac, false);
        assert_eq!(mac.bar_area(), None);
        for b in BUTTONS {
            assert_eq!(mac.button_area(b), None);
        }
    }

    #[test]
    fn windows_has_no_system_caption_and_the_bar_is_the_systems_caption() {
        use gpui::WindowControlArea as Area;
        for fullscreen in [false, true] {
            let win = Layout::for_os(Os::Windows, fullscreen);
            // The only title bar is the app's, its buttons at the right.
            assert!(win.app_buttons);
            assert_eq!(win.buttons(), "app");
            assert_eq!(win.traffic_lights, None);
            assert_eq!(win.left_padding, BAR_PADDING);
            let options = win.titlebar_options("Morpheus".into());
            assert!(options.appears_transparent, "no system caption");
            assert_eq!(options.traffic_light_position, None);
            // The system's resize borders stay: no app strips over them.
            assert!(!win.app_resize_edges);
            // The bar is the caption: drag, Aero snap, double-click maximise.
            assert_eq!(win.bar_area(), Some(Area::Drag));
            // The buttons are the caption's: snap layouts over maximise.
            assert_eq!(win.button_area(WindowButton::Minimize), Some(Area::Min));
            assert_eq!(win.button_area(WindowButton::Maximize), Some(Area::Max));
            assert_eq!(win.button_area(WindowButton::Close), Some(Area::Close));
            // The system minimises, maximises and restores; the app closes
            // (to the tray).
            assert!(!win.app_clicks(WindowButton::Minimize));
            assert!(!win.app_clicks(WindowButton::Maximize));
            assert!(win.app_clicks(WindowButton::Close));
            // The buttons fill the bar's height and reach its right end, as
            // Windows's own do: the top right corner is Close.
            assert_eq!(win.caption_buttons(), Some((46.0, BAR_HEIGHT)));
            assert_eq!(win.right_padding(), 0.0);
        }
        assert_eq!(Layout::for_os(Os::Mac, false).caption_buttons(), None);
        assert_eq!(Layout::for_os(Os::Mac, false).right_padding(), BAR_PADDING);
        // Linux: the same full-height buttons, to the window's right edge.
        assert_eq!(
            Layout::for_os(Os::Linux, false).caption_buttons(),
            Some((46.0, BAR_HEIGHT))
        );
        assert_eq!(Layout::for_os(Os::Linux, false).right_padding(), 0.0);
    }
}
