//! What every desktop camera backend shares (Video4Linux in camera.rs,
//! Media Foundation in camera_win.rs; AVFoundation would slot in the same
//! way): which mode to capture in, and turning the pictures the camera gives
//! into the I420 that WebRTC sends. No platform code here, so it is tested
//! on any machine.
//!
//! Media Foundation's modes are what most of it is for; Linux uses the
//! mode choice and YUYV, macOS (camera_mac.rs) the mode choice and NV12.
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use livekit::webrtc::video_frame::I420Buffer;

/// A mode a camera offers: width, height, and the most frames a second.
pub(crate) type Mode = (u32, u32, f32);

fn is_wide((w, h, _): Mode) -> bool {
    (w as f32 / h as f32 - 16.0 / 9.0).abs() < 0.03
}

/// Which of the modes to capture in. Not the 640x480 every webcam has (a 4:3
/// crop of what Discord shows): the widest picture, 16:9, as large as 1280x720
/// (what is sent: the camera's whole view, nothing cropped by us), one that
/// goes at 15 frames a second or more (some cameras give 720p of YUYV at 10).
/// Failing that a 16:9 above 720p, then the widest shape of at most 1280x720,
/// then whatever is largest.
pub(crate) fn pick_mode(modes: &[Mode]) -> Option<(u32, u32)> {
    let fast: Vec<Mode> = modes.iter().copied().filter(|m| m.2 >= 14.0).collect();
    let pool = if fast.is_empty() { modes } else { &fast };
    let area = |m: &Mode| m.0 * m.1;
    let small = |m: &&Mode| m.0 <= 1280 && m.1 <= 720;
    let best = pool
        .iter()
        .filter(small)
        .filter(|m| is_wide(**m))
        .max_by_key(|m| area(m))
        .or_else(|| {
            pool.iter()
                .filter(|m| is_wide(**m) && m.0 <= 1920)
                .min_by_key(|m| area(m))
        })
        .or_else(|| {
            pool.iter().filter(small).max_by(|a, b| {
                let shape = |m: &Mode| m.0 as f32 / m.1 as f32;
                shape(a).total_cmp(&shape(b)).then(area(a).cmp(&area(b)))
            })
        })
        .or_else(|| pool.iter().max_by_key(|m| area(m)))?;
    Some((best.0, best.1))
}

/// How a camera hands over its pictures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PixelFormat {
    /// 4:2:0, a luma plane then interleaved U/V (what most Windows cameras
    /// and their drivers' pipelines give).
    Nv12,
    /// 4:2:0 in three planes: what WebRTC sends, copied as it is.
    I420,
    /// 4:2:2 packed `Y0 U Y1 V` (Video4Linux's YUYV).
    Yuy2,
    /// Motion JPEG: decoded by the platform (Media Foundation's decoder)
    /// into NV12 before it reaches us.
    Mjpg,
}

impl PixelFormat {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Nv12 => "NV12",
            Self::I420 => "I420",
            Self::Yuy2 => "YUY2",
            Self::Mjpg => "MJPG",
        }
    }

    /// Which comes first when two give the same size and speed: the ones
    /// needing least work.
    fn rank(self) -> u8 {
        match self {
            Self::Nv12 => 0,
            Self::I420 => 1,
            Self::Yuy2 => 2,
            Self::Mjpg => 3,
        }
    }
}

/// One of the formats a device lists (Media Foundation's native media
/// types): its place in the list, its format, size and frames a second.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NativeMode {
    pub index: u32,
    pub format: PixelFormat,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
}

/// What to capture in: the device's mode (`index`), and the format its
/// pictures are asked for in (`output`; a JPEG camera's decoded to NV12).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Choice {
    pub index: u32,
    pub native: PixelFormat,
    pub output: PixelFormat,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
}

/// The format pictures in `native` are read in.
pub(crate) fn output_for(native: PixelFormat) -> PixelFormat {
    match native {
        PixelFormat::Mjpg => PixelFormat::Nv12,
        raw => raw,
    }
}

