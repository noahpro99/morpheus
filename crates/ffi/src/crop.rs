//! The picture crop of the phone: the desktop's maths (`view_model::crop`) behind an object the screen drives
//! (drag, a zoom slider), and the cut that gives the square PNG to upload.

use std::sync::{Arc, Mutex};

use morpheus_view_model::crop::{self, Crop};

use crate::FfiError;

/// Two numbers: a point, or a size, in screen pixels.
#[derive(Clone, Copy, Debug, uniffi::Record)]
pub struct Pair {
    pub x: f32,
    pub y: f32,
}

/// A picture in a square frame: where it is panned and how far it is zoomed.
#[derive(uniffi::Object)]
pub struct CropFrame {
    bytes: Vec<u8>,
    crop: Mutex<Crop>,
}

/// Start cropping a picture (the file's bytes); `None` when it is not a picture that can be shown and cut.
#[uniffi::export]
pub fn open_crop(bytes: Vec<u8>) -> Option<Arc<CropFrame>> {
    let (width, height) = crop::picture_size(&bytes)?;
    Some(Arc::new(CropFrame { crop: Mutex::new(Crop::new(width, height)), bytes }))
}

impl CropFrame {
    fn with<T>(&self, f: impl FnOnce(&mut Crop) -> T) -> T {
        let mut guard = self.crop.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut guard)
    }
}

#[uniffi::export]
impl CropFrame {
    /// The zoom as a fraction of the slider's run (0 the smallest, 1 the largest).
    pub fn zoom_fraction(&self) -> f32 {
        self.with(|c| c.zoom_fraction())
    }

    pub fn set_zoom_fraction(&self, fraction: f32) {
        self.with(|c| c.set_zoom_fraction(fraction));
    }

    /// Drag the picture by `dx`, `dy` screen pixels in a frame `frame` wide.
    pub fn pan_by(&self, dx: f32, dy: f32, frame: f32) {
        self.with(|c| c.pan_by(dx, dy, frame));
    }

    /// Where the picture's top left corner is in a frame `frame` wide, from the frame's own top left.
    pub fn offset(&self, frame: f32) -> Pair {
        let (x, y) = self.with(|c| c.offset(frame));
        Pair { x, y }
    }

    /// The picture's size on screen in a frame `frame` wide.
    pub fn shown_size(&self, frame: f32) -> Pair {
        let (x, y) = self.with(|c| c.shown_size(frame));
        Pair { x, y }
    }

    /// The file's bytes, for drawing.
    pub fn picture(&self) -> Vec<u8> {
        self.bytes.clone()
    }

    /// The square the frame shows, as a PNG as wide as pictures are uploaded.
    pub fn cut(&self) -> Result<Vec<u8>, FfiError> {
        let rect = self.with(|c| c.source_rect());
        crop::cropped_png(&self.bytes, rect, crop::OUTPUT_SIDE).map_err(|reason| FfiError::Failed { reason })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_panned_zoomed_and_cut() {
        let mut out = std::io::Cursor::new(Vec::new());
        image_png(&mut out);
        let frame = open_crop(out.into_inner()).expect("a picture");
        assert_eq!(frame.zoom_fraction(), 0.0);
        frame.set_zoom_fraction(1.0);
        assert!(frame.shown_size(100.0).x > 100.0, "zoomed in, the picture is wider than the frame");
        frame.pan_by(500.0, 0.0, 100.0);
        assert!(frame.offset(100.0).x <= 0.0);
        let cut = frame.cut().expect("cut");
        assert_eq!(crop::picture_size(&cut), Some((crop::OUTPUT_SIDE, crop::OUTPUT_SIDE)));
        assert!(open_crop(b"nope".to_vec()).is_none());
    }

    fn image_png(out: &mut std::io::Cursor<Vec<u8>>) {
        // The smallest PNG there is to make: a 40 x 20 grey picture.
        let mut bytes = Vec::new();
        let mut encoder = png_bytes(40, 20);
        bytes.append(&mut encoder);
        std::io::Write::write_all(out, &bytes).expect("write");
    }

    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        // Stored (uncompressed) deflate blocks keep this free of an image crate in the ffi tests.
        fn crc(data: &[u8]) -> u32 {
            let mut c = 0xFFFF_FFFFu32;
            for &b in data {
                c ^= u32::from(b);
                for _ in 0..8 {
                    c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
                }
            }
            !c
        }
        fn adler(data: &[u8]) -> u32 {
            let (mut a, mut b) = (1u32, 0u32);
            for &x in data {
                a = (a + u32::from(x)) % 65521;
                b = (b + a) % 65521;
            }
            (b << 16) | a
        }
        fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
            out.extend((data.len() as u32).to_be_bytes());
            let mut body = kind.to_vec();
            body.extend(data);
            out.extend(&body);
            out.extend(crc(&body).to_be_bytes());
        }
        let mut raw = Vec::new();
        for _ in 0..height {
            raw.push(0);
            raw.extend(std::iter::repeat_n(128u8, width as usize));
        }
        let mut z = vec![0x78, 0x01, 0x01];
        z.extend((raw.len() as u16).to_le_bytes());
        z.extend((!(raw.len() as u16)).to_le_bytes());
        z.extend(&raw);
        z.extend(adler(&raw).to_be_bytes());
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = Vec::new();
        header.extend(width.to_be_bytes());
        header.extend(height.to_be_bytes());
        header.extend([8, 0, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &header);
        chunk(&mut out, b"IDAT", &z);
        chunk(&mut out, b"IEND", &[]);
        out
    }
}
