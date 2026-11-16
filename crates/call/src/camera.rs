//! The camera on Linux, through Video4Linux: the first device that catures
//! YUYV (webcams do; the infrared cameras next to them only give grey), at
//! the widest 16:9 mode it has up to 1280×720 (see `camera_format::pick_mode`, shared with Windows), converted to the I420 that WebRTC sends.

use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use linuxvideo::{
    BufType,
    format::{PixFormat, PixelFormat},
    stream::ReadStream,
};
use livekit::webrtc::{
    video_frame::{VideoFrame, VideoRotation},
    video_source::{VideoResolution, native::NativeVideoSource},
};
use tracing::{info, warn};

use crate::imp::{
    camera_format::{Mode, pick_mode, yuyv_rows, yuyv_to_i420},
    video::{self, LOCAL_CAMERA},
};

/// What the device does in YUYV.
fn modes_of(device: &linuxvideo::Device) -> Vec<Mode> {
    use linuxvideo::format::FrameSizes;
    let speed = |w: u32, h: u32| {
        device
            .frame_intervals(PixelFormat::YUYV, w, h)
            .ok()
            .map(|i| i.min())
            .filter(|f| f.numerator() > 0)
            .map_or(30.0, |f| f.denominator() as f32 / f.numerator() as f32)
    };
    match device.frame_sizes(PixelFormat::YUYV) {
        Ok(FrameSizes::Discrete(sizes)) => sizes
            .iter()
            .map(|s| (s.width(), s.height(), speed(s.width(), s.height())))
            .collect(),
        Ok(FrameSizes::Stepwise(range) | FrameSizes::Continuous(range)) => {
            [(1280, 720), (960, 540), (640, 360), (640, 480)]
                .into_iter()
                .filter(|&(w, h)| {
                    (range.min_width()..=range.max_width()).contains(&w)
                        && (range.min_height()..=range.max_height()).contains(&h)
                })
                .map(|(w, h)| (w, h, 30.0))
                .collect()
        }
        Err(_) => vec![(640, 480, 30.0)],
    }
}

/// A camera, open and streaming.
pub(crate) struct Camera {
    stream: ReadStream,
    width: u32,
    height: u32,
    stride: u32,
    /// The device's path.
    id: String,
    _open: crate::imp::preview::OpenGuard,
}