/// The mode to capture in: the size [`pick_mode`] takes (as fast as any
/// format gives it), then at that size the format that reaches 30 frames a
/// second (or comes closest), the least work first, at 30 rather than 60.
pub(crate) fn choose_mode(modes: &[NativeMode]) -> Option<Choice> {
    let mut sizes: Vec<Mode> = Vec::new();
    for m in modes {
        match sizes.iter_mut().find(|s| (s.0, s.1) == (m.width, m.height)) {
            Some(size) => size.2 = size.2.max(m.fps),
            None => sizes.push((m.width, m.height, m.fps)),
        }
    }
    let (width, height) = pick_mode(&sizes)?;
    let full = |m: &NativeMode| m.fps >= 29.0;
    let best = modes
        .iter()
        .filter(|m| (m.width, m.height) == (width, height))
        .min_by(|a, b| {
            let speed = match (full(a), full(b)) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                // Both reach 30: the slower (30, not 60).
                (true, true) => a.fps.total_cmp(&b.fps),
                (false, false) => b.fps.total_cmp(&a.fps),
            };
            speed
                .then(a.format.rank().cmp(&b.format.rank()))
                .then(a.index.cmp(&b.index))
        })?;
    Some(Choice {
        index: best.index,
        native: best.format,
        output: output_for(best.format),
        width,
        height,
        fps: best.fps,
    })
}

/// The bytes from one row to the next in a picture of `len` bytes (a
/// contiguous buffer: the planes one after the other), or `None` when it is
/// too small for `width` x `height`.
pub(crate) fn frame_stride(len: usize, width: u32, height: u32, format: PixelFormat) -> Option<u32> {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 {
        return None;
    }
    let (rows, least) = match format {
        PixelFormat::Nv12 | PixelFormat::I420 => (h + h.div_ceil(2), w),
        PixelFormat::Yuy2 => (h, w * 2),
        PixelFormat::Mjpg => return None,
    };
    let stride = len / rows;
    (stride >= least).then_some(stride as u32)
}

/// A picture in `format` as I420 (`None` for JPEG, which the platform
/// decodes first, or a buffer too small).
pub(crate) fn to_i420(format: PixelFormat, data: &[u8], width: u32, height: u32) -> Option<I420Buffer> {
    let stride = frame_stride(data.len(), width, height, format)?;
    Some(match format {
        PixelFormat::Nv12 => nv12_to_i420(data, width, height, stride),
        PixelFormat::I420 => i420_copy(data, width, height, stride),
        PixelFormat::Yuy2 => yuyv_to_i420(data, width, height, stride),
        PixelFormat::Mjpg => return None,
    })
}

/// NV12 whose two planes are apart, each with its own stride (CoreVideo's
/// bi-planar pixel buffers), to I420.
pub(crate) fn nv12_planes_to_i420(
    luma: &[u8],
    luma_stride: u32,
    chroma: &[u8],
    chroma_stride: u32,
    width: u32,
    height: u32,
) -> I420Buffer {
    let mut out = I420Buffer::new(width, height);
    let (stride_y, stride_u, stride_v) = out.strides();
    let (y, u, v) = out.data_mut();
    let (width, height) = (width as usize, height as usize);
    let (luma_stride, chroma_stride) = (luma_stride as usize, chroma_stride as usize);
    let (stride_y, stride_u, stride_v) = (stride_y as usize, stride_u as usize, stride_v as usize);
    let (half_w, half_h) = (width / 2, height / 2);
    let fits = |plane: &[u8], stride: usize, rows: usize, row: usize| {
        rows == 0 || (stride >= row && plane.len() >= stride * (rows - 1) + row)
    };
    if !fits(luma, luma_stride, height, width) || !fits(chroma, chroma_stride, half_h, half_w * 2) {
        return out;
    }
    for row in 0..height {
        y[row * stride_y..row * stride_y + width]
            .copy_from_slice(&luma[row * luma_stride..row * luma_stride + width]);
    }
    for row in 0..half_h {
        let src = &chroma[row * chroma_stride..row * chroma_stride + half_w * 2];
        for col in 0..half_w {
            u[row * stride_u + col] = src[col * 2];
            v[row * stride_v + col] = src[col * 2 + 1];
        }
    }
    out
}

