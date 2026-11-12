//! The underline a link-like label gets under the pointer.
//!
//! gpui shapes text before it knows where the pointer is, so
//! `.hover(|s| s.underline())` does nothing. Instead a 1 px line is laid
//! under the label and shown by `group_hover`: every such label (a sender's
//! name, the profile's id, "mutual servers", a toast's Undo) uses this one
//! helper, as Dioxus uses `:hover { text-decoration: underline }`.

use gpui::{InteractiveElement, ParentElement, SharedString, Styled, div, px, rgb};

/// `el` (a label that holds its text as children, as wide as the text: give
/// it `self_start()` in a column) with a line `bottom` px above its bottom
/// edge in `colour`, shown while the pointer is over it. Call it after the
/// text has been added as a child. `group` names the hover group: unique per
/// label on screen.
pub fn underline_on_hover<E>(el: E, group: impl Into<SharedString>, colour: u32, bottom: f32) -> E
where
    E: InteractiveElement + ParentElement + Styled,
{
    let group = group.into();
    el.group(group.clone()).relative().child(
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(px(bottom))
            .h(px(1.0))
            .group_hover(group, move |s| s.bg(rgb(colour))),
    )
}

/// Makes `el` as wide as its text inside a column (its hover line then spans
/// the text, not the column).
pub fn as_wide_as_its_text<E: Styled>(mut el: E) -> E {
    el.style().align_self = Some(gpui::AlignSelf::FlexStart);
    el
}
