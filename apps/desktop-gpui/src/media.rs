//! Pictures: an avatar is the thumbnail the core fetches for it, or the first
//! letter of its name while there is none (`crates/ui/src/media.rs`).

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use gpui::{Div, FontWeight, Image, ImageFormat, div, img, prelude::*, px, rgb};
use morpheus_core::MediaRef;

use crate::{
    e2e::{E2eExt as _, Tagged},
    theme,
};

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

    /// Marks `media` as being fetched; false when it already was or is here.
    pub fn begin(&mut self, media: &MediaRef, size: u32) -> bool {
        let key = (media.key().to_owned(), size);
        !self.images.contains_key(&key) && self.pending.insert(key)
    }

    pub fn finish(&mut self, media: &MediaRef, size: u32, image: Option<Arc<Image>>) {
        let key = (media.key().to_owned(), size);
        self.pending.remove(&key);
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
        Some(image) => base
            .child(img(image).size_full().rounded(px(radius.unwrap_or(size / 2.0))))
            .e2e("img", "avatar"),
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
