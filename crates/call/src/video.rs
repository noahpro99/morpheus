//! Video in calls. Frames, this device's and everyone else's, end up here
//! as small JPEGs under a key ([`LOCAL_CAMERA`], or `<identity>/<track>` for
//! a remote track); the interface shows the latest of each
//! ([`latest_frame`]) and asks again when it has drawn it. A web view draws
//! a JPEG with no help, on every platform.
//!
//! In [`AudioMode::Tone`](crate::AudioMode::Tone) the camera is a test
//! pattern ([`TEST_PATTERN`]): red on the left, blue on the right, so a test
//! can tell the far side really sees it.

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

/// The test pattern's size.
pub const TEST_PATTERN: (u32, u32) = (320, 240);

/// Frames per second shown (and sent, for the test pattern).
pub(crate) const FPS: u64 = 15;

/// Remote video is shown no larger than this across.
const MAX_WIDTH: u32 = 640;

/// One picture, ready to show.
#[derive(Debug, Clone)]
pub struct Frame {
    /// Goes up with each new picture under the same key.
    pub seq: u64,
    pub jpeg: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
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
    let (mut width, mut height) = (buffer.width(), buffer.height());
    if width == 0 || height == 0 {
        return;
    }
    let mut i420 = buffer.to_i420();
    if width > MAX_WIDTH {
        height = (height * MAX_WIDTH / width).max(2) & !1;
        width = MAX_WIDTH;
        i420 = i420.scale(width as i32, height as i32);
    }
    // libyuv's "ABGR" is R, G, B, A in memory.
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    (&i420 as &dyn VideoBuffer).to_argb(
        VideoFormatType::ABGR,
        &mut rgba,
        width * 4,
        width as i32,
        height as i32,
    );
    let mut jpeg = Vec::new();
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
    if let Ok(mut frames) = FRAMES.lock() {
        let frames = frames.get_or_insert_with(HashMap::new);
        let seq = frames.get(key).map_or(1, |f| f.seq + 1);
        frames.insert(
            key.to_owned(),
            Frame {
                seq,
                jpeg: Arc::new(jpeg),
                width,
                height,
            },
        );
    }
}

/// Show a remote video track under `key` until it ends.
pub(crate) async fn show_remote(track: RemoteVideoTrack, key: String) {
    let mut stream = NativeVideoStream::new(track.rtc_track());
    let every = Duration::from_millis(1000 / FPS);
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
    forget(&key);
}

/// Send the test pattern to `source` (and show it as this device's camera)
/// until `stop` is set: red and blue halves, with a white bar sweeping
/// across so every frame differs.
pub(crate) async fn play_test_pattern(source: NativeVideoSource, stop: Arc<AtomicBool>) {
    let (width, height) = TEST_PATTERN;
    let mut tick = tokio::time::interval(Duration::from_millis(1000 / FPS));
    let mut n = 0u32;
    while !stop.load(Ordering::Relaxed) {
        tick.tick().await;
        let mut buffer = I420Buffer::new(width, height);
        let (stride_y, stride_u, stride_v) = buffer.strides();
        let bar = (n * 4) % width;
        let (y, u, v) = buffer.data_mut();
        for row in 0..height {
            for col in 0..width {
                // BT.601: red is (81, 90, 240), blue (41, 240, 110), white (235, 128, 128).
                let luma = if col.abs_diff(bar) < 6 {
                    235
                } else if col < width / 2 {
                    81
                } else {
                    41
                };
                y[(row * stride_y + col) as usize] = luma;
            }
        }
        for row in 0..height / 2 {
            for col in 0..width / 2 {
                let (cu, cv) = if (col * 2).abs_diff(bar) < 6 {
                    (128, 128)
                } else if col < width / 4 {
                    (90, 240)
                } else {
                    (240, 110)
                };
                u[(row * stride_u + col) as usize] = cu;
                v[(row * stride_v + col) as usize] = cv;
            }
        }
        let frame = VideoFrame::new(VideoRotation::VideoRotation0, buffer);
        source.capture_frame(&frame);
        keep(LOCAL_CAMERA, &frame.buffer);
        n = n.wrapping_add(1);
    }
    forget(LOCAL_CAMERA);
}
