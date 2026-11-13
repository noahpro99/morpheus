//! The full-size picture viewer (`crates/ui/src/components/lightbox.rs`): a
//! picture of the room opened from its message, dimming the whole window,
//! with the room's other pictures a Left or Right away; Escape, a click
//! outside and the ✕ close it. The wheel zooms around the pointer (fit to the
//! window up to 8x), a drag pans a zoomed picture, a double click toggles
//! fit and actual size, and the buttons and + - 0 zoom too.

use std::sync::Arc;

use gpui::{
    AnyElement, ClickEvent, Context, EventEmitter, FocusHandle, FontWeight, Image, IntoElement,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ScrollWheelEvent,
    MouseButton, ObjectFit, Render, StyledImage as _, Window, deferred, div, img, prelude::*, px,
    rgb, rgba,
};
use morpheus_core::{MediaRef, Session};

use crate::{
    dialogs::Closed,
    e2e::E2eExt as _,
    media, theme,
    zoom::{MAX_ZOOM, ZoomPan},
};

/// One picture of the room's gallery.
#[derive(Clone)]
pub struct GalleryImage {
    pub media: MediaRef,
    pub body: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

pub struct Lightbox {
    session: Session,
    images: Vec<GalleryImage>,
    index: usize,
    /// Full-size pictures fetched so far, by media key.
    full: std::collections::HashMap<String, Arc<Image>>,
    asked: std::collections::HashSet<String>,
    focus: FocusHandle,
    take_focus: bool,
    /// The zoom (1 is the picture fitted to the window) and the pan.
    view: ZoomPan,
    /// A pan in progress: where the pointer went down and the offset then.
    panning: Option<((f32, f32), (f32, f32))>,
    /// The fitted size and window size of the last frame.
    fit: (f32, f32),
    viewport: (f32, f32),
}

const STEP: f32 = 1.25;

impl EventEmitter<Closed> for Lightbox {}

impl Lightbox {
    pub fn new(session: Session, images: Vec<GalleryImage>, index: usize, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            session,
            index: index.min(images.len().saturating_sub(1)),
            images,
            full: Default::default(),
            asked: Default::default(),
            focus: cx.focus_handle(),
            take_focus: true,
            view: ZoomPan::default(),
            panning: None,
            fit: (0.0, 0.0),
            viewport: (0.0, 0.0),
        };
        this.fetch(cx);
        this
    }