/// NV12 (a luma plane, then U and V interleaved at half size) to I420.
pub(crate) fn nv12_to_i420(nv12: &[u8], width: u32, height: u32, stride: u32) -> I420Buffer {
    let split = (stride as usize * height as usize).min(nv12.len());
    let (luma, chroma) = nv12.split_at(split);
    nv12_planes_to_i420(luma, stride, chroma, stride, width, height)
}

/// I420 as it is, its planes one after the other, `stride` apart (half
/// that for U and V).
fn i420_copy(data: &[u8], width: u32, height: u32, stride: u32) -> I420Buffer {
    let mut out = I420Buffer::new(width, height);
    let (stride_y, stride_u, stride_v) = out.strides();
    let (y, u, v) = out.data_mut();
    let (width, height, stride) = (width as usize, height as usize, stride as usize);
    let (stride_y, stride_u, stride_v) = (stride_y as usize, stride_u as usize, stride_v as usize);
    let (half_w, half_h, half_stride) = (width / 2, height / 2, stride / 2);
    if data.len() < stride * height + 2 * half_stride * half_h {
        return out;
    }
    let copy = |dst: &mut [u8], dst_stride: usize, src: &[u8], src_stride: usize, w: usize, rows: usize| {
        for row in 0..rows {
            dst[row * dst_stride..row * dst_stride + w]
                .copy_from_slice(&src[row * src_stride..row * src_stride + w]);
        }
    };
    copy(y, stride_y, data, stride, width, height);
    let u_plane = &data[stride * height..];
    copy(u, stride_u, u_plane, half_stride, half_w, half_h);
    let v_plane = &u_plane[half_stride * half_h..];
    copy(v, stride_v, v_plane, half_stride, half_w, half_h);
    out
}

/// How many whole rows of a `width` x `height` YUYV picture, `stride` bytes
/// apart, are in `len` bytes (all of them in a frame that came whole).
pub(crate) fn yuyv_rows(len: usize, width: u32, height: u32, stride: u32) -> u32 {
    let (row, stride) = (width as usize * 2, stride as usize);
    if row == 0 || stride < row || len < row {
        return 0;
    }
    (((len - row) / stride + 1) as u32).min(height)
}

/// YUYV (4:2:2, packed `Y0 U Y1 V`) to I420 (4:2:0, planar): each chroma
/// sample of the result averages the two rows it covers.
///
/// A frame cut short (a USB webcam losing packets: Video4Linux hands it over
/// with its error flag, a few rows missing at the bottom; some do it on every
/// frame, #117) keeps the rows that came, and the missing ones repeat the
/// last whole row. Nothing at all is black, never the green of an empty
/// buffer (Y, U and V all 0).
pub(crate) fn yuyv_to_i420(yuyv: &[u8], width: u32, height: u32, stride: u32) -> I420Buffer {
    let mut out = I420Buffer::new(width, height);
    let (stride_y, stride_u, stride_v) = out.strides();
    let rows = yuyv_rows(yuyv.len(), width, height, stride) as usize;
    let (y, u, v) = out.data_mut();
    let (width, height, stride) = (width as usize, height as usize, stride as usize);
    let (stride_y, stride_u, stride_v) = (stride_y as usize, stride_u as usize, stride_v as usize);
    if rows == 0 {
        y.fill(16);
        u.fill(128);
        v.fill(128);
        return out;
    }
    let line = |row: usize| {
        let at = row.min(rows - 1) * stride;
        &yuyv[at..at + width * 2]
    };
    for row in 0..height {
        let src = line(row);
        let dst = &mut y[row * stride_y..row * stride_y + width];
        for (x, luma) in dst.iter_mut().enumerate() {
            *luma = src[x * 2];
        }
    }
    for row in 0..height / 2 {
        let (top, bottom) = (line(2 * row), line(2 * row + 1));
        for col in 0..width / 2 {
            let at = col * 4;
            u[row * stride_u + col] =
                ((u16::from(top[at + 1]) + u16::from(bottom[at + 1])) / 2) as u8;
            v[row * stride_v + col] =
                ((u16::from(top[at + 3]) + u16::from(bottom[at + 3])) / 2) as u8;
        }
    }
    out
}

