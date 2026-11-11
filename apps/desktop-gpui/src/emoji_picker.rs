//! The full emoji picker (`crates/ui/src/components/emoji_picker.rs`),
//! opened by the hover bar's "more" button: a search and a skin tone on top,
//! a tab per category down the left, every emoji in a grid by category,
//! the recently used first. What it lists is `view_model::emoji_picker`.

use gpui::{
    AnyElement, Context, ElementId, EventEmitter, FocusHandle, FontWeight, IntoElement,
    MouseButton, Pixels, Point, Render, ScrollHandle, SharedString, Subscription, Window, anchored,
    deferred, div, point, prelude::*, px, relative, rgb, rgba,
};

use crate::{
    e2e::{E2eExt as _, HandleProbe},
    icons::icon,
    input::{Submit, TextInput},
    theme,
    view_model::emoji_picker::{GROUPS, Remembered, TONES, hand, sections, tone_of},
};

/// What the picker tells whoever opened it.
pub enum PickerEvent {
    /// This emoji was chosen.
    Picked(String),
    Closed,
}

impl EventEmitter<PickerEvent> for EmojiPicker {}

const WIDTH: f32 = 360.0;
const HEIGHT: f32 = 440.0;

pub struct EmojiPicker {
    /// Where it was opened (the button's click).
    at: Point<Pixels>,
    search: gpui::Entity<TextInput>,
    remembered: Remembered,
    tones_open: bool,
    scroll: ScrollHandle,
    focus: FocusHandle,
    take_focus: bool,
    _subscription: Subscription,
}

impl EmojiPicker {
    pub fn new(at: Point<Pixels>, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| {
            TextInput::new("emoji-search", "Find the perfect emoji", false, "", cx)
                .bare("input", "emoji-search")
        });
        let subscription = cx.subscribe(&search, |this, _, _: &Submit, cx| {
            // Enter picks the first match.
            let query = this.search.read(cx).value().to_owned();
            let tone = tone_of(this.remembered.tone);
            if let Some(first) = sections(&query, tone, &[])
                .into_iter()
                .next()
                .filter(|_| !query.trim().is_empty())
                .and_then(|s| s.cells.into_iter().next())
            {
                this.pick(first, cx);
            }
        });
        // Typing changes what is listed.
        cx.observe(&search, |_, _, cx| cx.notify()).detach();
        Self {
            at,
            search,
            remembered: Remembered::load(&crate::data_dir()),
            tones_open: false,
            scroll: ScrollHandle::new(),
            focus: cx.focus_handle(),
            take_focus: true,
            _subscription: subscription,
        }
    }

    fn pick(&mut self, emoji: String, cx: &mut Context<Self>) {
        self.remembered.note_used(&emoji);
        if let Err(err) = self.remembered.save(&crate::data_dir()) {
            eprintln!("morpheus-gpui: could not save the recent emoji: {err}");
        }
        cx.emit(PickerEvent::Picked(emoji));
    }
}

