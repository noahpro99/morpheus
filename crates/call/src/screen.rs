//! Sharing the screen, through libwebrtc's desktop capturer. On Wayland that
//! asks the desktop portal, which lets the person pick a screen or a window
//! (or cancel); elsewhere it takes the whole screen.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use livekit::webrtc::{
    desktop_capturer::{
        CaptureError, DesktopCaptureSourceType, DesktopCapturer, DesktopCapturerOptions,
        DesktopFrame,
    },
    native::yuv_helper,
    video_frame::{I420Buffer, VideoFrame, VideoRotation},
    video_source::native::NativeVideoSource,
};
use tokio::sync::oneshot;
use tracing::{info, warn};

use crate::imp::video::{self, LOCAL_SCREEN};

/// Capture the screen into `source` on a thread of its own until `stop` is
/// set. `started` hears once: the first picture arrived, or capture could
/// not start (no capturer, or the picker was cancelled). Frames go out only
/// while `sending` is set.
pub(crate) fn run(
    source: NativeVideoSource,
    stop: Arc<AtomicBool>,
    sending: Arc<AtomicBool>,
    started: oneshot::Sender<Result<(), String>>,
) {
    let spawned = std::thread::Builder::new()
        .name("morpheus-screen".into())
        .spawn(move || {
            #[cfg(target_os = "linux")]
            let kind = DesktopCaptureSourceType::Generic;
            #[cfg(not(target_os = "linux"))]
            let kind = DesktopCaptureSourceType::Screen;
            let mut options = DesktopCapturerOptions::new(kind);
            options.set_include_cursor(true);
            let Some(mut capturer) = DesktopCapturer::new(options) else {
                let _ = started.send(Err("screen capture is not available here".into()));
                return;
            };
            // Wayland picks through the portal; elsewhere, the first screen.
            let chosen = if cfg!(target_os = "linux") {
                None
            } else {
                capturer.get_source_list().into_iter().next()
            };
            let mut started = Some(started);
            let failed = Arc::new(AtomicBool::new(false));
            let every = Duration::from_millis(1000 / video::FPS);
            let mut shown: Option<Instant> = None;
            let (failed_in, stop_in) = (failed.clone(), stop.clone());
            let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
            capturer.start_capture(chosen, move |result: Result<DesktopFrame, CaptureError>| {
                match result {
                    Ok(frame) => {
                        let Some(buffer) = to_i420(&frame) else {
                            return;
                        };
                        let _ = tx.send(Ok(()));
                        let frame = VideoFrame::new(VideoRotation::VideoRotation0, buffer);
                        if sending.load(Ordering::Relaxed) {
                            source.capture_frame(&frame);
                        }
                        if shown.is_none_or(|at| at.elapsed() >= every) {
                            shown = Some(Instant::now());
                            video::keep(LOCAL_SCREEN, &frame.buffer);
                        }
                    }
                    // Nothing new on screen yet, or the portal still asking.
                    Err(CaptureError::Temporary) => {}
                    Err(CaptureError::Permanent) => {
                        failed_in.store(true, Ordering::Relaxed);
                        stop_in.store(true, Ordering::Relaxed);
                        let _ = tx.send(Err("screen sharing was cancelled".into()));
                    }
                }
            });
            info!("screen capture started");
            while !stop.load(Ordering::Relaxed) {
                capturer.capture_frame();
                if let Ok(first) = rx.try_recv()
                    && let Some(started) = started.take()
                {
                    let _ = started.send(first);
                }
                std::thread::sleep(Duration::from_millis(1000 / video::FPS));
            }
            if let Some(started) = started.take() {
                let _ = started.send(Err(if failed.load(Ordering::Relaxed) {
                    "screen sharing was cancelled".into()
                } else {
                    "screen sharing stopped".into()
                }));
            }
            video::forget(LOCAL_SCREEN);
        });
    if let Err(err) = spawned {
        warn!(%err, "no screen capture thread");
    }
}

/// A desktop frame (BGRA, which libyuv calls ARGB) as I420.
fn to_i420(frame: &DesktopFrame) -> Option<I420Buffer> {
    let (width, height) = (frame.width() & !1, frame.height() & !1);
    if width <= 0 || height <= 0 {
        return None;
    }
    let mut buffer = I420Buffer::new(width as u32, height as u32);
    let (stride_y, stride_u, stride_v) = buffer.strides();
    let (y, u, v) = buffer.data_mut();
    yuv_helper::argb_to_i420(
        frame.data(),
        frame.stride(),
        y,
        stride_y,
        u,
        stride_u,
        v,
        stride_v,
        width,
        height,
    );
    Some(buffer)
}
