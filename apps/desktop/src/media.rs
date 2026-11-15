//! Pictures: an avatar is the thumbnail the core fetches for it, or the first
//! letter of its name while there is none (`crates/ui/src/media.rs`).

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use gpui::{Div, FontWeight, Image, ImageFormat, ObjectFit, div, img, prelude::*, px, rgb};

use crate::view_model::lru::{Budget, ByteLru, Lru};
use morpheus_core::MediaRef;

use crate::{
    e2e::{E2eExt as _, Tagged},
    theme,
};

const MB: usize = 1024 * 1024;

/// A cache shared by every view.
type Shared<T> = std::sync::LazyLock<std::sync::Mutex<T>>;

/// What the shared copies of pictures may hold (their encoded bytes): the
/// biggest copy of each, and the copies cut to a shape. Small thumbnails
/// mostly, so the item counts are what usually binds.
const SHARED_BUDGET: Budget = Budget {
    max_items: 512,
    max_weight: 8 * MB,
};
const CUT_BUDGET: Budget = Budget {
    max_items: 1024,
    max_weight: 16 * MB,
};
/// Call card colours remembered (a few bytes each).
const CARD_COLOR_BUDGET: Budget = Budget {
    max_items: 2048,
    max_weight: usize::MAX,
};

/// What the pictures drawn may cost decoded (GPUI keeps a decoded copy of
/// every picture drawn, and the window's atlas another, for as long as the
/// app runs unless told otherwise; see [`drawn`]). Pictures on screen are
/// kept even when one screen needs more.
pub const DRAWN_BUDGET: usize = 64 * MB;

/// Frames a drawn picture is kept for regardless of the budget: the one
/// being drawn and the one before (what is on screen).
const KEEP_FRAMES: u64 = 2;

/// The biggest copy fetched of each picture, shared by every list and card:
/// it stands in while the size wanted loads, so a card opens with the member
/// row's picture instead of a letter.
static ANY_SIZE: Shared<Lru<String, (u32, Arc<Image>)>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(Lru::new(SHARED_BUDGET)));

/// Thumbnails fetched so far, by media and size. A failed fetch is not
/// tried again on every frame, but it is after [`RETRY_AFTER`] (a server
/// that was down, a picture put back).
#[derive(Default)]
pub struct Avatars {
    images: HashMap<(String, u32), Arc<Image>>,
    pending: HashSet<(String, u32)>,
    failed: HashMap<(String, u32), std::time::Instant>,
}

/// How long a picture that could not be had is left before it is asked for again.
const RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(60);

impl Avatars {
    pub fn get(&self, media: &MediaRef, size: u32) -> Option<Arc<Image>> {
        self.images.get(&(media.key().to_owned(), size)).cloned()
    }

    /// The copy of the size wanted, else the biggest one fetched anywhere
    /// (a stand-in while the size loads), else nothing.
    pub fn get_or_stand_in(&self, media: &MediaRef, size: u32) -> Option<Arc<Image>> {
        self.get(media, size).or_else(|| {
            ANY_SIZE
                .lock()
                .ok()?
                .get(media.key())
                .map(|(_, image)| image.clone())
        })
    }

    /// The media these pictures are of (to drop them from the shared copies).
    pub fn keys(&self) -> Vec<String> {
        self.images.keys().map(|(key, _)| key.clone()).collect()
    }

    /// Drops this view's pictures from the shared copies (not from itself).
    pub fn forget_shared(&self) {
        let ids: Vec<u64> = self.images.values().map(|image| image.id()).collect();
        forget(&self.keys(), &ids);
    }

    /// How many pictures are held (a weight for what keeps them).
    pub fn len(&self) -> usize {
        self.images.len()
    }

    /// Marks `media` as being fetched; false when it is here, on its way, or
    /// failed a moment ago.
    pub fn begin(&mut self, media: &MediaRef, size: u32) -> bool {
        let key = (media.key().to_owned(), size);
        if self.images.contains_key(&key)
            || self
                .failed
                .get(&key)
                .is_some_and(|at| at.elapsed() < RETRY_AFTER)
        {
            return false;
        }
        self.failed.remove(&key);
        self.pending.insert(key)
    }

