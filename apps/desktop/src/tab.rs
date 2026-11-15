//! Keyboard navigation of forms: Tab and Shift+Tab move through the text
//! boxes and buttons in the order they are drawn, Enter or Space presses the
//! focused button (gpui turns those into its click), and a ring shows which
//! button has the focus. Inside a dialog the focus wraps round it instead of
//! leaving for the window behind.

use gpui::{
    App, BoxShadow, Div, FocusHandle, Global, InteractiveElement as _, KeyDownEvent, Stateful,
    Window, point, px,
};

use crate::theme;

/// Makes a button a tab stop with a visible focus ring.
pub trait Tabbable {
    fn tabbable(self) -> Self;
}

impl Tabbable for Stateful<Div> {
    fn tabbable(self) -> Self {
        self.tab_index(0).focus(|mut style| {
            let ring = gpui::Hsla::from(gpui::rgb(theme::accent()));
            style.box_shadow = Some(vec![BoxShadow {
                color: ring,
                offset: point(px(0.0), px(0.0)),
                blur_radius: px(0.0),
                spread_radius: px(2.0),
            }]);
            style
        })
    }
}

/// The focus handle of the window's current dialog layer.
struct Trap(FocusHandle);

impl Global for Trap {}

/// The handle the dialog layer tracks, so Tab can tell inside from outside.
pub fn trap(cx: &mut App) -> FocusHandle {
    if let Some(Trap(handle)) = cx.try_global::<Trap>() {
        return handle.clone();
    }
    let handle = cx.focus_handle();
    cx.set_global(Trap(handle.clone()));
    handle
}

/// Tab (Shift+Tab backwards): to the next tab stop. With `within`, only
/// stops inside it count, wrapping at its ends. Returns whether `event` was
/// a Tab.
pub fn on_key(
    event: &KeyDownEvent,
    within: Option<&FocusHandle>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let keystroke = &event.keystroke;
    if keystroke.key != "tab"
        || keystroke.modifiers.control
        || keystroke.modifiers.alt
        || keystroke.modifiers.platform
    {
        return false;
    }
    let back = keystroke.modifiers.shift;
    // Round the whole window at most once to find a stop inside.
    for _ in 0..512 {
        if back {
            window.focus_prev();
        } else {
            window.focus_next();
        }
        match within {
            Some(handle) if !handle.contains_focused(window, cx) => continue,
            _ => break,
        }
    }
    true
}
