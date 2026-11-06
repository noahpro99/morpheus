//! Blurred placeholders for pictures ([BlurHash](https://blurha.sh), MSC2448:
//! `xyz.amorgan.blurhash` in an image's info): a few dozen characters that
//! describe a picture's colours, shown blurred while it loads. Sent with
//! the pictures this app uploads, and decoded for the ones it shows.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use matrix_sdk::attachment::{AttachmentInfo, BaseImageInfo};
use matrix_sdk::ruma::UInt;

/// Encoding looks at a small copy: a blur needs no more.
const SAMPLE: usize = 64;

/// A data URL (a tiny PNG) of the picture `hash` describes, for showing
/// stretched and blurred behind a picture while it loads.
pub fn placeholder(hash: &str) -> Option<String> {
    const SIZE: u32 = 32;
    let rgba = blurhash::decode(hash, SIZE, SIZE, 1.0).ok()?;
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, SIZE, SIZE);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&rgba).ok()?;
    }
    Some(format!("data:image/png;base64,{}", STANDARD.encode(png)))
}

/// What to send with a picture: its size and BlurHash, when it is one this
/// app can read (PNG or JPEG).
pub(crate) fn image_info(bytes: &[u8], mime: &str) -> Option<AttachmentInfo> {
    let (width, height, rgba) = match mime {
        "image/png" => decode_png(bytes)?,
        "image/jpeg" | "image/jpg" => decode_jpeg(bytes)?,
        _ => return None,
    };
    let (small_w, small_h, small) = shrink(width, height, &rgba);
    let hash = blurhash::encode(4, 3, small_w as u32, small_h as u32, &small).ok()?;
    Some(AttachmentInfo::Image(BaseImageInfo {
        width: UInt::new(width as u64),
        height: UInt::new(height as u64),
        size: UInt::new(bytes.len() as u64),
        blurhash: Some(hash),
        is_animated: None,
    }))
}

/// Width, height and RGBA pixels of a PNG.
fn decode_png(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (frame.width as usize, frame.height as usize);
    let pixels = &buf[..frame.buffer_size()];
    let rgba = match frame.color_type {
        png::ColorType::Rgba => pixels.to_vec(),
        png::ColorType::Rgb => pixels
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::Grayscale => pixels.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => pixels
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Indexed => return None,
    };
    (rgba.len() == w * h * 4).then_some((w, h, rgba))
}

/// Width, height and RGBA pixels of a JPEG.
fn decode_jpeg(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let mut decoder = zune_jpeg::JpegDecoder::new(std::io::Cursor::new(bytes));
    let rgb = decoder.decode().ok()?;
    let (w, h) = decoder.dimensions()?;
    let rgba: Vec<u8> = rgb
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|p| [p[0], p[1], p[2], 255])
        .collect();
    (rgba.len() == w * h * 4).then_some((w, h, rgba))
}

/// A copy at most [`SAMPLE`] pixels wide or tall (nearest pixel).
fn shrink(width: usize, height: usize, rgba: &[u8]) -> (usize, usize, Vec<u8>) {
    let scale = (width.max(height) as f64 / SAMPLE as f64).max(1.0);
    let (w, h) = (
        ((width as f64 / scale) as usize).max(1),
        ((height as f64 / scale) as usize).max(1),
    );
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let sy = ((y as f64 * scale) as usize).min(height - 1);
        for x in 0..w {
            let sx = ((x as f64 * scale) as usize).min(width - 1);
            let at = (sy * width + sx) * 4;
            out.extend_from_slice(&rgba[at..at + 4]);
        }
    }
    (w, h, out)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn a_hash_round_trips_into_a_placeholder() {
        // A 2x2 PNG made here: red, green, blue, white.
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 2, 2);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("header");
            writer
                .write_image_data(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255])
                .expect("pixels");
        }
        let Some(AttachmentInfo::Image(info)) = image_info(&png, "image/png") else {
            panic!("image info for a PNG");
        };
        assert_eq!((info.width, info.height), (UInt::new(2), UInt::new(2)));
        let hash = info.blurhash.expect("a blurhash");
        assert!(
            placeholder(&hash)
                .expect("a placeholder")
                .starts_with("data:image/png;base64,")
        );
        assert!(image_info(b"not a picture", "image/png").is_none());
    }
}
