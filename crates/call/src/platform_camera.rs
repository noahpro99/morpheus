//! A camera the app drives itself (on Android, Camera2 from Kotlin): the
//! app registers how to start and stop it ([`set_platform_camera`]) and
//! hands over each frame ([`camera_frame`]), which goes to the call.

use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use livekit::webrtc::{
    video_frame::{I420Buffer, VideoFrame, VideoRotation},
    video_source::native::NativeVideoSource,
};
use tracing::warn;

use crate::imp::video::{self, LOCAL_CAMERA};

/// Starting and stopping the app's camera. `start` returns whether it did.
pub struct PlatformCamera {
    pub start: Box<dyn Fn() -> bool + Send + Sync>,
    pub stop: Box<dyn Fn() + Send + Sync>,
}

static CAMERA: OnceLock<PlatformCamera> = OnceLock::new();

/// Where frames go while the camera is on in a call.
static SINK: Mutex<Option<Sink>> = Mutex::new(None);

struct Sink {
    /// None while only previewing (before a call): the picture is shown here and sent nowhere.
    source: Option<NativeVideoSource>,
    shown: Option<Instant>,
}

/// Register the app's camera (once, at startup).
pub fn set_platform_camera(camera: PlatformCamera) {
    let _ = CAMERA.set(camera);
}

pub(crate) fn available() -> bool {
    CAMERA.get().is_some()
}

/// Turn the app's camera on, its frames going to `source` until `stop` is set.
pub(crate) fn run(source: NativeVideoSource, stop: Arc<AtomicBool>) -> bool {
    let Some(camera) = CAMERA.get() else {
        return false;
    };
    // A call takes the camera over from a preview.
    PREVIEW.store(false, Ordering::Relaxed);
    if let Ok(mut sink) = SINK.lock() {
        *sink = Some(Sink {
            source: Some(source),
            shown: None,
        });
    }
    if !(camera.start)() {
        clear();
        return false;
    }
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
        }
        (camera.stop)();
        clear();
        video::forget(LOCAL_CAMERA);
    });
    true
}

fn clear() {
    if let Ok(mut sink) = SINK.lock() {
        *sink = None;
    }
}

/// One camera picture, in planes as Android's YUV_420_888 gives them: a
/// pixel stride of 1 for planar chroma, 2 where U and V interleave.
/// `rotation` is how far (0, 90, 180, 270 degrees) it must turn to stand up.
#[allow(clippy::too_many_arguments)]
pub fn camera_frame(
    y: &[u8],
    u: &[u8],
    v: &[u8],
    y_stride: usize,
    uv_stride: usize,
    uv_pixel_stride: usize,
    width: u32,
    height: u32,
    rotation: u32,
) {
    let Ok(mut sink) = SINK.lock() else {
        return;
    };
    let Some(sink) = sink.as_mut() else {
        return;
    };
    let mut buffer = I420Buffer::new(width, height);
    let (stride_y, stride_u, stride_v) = buffer.strides();
    let (dst_y, dst_u, dst_v) = buffer.data_mut();
    let (w, h) = (width as usize, height as usize);
    let (stride_y, stride_u, stride_v) = (stride_y as usize, stride_u as usize, stride_v as usize);
    for row in 0..h {
        let Some(src) = y.get(row * y_stride..row * y_stride + w) else {
            warn!("camera frame shorter than its size");
            return;
        };
        dst_y[row * stride_y..row * stride_y + w].copy_from_slice(src);
    }
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    for row in 0..ch {
        for col in 0..cw {
            let at = row * uv_stride + col * uv_pixel_stride;
            dst_u[row * stride_u + col] = u.get(at).copied().unwrap_or(128);
            dst_v[row * stride_v + col] = v.get(at).copied().unwrap_or(128);
        }
    }
    let degrees = rotation;
    let rotation = match rotation {
        90 => VideoRotation::VideoRotation90,
        180 => VideoRotation::VideoRotation180,
        270 => VideoRotation::VideoRotation270,
        _ => VideoRotation::VideoRotation0,
    };
    let frame = VideoFrame::new(rotation, buffer);
    if let Some(source) = &sink.source {
        source.capture_frame(&frame);
    }
    let every = Duration::from_millis(1000 / video::FPS);
    if sink.shown.is_none_or(|at| at.elapsed() >= every) {
        sink.shown = Some(Instant::now());
        // The preview turned upright; the call gets the rotation with the frame.
        video::keep_turned(LOCAL_CAMERA, &frame.buffer, degrees);
    }
}

/// Whether a preview (the camera shown, sent nowhere) is on.
static PREVIEW: AtomicBool = AtomicBool::new(false);

/// Turn the camera on for a preview (Settings, before a call): its pictures
/// are kept under [`LOCAL_CAMERA`] and go nowhere else. False when there is
/// no camera or a call has it.
pub fn start_camera_preview() -> bool {
    let Some(camera) = CAMERA.get() else {
        return false;
    };
    if let Ok(mut sink) = SINK.lock() {
        if sink.is_some() {
            return false;
        }
        *sink = Some(Sink {
            source: None,
            shown: None,
        });
    }
    if !(camera.start)() {
        clear();
        return false;
    }
    PREVIEW.store(true, Ordering::Relaxed);
    true
}

/// Turn the preview off (and let the camera go), unless a call took over.
pub fn stop_camera_preview() {
    if !PREVIEW.swap(false, Ordering::Relaxed) {
        return;
    }
    if let Some(camera) = CAMERA.get() {
        (camera.stop)();
    }
    if let Ok(mut sink) = SINK.lock()
        && sink.as_ref().is_some_and(|s| s.source.is_none())
    {
        *sink = None;
    }
    video::forget(LOCAL_CAMERA);
}

pub fn camera_preview_on() -> bool {
    PREVIEW.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    static STARTS: AtomicUsize = AtomicUsize::new(0);
    static STOPS: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn a_preview_shows_the_camera_before_a_call_and_lets_it_go() {
        set_platform_camera(PlatformCamera {
            start: Box::new(|| {
                STARTS.fetch_add(1, Ordering::Relaxed);
                true
            }),
            stop: Box::new(|| {
                STOPS.fetch_add(1, Ordering::Relaxed);
            }),
        });
        assert!(!camera_preview_on());
        assert!(start_camera_preview());
        assert!(camera_preview_on());
        assert_eq!(STARTS.load(Ordering::Relaxed), 1);
        // A second preview is refused while one is on (the camera is taken).
        assert!(!start_camera_preview());
        // One picture, 4x4 grey: it is kept to be shown, and sent nowhere.
        let (y, u, v) = (vec![128u8; 16], vec![128u8; 4], vec![128u8; 4]);
        camera_frame(&y, &u, &v, 4, 2, 1, 4, 4, 0);
        assert!(video::latest_frame(LOCAL_CAMERA).is_some(), "the preview shows a picture");
        stop_camera_preview();
        assert!(!camera_preview_on());
        assert_eq!(STOPS.load(Ordering::Relaxed), 1);
        assert!(video::latest_frame(LOCAL_CAMERA).is_none(), "nothing is kept after");
        // Stopping again does nothing.
        stop_camera_preview();
        assert_eq!(STOPS.load(Ordering::Relaxed), 1);
    }
}