    pub fn finish(&mut self, media: &MediaRef, size: u32, image: Option<Arc<Image>>) {
        let key = (media.key().to_owned(), size);
        self.pending.remove(&key);
        let Some(image) = image else {
            self.failed.insert(key, std::time::Instant::now());
            return;
        };
        if let Ok(mut any) = ANY_SIZE.lock()
            && any.get(media.key()).is_none_or(|(had, _)| *had < size)
        {
            let bytes = image.bytes().len();
            drop(any.insert(media.key().to_owned(), (size, image.clone()), bytes));
        }
        self.images.insert(key, image);
    }
}

/// The fetch of a picture: a person's (`person`, with the room it shows in)
/// goes through the core's one lookup for people's pictures, which falls
/// back to their other picture when this one is gone; anything else is a
/// plain thumbnail.
pub fn fetch_picture(
    session: &morpheus_core::Session,
    person: Option<(
        morpheus_core::OwnedUserId,
        Option<morpheus_core::OwnedRoomId>,
    )>,
    media: &MediaRef,
    width: u32,
    height: u32,
) -> morpheus_core::CoreFuture<Option<morpheus_core::Media>> {
    match person {
        Some((user, room)) => session.user_avatar(user, room, media.clone(), width, height),
        None => session.thumbnail(media.clone(), width, height),
    }
}

/// Drops every shared in-memory copy of the pictures of `keys` (the biggest
/// copy of each, and the cut copies made from them), as when the room they
/// were in locks; their decoded copies leave GPUI at the next frame.
pub fn forget(keys: &[String], also_images: &[u64]) {
    let mut ids: Vec<u64> = also_images.to_vec();
    if let Ok(mut any) = ANY_SIZE.lock() {
        for key in keys {
            if let Some((_, image)) = any.take(key) {
                ids.push(image.id());
            }
        }
    }
    let mut gone = ids.clone();
    if let Ok(mut cut) = SQUARED.lock() {
        gone.extend(
            cut.retain(|(id, _)| !ids.contains(id))
                .iter()
                .map(|image| image.id()),
        );
    }
    let _ = DRAWN.try_with(|drawn| {
        let mut drawn = drawn.borrow_mut();
        for id in gone {
            drawn.lru.remove(&id);
            if let Some(image) = drawn.images.remove(&id) {
                drawn.forgotten.push(image);
            }
        }
    });
}

/// How many shared pictures are held in memory (for tests to watch).
pub fn held() -> usize {
    ANY_SIZE.lock().map_or(0, |any| any.len()) + SQUARED.lock().map_or(0, |cut| cut.len())
}

/// The pictures drawn lately, by id, within [`DRAWN_BUDGET`] of decoded
/// bytes, and those to take out of GPUI at the next frame.
struct Drawn {
    lru: ByteLru<u64>,
    images: HashMap<u64, Arc<Image>>,
    forgotten: Vec<Arc<Image>>,
}

thread_local! {
    static DRAWN: std::cell::RefCell<Drawn> = std::cell::RefCell::new(Drawn {
        lru: ByteLru::new(DRAWN_BUDGET),
        images: HashMap::new(),
        forgotten: Vec::new(),
    });
}

/// `image`, noted as drawn now: every picture handed to `img` goes through
/// this, so that what GPUI decodes for it can be let go of (by [`sweep`])
/// once it has not been drawn for a while and the budget is spent. Drawn
/// again after that, it is decoded again.
pub fn drawn(image: Arc<Image>) -> Arc<Image> {
    let _ = DRAWN.try_with(|drawn| {
        let mut drawn = drawn.borrow_mut();
        if drawn.lru.touch(image.id(), || decoded_bytes(&image)) {
            drawn.images.insert(image.id(), image.clone());
        }
    });
    image
}

/// What a picture costs decoded (BGRA), from its header.
fn decoded_bytes(image: &Image) -> usize {
    dimensions(image.bytes()).map_or(0, |(w, h)| w as usize * h as usize * 4)
}

fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

