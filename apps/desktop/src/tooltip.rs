//! Hover tooltips for icon-only buttons (#196): `.tip("Inbox")` on a button
//! with an id shows its name in a small label after the pointer rests on it.

use gpui::{
    AnyView, App, Context, IntoElement, Render, SharedString, StatefulInteractiveElement, Window,
    div, prelude::*, px, rgb, rgba,
};

use crate::{e2e::E2eExt as _, theme};

/// The label drawn beside the pointer.
struct Tip(SharedString);

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(8.0))
            .py(px(4.0))
            .rounded(px(theme::RADIUS_SM))
            .bg(rgb(theme::bg_elevated()))
            .border_1()
            .border_color(rgba(theme::edge()))
            .text_size(px(0.8 * theme::REM))
            .text_color(rgb(theme::text()))
            .max_w(px(280.0))
            .child(self.0.clone())
            .e2e("div", "tooltip")
            .text(self.0.to_string())
    }
}

fn view(label: SharedString, cx: &mut App) -> AnyView {
    cx.new(|_| Tip(label)).into()
}

pub trait TipExt: StatefulInteractiveElement + Sized {
    /// Shows `label` in a tooltip while the pointer rests on the element.
    fn tip(self, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        self.tooltip(move |_, cx| view(label.clone(), cx))
    }
}

impl<T: StatefulInteractiveElement> TipExt for T {}
