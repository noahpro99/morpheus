//! What a paste attaches (#400). A clipboard can offer text, pictures in several formats and
//! files at once, and platforms differ in which they put there: a Finder copy has the file and
//! its name as text, a screenshot only TIFF (macOS) or PNG, a browser's "Copy image" a picture
//! and often its address as text. This picks one thing to do, the same on every platform, and
//! turns a format nobody displays (TIFF, BMP) into PNG.

use std::path::PathBuf;

/// What the clipboard holds, as read by the platform.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Offer {
    pub text: Option<String>,
    /// Pictures: mime type and bytes, in the platform's order.
    pub images: Vec<(String, Vec<u8>)>,
    /// Files copied (in Finder, a file manager).
    pub paths: Vec<PathBuf>,
}

/// A picture to attach.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// What a paste does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Insert the text.
    Text,
    /// Attach this picture.
    Picture(Picture),
    /// Attach these files, as a drop of them does.
    Files(Vec<PathBuf>),
    /// Nothing to paste.
    Nothing,
}

/// Formats that can go out as they are, best first.
const AS_IS: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

/// Formats turned into PNG.
const CONVERTED: &[&str] = &["image/tiff", "image/bmp"];

/// Text that is only the address of a picture or a file, which a browser or file manager puts next
/// to the picture it copied: a picture beside it wins.
fn is_only_an_address(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty()
        && !text.contains(char::is_whitespace)
        && ["http://", "https://", "file://", "data:image/"]
            .iter()
            .any(|scheme| text.starts_with(scheme))
}

/// PNG bytes for a picture in another format (TIFF, BMP); `None` if it cannot be read.
pub fn to_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let decoded = image::load_from_memory(bytes).ok()?;
    let mut out = std::io::Cursor::new(Vec::new());
    decoded.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

fn extension(mime: &str) -> &str {
    match mime {
        "image/jpeg" => "jpg",
        other => other.rsplit('/').next().unwrap_or("png"),
    }
}

/// The picture to attach from the offered ones: a format that goes out as it is, else a
/// converted one.
fn best_picture(images: &[(String, Vec<u8>)]) -> Option<Picture> {
    for mime in AS_IS {
        if let Some((_, bytes)) = images.iter().find(|(m, b)| m == mime && !b.is_empty()) {
            return Some(Picture {
                name: format!("image.{}", extension(mime)),
                mime: (*mime).to_owned(),
                bytes: bytes.clone(),
            });
        }
    }
    for mime in CONVERTED {
        for (_, bytes) in images.iter().filter(|(m, _)| m == mime) {
            if let Some(png) = to_png(bytes) {
                return Some(Picture {
                    name: "image.png".to_owned(),
                    mime: "image/png".to_owned(),
                    bytes: png,
                });
            }
        }
    }
    None
}

/// What pasting this does: files win (their name is also on the clipboard as text), then a
/// picture when there is no text, or only its address, then the text.
pub fn choose(offer: &Offer) -> Choice {
    if !offer.paths.is_empty() {
        return Choice::Files(offer.paths.clone());
    }
    let text = offer.text.as_deref().unwrap_or("");
    let wants_picture = text.trim().is_empty() || is_only_an_address(text);
    if wants_picture && let Some(picture) = best_picture(&offer.images) {
        return Choice::Picture(picture);
    }
    if text.is_empty() {
        Choice::Nothing
    } else {
        Choice::Text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(3, 2)
            .write_to(&mut out, image::ImageFormat::Png)
            .expect("png");
        out.into_inner()
    }

    fn tiff() -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(3, 2)
            .write_to(&mut out, image::ImageFormat::Tiff)
            .expect("tiff");
        out.into_inner()
    }

    fn images(list: &[(&str, Vec<u8>)]) -> Vec<(String, Vec<u8>)> {
        list.iter().map(|(m, b)| ((*m).to_owned(), b.clone())).collect()
    }

    #[test]
    fn a_tiff_only_pasteboard_gives_a_png_attachment() {
        let offer = Offer {
            images: images(&[("image/tiff", tiff())]),
            ..Offer::default()
        };
        let Choice::Picture(picture) = choose(&offer) else {
            panic!("a picture");
        };
        assert_eq!(picture.mime, "image/png");
        assert_eq!(picture.name, "image.png");
        let decoded = image::load_from_memory(&picture.bytes).expect("a readable png");
        assert_eq!((decoded.width(), decoded.height()), (3, 2));
        assert_eq!(picture.bytes[..4], [0x89, b'P', b'N', b'G']);
    }

    #[test]
    fn a_png_goes_out_as_it_is_and_beats_a_tiff_beside_it() {
        let bytes = png();
        let offer = Offer {
            images: images(&[("image/tiff", tiff()), ("image/png", bytes.clone())]),
            ..Offer::default()
        };
        assert_eq!(
            choose(&offer),
            Choice::Picture(Picture {
                name: "image.png".into(),
                mime: "image/png".into(),
                bytes
            })
        );
    }

    #[test]
    fn a_jpeg_keeps_its_type_and_gets_a_jpg_name() {
        let offer = Offer {
            images: images(&[("image/jpeg", vec![0xFF, 0xD8, 0xFF, 1, 2])]),
            ..Offer::default()
        };
        let Choice::Picture(picture) = choose(&offer) else {
            panic!("a picture");
        };
        assert_eq!((picture.mime.as_str(), picture.name.as_str()), ("image/jpeg", "image.jpg"));
    }

    #[test]
    fn a_copied_file_attaches_as_a_file_though_its_name_is_on_the_clipboard_as_text() {
        let offer = Offer {
            text: Some("cat.png".into()),
            images: images(&[("image/png", png())]),
            paths: vec![PathBuf::from("/Users/a/cat.png")],
        };
        assert_eq!(choose(&offer), Choice::Files(vec![PathBuf::from("/Users/a/cat.png")]));
    }

    #[test]
    fn text_wins_over_a_picture_beside_it_unless_it_is_only_an_address() {
        let with = |text: &str| Offer {
            text: Some(text.into()),
            images: images(&[("image/png", png())]),
            paths: vec![],
        };
        assert_eq!(choose(&with("some cells\tof a sheet")), Choice::Text);
        assert!(matches!(choose(&with("https://example.org/cat.png")), Choice::Picture(_)));
        assert!(matches!(choose(&with("  ")), Choice::Picture(_)));
    }

    #[test]
    fn a_broken_tiff_and_no_other_picture_pastes_nothing() {
        let offer = Offer {
            images: images(&[("image/tiff", vec![1, 2, 3])]),
            ..Offer::default()
        };
        assert_eq!(choose(&offer), Choice::Nothing);
        assert_eq!(choose(&Offer::default()), Choice::Nothing);
    }
}