impl Camera {
    /// The camera chosen ([`crate::imp::choose_camera`]), else the first that can
    /// give YUYV.
    pub(crate) fn open() -> io::Result<Self> {
        let chosen = crate::imp::chosen_camera()
            .and_then(|path| linuxvideo::Device::open(path).ok())
            .map(Ok);
        for device in chosen.into_iter().chain(linuxvideo::list()?) {
            let Ok(device) = device else { continue };
            let yuyv = device
                .formats(BufType::VIDEO_CAPTURE)
                .filter_map(Result::ok)
                .any(|format| format.pixel_format() == PixelFormat::YUYV);
            if !yuyv {
                continue;
            }
            let name = device
                .capabilities()
                .map(|caps| caps.card().to_owned())
                .unwrap_or_default();
            let id = device
                .path()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            let modes = modes_of(&device);
            let (want_w, want_h) = pick_mode(&modes).unwrap_or((640, 480));
            let capture =
                device.video_capture(PixFormat::new(want_w, want_h, PixelFormat::YUYV))?;
            // As fast as it goes up to 30 (the driver picks the nearest it has).
            let _ = capture.set_frame_interval(linuxvideo::Fract::new(1, 30));
            let format = capture.format();
            let (width, height, stride) =
                (format.width(), format.height(), format.bytes_per_line());
            if format.pixel_format() != PixelFormat::YUYV || width < 2 || height < 2 {
                continue;
            }
            info!(name, width, height, "camera open");
            let fps = modes
                .iter()
                .find(|m| (m.0, m.1) == (width, height))
                .map_or(30.0, |m| m.2.min(30.0));
            crate::diag::note_camera(Some(
                crate::imp::camera_format::CameraInfo {
                    name: name.clone(),
                    native: crate::imp::camera_format::PixelFormat::Yuy2,
                    output: crate::imp::camera_format::PixelFormat::Yuy2,
                    width,
                    height,
                    fps,
                    backend: "Video4Linux",
                }
                .to_string(),
            ));
            return Ok(Self {
                id,
                _open: crate::imp::preview::OpenGuard::new(),
                stream: capture.into_stream()?,
                width: width & !1,
                height: height & !1,
                stride,
            });
        }
        Err(io::Error::new(io::ErrorKind::NotFound, "no camera found"))
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn resolution(&self) -> VideoResolution {
        VideoResolution {
            width: self.width,
            height: self.height,
        }
    }

    /// Send frames to `source` (and show them as this device's camera) on a
    /// thread of its own, until `stop` is set.
    pub(crate) fn run(self, source: NativeVideoSource, stop: Arc<AtomicBool>) {
        let spawned = std::thread::Builder::new()
            .name("morpheus-camera".into())
            .spawn(move || self.run_blocking(source, stop, LOCAL_CAMERA));
        if let Err(err) = spawned {
            warn!(%err, "no camera thread");
        }
    }

    /// Frames to `source`, shown under `key`, on this thread until `stop` is
    /// set; the camera is let go when it returns.
    pub(crate) fn run_blocking(
        mut self,
        source: NativeVideoSource,
        stop: Arc<AtomicBool>,
        key: &'static str,
    ) {
        let every = video::show_every();
        let mut shown: Option<Instant> = None;
        // Frames that came short (#117): some webcams lose a few rows of
        // every frame; noted once, and one with under half its rows is
        // skipped (a mostly repeated picture).
        let mut short_noted = false;
        while !stop.load(Ordering::Relaxed) {
            let (width, height, stride) = (self.width, self.height, self.stride);
            let frame = self.stream.dequeue(|buf| {
                let rows = yuyv_rows(buf.len(), width, height, stride);
                if rows < height && !short_noted {
                    short_noted = true;
                    warn!(
                        rows,
                        height,
                        bytes = buf.len(),
                        error = buf.is_error(),
                        "camera frames arrive cut short (the USB connection loses data); missing rows repeat the last one"
                    );
                }
                Ok((rows * 2 >= height).then(|| yuyv_to_i420(&buf, width, height, stride)))
            });
            let buffer = match frame {
                Ok(Some(buffer)) => buffer,
                Ok(None) => continue,
                Err(err) => {
                    warn!(%err, "camera stopped");
                    break;
                }
            };
            let frame = VideoFrame::new(VideoRotation::VideoRotation0, buffer);
            source.capture_frame(&frame);
            if shown.is_none_or(|at| at.elapsed() >= every) {
                shown = Some(Instant::now());
                video::keep(key, &frame.buffer);
            }
        }
        video::forget(key);
        crate::diag::note_camera(None);
    }
}

/// Run by hand (`cargo test -p morpheus-call -- --ignored real_camera`): the
/// webcam gives real pictures. Nothing is kept or shown.
#[cfg(test)]
#[allow(clippy::expect_used)]
mod device_tests {
    use std::sync::{Arc, atomic::AtomicBool};

    use livekit::webrtc::video_source::native::NativeVideoSource;

    use super::Camera;
    use crate::imp::video::{LOCAL_CAMERA, latest_frame};

    #[test]
    #[ignore = "needs a webcam"]
    fn real_camera_gives_pictures() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let _inside = runtime.enter();
        let camera = Camera::open().expect("a camera");
        let resolution = camera.resolution();
        let source = NativeVideoSource::new(resolution.clone(), false);
        let stop = Arc::new(AtomicBool::new(false));
        camera.run(source, stop.clone());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut seen = 0;
        while seen < 5 && std::time::Instant::now() < deadline {
            if let Some(frame) = latest_frame(LOCAL_CAMERA) {
                assert_eq!(
                    (frame.width, frame.height),
                    (resolution.width.min(640), resolution.height.min(480))
                );
                assert!(frame.jpeg.len() > 2_000, "a real picture, not a blank one");
                seen = frame.seq;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(seen >= 5, "frames kept coming ({seen})");
        eprintln!(
            "camera: {}x{}, {seen} pictures",
            resolution.width, resolution.height
        );
    }
}

/// The cameras that can give YUYV (webcams; not the infrared ones beside
/// them), by device path and name.
pub(crate) fn list() -> Vec<crate::imp::CameraDevice> {
    let Ok(devices) = linuxvideo::list() else {
        return Vec::new();
    };
    devices
        .filter_map(Result::ok)
        .filter(|device| {
            device
                .formats(BufType::VIDEO_CAPTURE)
                .filter_map(Result::ok)
                .any(|format| format.pixel_format() == PixelFormat::YUYV)
        })
        .filter_map(|device| {
            let id = device.path().ok()?.to_string_lossy().into_owned();
            let name = device
                .capabilities()
                .map(|caps| caps.card().to_owned())
                .unwrap_or_else(|_| id.clone());
            Some(crate::imp::CameraDevice { id, name })
        })
        .collect()
}

/// What reads the camera here, for the self-test's report.
pub(crate) fn status() -> String {
    "Video4Linux".to_owned()
}
