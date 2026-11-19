//! Hover tooltips for icon-only buttons (#196, #206): `.tip("Inbox (Ctrl+I)")`
//! on a button with an id shows its name in a small bubble after the pointer
//! rests on it. The bubble sits below the button (above it when there is no
//! room), centred on it and inside the window, with an arrow pointing at the
//! button, so the pointer never covers it. A trailing "(Ctrl+I)" is the
//! button's shortcut and shows as key caps, like those of the Jump to
//! latest message button.

use gpui::{
    AnyView, App, Context, FontWeight, IntoElement, Render, SharedString,
    StatefulInteractiveElement, Window, canvas, div, point, prelude::*, px, rgb, rgba,
};

use crate::{dialogs::key_cap, e2e::E2eExt as _, theme};

/// The arrow's height; the bubble keeps this much room on the arrow's side.
const ARROW: f32 = 6.0;

/// The label drawn by the button, and its shortcut's keys.
struct Tip {
    label: SharedString,
    keys: Vec<SharedString>,
}

/// "Inbox (Ctrl+I)" is the label "Inbox" and the keys Ctrl and I; any other
/// text is all label.
fn split_shortcut(text: &str) -> (String, Vec<String>) {
    if let Some(open) = text.rfind(" (")
        && let Some(inner) = text[open + 2..].strip_suffix(')')
        && !inner.contains(' ')
        && (inner.contains('+') || matches!(inner, "Esc" | "Enter" | "Tab"))
    {
        let keys = inner.split('+').map(str::to_owned).collect();
        return (text[..open].to_owned(), keys);
    }
    (text.to_owned(), Vec::new())
}

impl Render for Tip {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // Which side of the bubble the arrow is on: decided by the window
        // before this is laid out.
        let below = window.tooltip_placement().is_none_or(|p| p.below);
        let arrow = div()
            .w_full()
            .h(px(ARROW))
            .flex_none()
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        let Some(placement) = window.tooltip_placement() else {
                            return;
                        };
                        // Pointing at the middle of the button, kept off the
                        // bubble's rounded corners.
                        let reach = px(14.0);
                        let tip_x = placement
                            .anchor
                            .center()
                            .x
                            .clamp(bounds.left() + reach, (bounds.right() - reach).max(bounds.left() + reach));
                        let (tip_y, base_y) = if below {
                            (bounds.top(), bounds.bottom())
                        } else {
                            (bounds.bottom(), bounds.top())
                        };
                        let half = px(ARROW);
                        // The edge colour first, a little larger, then the fill
                        // over it and over the bubble's border: the border
                        // continues round the arrow.
                        let sign = if below { 1.0 } else { -1.0 };
                        for (grow, cover, colour) in [
                            (px(1.5), px(0.0), rgba(theme::edge())),
                            (px(0.0), px(1.0), rgba((theme::bg_elevated() << 8) | 0xff)),
                        ] {
                            let mut path = gpui::PathBuilder::fill();
                            path.move_to(point(tip_x, tip_y - grow * sign));
                            path.line_to(point(tip_x + half + grow, base_y + cover * sign));
                            path.line_to(point(tip_x - half - grow, base_y + cover * sign));
                            path.close();
                            if let Ok(path) = path.build() {
                                window.paint_path(path, colour);
                            }
                        }
                    },
                )
                .size_full(),
            )
            .e2e("div", "tooltip-arrow");
        let mut bubble = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(10.0))
            .py(px(6.0))
            .rounded(px(theme::RADIUS))
            .bg(rgb(theme::bg_elevated()))
            .border_1()
            .border_color(rgba(theme::edge()))
            .shadow_md()
            .font_family("Inter")
            .font_weight(FontWeight::MEDIUM)
            .text_size(px(0.85 * theme::REM))
            .text_color(rgb(theme::text()))
            .max_w(px(300.0))
            .child(self.label.clone());
        for key in &self.keys {
            bubble = bubble.child(key_cap(key.clone(), None));
        }
        let bubble = bubble.e2e("div", "tooltip").text(self.label.to_string());
        // The arrow's strip is on the side facing the button.
        let mut root = div().flex().flex_col();
        if below {
            root = root.child(arrow).child(bubble);
        } else {
            root = root.child(bubble).child(arrow);
        }
        root
    }
}

fn view(text: SharedString, cx: &mut App) -> AnyView {
    let (label, keys) = split_shortcut(&text);
    cx.new(|_| Tip {
        label: label.into(),
        keys: keys.into_iter().map(SharedString::from).collect(),
    })
    .into()
}

pub trait TipExt: StatefulInteractiveElement + Sized {
    /// Shows `label` in a tooltip while the pointer rests on the element; a
    /// trailing "(Ctrl+I)" is its shortcut, drawn as key caps.
    fn tip(self, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        self.tooltip(move |_, cx| view(label.clone(), cx))
    }
}

impl<T: StatefulInteractiveElement> TipExt for T {}

#[cfg(test)]
mod tests {
    use super::split_shortcut;

    #[test]
    fn a_trailing_shortcut_becomes_keys() {
        assert_eq!(split_shortcut("Inbox (Ctrl+I)"), ("Inbox".into(), vec!["Ctrl".into(), "I".into()]));
        assert_eq!(
            split_shortcut("Mute (Ctrl+Shift+M)"),
            ("Mute".into(), vec!["Ctrl".into(), "Shift".into(), "M".into()])
        );
        assert_eq!(split_shortcut("Cancel (Esc)"), ("Cancel".into(), vec!["Esc".into()]));
        assert_eq!(split_shortcut("Back (Alt+←)"), ("Back".into(), vec!["Alt".into(), "←".into()]));
    }

    #[test]
    fn other_text_is_all_label() {
        assert_eq!(split_shortcut("Create a server"), ("Create a server".into(), vec![]));
        assert_eq!(split_shortcut("Call (voice only)"), ("Call (voice only)".into(), vec![]));
        assert_eq!(split_shortcut("Streamer mode is on (click to turn off)"), ("Streamer mode is on (click to turn off)".into(), vec![]));
    }
}
