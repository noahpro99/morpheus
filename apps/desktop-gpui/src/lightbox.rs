//! The full-size picture viewer (`crates/ui/src/components/lightbox.rs`): a
//! picture of the room opened from its message, dimming the whole window,
//! with the room's other pictures a Left or Right away; Escape, a click
//! outside and the ✕ close it.

use std::sync::Arc;

use gpui::{
    AnyElement, ClickEvent, Context, EventEmitter, FocusHandle, FontWeight, Image, IntoElement,
    MouseButton, ObjectFit, Render, StyledImage as _, Window, deferred, div, img, prelude::*, px,
    rgb, rgba,
};
use morpheus_core::{MediaRef, Session};

use crate::{dialogs::Closed, e2e::E2eExt as _, media, theme};

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
}

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
            self.fetch(cx);
            cx.notify();
        }
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
        let picture = match self.full.get(image.media.key()) {
            Some(full) => div()
                .size_full()
                .child(img(full.clone()).size_full().object_fit(ObjectFit::Contain))
                .e2e("img", "lightbox-image")
                .attr("alt", image.body.clone()),
            // Not here yet: the picture's shape.
            None => div()
                .size_full()
                .rounded(px(theme::RADIUS))
                .bg(rgb(theme::bg_elevated()))
                .e2e("div", "lightbox-loading skeleton"),
        };
        let mut row = div().relative().flex().items_center();
        if count > 1 && self.index > 0 {
            row = row.child(self.nav_button("prev", "Previous image", "‹", -1, h / 2.0 - 20.0, cx));
        }
        row = row.child(div().w(px(w)).h(px(h)).flex_none().child(picture).e2e("div", "lightbox-image-wrap"));
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
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.0))
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
            .child(caption.e2e("div", "lightbox-caption"))
            .relative()
            .e2e("div", "lightbox");
        let layer = div()
            .id("lightbox")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => cx.emit(Closed),
                    "left" => this.go(-1, cx),
                    "right" => this.go(1, cx),
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
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(Closed)),
            )
            .child(boxed)
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
