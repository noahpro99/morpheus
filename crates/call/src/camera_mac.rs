//! The camera on macOS, through AVFoundation: the cameras are the built-in
//! one, external (USB) ones and Continuity Cameras (an iPhone), found by a
//! discovery session; one is read by a capture session whose video data
//! output hands over NV12 pixel buffers (CoreVideo's
//! `420YpCbCr8BiPlanarVideoRange`), converted to the I420 that WebRTC sends
//! by camera_format.rs. Of the formats a camera lists, the one taken is
//! chosen as on Linux and Windows (16:9 up to 1280x720, at 30).
//!
//! The camera permission is asked (the system's prompt, with the bundle's
//! NSCameraUsageDescription) the first time a camera is opened, never just
//! for listing; refused, the preview says Morpheus is not allowed.
//!
//! As on Windows, the session lives on the camera's own thread: it opens the
//! device, says whether that worked, and streams once it is told where to
//! (or lets the device go if it never is). Frames come on a dispatch queue
//! of their own and are passed to that thread. A camera that goes away
//! (unplugged, an iPhone walked off) is looked for again every half second
//! and taken back when it returns; stopping the session lets the device go
//! (its green light off).
//!
//! Objective-C through raw calls: this module alone may use `unsafe`.
#![allow(unsafe_code)]

use std::{
    ffi::{CStr, c_char, c_void},
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use livekit::webrtc::{
    video_frame::{I420Buffer, VideoFrame, VideoRotation},
    video_source::{VideoResolution, native::NativeVideoSource},
};
use objc2::{
    AnyThread, DefinedClass, Message, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool, ProtocolObject},
};
use objc2_av_foundation::{
    AVAuthorizationStatus, AVCaptureConnection, AVCaptureDevice, AVCaptureDeviceDiscoverySession,
    AVCaptureDeviceFormat, AVCaptureDeviceInput, AVCaptureDevicePosition, AVCaptureOutput,
    AVCaptureSession, AVCaptureVideoDataOutput, AVCaptureVideoDataOutputSampleBufferDelegate,
    AVMediaTypeVideo,
};
use objc2_core_media::{CMSampleBuffer, CMTime, CMTimeFlags, CMVideoFormatDescriptionGetDimensions};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBaseAddressOfPlane,
    CVPixelBufferGetBytesPerRow, CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferGetHeight,
    CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    kCVPixelBufferPixelFormatTypeKey, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, kCVPixelFormatType_422YpCbCr8_yuvs,
};
use objc2_foundation::{NSArray, NSDictionary, NSError, NSNumber, NSObject, NSObjectProtocol, NSString};
use tracing::{info, warn};

use super::{
    autorelease,
    camera_format::{CameraInfo, NativeMode, PixelFormat, choose_mode, nv12_planes_to_i420, yuyv_to_i420},
    preview::OpenGuard,
    video::{self, LOCAL_CAMERA},
};

/// CoreVideo's pixel formats: NV12 (video and full range), YUY2 (`yuvs`).
const NV12_VIDEO: u32 = kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange;
const NV12_FULL: u32 = kCVPixelFormatType_420YpCbCr8BiPlanarFullRange;
const YUVS: u32 = kCVPixelFormatType_422YpCbCr8_yuvs;
/// UYVY ('2vuy'), the packed 4:2:2 many Mac cameras give natively.
const UYVY: u32 = 0x3276_7579;

/// Where the frames go, sent to the camera's thread once the track is up.
struct Start {
    source: NativeVideoSource,
    stop: Arc<AtomicBool>,
    key: &'static str,
}

/// A camera, open (on its own thread) and ready to stream.
pub(crate) struct Camera {
    /// The device's unique ID.
    id: String,
    resolution: VideoResolution,
    start: mpsc::Sender<Start>,
    thread: Option<JoinHandle<()>>,
}

