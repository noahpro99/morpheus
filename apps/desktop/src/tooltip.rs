//! Hover tooltips for icon-only buttons (#196, #206): `.tip("Inbox (Ctrl+I)")`
//! on a button with an id shows its name in a small bubble after the pointer
//! rests on it. The bubble sits below the button (above it when there is no
//! room), centred on it and inside the window, with an arrow pointing at the
//! button, so the pointer never covers it. A trailing "(Ctrl+I)" is the
//! button's shortcut and shows as key caps, like those of the Jump to
//! latest message button.

use gpui::{
    AnyView, App, Context, TooltipSide, FontWeight, IntoElement, Render, SharedString,
    StatefulInteractiveElement, Window, canvas, div, point, prelude::*, px, rgb, rgba,
};

use crate::{dialogs::key_cap, e2e::E2eExt as _, theme};

/// The arrow's height; the bubble keeps this much room on the arrow's side.
const ARROW: f32 = 6.0;

/// How long a tooltip takes to fade in.
const FADE: std::time::Duration = std::time::Duration::from_millis(120);

/// The label drawn by the button, and its shortcut's keys.
struct Tip {
    label: SharedString,
    keys: Vec<SharedString>,
    /// Where the arrow's tip was last painted (window coordinates).
    tip: Option<(f32, f32)>,
    /// When the tooltip appeared: it fades in over [`FADE`] (#517).
    born: std::time::Instant,
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Which side of the bubble the arrow is on: decided by the window
        // before this is laid out.
        let placement = window.tooltip_placement();
        let below = placement.is_none_or(|p| p.below);
        let right = placement.is_some_and(|p| p.right);
        let me = cx.entity();
        let paint_arrow = move |bounds: gpui::Bounds<gpui::Pixels>,
                                _: (),
                                window: &mut Window,
                                cx: &mut App| {
            let Some(placement) = window.tooltip_placement() else {
                return;
            };
            // Pointing at the middle of the button, kept off the bubble's
            // rounded corners.
            let reach = px(gpui::TOOLTIP_ARROW_REACH);
            let (tip, base_a, base_b) = if right {
                let pb = placement.bounds;
                let tip_y = placement
                    .anchor
                    .center()
                    .y
                    .clamp(pb.top() + reach, (pb.bottom() - reach).max(pb.top() + reach));
                let half = px(ARROW);
                (
                    point(bounds.left(), tip_y),
                    point(bounds.right(), tip_y - half),
                    point(bounds.right(), tip_y + half),
                )
            } else {
                let pb = placement.bounds;
                let tip_x = placement
                    .anchor
                    .center()
                    .x
                    .clamp(pb.left() + reach, (pb.right() - reach).max(pb.left() + reach));
                let (tip_y, base_y) = if below {
                    (bounds.top(), bounds.bottom())
                } else {
                    (bounds.bottom(), bounds.top())
                };
                let half = px(ARROW);
                (
                    point(tip_x, tip_y),
                    point(tip_x + half, base_y),
                    point(tip_x - half, base_y),
                )
            };
            // The edge colour first, a little larger, then the fill over it
            // and over the bubble's border: the border continues round the
            // arrow.
            let outward = |p: gpui::Point<gpui::Pixels>, from: gpui::Point<gpui::Pixels>, by: f32| {
                // `p` moved `by` px away from `from`, along the arrow's axis.
                if right {
                    point(p.x + px(by) * if p.x > from.x { 1.0 } else { -1.0 }, p.y)
                } else {
                    point(p.x, p.y + px(by) * if p.y > from.y { 1.0 } else { -1.0 })
                }
            };
            let centre = point((base_a.x + base_b.x) / 2.0, (base_a.y + base_b.y) / 2.0);
            for (grow, cover, colour) in [
                (1.5, 0.0, rgba(theme::edge())),
                (0.0, 1.0, rgba((theme::bg_elevated() << 8) | 0xff)),
            ] {
                let mut path = gpui::PathBuilder::fill();
                let spread = |p: gpui::Point<gpui::Pixels>| {
                    if right {
                        point(p.x, p.y + px(grow) * if p.y > tip.y { 1.0 } else { -1.0 })
                    } else {
                        point(p.x + px(grow) * if p.x > tip.x { 1.0 } else { -1.0 }, p.y)
                    }
                };
                path.move_to(outward(tip, centre, grow));
                path.line_to(outward(spread(base_a), tip, cover));
                path.line_to(outward(spread(base_b), tip, cover));
                path.close();
                if let Ok(path) = path.build() {
                    window.paint_path(path, colour);
                }
            }
            // Where it points, for the tests (and nothing else): the view
            // shows it on its next frame.
            let at = (f32::from(tip.x), f32::from(tip.y));
            me.update(cx, |tip, cx| {
                if tip.tip != Some(at) {
                    tip.tip = Some(at);
                    cx.notify();
                }
            });
        };
        let arrow = div().flex_none();
        let arrow = if right {
            arrow.w(px(ARROW))
        } else {
            arrow.h(px(ARROW))
        };
        let arrow = arrow
            .child(canvas(|_, _, _| (), paint_arrow).size_full())
            .e2e("div", "tooltip-arrow")
            .attr("data-side", if right { "right" } else if below { "below" } else { "above" })
            .attr("data-tip-x", self.tip.map(|t| t.0.to_string()).unwrap_or_default())
            .attr("data-tip-y", self.tip.map(|t| t.1.to_string()).unwrap_or_default());
        let mut bubble = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(10.0))
            .py(px(6.0))
            .rounded(px(theme::RADIUS))
            .bg(theme::fill_elevated())
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
        let root = if right {
            div().flex().flex_row().child(arrow).child(bubble)
        } else if below {
            div().flex().flex_col().child(arrow).child(bubble)
        } else {
            div().flex().flex_col().child(bubble).child(arrow)
        };
        // Fade in (ease out), a frame at a time until it is whole.
        let t = (self.born.elapsed().as_secs_f32() / FADE.as_secs_f32()).min(1.0);
        if t < 1.0 {
            window.request_animation_frame();
        }
        let opacity = 1.0 - (1.0 - t).powi(3);
        root.opacity(opacity)
            .e2e("div", "tooltip-root")
            .attr("data-opacity", opacity.to_string())
    }
}

fn view(text: SharedString, side: TooltipSide, window: &mut Window, cx: &mut App) -> AnyView {
    window.set_tooltip_side(side);
    let (label, keys) = split_shortcut(&text);
    cx.new(|_| Tip {
        label: label.into(),
        keys: keys.into_iter().map(SharedString::from).collect(),
        tip: None,
        born: std::time::Instant::now(),
    })
    .into()
}

pub trait TipExt: StatefulInteractiveElement + Sized {
    /// Shows `label` in a tooltip while the pointer rests on the element; a
    /// trailing "(Ctrl+I)" is its shortcut, drawn as key caps.
    fn tip(self, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        self.tooltip(move |window, cx| view(label.clone(), TooltipSide::Below, window, cx))
    }

    /// Like `tip`, with the tooltip to the right of the element (the server
    /// rail's, at the window's left edge), vertically centred on it.
    fn tip_beside(self, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        self.tooltip(move |window, cx| view(label.clone(), TooltipSide::Right, window, cx))
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
