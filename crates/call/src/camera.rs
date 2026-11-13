//! The camera on Linux, through Video4Linux: the first device that captures
//! YUYV (webcams do; the infrared cameras next to them only give grey), at
//! the widest 16:9 mode it has up to 1280×720 (see [`pick_mode`]), converted to the I420 that WebRTC sends.

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
    video_frame::{I420Buffer, VideoFrame, VideoRotation},
    video_source::{VideoResolution, native::NativeVideoSource},
};
use tracing::{info, warn};

use crate::imp::video::{self, LOCAL_CAMERA};

/// A mode a camera offers: width, height, and the most frames a second.
type Mode = (u32, u32, f32);

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
            let (want_w, want_h) = pick_mode(&modes_of(&device)).unwrap_or((640, 480));
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
            return Ok(Self {
                stream: capture.into_stream()?,
                width: width & !1,
                height: height & !1,
                stride,
            });
        }
        Err(io::Error::new(io::ErrorKind::NotFound, "no camera found"))
    }

    pub(crate) fn resolution(&self) -> VideoResolution {
        VideoResolution {
            width: self.width,
            height: self.height,
        }
    }

    /// Send frames to `source` (and show them as this device's camera) on a
    /// thread of its own, until `stop` is set.
    pub(crate) fn run(mut self, source: NativeVideoSource, stop: Arc<AtomicBool>) {
        let spawned = std::thread::Builder::new()
            .name("morpheus-camera".into())
            .spawn(move || {
                let every = video::show_every();
                let mut shown: Option<Instant> = None;
                while !stop.load(Ordering::Relaxed) {
                    let frame = self.stream.dequeue(|buf| {
                        Ok(yuyv_to_i420(&buf, self.width, self.height, self.stride))
                    });
                    let buffer = match frame {
                        Ok(buffer) => buffer,
                        Err(err) => {
                            warn!(%err, "camera stopped");
                            break;
                        }
                    };
                    let frame = VideoFrame::new(VideoRotation::VideoRotation0, buffer);
                    source.capture_frame(&frame);
                    if shown.is_none_or(|at| at.elapsed() >= every) {
                        shown = Some(Instant::now());
                        video::keep(LOCAL_CAMERA, &frame.buffer);
                    }
                }
                video::forget(LOCAL_CAMERA);
            });
        if let Err(err) = spawned {
            warn!(%err, "no camera thread");
        }
    }
}

/// YUYV (4:2:2, packed `Y0 U Y1 V`) to I420 (4:2:0, planar): each chroma
/// sample of the result averages the two rows it covers.
pub(crate) fn yuyv_to_i420(yuyv: &[u8], width: u32, height: u32, stride: u32) -> I420Buffer {
    let mut out = I420Buffer::new(width, height);
    let (stride_y, stride_u, stride_v) = out.strides();
    let (y, u, v) = out.data_mut();
    let (width, height, stride) = (width as usize, height as usize, stride as usize);
    let (stride_y, stride_u, stride_v) = (stride_y as usize, stride_u as usize, stride_v as usize);
    if yuyv.len() < stride * height {
        return out;
    }
    for row in 0..height {
        let src = &yuyv[row * stride..row * stride + width * 2];
        let dst = &mut y[row * stride_y..row * stride_y + width];
        for (x, luma) in dst.iter_mut().enumerate() {
            *luma = src[x * 2];
        }
    }
    for row in 0..height / 2 {
        let top = &yuyv[2 * row * stride..];
        let bottom = &yuyv[(2 * row + 1) * stride..];
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

#[cfg(test)]
mod tests {
    use super::{pick_mode, yuyv_to_i420};

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