/// The camera as opened, for Diagnostics: its name, what it gives and what
/// is read, size and speed, and through what.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CameraInfo {
    pub name: String,
    pub native: PixelFormat,
    pub output: PixelFormat,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub backend: &'static str,
}

impl std::fmt::Display for CameraInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}, {}x{} at {} fps, {}", self.name, self.width, self.height, self.fps, self.native.name())?;
        match (self.native, self.output) {
            (native, output) if native == output => {}
            (PixelFormat::Mjpg, output) => write!(f, " decoded to {}", output.name())?,
            (_, output) => write!(f, " converted to {}", output.name())?,
        }
        write!(f, " ({})", self.backend)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_widest_16_9_mode_up_to_720p_is_chosen() {
        // A typical webcam: 4:3 and 16:9 modes at 30, 1080p only slower.
        let modes = [
            (640, 480, 30.0),
            (320, 240, 30.0),
            (640, 360, 30.0),
            (1280, 720, 30.0),
            (1920, 1080, 5.0),
            (1600, 1200, 5.0),
        ];
        assert_eq!(pick_mode(&modes), Some((1280, 720)));
        // 720p of YUYV at 10: the best that goes at 15 or more.
        let slow = [(640, 480, 30.0), (640, 360, 30.0), (1280, 720, 10.0)];
        assert_eq!(pick_mode(&slow), Some((640, 360)));
        // Only 4:3 modes: the biggest of them up to 1280x720.
        let square = [(640, 480, 30.0), (320, 240, 30.0), (800, 600, 15.0)];
        assert_eq!(pick_mode(&square), Some((800, 600)));
        // 16:9 above 720p only (a 1080p camera with no 720p): that.
        let big = [(1920, 1080, 30.0), (3840, 2160, 15.0)];
        assert_eq!(pick_mode(&big), Some((1920, 1080)));
        assert_eq!(pick_mode(&[]), None);
    }

    fn mode(index: u32, format: PixelFormat, width: u32, height: u32, fps: f32) -> NativeMode {
        NativeMode { index, format, width, height, fps }
    }

    #[test]
    fn a_usb_webcam_gives_720p_at_30_through_its_jpeg_mode() {
        // USB 2 webcams: raw YUY2 at 720p only at 10, JPEG at 30.
        use PixelFormat::*;
        let modes = [
            mode(0, Yuy2, 640, 480, 30.0),
            mode(1, Yuy2, 1280, 720, 10.0),
            mode(2, Mjpg, 1280, 720, 30.0),
            mode(3, Mjpg, 1920, 1080, 30.0),
            mode(4, Yuy2, 640, 360, 30.0),
        ];
        let choice = choose_mode(&modes).expect("a mode");
        assert_eq!((choice.width, choice.height, choice.fps), (1280, 720, 30.0));
        assert_eq!((choice.index, choice.native, choice.output), (2, Mjpg, Nv12));
    }

    #[test]
    fn at_the_same_size_and_speed_the_least_work_wins_and_30_beats_60() {
        use PixelFormat::*;
        let modes = [
            mode(0, Mjpg, 1280, 720, 30.0),
            mode(1, Yuy2, 1280, 720, 30.0),
            mode(2, Nv12, 1280, 720, 60.0),
            mode(3, Nv12, 1280, 720, 30.0),
            mode(4, Nv12, 960, 540, 30.0),
        ];
        let choice = choose_mode(&modes).expect("a mode");
        assert_eq!((choice.index, choice.native, choice.output), (3, Nv12, Nv12));
        assert_eq!(choice.fps, 30.0);
        // Raw YUY2 over JPEG, and read as it is.
        let choice = choose_mode(&modes[..2]).expect("a mode");
        assert_eq!((choice.index, choice.output), (1, Yuy2));
    }

    #[test]
    fn without_30_the_fastest_is_taken_and_without_modes_none() {
        use PixelFormat::*;
        let modes = [
            mode(0, Nv12, 1280, 720, 15.0),
            mode(1, Nv12, 1280, 720, 24.0),
            mode(2, Nv12, 1280, 720, 7.5),
        ];
        let choice = choose_mode(&modes).expect("a mode");
        assert_eq!((choice.index, choice.fps), (1, 24.0));
        assert_eq!(choose_mode(&[]), None);
        // A 60-only mode is still a mode.
        let only_60 = [mode(7, I420, 960, 540, 60.0)];
        assert_eq!(choose_mode(&only_60).map(|c| (c.index, c.output)), Some((7, I420)));
    }

    #[test]
    fn rows_are_found_from_the_buffers_length() {
        use PixelFormat::*;
        // NV12 and I420: 1.5 bytes a pixel, rows padded to 64 here.
        assert_eq!(frame_stride(1280 * 720 * 3 / 2, 1280, 720, Nv12), Some(1280));
        assert_eq!(frame_stride(1344 * 540 * 3 / 2, 960, 540, Nv12), Some(1344));
        assert_eq!(frame_stride(960 * 540 * 3 / 2, 960, 540, I420), Some(960));
        // YUY2: two bytes a pixel.
        assert_eq!(frame_stride(1280 * 2 * 720, 1280, 720, Yuy2), Some(2560));
        // Too short, or JPEG (decoded elsewhere): none.
        assert_eq!(frame_stride(1000, 1280, 720, Nv12), None);
        assert_eq!(frame_stride(1280 * 720, 1280, 720, Yuy2), None);
        assert_eq!(frame_stride(1 << 20, 1280, 720, Mjpg), None);
    }

    #[test]
    fn nv12_becomes_i420_with_its_chroma_split() {
        // 4x2 with a stride of 6: lumas 10..17, then one chroma row of two
        // U/V pairs (U 100, V 200; U 120, V 220); padding is 0xEE.
        let nv12 = [
            10, 11, 12, 13, 0xEE, 0xEE, //
            14, 15, 16, 17, 0xEE, 0xEE, //
            100, 200, 120, 220, 0xEE, 0xEE,
        ];
        let i420 = nv12_to_i420(&nv12, 4, 2, 6);
        let (sy, _, _) = i420.strides();
        let (y, u, v) = i420.data();
        assert_eq!(&y[..4], &[10, 11, 12, 13]);
        assert_eq!(&y[sy as usize..sy as usize + 4], &[14, 15, 16, 17]);
        assert_eq!(&u[..2], &[100, 120]);
        assert_eq!(&v[..2], &[200, 220]);
        // Through the dispatcher, the stride found from the length.
        let packed = [10, 11, 12, 13, 14, 15, 16, 17, 100, 200, 120, 220];
        let i420 = to_i420(PixelFormat::Nv12, &packed, 4, 2).expect("a picture");
        assert_eq!(&i420.data().1[..2], &[100, 120]);
        assert!(to_i420(PixelFormat::Nv12, &packed[..5], 4, 2).is_none());
        assert!(to_i420(PixelFormat::Mjpg, &packed, 4, 2).is_none());
    }


    #[test]
    fn nv12_planes_apart_with_their_own_strides_become_i420() {
        // 4x2: luma rows 5 apart, the chroma row 8 long (0xEE is padding).
        let luma = [10, 11, 12, 13, 0xEE, 14, 15, 16, 17, 0xEE];
        let chroma = [100, 200, 120, 220, 0xEE, 0xEE, 0xEE, 0xEE];
        let i420 = nv12_planes_to_i420(&luma, 5, &chroma, 8, 4, 2);
        let (sy, _, _) = i420.strides();
        let (y, u, v) = i420.data();
        assert_eq!(&y[..4], &[10, 11, 12, 13]);
        assert_eq!(&y[sy as usize..sy as usize + 4], &[14, 15, 16, 17]);
        assert_eq!((&u[..2], &v[..2]), (&[100, 120][..], &[200, 220][..]));
        // Planes too short for the size: a blank picture, no panic.
        let blank = nv12_planes_to_i420(&luma[..6], 5, &chroma, 8, 4, 2);
        assert_eq!(blank.data().0[sy as usize], 0);
    }
    #[test]
    fn i420_is_copied_plane_by_plane() {
        // 4x2: Y 0..8, then U (2x1) 50 51, then V 60 61.
        let i420 = [0, 1, 2, 3, 4, 5, 6, 7, 50, 51, 60, 61];
        let out = to_i420(PixelFormat::I420, &i420, 4, 2).expect("a picture");
        let (sy, _, _) = out.strides();
        let (y, u, v) = out.data();
        assert_eq!(&y[sy as usize..sy as usize + 4], &[4, 5, 6, 7]);
        assert_eq!((&u[..2], &v[..2]), (&[50, 51][..], &[60, 61][..]));
    }

    #[test]
    fn yuyv_becomes_i420_with_chroma_averaged_over_two_rows() {
        // 4×2: lumas 10..17, U 100/120 and V 200/220 on the top row's two
        // pairs, U 110/130 and V 210/230 below.
        let yuyv = [
            10, 100, 11, 200, 12, 120, 13, 220, //
            14, 110, 15, 210, 16, 130, 17, 230,
        ];
        let i420 = yuyv_to_i420(&yuyv, 4, 2, 8);
        let (sy, _, _) = i420.strides();
        let (y, u, v) = i420.data();
        assert_eq!(&y[..4], &[10, 11, 12, 13]);
        assert_eq!(&y[sy as usize..sy as usize + 4], &[14, 15, 16, 17]);
        assert_eq!(&u[..2], &[105, 125]);
        assert_eq!(&v[..2], &[205, 225]);
        // Through the dispatcher too.
        let i420 = to_i420(PixelFormat::Yuy2, &yuyv, 4, 2).expect("a picture");
        assert_eq!(&i420.data().1[..2], &[105, 125]);
    }

    #[test]
    fn a_yuyv_frame_cut_short_keeps_its_picture_instead_of_going_green() {
        // #117: a USB webcam whose every frame comes a few rows short (lost
        // packets; the kernel flags it but hands it over). 4x4, each row's
        // luma 10*(row+1), U 100 + row, V 200 + row; the last row is
        // missing, and the one before it only half there.
        let (width, height, stride) = (4u32, 4u32, 8usize);
        let mut yuyv = Vec::new();
        for row in 0..3u8 {
            let l = 10 * (row + 1);
            yuyv.extend_from_slice(&[l, 100 + row, l, 200 + row, l, 100 + row, l, 200 + row]);
        }
        yuyv.truncate(2 * stride + 4);
        let i420 = yuyv_to_i420(&yuyv, width, height, stride as u32);
        let (sy, su, _) = i420.strides();
        let (y, u, v) = i420.data();
        let luma = |row: usize| &y[row * sy as usize..row * sy as usize + 4];
        // The rows that came are there as sent.
        assert_eq!(luma(0), &[10, 10, 10, 10]);
        assert_eq!(luma(1), &[20, 20, 20, 20]);
        // The missing ones repeat the last whole row: no green, no black.
        assert_eq!(luma(2), &[20, 20, 20, 20]);
        assert_eq!(luma(3), &[20, 20, 20, 20]);
        assert_eq!((u[0], v[0]), (100, 200));
        let second = su as usize;
        assert!(u[second] > 0 && v[second] > 0, "chroma of the missing rows is not zero");
        assert_eq!(yuyv_rows(yuyv.len(), width, height, stride as u32), 2);
    }

    #[test]
    fn diagnostics_say_what_the_camera_gives_and_what_is_read() {
        let info = CameraInfo {
            name: "Integrated Webcam".into(),
            native: PixelFormat::Mjpg,
            output: PixelFormat::Nv12,
            width: 1280,
            height: 720,
            fps: 30.0,
            backend: "Media Foundation",
        };
        assert_eq!(
            info.to_string(),
            "Integrated Webcam, 1280x720 at 30 fps, MJPG decoded to NV12 (Media Foundation)"
        );
        let raw = CameraInfo { native: PixelFormat::Yuy2, output: PixelFormat::Yuy2, fps: 7.5, backend: "Video4Linux", ..info };
        assert_eq!(raw.to_string(), "Integrated Webcam, 1280x720 at 7.5 fps, YUY2 (Video4Linux)");
    }
}