/// Once a frame, before the views draw (the shell's render): the pictures
/// over [`DRAWN_BUDGET`] that were not drawn in the last frames, and those
/// forgotten, leave GPUI's cache and the atlas.
pub fn sweep(window: &mut gpui::Window, cx: &mut gpui::App) {
    let gone = DRAWN
        .try_with(|drawn| {
            let mut drawn = drawn.borrow_mut();
            drawn.lru.next_round();
            let mut gone: Vec<Arc<Image>> = drawn
                .lru
                .evict(KEEP_FRAMES)
                .into_iter()
                .filter_map(|id| drawn.images.remove(&id))
                .collect();
            gone.append(&mut drawn.forgotten);
            gone
        })
        .unwrap_or_default();
    for image in gone {
        image.evict(Some(&mut *window), cx);
    }
}

/// What the shared picture caches hold (for the memory report).
pub struct Stats {
    pub any_size: crate::memory::Held,
    pub squared: crate::memory::Held,
    pub drawn: crate::memory::Held,
    pub card_colors: usize,
}

pub fn stats() -> Stats {
    let held = |images: &mut dyn Iterator<Item = &Arc<Image>>| {
        images.fold(crate::memory::Held::default(), |mut held, image| {
            held.entries += 1;
            held.bytes += image.bytes().len();
            held
        })
    };
    Stats {
        any_size: ANY_SIZE
            .lock()
            .map(|any| held(&mut any.values().map(|(_, image)| image)))
            .unwrap_or_default(),
        squared: SQUARED
            .lock()
            .map(|cut| held(&mut cut.values()))
            .unwrap_or_default(),
        drawn: DRAWN
            .try_with(|drawn| {
                let drawn = drawn.borrow();
                crate::memory::Held {
                    entries: drawn.lru.len(),
                    bytes: drawn.lru.bytes(),
                }
            })
            .unwrap_or_default(),
        card_colors: CARD_COLORS.lock().map_or(0, |c| c.len()),
    }
}

fn format_of(mime: &str) -> Option<ImageFormat> {
    Some(match mime {
        "image/png" => ImageFormat::Png,
        "image/jpeg" => ImageFormat::Jpeg,
        "image/gif" => ImageFormat::Gif,
        "image/webp" => ImageFormat::Webp,
        _ => return None,
    })
}

/// The picture for fetched bytes, if their format is one GPUI draws, as it
/// came (the lightbox's full picture).
pub fn decode(media: &morpheus_core::Media) -> Option<Arc<Image>> {
    let format = format_of(media.mime())?;
    Some(Arc::new(Image::from_bytes(format, media.bytes.clone())))
}

/// [`decode`] for a picture drawn in a box of at most `w` x `h` device
/// pixels: one much bigger (a server that sent the original, not a
/// thumbnail: a phone photo is 12 megapixels, 48 MB decoded) is shrunk to
/// just cover the box. Animated pictures (GIF, WebP) are kept as they are.
/// Slow for a big picture: call it off the UI thread.
pub fn decode_fit(media: &morpheus_core::Media, w: u32, h: u32) -> Option<Arc<Image>> {
    let format = format_of(media.mime())?;
    Some(Arc::new(shrunk(format, &media.bytes, w, h).unwrap_or_else(
        || Image::from_bytes(format, media.bytes.clone()),
    )))
}

/// A fetched picture, decoded off the UI thread to fit `w` x `h` (see
/// [`decode_fit`]); `avatar` also works out its card colour.
pub async fn fetched(
    cx: &gpui::AsyncApp,
    fetching: morpheus_core::CoreFuture<Option<morpheus_core::Media>>,
    (w, h): (u32, u32),
    avatar: bool,
) -> Option<Arc<Image>> {
    let bytes = fetching.await.ok().flatten()?;
    cx.background_executor()
        .spawn(async move {
            if avatar {
                decode_avatar(&bytes, w, h)
            } else {
                decode_fit(&bytes, w, h)
            }
        })
        .await
}

