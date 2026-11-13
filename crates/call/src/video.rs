//! Video in calls. Frames, this device's and everyone else's, end up here
//! as small JPEGs under a key ([`LOCAL_CAMERA`], or `<identity>/<track>` for
//! a remote track); the interface shows the latest of each
//! ([`latest_frame`]) and asks again when it has drawn it. A web view draws
//! a JPEG with no help, on every platform.
//!
//! In [`AudioMode::Tone`](crate::imp::AudioMode::Tone) the camera and the screen
//! are test patterns ([`CAMERA_PATTERN`], [`SCREEN_PATTERN`]).

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use livekit::{
    prelude::*,
    webrtc::{
        video_frame::{I420Buffer, VideoBuffer, VideoFormatType, VideoFrame, VideoRotation},
        video_source::native::NativeVideoSource,
        video_stream::native::NativeVideoStream,
    },
};
use tracing::{debug, warn};

/// This device's camera, as sent.
pub const LOCAL_CAMERA: &str = "local/camera";

/// This device's screen, as shared.
pub const LOCAL_SCREEN: &str = "local/screen";

/// Frames per second shown (and sent, for the test pattern).
pub(crate) const FPS: u64 = 15;

/// A web view gets small JPEGs: no wider than this across.
const MAX_WIDTH: u32 = 640;

/// A renderer that draws the raw pixels itself (no JPEGs) shows a camera up to
/// this wide, and a screen up to the next (text needs its pixels).
const CAMERA_WIDTH: u32 = 1280;
const SCREEN_WIDTH: u32 = 2560;

/// Keys of the screens shown (the other side's, whose key says nothing).
static SCREENS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn is_screen(key: &str) -> bool {
    key == LOCAL_SCREEN || SCREENS.lock().is_ok_and(|keys| keys.iter().any(|k| k == key))
}

/// How wide `key`'s pictures are kept.
fn max_width(key: &str) -> u32 {
    if JPEG.load(Ordering::Relaxed) {
        MAX_WIDTH
    } else if is_screen(key) {
        SCREEN_WIDTH
    } else {
        CAMERA_WIDTH
    }
}

/// How often a picture is renewed for the interface: [`FPS`] for a web view,
/// 30 when drawn natively.
pub(crate) fn show_every() -> Duration {
    Duration::from_millis(1000 / if JPEG.load(Ordering::Relaxed) { FPS } else { 30 })
}

/// One picture, ready to show.
#[derive(Debug, Clone)]
pub struct Frame {
    /// Goes up with each new picture under the same key.
    pub seq: u64,
    /// The picture as a JPEG (empty after [`skip_jpeg_frames`]).
    pub jpeg: Arc<Vec<u8>>,
    /// The same picture as raw pixels, 4 bytes each (B, G, R, A in memory;
    /// A is 255), `width * height` of them: what a native renderer draws
    /// without decoding anything.
    pub bgra: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
}

/// Whether pictures are JPEG-encoded (a web view needs them).
static JPEG: AtomicBool = AtomicBool::new(true);

/// A renderer that draws [`Frame::bgra`] itself calls this once, so the
/// frames are not also encoded as JPEGs nobody reads.
pub fn skip_jpeg_frames() {
    JPEG.store(false, Ordering::Relaxed);
}

static FRAMES: Mutex<Option<HashMap<String, Frame>>> = Mutex::new(None);

/// The latest picture under `key`, if any.
pub fn latest_frame(key: &str) -> Option<Frame> {
    FRAMES.lock().ok()?.as_ref()?.get(key).cloned()
}

pub(crate) fn forget(key: &str) {
    if let Ok(mut frames) = FRAMES.lock()
        && let Some(frames) = frames.as_mut()
    {
        frames.remove(key);
    }
}

/// Keep `buffer` as the picture under `key`.
pub(crate) fn keep(key: &str, buffer: &dyn VideoBuffer) {
    keep_turned(key, buffer, 0);
}

/// [`keep`], turned `degrees` clockwise (0, 90, 180, 270) to stand upright:
/// a phone's camera sensor is mounted turned, and its frames say so.
pub(crate) fn keep_turned(key: &str, buffer: &dyn VideoBuffer, degrees: u32) {
    let (mut width, mut height) = (buffer.width(), buffer.height());
    if width == 0 || height == 0 {
        return;
    }
    let mut i420 = buffer.to_i420();
    let cap = max_width(key);
    if width > cap {
        height = (height * cap / width).max(2) & !1;
        width = cap;
        i420 = i420.scale(width as i32, height as i32);
    }
    // libyuv's "ARGB" is B, G, R, A in memory.
    let mut bgra = vec![0u8; (width * height * 4) as usize];
    (&i420 as &dyn VideoBuffer).to_argb(
        VideoFormatType::ARGB,
        &mut bgra,
        width * 4,
        width as i32,
        height as i32,
    );
    let (bgra, width, height) = turn(bgra, width, height, degrees);
    let mut jpeg = Vec::new();
    if JPEG.load(Ordering::Relaxed) {
        let mut rgba = bgra.clone();
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        let encoder = jpeg_encoder::Encoder::new(&mut jpeg, 75);
        if let Err(err) = encoder.encode(
            &rgba,
            width as u16,
            height as u16,
            jpeg_encoder::ColorType::Rgba,
        ) {
            warn!(%err, "video frame not encoded");
            return;
        }
    }
    if let Ok(mut frames) = FRAMES.lock() {
        let frames = frames.get_or_insert_with(HashMap::new);
        let seq = frames.get(key).map_or(1, |f| f.seq + 1);
        frames.insert(
            key.to_owned(),
            Frame {
                seq,
                jpeg: Arc::new(jpeg),
                bgra: Arc::new(bgra),
                width,
                height,
            },
        );
    }
}