    /// Starts fetching the picture shown, if it is not here or asked for.
    fn fetch(&mut self, cx: &mut Context<Self>) {
        let Some(image) = self.images.get(self.index) else {
            return;
        };
        let key = image.media.key().to_owned();
        if self.full.contains_key(&key) || !self.asked.insert(key.clone()) {
            return;
        }
        let fetching = self.session.media_file(image.media.clone());
        cx.spawn(async move |this, cx| {
            let picture = fetching.await.ok().flatten().and_then(|bytes| media::decode(&bytes));
            this.update(cx, |this, cx| {
                if let Some(picture) = picture {
                    this.full.insert(key, picture);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn go(&mut self, delta: isize, cx: &mut Context<Self>) {
        let last = self.images.len().saturating_sub(1) as isize;
        let next = (self.index as isize + delta).clamp(0, last) as usize;
        if next != self.index {
            self.index = next;
            self.view.reset();
            self.panning = None;
            self.fetch(cx);
            cx.notify();
        }
    }

    fn zoom_at(&mut self, to: f32, at: (f32, f32), cx: &mut Context<Self>) {
        self.view.zoom_at(to, at, self.fit, self.viewport);
        cx.notify();
    }

    fn zoom_middle(&mut self, to: f32, cx: &mut Context<Self>) {
        let middle = (self.viewport.0 / 2.0, self.viewport.1 / 2.0);
        self.zoom_at(to, middle, cx);
    }

    /// The zoom that shows the picture at its own pixels (2 if that is no
    /// more than fit: a small picture).
    fn actual_zoom(&self) -> f32 {
        let natural = self.images.get(self.index).and_then(|image| image.width).unwrap_or(0) as f32;
        let actual = if self.fit.0 > 0.0 { natural / self.fit.0 } else { 0.0 };
        if actual > 1.05 { actual.min(MAX_ZOOM) } else { 2.0 }
    }

    fn wheel(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let Some(factor) = ZoomPan::wheel_factor(event.delta) else {
            return;
        };
        let at = (f32::from(event.position.x), f32::from(event.position.y));
        self.zoom_at(self.view.zoom * factor, at, cx);
        cx.stop_propagation();
    }

    /// A press on the picture: a double click toggles fit and actual size,
    /// a press starts a pan when it is zoomed.
    fn press_picture(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let at = (f32::from(event.position.x), f32::from(event.position.y));
        if event.click_count >= 2 {
            self.panning = None;
            let to = if self.view.zoomed() { 1.0 } else { self.actual_zoom() };
            self.zoom_at(to, at, cx);
        } else if self.view.zoomed() {
            self.panning = Some((at, self.view.offset));
            cx.notify();
        }
    }

    fn drag(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some((start, offset)) = self.panning else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.panning = None;
            cx.notify();
            return;
        }
        self.view.offset = (
            offset.0 + f32::from(event.position.x) - start.0,
            offset.1 + f32::from(event.position.y) - start.1,
        );
        self.view.clamp(self.fit, self.viewport);
        cx.notify();
    }

    fn zoom_button(
        &self,
        class: &'static str,
        title: &'static str,
        label: String,
        cx: &mut Context<Self>,
        act: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        div()
            .id(class)
            .min_w(px(32.0))
            .h(px(32.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .text_size(px(1.0 * theme::REM))
            .text_color(rgb(0xffffff))
            .cursor_pointer()
            .hover(|d| d.bg(rgba(0xffffff26)))
            .child(label.clone())
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| act(this, cx)))
            .e2e("button", &format!("lightbox-zoom-button lightbox-{class}"))
            .attr("title", title)
            .text(label)
            .into_any_element()
    }

    fn nav_button(
        &self,
        class: &'static str,
        title: &'static str,
        glyph: &'static str,
        delta: isize,
        top: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(class)
            .absolute()
            .top(px(top))
            .when(delta < 0, |d| d.left(px(-60.0)))
            .when(delta > 0, |d| d.right(px(-60.0)))
            .px(px(12.0))
            .py(px(8.0))
            .rounded_full()
            .bg(rgba(0x00000066))
            .text_size(px(1.6 * theme::REM))
            .text_color(rgb(0xffffff))
            .cursor_pointer()
            .child(glyph)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.go(delta, cx)))
            .e2e("button", &format!("lightbox-nav {class}"))
            .attr("title", title)
            .text(glyph)
            .into_any_element()
    }
}

impl Render for Lightbox {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.take_focus) {
            window.focus(&self.focus);
        }
        let Some(image) = self.images.get(self.index).cloned() else {
            cx.emit(Closed);
            return div().into_any_element();
        };
        let count = self.images.len();
        let viewport = window.viewport_size();
        let (avail_w, avail_h) = (
            0.9 * f32::from(viewport.width) - 120.0,
            0.85 * f32::from(viewport.height) - 60.0,
        );
        let (w, h) = match (image.width, image.height) {
            (Some(w), Some(h)) if w > 0 && h > 0 => (w as f32, h as f32),
            _ => (640.0, 480.0),
        };
        let scale = (avail_w / w).min(avail_h / h).min(1.0);
        let (w, h) = (w * scale, h * scale);
        self.fit = (w, h);
        self.viewport = (f32::from(viewport.width), f32::from(viewport.height));
        self.view.clamp(self.fit, self.viewport);
        let ZoomPan { zoom, offset } = self.view;
        let panning = self.panning.is_some();
        let picture = match self.full.get(image.media.key()) {
            Some(full) => div()
                .absolute()
                .left(px((w - w * zoom) / 2.0 + offset.0))
                .top(px((h - h * zoom) / 2.0 + offset.1))
                .w(px(w * zoom))
                .h(px(h * zoom))
                .when(zoom > 1.001, |d| if panning { d.cursor_grabbing() } else { d.cursor_grab() })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| this.press_picture(event, cx)),
                )
                .child(img(full.clone()).size_full().rounded(px(6.0)).object_fit(ObjectFit::Contain))
                .e2e("img", "lightbox-image")
                .attr("alt", image.body.clone())
                .attr("data-zoom", format!("{zoom:.4}"))
                .attr("data-x", format!("{:.1}", offset.0))
                .attr("data-y", format!("{:.1}", offset.1)),
            // Not here yet: the picture's shape.
            None => div()
                .absolute()
                .size_full()
                .rounded(px(theme::RADIUS))
                .bg(rgb(theme::bg_elevated()))
                .e2e("div", "lightbox-loading skeleton"),
        };
        let mut row = div().relative().flex().items_center();
        if count > 1 && self.index > 0 {
            row = row.child(self.nav_button("prev", "Previous image", "‹", -1, h / 2.0 - 20.0, cx));
        }
        row = row.child(div().relative().w(px(w)).h(px(h)).flex_none().child(picture).e2e("div", "lightbox-image-wrap"));
        if count > 1 && self.index + 1 < count {
            row = row.child(self.nav_button("next", "Next image", "›", 1, h / 2.0 - 20.0, cx));
        }
        let mut caption = div()
            .flex()
            .gap(px(12.0))
            .text_size(px(0.85 * theme::REM))
            .text_color(rgb(theme::text_muted()))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(theme::text()))
                    .child(image.body.clone())
                    .e2e("span", "lightbox-filename")
                    .text(image.body.clone()),
            );
        if let (Some(w), Some(h)) = (image.width, image.height) {
            let dims = format!("{w} × {h}");
            caption = caption.child(div().child(dims.clone()).e2e("span", "lightbox-dims").text(dims));
        }
        if count > 1 {
            let place = format!("{} / {count}", self.index + 1);
            caption = caption.child(div().child(place.clone()).e2e("span", "lightbox-count").text(place));
        }
        let boxed = div()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("lightbox-close")
                    .absolute()
                    .top(px(-28.0))
                    .right(px(-8.0))
                    .text_size(px(1.2 * theme::REM))
                    .text_color(rgb(theme::text_muted()))
                    .cursor_pointer()
                    .child("✕")
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(Closed)))
                    .e2e("button", "lightbox-close")
                    .attr("title", "Close")
                    .text("✕"),
            )
            .child(row)
            .when(zoom <= 1.001, |d| {
                d.child(
                    div()
                        .absolute()
                        .top(px(h + 10.0))
                        .left_0()
                        .w_full()
                        .flex()
                        .justify_center()
                        .child(caption.e2e("div", "lightbox-caption")),
                )
            })
            .relative()
            .e2e("div", "lightbox");
        let percent = if zoom > 1.001 { format!("{:.0}%", zoom * 100.0) } else { "Fit".to_owned() };
        let toolbar = div()
            .absolute()
            .bottom(px(20.0))
            .left_0()
            .w_full()
            .flex()
            .justify_center()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .p(px(4.0))
                    .rounded_full()
                    .bg(rgba(0x000000a6))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(self.zoom_button("zoom-out", "Zoom out", "−".to_owned(), cx, |this, cx| this.zoom_middle(this.view.zoom / STEP, cx)))
                    .child(self.zoom_button("zoom-reset", "Fit to window", percent, cx, |this, cx| this.zoom_middle(1.0, cx)))
                    .child(self.zoom_button("zoom-in", "Zoom in", "+".to_owned(), cx, |this, cx| this.zoom_middle(this.view.zoom * STEP, cx)))
                    .e2e("div", "lightbox-zoombar"),
            );
        let layer = div()
            .id("lightbox")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => cx.emit(Closed),
                    "left" => this.go(-1, cx),
                    "right" => this.go(1, cx),
                    "+" | "=" => this.zoom_middle(this.view.zoom * STEP, cx),
                    "-" | "_" => this.zoom_middle(this.view.zoom / STEP, cx),
                    "0" => this.zoom_middle(1.0, cx),
                    _ => {}
                }
            }))
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x000000d9))
            .occlude()
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| this.wheel(event, cx)))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| this.drag(event, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    if this.panning.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    if this.panning.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(Closed)),
            )
            .child(boxed)
            .child(toolbar)
            .e2e("div", "overlay lightbox-overlay")
            .focusable(&self.focus);
        deferred(layer).with_priority(30).into_any_element()
    }
}

impl crate::shell::Shell {
    /// Opens the full-size viewer on picture `index` of the room's pictures.
    pub fn open_lightbox(&mut self, images: Vec<GalleryImage>, index: usize, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let viewer = cx.new(|cx| Lightbox::new(session, images, index, cx));
        cx.subscribe(&viewer, |this, _, _: &Closed, cx| this.dialog_close(cx))
            .detach();
        self.dialog_open(crate::dialogs::Dialog::Lightbox(viewer), cx);
    }
}