/// `bytes` scaled down to just cover `w` x `h`, when that saves much.
fn shrunk(format: ImageFormat, bytes: &[u8], w: u32, h: u32) -> Option<Image> {
    let (codec, jpeg) = match format {
        ImageFormat::Png => (image::ImageFormat::Png, false),
        ImageFormat::Jpeg => (image::ImageFormat::Jpeg, true),
        _ => return None,
    };
    let (iw, ih) = dimensions(bytes)?;
    let scale = (w.max(1) as f32 / iw.max(1) as f32).max(h.max(1) as f32 / ih.max(1) as f32);
    if scale >= 0.8 {
        return None;
    }
    let (nw, nh) = (
        ((iw as f32 * scale).round() as u32).max(1),
        ((ih as f32 * scale).round() as u32).max(1),
    );
    let small = image::load_from_memory_with_format(bytes, codec)
        .ok()?
        .resize_exact(nw, nh, image::imageops::FilterType::Triangle);
    let mut out = std::io::Cursor::new(Vec::new());
    if jpeg {
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
            .encode_image(&small.to_rgb8())
            .ok()?;
        Some(Image::from_bytes(ImageFormat::Jpeg, out.into_inner()))
    } else {
        small.write_to(&mut out, image::ImageFormat::Png).ok()?;
        Some(Image::from_bytes(ImageFormat::Png, out.into_inner()))
    }
}

/// The card colour of each avatar picture, by the picture's id, worked out
/// once when the avatar is decoded (`decode_avatar`).
static CARD_COLORS: std::sync::LazyLock<std::sync::Mutex<Lru<u64, u32>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(Lru::new(CARD_COLOR_BUDGET)));

/// [`decode_fit`] for an avatar: also keeps the colour a call card takes
/// from it (see [`card_color_of`]).
pub fn decode_avatar(media: &morpheus_core::Media, w: u32, h: u32) -> Option<Arc<Image>> {
    let image = decode_fit(media, w, h)?;
    let color = image::load_from_memory(image.bytes())
        .ok()
        .and_then(|decoded| {
            crate::view_model::avatar_color::average_rgba(decoded.to_rgba8().as_raw())
        })
        .map(crate::view_model::avatar_color::card_color);
    if let (Some(color), Ok(mut colors)) = (color, CARD_COLORS.lock()) {
        drop(colors.insert(image.id(), color, 1));
    }
    Some(image)
}

/// The call card colour (RRGGBB) of a picture made by `decode_avatar`.
pub fn card_color_of(image: &Image) -> Option<u32> {
    CARD_COLORS.lock().ok()?.get(&image.id()).copied()
}

/// Copies of pictures cut to a shape, by the original's id and the shape.
static SQUARED: Shared<Lru<(u64, u32), Arc<Image>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(Lru::new(CUT_BUDGET)));

/// `image` cut to its middle square, for an avatar to fill its circle or
/// rounded square. gpui's `ObjectFit::Cover` rounds the enlarged picture's
/// corners, not the frame's, so a wide picture showed as a square: the crop
/// happens here and the square is drawn edge to edge. A square picture (an
/// animated one too) is drawn as it is; one that cannot be read, too.
pub fn squared(image: &Arc<Image>) -> Arc<Image> {
    covering(image, 1.0, 1.0)
}