/// Show a remote video track under `key` until it ends.
pub(crate) async fn show_remote(track: RemoteVideoTrack, key: String, screen: bool) {
    if screen && let Ok(mut keys) = SCREENS.lock() {
        keys.push(key.clone());
    }
    let mut stream = NativeVideoStream::new(track.rtc_track());
    let every = show_every();
    let mut last: Option<Instant> = None;
    while let Some(frame) = stream.next().await {
        // The interface needs no more than FPS; the rest are dropped here.
        if last.is_some_and(|at| at.elapsed() < every) {
            continue;
        }
        last = Some(Instant::now());
        keep(&key, frame.buffer.as_ref());
    }
    debug!(key, "video track ended");
    if let Ok(mut keys) = SCREENS.lock() {
        keys.retain(|k| *k != key);
    }
    forget(&key);
}

/// A test run's stand-in for a camera or a screen: two colours side by side
/// (so a test can tell the far side really sees it), with a white bar
/// sweeping across so every frame differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TestPattern {
    pub width: u32,
    pub height: u32,
    /// Where this device shows it.
    pub key: &'static str,
    /// BT.601 (Y, U, V) of each half.
    left: (u8, u8, u8),
    right: (u8, u8, u8),
}

/// The camera in test runs: red on the left, blue on the right.
pub const CAMERA_PATTERN: TestPattern = TestPattern {
    width: 1280,
    height: 720,
    key: LOCAL_CAMERA,
    left: (81, 90, 240),
    right: (41, 240, 110),
};

/// The shared screen in test runs: green on the left, yellow on the right.
pub const SCREEN_PATTERN: TestPattern = TestPattern {
    width: 2560,
    height: 1440,
    key: LOCAL_SCREEN,
    left: (145, 54, 34),
    right: (210, 16, 146),
};

/// The size a test run's camera or screen has: `var` ("640x480") if set,
/// else the pattern's own.
pub(crate) fn test_size(var: &str, pattern: TestPattern) -> (u32, u32) {
    std::env::var(var)
        .ok()
        .and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some((w.parse::<u32>().ok()? & !1, h.parse::<u32>().ok()? & !1))
        })
        .filter(|(w, h)| *w >= 2 && *h >= 2)
        .unwrap_or((pattern.width, pattern.height))
}

/// Send `pattern`, `size` = (width, height, frames a second), to `source`
/// (and show it under its key) until `stop` is set.
pub(crate) async fn play_test_pattern(
    pattern: TestPattern,
    (width, height, fps): (u32, u32, u32),
    source: NativeVideoSource,
    stop: Arc<AtomicBool>,
) {
    let colour = |col: u32, bar: u32| {
        if col.abs_diff(bar) < 6 {
            (235, 128, 128)
        } else if col < width / 2 {
            pattern.left
        } else {
            pattern.right
        }
    };
    // The picture without its bar, drawn once; every frame copies it and
    // draws the bar (a big screen costs a copy, not a pixel loop).
    let mut base = I420Buffer::new(width, height);
    let (stride_y, stride_u, stride_v) = base.strides();
    {
        let (y, u, v) = base.data_mut();
        for row in 0..height {
            for col in 0..width {
                y[(row * stride_y + col) as usize] = colour(col, u32::MAX).0;
            }
        }
        for row in 0..height / 2 {
            for col in 0..width / 2 {
                let (_, cu, cv) = colour(col * 2, u32::MAX);
                u[(row * stride_u + col) as usize] = cu;
                v[(row * stride_v + col) as usize] = cv;
            }
        }
    }
    let mut tick = tokio::time::interval(Duration::from_millis(1000 / u64::from(fps.max(1))));
    let mut n = 0u32;
    while !stop.load(Ordering::Relaxed) {
        tick.tick().await;
        let mut buffer = I420Buffer::new(width, height);
        let bar = (n * 4) % width;
        {
            let (y, u, v) = buffer.data_mut();
            let (base_y, base_u, base_v) = base.data();
            y.copy_from_slice(base_y);
            u.copy_from_slice(base_u);
            v.copy_from_slice(base_v);
            let from = bar.saturating_sub(5);
            let to = (bar + 6).min(width);
            for row in 0..height {
                let at = (row * stride_y) as usize;
                y[at + from as usize..at + to as usize].fill(235);
            }
            for row in 0..height / 2 {
                let at = (row * stride_u) as usize;
                u[at + (from / 2) as usize..at + (to / 2) as usize].fill(128);
                v[at + (from / 2) as usize..at + (to / 2) as usize].fill(128);
            }
        }
        let frame = VideoFrame::new(VideoRotation::VideoRotation0, buffer);
        source.capture_frame(&frame);
        // The interface is shown fewer pictures than are sent.
        if n % (fps / 15).max(1) == 0 {
            keep(pattern.key, &frame.buffer);
        }
        n = n.wrapping_add(1);
    }
    forget(pattern.key);
}

/// RGBA pixels turned `degrees` clockwise; the new width and height.
fn turn(rgba: Vec<u8>, width: u32, height: u32, degrees: u32) -> (Vec<u8>, u32, u32) {
    let (w, h) = (width as usize, height as usize);
    let (out_w, out_h) = match degrees {
        90 | 270 => (h, w),
        180 => (w, h),
        _ => return (rgba, width, height),
    };
    let mut out = vec![0u8; rgba.len()];
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = match degrees {
                90 => (h - 1 - y, x),
                180 => (w - 1 - x, h - 1 - y),
                _ => (y, w - 1 - x),
            };
            let from = (y * w + x) * 4;
            let to = (ny * out_w + nx) * 4;
            out[to..to + 4].copy_from_slice(&rgba[from..from + 4]);
        }
    }
    (out, out_w as u32, out_h as u32)
}
