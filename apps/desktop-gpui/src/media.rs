//! Pictures: an avatar is the thumbnail the core fetches for it, or the first
//! letter of its name while there is none (`crates/ui/src/media.rs`).

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use gpui::{Div, FontWeight, Image, ImageFormat, ObjectFit, div, img, prelude::*, px, rgb};
use morpheus_core::MediaRef;

use crate::{
    e2e::{E2eExt as _, Tagged},
    theme,
};

/// The biggest copy fetched of each picture, shared by every list and card:
/// it stands in while the size wanted loads, so a card opens with the member
/// row's picture instead of a letter.
static ANY_SIZE: std::sync::LazyLock<std::sync::Mutex<HashMap<String, (u32, Arc<Image>)>>> =
    std::sync::LazyLock::new(Default::default);

/// Thumbnails fetched so far, by media and size. `None` remembers a failed
/// fetch so dead media is not retried on every frame.
#[derive(Default)]
pub struct Avatars {
    images: HashMap<(String, u32), Option<Arc<Image>>>,
    pending: HashSet<(String, u32)>,
}

impl Avatars {
    pub fn get(&self, media: &MediaRef, size: u32) -> Option<Arc<Image>> {
        self.images
            .get(&(media.key().to_owned(), size))
            .cloned()
            .flatten()
    }

    /// The copy of the size wanted, else the biggest one fetched anywhere
    /// (a stand-in while the size loads), else nothing.
    pub fn get_or_stand_in(&self, media: &MediaRef, size: u32) -> Option<Arc<Image>> {
        self.get(media, size)
            .or_else(|| ANY_SIZE.lock().ok()?.get(media.key()).map(|(_, image)| image.clone()))
    }

    /// Marks `media` as being fetched; false when it already was or is here.
    pub fn begin(&mut self, media: &MediaRef, size: u32) -> bool {
        let key = (media.key().to_owned(), size);
        !self.images.contains_key(&key) && self.pending.insert(key)
    }

    pub fn finish(&mut self, media: &MediaRef, size: u32, image: Option<Arc<Image>>) {
        let key = (media.key().to_owned(), size);
        self.pending.remove(&key);
        if let (Some(image), Ok(mut any)) = (&image, ANY_SIZE.lock()) {
            if any.get(media.key()).is_none_or(|(had, _)| *had < size) {
                any.insert(media.key().to_owned(), (size, image.clone()));
            }
        }
        self.images.insert(key, image);
    }
}

/// The picture for fetched bytes, if their format is one GPUI draws.
pub fn decode(media: &morpheus_core::Media) -> Option<Arc<Image>> {
    let format = match media.mime() {
        "image/png" => ImageFormat::Png,
        "image/jpeg" => ImageFormat::Jpeg,
        "image/gif" => ImageFormat::Gif,
        "image/webp" => ImageFormat::Webp,
        _ => return None,
    };
    Some(Arc::new(Image::from_bytes(format, media.bytes.clone())))
}

/// Square copies of non-square pictures, by the original's id.
static SQUARED: std::sync::LazyLock<std::sync::Mutex<HashMap<u64, Arc<Image>>>> =
    std::sync::LazyLock::new(Default::default);

/// `image` cut to its middle square, for an avatar to fill its circle or
/// rounded square. gpui's `ObjectFit::Cover` rounds the enlarged picture's
/// corners, not the frame's, so a wide picture showed as a square: the crop
/// happens here and the square is drawn edge to edge. A square picture (an
/// animated one too) is drawn as it is; one that cannot be read, too.
pub fn squared(image: &Arc<Image>) -> Arc<Image> {
    if let Some(done) = SQUARED.lock().ok().and_then(|cache| cache.get(&image.id()).cloned()) {
        return done;
    }
    let squared = (|| {
        let decoded = image::load_from_memory(image.bytes()).ok()?;
        let (w, h) = (decoded.width(), decoded.height());
        if w == h {
            return None;
        }
        let side = w.min(h);
        let cropped = decoded.crop_imm((w - side) / 2, (h - side) / 2, side, side);
        let mut png = std::io::Cursor::new(Vec::new());
        cropped.write_to(&mut png, image::ImageFormat::Png).ok()?;
        Some(Arc::new(Image::from_bytes(ImageFormat::Png, png.into_inner())))
    })()
    .unwrap_or_else(|| image.clone());
    if let Ok(mut cache) = SQUARED.lock() {
        cache.insert(image.id(), squared.clone());
    }
    squared
}

/// The first letter or digit of `name`, upper-cased (`?` when none).
pub fn letter(name: &str) -> String {
    name.chars()
        .find(|c| c.is_alphanumeric())
        .map_or_else(|| "?".to_owned(), |c| c.to_uppercase().collect())
}

/// A round (or `radius`-cornered) avatar of `size` pixels: the picture, else
/// the letter on the accent colour.
pub fn avatar(
    picture: Option<Arc<Image>>,
    fallback: &str,
    size: f32,
    radius: Option<f32>,
) -> Tagged<Div> {
    let base = div()
        .size(px(size))
        .flex_none()
        .overflow_hidden()
        .flex()
        .items_center()
        .justify_center()
        .map(|d| match radius {
            Some(r) => d.rounded(px(r)),
            None => d.rounded_full(),
        });
    match picture {
        // gpui clips children to a rectangle: the picture is rounded itself.
        Some(image) => {
            // Tests tell pictures apart by this (the image's identity), as by an <img>'s src.
            let src = {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                image.hash(&mut hasher);
                format!("picture:{:x}", hasher.finish())
            };
            base.child(
                img(squared(&image))
                    .size_full()
                    .rounded(px(radius.unwrap_or(size / 2.0)))
                    .object_fit(ObjectFit::Fill),
            )
                .e2e("img", "avatar")
                .attr("data-fit", "cover")
                .attr("src", src)
        }
        None => base
            .bg(rgb(theme::accent_strong()))
            .font_weight(FontWeight::BOLD)
            .text_size(px(size / 2.0))
            .child(letter(fallback))
            .e2e("div", "avatar")
            .text(letter(fallback)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters() {
        assert_eq!(letter("general"), "G");
        assert_eq!(letter("#!? dev"), "D");
        assert_eq!(letter("!!"), "?");
    }
}