impl Render for EmojiPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.take_focus) {
            self.search.read(cx).focus(window);
        }
        let query = self.search.read(cx).value().to_owned();
        let searching = !query.trim().is_empty();
        let tone = tone_of(self.remembered.tone);
        let listed = sections(&query, tone, &self.remembered.recent);

        // The search box and the skin tone.
        let tones = div().relative().child(
            div()
                .id("skin-tone-button")
                .size(px(34.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(theme::RADIUS_SM))
                .text_size(px(1.3 * theme::REM))
                .cursor_pointer()
                .hover(|s| s.bg(rgba(theme::BG_HOVER)))
                .child(hand(tone))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.tones_open = !this.tones_open;
                    cx.notify();
                }))
                .e2e("button", "skin-tone-button")
                .attr("title", "Skin tone")
                .text(hand(tone)),
        );
        let mut tone_menu = None;
        if self.tones_open {
            tone_menu = Some(
                div()
                    .absolute()
                    .top(px(48.0))
                    .right(px(10.0))
                    .flex()
                    .flex_col()
                    .p(px(4.0))
                    .rounded(px(theme::RADIUS_SM))
                    .bg(rgb(theme::BG_RAIL))
                    .shadow_lg()
                    .occlude()
                    .children(TONES.into_iter().map(|(stored, name, option)| {
                        let active = option == tone;
                        div()
                            .id(ElementId::from(SharedString::from(format!("tone-{name}"))))
                            .size(px(34.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(theme::RADIUS_SM))
                            .text_size(px(1.3 * theme::REM))
                            .cursor_pointer()
                            .when(active, |d| d.bg(rgba(theme::BG_HOVER)))
                            .hover(|s| s.bg(rgba(theme::BG_HOVER)))
                            .child(hand(option))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.remembered.set_tone(stored);
                                this.tones_open = false;
                                if let Err(err) = this.remembered.save(&crate::data_dir()) {
                                    eprintln!("morpheus-gpui: could not save the skin tone: {err}");
                                }
                                cx.notify();
                            }))
                            .e2e("button", if active { "skin-tone-option active" } else { "skin-tone-option" })
                            .attr("data-tone", name)
                            .attr("title", name)
                            .text(hand(option))
                    }))
                    .e2e("div", "skin-tone-menu"),
            );
        }
        let top = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.0))
            .p(px(10.0))
            .border_b_1()
            .border_color(rgba(theme::BG_SELECTED))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .px(px(10.0))
                    .rounded(px(theme::RADIUS_SM))
                    .bg(rgb(theme::BG_RAIL))
                    .text_color(rgb(theme::TEXT))
                    .child(div().flex_1().min_w_0().child(self.search.clone()))
                    .child(icon("search", 16.0).text_color(rgb(theme::TEXT_MUTED)))
                    .e2e("label", "emoji-search-box"),
            )
            .child(tones.e2e("div", "skin-tone"));

        // The category tabs (not while searching).
        let mut body = div().flex().flex_1().min_h_0();
        if !searching {
            let mut tabs = div()
                .flex()
                .flex_col()
                .flex_none()
                .items_center()
                .gap(px(2.0))
                .w(px(44.0))
                .py(px(6.0))
                .bg(rgb(theme::BG_RAIL));
            let mut entries: Vec<(&'static str, &'static str)> = Vec::new();
            if !self.remembered.recent.is_empty() {
                entries.push(("Recent", "🕘"));
            }
            entries.extend(GROUPS.iter().map(|(_, name, glyph)| (*name, *glyph)));
            for (index, (name, glyph)) in entries.into_iter().enumerate() {
                tabs = tabs.child(
                    div()
                        .id(ElementId::from(SharedString::from(format!("tab-{name}"))))
                        .size(px(34.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(theme::RADIUS_SM))
                        .text_size(px(1.15 * theme::REM))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgba(theme::BG_HOVER)))
                        .child(glyph)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.scroll.scroll_to_top_of_item(index);
                            cx.notify();
                        }))
                        .e2e("button", "emoji-tab")
                        .attr("title", name)
                        .text(glyph),
                );
            }
            body = body.child(tabs.e2e("nav", "emoji-tabs"));
        }

        // The grid: each section is one child of the scrolling column.
        let mut grid = div()
            .id("emoji-grid")
            .flex_1()
            .min_w_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .flex()
            .flex_col()
            .px(px(8.0))
            .pb(px(8.0));
        if listed.is_empty() {
            grid = grid.child(
                div()
                    .px(px(8.0))
                    .py(px(24.0))
                    .flex()
                    .justify_center()
                    .text_color(rgb(theme::TEXT_MUTED))
                    .child("No emoji found")
                    .e2e("p", "emoji-none muted")
                    .text("No emoji found"),
            );
        }
        for section in listed {
            let mut cells = div().flex().flex_wrap();
            for cell in &section.cells {
                let emoji = cell.clone();
                cells = cells.child(
                    div()
                        .id(ElementId::from(SharedString::from(format!("{}-{cell}", section.group))))
                        .w(px(37.0))
                        .h(px(37.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(theme::RADIUS_SM))
                        .text_size(px(1.45 * theme::REM))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgba(theme::BG_HOVER)))
                        .child(cell.clone())
                        .on_click(cx.listener(move |this, _, _, cx| this.pick(emoji.clone(), cx)))
                        .e2e("button", "emoji-cell")
                        .text(cell.clone()),
                );
            }
            let mut block = div().flex().flex_col();
            if !section.heading.is_empty() {
                block = block.child(
                    div()
                        .pt(px(8.0))
                        .pb(px(4.0))
                        .px(px(2.0))
                        .text_size(px(0.72 * theme::REM))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(theme::TEXT_MUTED))
                        .child(section.heading.to_uppercase())
                        .e2e("h3", "")
                        .text(section.heading),
                );
            }
            grid = grid.child(
                block
                    .child(cells.e2e("div", "emoji-cells"))
                    .e2e("section", "emoji-section")
                    .attr("data-group", section.group),
            );
        }
        body = body.child(
            grid.e2e("div", "emoji-grid-scroll")
                .scrollable(std::rc::Rc::new(HandleProbe(self.scroll.clone()))),
        );

        let size = window.viewport_size();
        // Beside the button, to its left, kept inside the window.
        let x = (f32::from(self.at.x) - WIDTH - 8.0).max(8.0).min(f32::from(size.width) - WIDTH - 8.0);
        let y = f32::from(self.at.y).max(8.0).min(f32::from(size.height) - HEIGHT - 8.0);
        let panel = div()
            .id("emoji-picker")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(PickerEvent::Closed);
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .occlude()
            .relative()
            .w(px(WIDTH))
            .h(px(HEIGHT))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(10.0))
            .bg(rgb(theme::BG_SIDEBAR))
            .border_1()
            .border_color(rgba(0xffffff0f))
            .shadow_lg()
            .text_color(rgb(theme::TEXT))
            .line_height(relative(theme::LINE_HEIGHT))
            .child(top)
            .child(body)
            .children(tone_menu)
            .e2e("div", "emoji-picker")
            .attr("role", "dialog")
            .focusable(&self.focus);
        let layer = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .on_mouse_down(MouseButton::Left, cx.listener(|_, _, _, cx| cx.emit(PickerEvent::Closed)))
            .on_mouse_down(MouseButton::Right, cx.listener(|_, _, _, cx| cx.emit(PickerEvent::Closed)))
            .child(
                anchored()
                    .position(point(px(x), px(y)))
                    .snap_to_window_with_margin(px(8.0))
                    .child(panel),
            )
            .e2e("div", "emoji-picker-backdrop");
        let element: AnyElement = deferred(layer).with_priority(35).into_any_element();
        element
    }
}
