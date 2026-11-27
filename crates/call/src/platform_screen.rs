//! A screen the app captures itself (on Android, MediaProjection from Kotlin):
//! the app registers how to start and stop it ([`set_platform_screen`]) and
//! hands over each picture ([`screen_frame`]), which goes to the call. The
//! system's permission is asked by the app before the share is turned on.

use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use livekit::webrtc::{
    native::yuv_helper,
    video_frame::{I420Buffer, VideoFrame, VideoRotation},
    video_source::native::NativeVideoSource,
};
use tracing::warn;

use crate::imp::video::{self, LOCAL_SCREEN};

/// Starting and stopping the app's screen capture. `start` returns the size
/// (width, height) of the pictures it will hand over, or `None` when it could
/// not start (no permission given, the system refused).
pub struct PlatformScreen {
    pub start: Box<dyn Fn() -> Option<(u32, u32)> + Send + Sync>,
    pub stop: Box<dyn Fn() + Send + Sync>,
}

static SCREEN: OnceLock<PlatformScreen> = OnceLock::new();
static SINK: Mutex<Option<NativeVideoSource>> = Mutex::new(None);

/// Register the app's screen capture (once, at startup).
pub fn set_platform_screen(screen: PlatformScreen) {
    let _ = SCREEN.set(screen);
}

pub(crate) fn available() -> bool {
    SCREEN.get().is_some()
}

/// The capture's size before it starts (the call makes its video source from it).
pub(crate) fn start() -> Option<(u32, u32)> {
    (SCREEN.get()?.start)()
}

/// Send the capture's pictures to `source` until `stop` is set (or the system ends the share, which sets `stop`).
pub(crate) fn run(source: NativeVideoSource, stop: Arc<AtomicBool>) {
    if let Ok(mut sink) = SINK.lock() {
        *sink = Some(source);
    }
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
        }
        if let Some(screen) = SCREEN.get() {
            (screen.stop)();
        }
        if let Ok(mut sink) = SINK.lock() {
            *sink = None;
        }
        video::forget(LOCAL_SCREEN);
    });
}

/// One screen picture, RGBA_8888 as Android's ImageReader gives it: `stride`
/// bytes a row (more than `width * 4` when rows are padded).
pub fn screen_frame(rgba: &[u8], stride: usize, width: u32, height: u32) {
    let (width, height) = (width & !1, height & !1);
    if width == 0 || height == 0 || rgba.len() < stride * (height as usize - 1) + width as usize * 4 {
        warn!("screen frame shorter than its size");
        return;
    }
    let Ok(sink) = SINK.lock() else {
        return;
    };
    let Some(source) = sink.as_ref() else {
        return;
    };
    let mut buffer = I420Buffer::new(width, height);
    let (stride_y, stride_u, stride_v) = buffer.strides();
    let (y, u, v) = buffer.data_mut();
    // Android's RGBA bytes are libyuv's "ABGR".
    yuv_helper::abgr_to_i420(rgba, stride as u32, y, stride_y, u, stride_u, v, stride_v, width as i32, height as i32);
    let frame = VideoFrame::new(VideoRotation::VideoRotation0, buffer);
    source.capture_frame(&frame);
    video::keep(LOCAL_SCREEN, &frame.buffer);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_with_no_share_going_is_dropped_and_a_short_one_too() {
        // Nothing is registered or running: no panic, nothing kept.
        screen_frame(&[0; 16], 8, 2, 2);
        screen_frame(&[0; 4], 8, 2, 2);
        screen_frame(&[], 0, 0, 0);
        assert!(video::latest_frame(LOCAL_SCREEN).is_none());
    }
}
