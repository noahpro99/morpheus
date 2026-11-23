//! The pick-and-crop flow shared by every picture a person sets (profile picture, server or
//! room icon, custom emoji, #267): the chosen file in a square frame under a circular mask,
//! dragged to position, zoomed by the wheel or the slider; `cropped_png` then cuts the square.
//! The owner view keeps a `Picked` and passes `crop_view` a function to find it.

use gpui::{AnyElement, Context, MouseButton, canvas, div, prelude::*, px, rgb, rgba};

use crate::{e2e::E2eExt as _, theme};

fn muted(text: &str, class: &str) -> impl IntoElement {
    let text = crate::view_model::i18n::t(text);
    div()
        .text_color(rgb(theme::text_muted()))
        .child(text.clone())
        .e2e("p", class)
        .text(&text)
        .attr("data-color", format!("{:06x}", theme::text_muted()))
        .attr("data-bg", format!("{:06x}", theme::bg_chat()))
}

/// The picture in `bytes`, if it is a format GPUI draws.
pub(crate) fn picture_from(bytes: Vec<u8>) -> Option<std::sync::Arc<gpui::Image>> {
    let format = match image::guess_format(&bytes).ok()? {
        image::ImageFormat::Png => gpui::ImageFormat::Png,
        image::ImageFormat::Jpeg => gpui::ImageFormat::Jpeg,
        image::ImageFormat::Gif => gpui::ImageFormat::Gif,
        image::ImageFormat::WebP => gpui::ImageFormat::Webp,
        _ => return None,
    };
    Some(std::sync::Arc::new(gpui::Image::from_bytes(format, bytes)))
}

/// Side of the crop view's square frame, in pixels.
pub(crate) const CROP_FRAME: f32 = 280.0;

/// A picture chosen for the avatar and being cropped: what it looks like,
/// where the frame looks at it, the drag in progress and the zoom slider's
/// bounds (found when it is drawn).
pub(crate) struct Picked {
    pub(crate) image: std::sync::Arc<gpui::Image>,
    pub(crate) crop: crate::view_model::crop::Crop,
    drag: Option<gpui::Point<gpui::Pixels>>,
    bar: std::rc::Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
}

/// The chosen picture, if it is a format Morpheus can draw and cut.
pub(crate) fn picked_from(bytes: Vec<u8>) -> Option<Picked> {
    let (width, height) = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;
    let image = picture_from(bytes)?;
    Some(Picked {
        image,
        crop: crate::view_model::crop::Crop::new(width, height),
        drag: None,
        bar: Default::default(),
    })
}

pub(crate) use crate::view_model::crop::cropped_png;

/// The crop view: the picture in a square frame under a circular mask,
/// dragged to position, zoomed by the wheel or the slider.
pub(crate) fn crop_view<T: 'static>(
    picked: &Picked,
    get: fn(&mut T) -> Option<&mut Picked>,
    cx: &mut Context<T>,
) -> AnyElement {
    let frame = CROP_FRAME;
    let crop = picked.crop;
    let (left, top) = crop.offset(frame);
    let (width, height) = crop.shown_size(frame);
    let shade = div()
        .absolute()
        .left(px(-frame / 2.0))
        .top(px(-frame / 2.0))
        .size(px(frame * 2.0))
        .border(px(frame / 2.0))
        .border_color(rgba(0x000000aa))
        .rounded(px(frame));
    let ring = div()
        .absolute()
        .inset_0()
        .rounded_full()
        .border_1()
        .border_color(rgba(0xffffffcc));
    let area = div()
        .id("crop-frame")
        .relative()
        .flex_none()
        .size(px(frame))
        .overflow_hidden()
        .rounded(px(theme::RADIUS_SM))
        .bg(rgb(0x000000)) // fixed: the video's black
        .cursor_pointer()
        .child(
            gpui::img(crate::media::drawn(picked.image.clone()))
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(width))
                .h(px(height))
                .object_fit(gpui::ObjectFit::Fill),
        )
        .child(shade)
        .child(ring)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &gpui::MouseDownEvent, _, _| {
                if let Some(picked) = get(this) {
                    picked.drag = Some(event.position);
                }
            }),
        )
        .on_mouse_move(cx.listener(move |this, event: &gpui::MouseMoveEvent, _, cx| {
            let Some(picked) = get(this) else {
                return;
            };
            if event.pressed_button != Some(MouseButton::Left) {
                picked.drag = None;
                return;
            }
            if let Some(last) = picked.drag {
                let moved = event.position - last;
                picked
                    .crop
                    .pan_by(f32::from(moved.x), f32::from(moved.y), CROP_FRAME);
                picked.drag = Some(event.position);
                cx.notify();
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |this, _: &gpui::MouseUpEvent, _, _| {
                if let Some(picked) = get(this) {
                    picked.drag = None;
                }
            }),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(move |this, _: &gpui::MouseUpEvent, _, _| {
                if let Some(picked) = get(this) {
                    picked.drag = None;
                }
            }),
        )
        .on_scroll_wheel(cx.listener(move |this, event: &gpui::ScrollWheelEvent, _, cx| {
            if let Some(picked) = get(this) {
                // Wheel up zooms in.
                let up = f32::from(event.delta.pixel_delta(px(20.0)).y);
                picked.crop.zoom_by((up * 0.005).exp());
                cx.notify();
            }
            cx.stop_propagation();
        }))
        .e2e("div", "crop-frame")
        .attr("data-zoom", format!("{:.2}", crop.zoom()))
        .attr("data-crop", format!("{:?}", crop.source_rect()));

    let fraction = crop.zoom_fraction();
    let bar = picked.bar.clone();
    let set_at = {
        let bar = bar.clone();
        move |x: gpui::Pixels| -> Option<f32> {
            let bounds = bar.get();
            let width = f32::from(bounds.size.width);
            (width > 0.0).then(|| (f32::from(x - bounds.origin.x) / width).clamp(0.0, 1.0))
        }
    };
    let move_set = set_at.clone();
    let slider = div()
        .id("crop-zoom")
        .relative()
        .w(px(frame))
        .h(px(14.0))
        .flex()
        .items_center()
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                if let (Some(picked), Some(at)) = (get(this), set_at(event.position.x)) {
                    picked.crop.set_zoom_fraction(at);
                    cx.notify();
                }
            }),
        )
        .on_mouse_move(
            cx.listener(move |this, event: &gpui::MouseMoveEvent, _, cx| {
                if event.pressed_button == Some(MouseButton::Left)
                    && let (Some(picked), Some(at)) = (get(this), move_set(event.position.x))
                {
                    picked.crop.set_zoom_fraction(at);
                    cx.notify();
                }
            }),
        )
        .child(
            canvas(move |bounds, _, _| bar.set(bounds), |_, _, _, _| {})
                .absolute()
                .inset_0(),
        )
        .child(
            div()
                .relative()
                .w_full()
                .h(px(4.0))
                .rounded_full()
                .bg(rgba(theme::bg_selected()))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .h_full()
                        .w(gpui::relative(fraction))
                        .rounded_full()
                        .bg(rgb(theme::accent())),
                )
                .child(crate::menu::slider_handle(fraction, 4.0)),
        )
        .e2e("div", "crop-zoom")
        .attr("data-value", format!("{:.2}", crop.zoom()));

    div()
        .flex()
        .flex_col()
        .items_start()
        .gap(px(10.0))
        .child(muted(
            "Drag to position, scroll or use the slider to zoom.",
            "muted",
        ))
        .child(area)
        .child(slider)
        .e2e("div", "avatar-crop")
        .into_any_element()
}