impl Camera {
    /// The camera chosen ([`crate::imp::choose_camera`]), else the first that
    /// opens. Asks for the camera permission if it has not been asked (and
    /// there is a camera).
    pub(crate) fn open() -> io::Result<Self> {
        let wanted = crate::imp::chosen_camera();
        let (opened_tx, opened) = mpsc::channel();
        let (start, start_rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("morpheus-camera".into())
            .spawn(move || camera_thread(wanted, opened_tx, start_rx))?;
        match opened.recv() {
            Ok(Ok((id, width, height))) => Ok(Self {
                id,
                resolution: VideoResolution { width, height },
                start,
                thread: Some(thread),
            }),
            Ok(Err(err)) => {
                let _ = thread.join();
                Err(err)
            }
            Err(_) => Err(io::Error::other("the camera's thread ended")),
        }
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn resolution(&self) -> VideoResolution {
        self.resolution.clone()
    }

    /// Send frames to `source` (and show them as this device's camera) on
    /// the camera's thread, until `stop` is set.
    pub(crate) fn run(self, source: NativeVideoSource, stop: Arc<AtomicBool>) {
        let _ = self.start.send(Start { source, stop, key: LOCAL_CAMERA });
    }

    /// Frames to `source`, shown under `key`, until `stop` is set; returns
    /// when the camera has been let go.
    pub(crate) fn run_blocking(
        mut self,
        source: NativeVideoSource,
        stop: Arc<AtomicBool>,
        key: &'static str,
    ) {
        let _ = self.start.send(Start { source, stop, key });
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn camera_thread(
    wanted: Option<String>,
    opened: mpsc::Sender<io::Result<(String, u32, u32)>>,
    start: mpsc::Receiver<Start>,
) {
    let device = autorelease::pool(|| Open::first(wanted.as_deref()));
    let device = match device {
        Ok(device) => device,
        Err(err) => {
            let _ = opened.send(Err(err));
            return;
        }
    };
    if opened.send(Ok((device.id.clone(), device.width, device.height))).is_err() {
        return;
    }
    let Ok(Start { source, stop, key }) = start.recv() else {
        return;
    };
    stream(device, &source, &stop, key);
}

fn stream(device: Open, source: &NativeVideoSource, stop: &AtomicBool, key: &'static str) {
    let (id, name) = (device.id.clone(), device.name.clone());
    let mut device = Some(device);
    let every = video::show_every();
    let mut shown: Option<Instant> = None;
    let mut last_frame = Instant::now();
    let mut looked = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let Some(open) = device.as_ref() else {
            // Gone: only this camera is taken back, never another in its place.
            std::thread::sleep(Duration::from_millis(100));
            if looked.elapsed() >= Duration::from_millis(500) {
                looked = Instant::now();
                if let Ok(back) = autorelease::pool(|| Open::same(&id)) {
                    info!(name, "camera back");
                    last_frame = Instant::now();
                    device = Some(back);
                }
            }
            continue;
        };
        match open.frames.recv_timeout(Duration::from_millis(100)) {
            Ok(buffer) => {
                last_frame = Instant::now();
                let frame = VideoFrame::new(VideoRotation::VideoRotation0, buffer);
                source.capture_frame(&frame);
                if shown.is_none_or(|at| at.elapsed() >= every) {
                    shown = Some(Instant::now());
                    video::keep(key, &frame.buffer);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout)
                if last_frame.elapsed() < Duration::from_secs(1) || open.connected() => {}
            Err(_) => {
                warn!(name, "camera lost; waiting for it to come back");
                autorelease::pool(|| drop(device.take()));
                looked = Instant::now();
                video::forget(key);
            }
        }
    }
    autorelease::pool(|| drop(device));
    video::forget(key);
}

/// AVFoundation's string constants that only some macOS versions have,
/// looked up at run time (a linked one missing would stop the app from
/// starting on an older macOS).
fn av_string(name: &CStr) -> Option<Retained<NSString>> {
    unsafe extern "C" {
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }
    const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;
    // SAFETY: dlsym with a NUL-terminated name; a found symbol is one of
    // AVFoundation's `NSString *const` constants, read once.
    unsafe {
        let found = dlsym(RTLD_DEFAULT, name.as_ptr()).cast::<*const NSString>();
        if found.is_null() {
            return None;
        }
        (*found).as_ref().map(|s| s.retain())
    }
}

fn video_type() -> io::Result<&'static NSString> {
    // SAFETY: AVFoundation's constant, there on every macOS we run on.
    unsafe { AVMediaTypeVideo }.ok_or_else(|| io::Error::other("no AVMediaTypeVideo"))
}

/// The cameras: built in, external, Continuity Camera (each kind this
/// macOS knows).
fn devices() -> Vec<Retained<AVCaptureDevice>> {
    let Ok(video) = video_type() else {
        return Vec::new();
    };
    let mut kinds: Vec<Retained<NSString>> = Vec::new();
    for name in [c"AVCaptureDeviceTypeBuiltInWideAngleCamera", c"AVCaptureDeviceTypeContinuityCamera"] {
        kinds.extend(av_string(name));
    }
    // macOS 14's name for USB cameras, else the older one.
    kinds.extend(
        av_string(c"AVCaptureDeviceTypeExternal").or_else(|| av_string(c"AVCaptureDeviceTypeExternalUnknown")),
    );
    if kinds.is_empty() {
        return Vec::new();
    }
    let kinds = NSArray::from_retained_slice(&kinds);
    // SAFETY: device types this macOS has (looked up above) and the video
    // media type.
    unsafe {
        AVCaptureDeviceDiscoverySession::discoverySessionWithDeviceTypes_mediaType_position(
            &kinds,
            Some(video),
            AVCaptureDevicePosition::Unspecified,
        )
        .devices()
        .to_vec()
    }
}

/// The camera permission: granted, asked now (the system's prompt; waited
/// for up to two minutes), or refused.
fn authorize() -> io::Result<()> {
    let video = video_type()?;
    let denied = || {
        io::Error::new(io::ErrorKind::PermissionDenied, "camera access is not allowed in System Settings")
    };
    // SAFETY: the video media type, as these take.
    match unsafe { AVCaptureDevice::authorizationStatusForMediaType(video) } {
        AVAuthorizationStatus::Authorized => Ok(()),
        AVAuthorizationStatus::NotDetermined => {
            let (tx, rx) = mpsc::channel();
            let answer = RcBlock::new(move |granted: Bool| {
                let _ = tx.send(granted.as_bool());
            });
            // SAFETY: as above; the block is copied by AVFoundation and
            // called once, from any queue (it only sends).
            unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(video, &answer) };
            match rx.recv_timeout(Duration::from_secs(120)) {
                Ok(true) => Ok(()),
                _ => Err(denied()),
            }
        }
        _ => Err(denied()),
    }
}

fn error(err: &NSError) -> io::Error {
    let kind = match err.code() {
        -11852 => io::ErrorKind::PermissionDenied,
        -11804 | -11815 => io::ErrorKind::ResourceBusy,
        -11814 => io::ErrorKind::NotFound,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, format!("{} ({})", err.localizedDescription(), err.code()))
}

/// What a format gives, by its four-character code (AVFoundation converts
/// any of them to the NV12 asked for; this only ranks them).
fn format_of(subtype: u32) -> PixelFormat {
    match subtype {
        NV12_VIDEO | NV12_FULL => {
            PixelFormat::Nv12
        }
        YUVS | UYVY => PixelFormat::Yuy2,
        _ => PixelFormat::Mjpg,
    }
}

/// A camera's formats as modes: size, fastest rate, kind.
fn modes(formats: &[Retained<AVCaptureDeviceFormat>]) -> Vec<NativeMode> {
    formats
        .iter()
        .enumerate()
        .map(|(index, format)| {
            // SAFETY: getters on a format of a live device.
            unsafe {
                let description = format.formatDescription();
                let size = CMVideoFormatDescriptionGetDimensions(&description);
                let fps = format
                    .videoSupportedFrameRateRanges()
                    .to_vec()
                    .iter()
                    .map(|range| range.maxFrameRate())
                    .fold(0.0f64, f64::max);
                NativeMode {
                    index: index as u32,
                    format: format_of(description.media_sub_type()),
                    width: size.width.max(0) as u32,
                    height: size.height.max(0) as u32,
                    fps: fps as f32,
                }
            }
        })
        .collect()
}

/// The shortest frame duration to set for about 30 a second, from the
/// format's own ranges (a value outside them throws).
fn frame_duration(format: &AVCaptureDeviceFormat) -> Option<CMTime> {
    let thirty = CMTime { value: 1, timescale: 30, flags: CMTimeFlags::Valid, epoch: 0 };
    // SAFETY: getters on a format of a live device.
    unsafe {
        let ranges = format.videoSupportedFrameRateRanges().to_vec();
        if ranges.iter().any(|r| r.minFrameRate() <= 30.0 && r.maxFrameRate() > 30.5) {
            return Some(thirty);
        }
        ranges
            .iter()
            .filter(|r| r.maxFrameRate() <= 30.5)
            .max_by(|a, b| a.maxFrameRate().total_cmp(&b.maxFrameRate()))
            .or_else(|| ranges.iter().min_by(|a, b| a.maxFrameRate().total_cmp(&b.maxFrameRate())))
            .map(|range| range.minFrameDuration())
    }
}

/// Where the output's pictures go: the camera's thread, as I420 (at most
/// two waiting; more are dropped).
struct SinkState {
    frames: mpsc::SyncSender<I420Buffer>,
    told: AtomicBool,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop.
    #[unsafe(super(NSObject))]
    #[name = "MorpheusCameraSink"]
    #[ivars = SinkState]
    struct Sink;

    unsafe impl NSObjectProtocol for Sink {}

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for Sink {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn did_output(&self, _output: &AVCaptureOutput, sample: &CMSampleBuffer, _connection: &AVCaptureConnection) {
            autorelease::pool(|| {
                // SAFETY: a sample buffer AVFoundation handed over, read
                // only during this call.
                let picture = unsafe { sample.image_buffer() }.and_then(|image| to_i420(&image));
                match picture {
                    Some(picture) => {
                        let _ = self.ivars().frames.try_send(picture);
                    }
                    None => {
                        if !self.ivars().told.swap(true, Ordering::Relaxed) {
                            warn!("camera: a picture in a format Morpheus does not read");
                        }
                    }
                }
            });
        }
    }
);

impl Sink {
    fn new(frames: mpsc::SyncSender<I420Buffer>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(SinkState { frames, told: AtomicBool::new(false) });
        // SAFETY: NSObject's init.
        unsafe { msg_send![super(this), init] }
    }
}

/// A pixel buffer (NV12 as asked, or packed YUY2) as I420.
fn to_i420(image: &CVPixelBuffer) -> Option<I420Buffer> {
    // SAFETY: a live pixel buffer, unlocked below.
    if unsafe { CVPixelBufferLockBaseAddress(image, CVPixelBufferLockFlags::ReadOnly) } != 0 {
        return None;
    }
    let width = (CVPixelBufferGetWidth(image) & !1) as u32;
    let height = CVPixelBufferGetHeight(image);
    let format = CVPixelBufferGetPixelFormatType(image);
    // SAFETY: the base addresses are valid for their rows times bytes per
    // row while the buffer is locked (until the unlock below).
    let picture = unsafe {
        match format {
            NV12_VIDEO | NV12_FULL => {
                let luma = CVPixelBufferGetBaseAddressOfPlane(image, 0).cast::<u8>();
                let chroma = CVPixelBufferGetBaseAddressOfPlane(image, 1).cast::<u8>();
                let (luma_stride, chroma_stride) =
                    (CVPixelBufferGetBytesPerRowOfPlane(image, 0), CVPixelBufferGetBytesPerRowOfPlane(image, 1));
                (!luma.is_null() && !chroma.is_null()).then(|| {
                    let luma = std::slice::from_raw_parts(luma, luma_stride * height);
                    let chroma = std::slice::from_raw_parts(chroma, chroma_stride * height.div_ceil(2));
                    nv12_planes_to_i420(
                        luma,
                        luma_stride as u32,
                        chroma,
                        chroma_stride as u32,
                        width,
                        (height & !1) as u32,
                    )
                })
            }
            YUVS => {
                let base = CVPixelBufferGetBaseAddress(image).cast::<u8>();
                let stride = CVPixelBufferGetBytesPerRow(image);
                (!base.is_null()).then(|| {
                    let data = std::slice::from_raw_parts(base, stride * height);
                    yuyv_to_i420(data, width, (height & !1) as u32, stride as u32)
                })
            }
            _ => None,
        }
    };
    // SAFETY: locked above with the same flags.
    unsafe { CVPixelBufferUnlockBaseAddress(image, CVPixelBufferLockFlags::ReadOnly) };
    picture
}

/// A camera streaming into its capture session.
struct Open {
    id: String,
    name: String,
    width: u32,
    height: u32,
    device: Retained<AVCaptureDevice>,
    session: Retained<AVCaptureSession>,
    output: Retained<AVCaptureVideoDataOutput>,
    _sink: Retained<Sink>,
    _queue: DispatchRetained<DispatchQueue>,
    frames: mpsc::Receiver<I420Buffer>,
    _open: OpenGuard,
}

impl Open {
    /// The camera with unique ID `wanted` if it opens, else the first that
    /// does.
    fn first(wanted: Option<&str>) -> io::Result<Self> {
        let mut all = devices();
        if all.is_empty() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no camera found"));
        }
        // Asked only with a camera to open (CI's runner, with none, never
        // waits on a prompt).
        authorize()?;
        if let Some(wanted) = wanted
            // SAFETY: a getter on a live device.
            && let Some(at) = all.iter().position(|d| unsafe { d.uniqueID() }.to_string() == wanted)
        {
            all[..=at].rotate_right(1);
        }
        let mut first_error = None;
        for device in all {
            match Self::start(device) {
                Ok(open) => return Ok(open),
                Err(err) => {
                    warn!(%err, "a camera did not open");
                    first_error.get_or_insert(err);
                }
            }
        }
        Err(first_error.unwrap_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no camera found")))
    }

    /// This camera again, and no other.
    fn same(id: &str) -> io::Result<Self> {
        // SAFETY: a lookup by unique ID; a getter on what it finds.
        let device = unsafe { AVCaptureDevice::deviceWithUniqueID(&NSString::from_str(id)) }
            .filter(|d| unsafe { d.isConnected() })
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "camera not back"))?;
        Self::start(device)
    }

