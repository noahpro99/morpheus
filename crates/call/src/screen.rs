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
    video_frame::{VideoBuffer as _, native::VideoFrameBufferExt as _},
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

use crate::{
    imp::video::{self, LOCAL_SCREEN},
    quality::ScreenQuality,
};


/// libwebrtc's Linux portal capturer (xdg-desktop-portal ScreenCast over
/// GDBus) gets its replies through callbacks on GLib's default main context.
/// GTK hosts (the Dioxus app) iterate it already; a host with no GLib loop
/// (the GPUI app) never completes the portal request, so the picker does not
/// show. Once per process: a thread that runs a main loop on that context, or
/// ends at once when GTK owns it.
#[cfg(target_os = "linux")]
fn ensure_glib_loop() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("morpheus-glib".into())
            .spawn(|| {
                let ctx = glib::MainContext::default();
                match ctx.acquire() {
                    Ok(_owner) => {
                        tracing::info!("screen capture: running a GLib main loop for the portal");
                        glib::MainLoop::new(Some(&ctx), false).run();
                    }
                    Err(_) => tracing::info!("screen capture: GTK already owns GLib's main context"),
                }
            });
        if let Err(err) = spawned {
            tracing::warn!("screen capture: no GLib main loop thread: {err}");
        }
    });
}
/// Capture the screen on a thread of its own until `stop` is set, scaled to
/// `quality` and no faster than its frame rate. `started` hears once: the
/// size the pictures are sent at (the first one arrived), or that capture
/// could not start (no capturer, or the picker was cancelled). Frames go out
/// once `sink` holds the source to send them to.
pub(crate) fn run(
    sink: Arc<std::sync::OnceLock<NativeVideoSource>>,
    quality: ScreenQuality,
    stop: Arc<AtomicBool>,
    started: oneshot::Sender<Result<(u32, u32), String>>,
) {
    #[cfg(target_os = "linux")]
    ensure_glib_loop();
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
            let every = video::show_every();
            let send_every = Duration::from_millis(1000 / u64::from(quality.fps.max(1)));
            let mut shown: Option<Instant> = None;
            let mut sent_at: Option<Instant> = None;
            let mut kept_at: Option<Instant> = None;
            let (failed_in, stop_in) = (failed.clone(), stop.clone());
            let sink_in = sink.clone();
            let (tx, rx) = std::sync::mpsc::channel::<Result<(u32, u32), String>>();
            // The last picture, and how many it has been, for the loop: a
            // portal that sends pictures only when the screen changes leaves a
            // still screen without any, and a viewer who joins late (or an
            // encoder that wants a keyframe) would see nothing.
            let last: Arc<std::sync::Mutex<Option<VideoFrame<I420Buffer>>>> = Arc::default();
            let last_in = last.clone();
            let pictures = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let temporary = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let (pictures_in, temporary_in) = (pictures.clone(), temporary.clone());
            let sent_first = Arc::new(AtomicBool::new(false));
            let sent_first_in = sent_first.clone();
            capturer.start_capture(chosen, move |result: Result<DesktopFrame, CaptureError>| {
                match result {
                    Ok(frame) => {
                        let Some(buffer) = to_i420(&frame, quality) else {
                            return;
                        };
                        let size = (buffer.width(), buffer.height());
                        if pictures_in.fetch_add(1, Ordering::Relaxed) == 0 {
                            info!(
                                width = frame.width(),
                                height = frame.height(),
                                "screen capture: the first picture"
                            );
                        }
                        let _ = tx.send(Ok(size));
                        let frame = VideoFrame::new(VideoRotation::VideoRotation0, buffer);
                        // No faster than the frame rate chosen (a portal may
                        // give more); a little under, so a clock a hair fast
                        // does not drop every other one.
                        if let Some(source) = sink_in.get()
                            && sent_at.is_none_or(|at| at.elapsed() >= send_every * 9 / 10)
                        {
                            sent_at = Some(Instant::now());
                            source.capture_frame(&frame);
                            if !sent_first_in.swap(true, Ordering::Relaxed) {
                                info!("screen capture: the first picture sent to the call");
                            }
                        }
                        // A copy twice a second is plenty for the loop's repeat.
                        if kept_at.is_none_or(|at| at.elapsed() >= Duration::from_millis(500))
                            && let Ok(mut kept) = last_in.lock()
                        {
                            kept_at = Some(Instant::now());
                            *kept = Some(VideoFrame::new(
                                VideoRotation::VideoRotation0,
                                frame.buffer.to_i420(),
                            ));
                        }
                        if shown.is_none_or(|at| at.elapsed() >= every) {
                            shown = Some(Instant::now());
                            video::keep(LOCAL_SCREEN, &frame.buffer);
                        }
                    }
                    // Nothing new on screen yet, or the portal still asking.
                    Err(CaptureError::Temporary) => {
                        temporary_in.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(CaptureError::Permanent) => {
                        failed_in.store(true, Ordering::Relaxed);
                        stop_in.store(true, Ordering::Relaxed);
                        let _ = tx.send(Err("screen sharing was cancelled".into()));
                    }
                }
            });
            info!("screen capture started");
            let began = Instant::now();
            let mut resent = Instant::now();
            let mut warned = false;
            while !stop.load(Ordering::Relaxed) {
                capturer.capture_frame();
                if !warned && began.elapsed() >= Duration::from_secs(5) && pictures.load(Ordering::Relaxed) == 0 {
                    warned = true;
                    info!(
                        temporary = temporary.load(Ordering::Relaxed),
                        "screen capture: no picture after 5 s (the screen picker is still open, or the portal sends nothing yet)"
                    );
                }
                // Nothing new for a second while sharing: the last picture again.
                if let Some(source) = sink.get()
                    && resent.elapsed() >= Duration::from_secs(1)
                {
                    resent = Instant::now();
                    if pictures.load(Ordering::Relaxed) > 0
                        && let Ok(kept) = last.lock()
                        && let Some(frame) = kept.as_ref()
                    {
                        source.capture_frame(frame);
                    }
                }
                if let Ok(first) = rx.try_recv()
                    && let Some(started) = started.take()
                {
                    let _ = started.send(first);
                }
                std::thread::sleep((send_every / 2).max(Duration::from_millis(4)));
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
fn to_i420(frame: &DesktopFrame, quality: ScreenQuality) -> Option<I420Buffer> {
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
    // Smaller, if the choice is: scaled here, once, for the call and the
    // preview alike.
    let (fit_w, fit_h) = quality.fit(width as u32, height as u32);
    if (fit_w, fit_h) != (width as u32, height as u32) {
        return Some(buffer.scale(fit_w as i32, fit_h as i32));
    }
    Some(buffer)
}