/// `image` cut to the middle part with the proportions `w`:`h`, to be drawn
/// edge to edge in a rounded box of that shape (`ObjectFit::Fill`; see
/// `squared` for why not `Cover`). One already of that shape, or that cannot
/// be read, is returned as it is. Noted as [`drawn`].
pub fn covering(image: &Arc<Image>, w: f32, h: f32) -> Arc<Image> {
    let key = (image.id(), ((w / h) * 1000.0).round() as u32);
    if let Some(done) = SQUARED
        .lock()
        .ok()
        .and_then(|cache| cache.get(&key).cloned())
    {
        return drawn(done);
    }
    let cut = (|| {
        let decoded = image::load_from_memory(image.bytes()).ok()?;
        let (iw, ih) = (decoded.width() as f32, decoded.height() as f32);
        let (tw, th) = if iw * h > ih * w {
            ((ih * w / h).round().max(1.0), ih)
        } else {
            (iw, (iw * h / w).round().max(1.0))
        };
        if (tw - iw).abs() < 1.0 && (th - ih).abs() < 1.0 {
            return None;
        }
        let (tw, th) = (tw as u32, th as u32);
        let cropped = decoded.crop_imm(
            (decoded.width() - tw) / 2,
            (decoded.height() - th) / 2,
            tw,
            th,
        );
        let mut png = std::io::Cursor::new(Vec::new());
        cropped.write_to(&mut png, image::ImageFormat::Png).ok()?;
        Some(Arc::new(Image::from_bytes(
            ImageFormat::Png,
            png.into_inner(),
        )))
    })()
    .unwrap_or_else(|| image.clone());
    if let Ok(mut cache) = SQUARED.lock() {
        // The picture itself (already of that shape) costs nothing more.
        let bytes = if Arc::ptr_eq(&cut, image) {
            0
        } else {
            cut.bytes().len()
        };
        drop(cache.insert(key, cut.clone(), bytes));
    }
    drawn(cut)
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
    fn forgetting_drops_the_shared_copies_of_the_pictures() {
        let (mine, other) = (
            "mxc://test/forget-mine".to_owned(),
            "mxc://test/forget-other".to_owned(),
        );
        let picture = |bytes: &[u8]| Arc::new(Image::from_bytes(ImageFormat::Png, bytes.to_vec()));
        let (a, b) = (picture(b"one"), picture(b"two"));
        {
            let mut any = ANY_SIZE.lock().expect("lock");
            any.insert(mine.clone(), (80, a.clone()), 3);
            any.insert(other.clone(), (80, b.clone()), 3);
        }
        let (cut_a, cut_b) = (squared(&a), squared(&b));
        drop((cut_a, cut_b));
        forget(&[mine.clone()], &[]);
        let any = ANY_SIZE.lock().expect("lock");
        assert!(!any.contains(&mine), "its picture is still held");
        assert!(any.contains(&other), "another room's picture was dropped");
        let cut = SQUARED.lock().expect("lock");
        assert!(
            !cut.keys().any(|(id, _)| *id == a.id()),
            "its cut copy is still held"
        );
        assert!(
            cut.keys().any(|(id, _)| *id == b.id()),
            "another picture's cut copy was dropped"
        );
    }

    fn png(w: u32, h: u32) -> morpheus_core::Media {
        let buffer = image::RgbaImage::from_fn(w, h, |x, y| {
            image::Rgba([(x % 256) as u8, (y % 256) as u8, 90, 255])
        });
        let mut out = std::io::Cursor::new(Vec::new());
        buffer
            .write_to(&mut out, image::ImageFormat::Png)
            .expect("encode");
        morpheus_core::Media {
            bytes: out.into_inner(),
        }
    }

    /// A server that sends the original (a big phone photo) instead of a
    /// thumbnail: the picture kept is shrunk to just cover the box it is
    /// drawn in, keeping its shape.
    #[test]
    fn a_big_picture_is_shrunk_to_the_box_it_is_drawn_in() {
        let image = decode_fit(&png(2000, 1000), 160, 160).expect("a picture");
        assert_eq!(dimensions(image.bytes()), Some((320, 160)));
        assert_eq!(decoded_bytes(&image), 320 * 160 * 4);
        // Near the size already: kept as it came.
        let near = png(180, 170);
        let image = decode_fit(&near, 160, 160).expect("a picture");
        assert_eq!(image.bytes(), near.bytes.as_slice());
        // Smaller: never enlarged.
        let image = decode_fit(&png(40, 40), 160, 160).expect("a picture");
        assert_eq!(dimensions(image.bytes()), Some((40, 40)));
    }

    /// Every picture cut to a shape is counted as drawn, by its decoded size.
    #[test]
    fn cut_pictures_are_counted_as_drawn() {
        let picture = decode_fit(&png(300, 200), 300, 200).expect("a picture");
        let before = stats().drawn;
        let cut = squared(&picture);
        assert_eq!(dimensions(cut.bytes()), Some((200, 200)));
        let after = stats().drawn;
        assert_eq!(after.entries, before.entries + 1);
        assert_eq!(after.bytes, before.bytes + 200 * 200 * 4);
        // Drawn again: the same entry.
        drop(squared(&picture));
        assert_eq!(stats().drawn, after);
    }
    #[test]
    fn letters() {
        assert_eq!(letter("general"), "G");
        assert_eq!(letter("#!? dev"), "D");
        assert_eq!(letter("!!"), "?");
    }
}