    fn connected(&self) -> bool {
        // SAFETY: a getter on a live device.
        unsafe { self.device.isConnected() }
    }

    fn start(device: Retained<AVCaptureDevice>) -> io::Result<Self> {
        // SAFETY: AVFoundation's calls in the order Apple documents (input
        // and output added in a configuration, the device locked while its
        // format is set from its own list and its frame duration from that
        // format's ranges, the session started); each object stays alive in
        // `Open` until the session is stopped in drop.
        unsafe {
            let id = device.uniqueID().to_string();
            let name = device.localizedName().to_string();
            let formats = device.formats().to_vec();
            let choice = choose_mode(&modes(&formats))
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{name} gives no picture format")))?;
            let format = &formats[choice.index as usize];
            let input = AVCaptureDeviceInput::deviceInputWithDevice_error(&device).map_err(|e| error(&e))?;
            let session = AVCaptureSession::new();
            session.beginConfiguration();
            if !session.canAddInput(&input) {
                session.commitConfiguration();
                return Err(io::Error::new(io::ErrorKind::ResourceBusy, format!("{name} is in use")));
            }
            session.addInput(&input);
            let output = AVCaptureVideoDataOutput::new();
            let key: &NSString = &*(kCVPixelBufferPixelFormatTypeKey as *const _ as *const NSString);
            let nv12 = NSNumber::new_u32(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange);
            let settings = NSDictionary::<NSString, AnyObject>::from_slices(&[key], &[nv12.as_ref()]);
            output.setVideoSettings(Some(&settings));
            output.setAlwaysDiscardsLateVideoFrames(true);
            let (tx, frames) = mpsc::sync_channel(2);
            let sink = Sink::new(tx);
            let queue = DispatchQueue::new("morpheus.camera", None);
            output.setSampleBufferDelegate_queue(Some(ProtocolObject::from_ref(&*sink)), Some(&queue));
            if !session.canAddOutput(&output) {
                session.commitConfiguration();
                return Err(io::Error::other(format!("{name} gives no video output")));
            }
            session.addOutput(&output);
            let locked = device.lockForConfiguration();
            if locked.is_ok() {
                device.setActiveFormat(format);
                if let Some(duration) = frame_duration(format) {
                    device.setActiveVideoMinFrameDuration(duration);
                }
            }
            session.commitConfiguration();
            session.startRunning();
            if locked.is_ok() {
                device.unlockForConfiguration();
            }
            if !session.isRunning() {
                return Err(io::Error::new(io::ErrorKind::ResourceBusy, format!("{name} did not start")));
            }
            let size = CMVideoFormatDescriptionGetDimensions(&device.activeFormat().formatDescription());
            let (width, height) = (size.width.max(0) as u32 & !1, size.height.max(0) as u32 & !1);
            let info = CameraInfo {
                name: name.clone(),
                native: choice.native,
                output: PixelFormat::Nv12,
                width,
                height,
                fps: choice.fps.min(30.0),
                backend: "AVFoundation",
            };
            info!(camera = %info, "camera open");
            crate::diag::note_camera(Some(info.to_string()));
            Ok(Self {
                id,
                name,
                width,
                height,
                device,
                session,
                output,
                _sink: sink,
                _queue: queue,
                frames,
                _open: OpenGuard::new(),
            })
        }
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: the session and output this camera made; stopping waits
        // for the session, which lets the device go.
        unsafe {
            self.session.stopRunning();
            self.output.setSampleBufferDelegate_queue(None, None);
        }
        crate::diag::note_camera(None);
    }
}

/// The cameras, by unique ID and name.
pub(crate) fn list() -> Vec<crate::imp::CameraDevice> {
    autorelease::pool(|| {
        devices()
            .iter()
            .map(|device| {
                // SAFETY: getters on a live device.
                let (id, name) = unsafe { (device.uniqueID().to_string(), device.localizedName().to_string()) };
                crate::imp::CameraDevice { id, name }
            })
            .collect()
    })
}

/// What reads the camera here, for the self-test's report.
pub(crate) fn status() -> String {
    "AVFoundation".to_owned()
}
