//! The camera on Windows, through Media Foundation: the cameras are the
//! system's video capture devices (MFEnumDeviceSources), each read with a
//! source reader. Of the modes a camera lists, the one taken is chosen as on
//! Linux (camera_format.rs: 16:9 up to 1280x720 at 30, NV12 or YUY2 as they
//! are, a JPEG camera's pictures decoded to NV12 by Media Foundation), then
//! converted to the I420 that WebRTC sends.
//!
//! Media Foundation's objects stay on one thread, the camera's: it opens the
//! device, says whether that worked, and reads from it once it is told where
//! to send the frames (or lets the device go if it never is). A camera that
//! goes away (unplugged) is looked for again every half second and read
//! again when it is back; when the call or the preview stops it, the device
//! is shut down, so its light goes off.
//!
//! COM through raw calls: this module may use `unsafe`.
#![allow(unsafe_code)]

use std::{
    ffi::c_void,
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        OnceLock, mpsc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use livekit::webrtc::{
    video_frame::{I420Buffer, VideoFrame, VideoRotation},
    video_source::{VideoResolution, native::NativeVideoSource},
};
use tracing::{info, warn};
use windows::{
    Win32::{
        Foundation::{E_ACCESSDENIED, ERROR_BUSY, ERROR_SHARING_VIOLATION, HMODULE},
        Media::MediaFoundation::{
            IMFActivate, IMFAttributes, IMFMediaSource, IMFMediaType, IMFSourceReader,
            MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
            MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
            MF_E_HW_MFT_FAILED_START_STREAMING, MF_E_VIDEO_RECORDING_DEVICE_INVALIDATED,
            MF_E_VIDEO_RECORDING_DEVICE_PREEMPTED, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
            MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_SOURCE_READER_ALL_STREAMS,
            MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
            MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED, MF_SOURCE_READERF_ENDOFSTREAM,
            MF_SOURCE_READERF_ERROR, MF_VERSION, MFMediaType_Video,
            MFSTARTUP_LITE, MFVideoFormat_I420, MFVideoFormat_MJPG,
            MFVideoFormat_NV12, MFVideoFormat_YUY2,
        },
        System::{
            Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize},
            LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
        },
    },
    core::{GUID, HRESULT, Interface, PCSTR, PCWSTR, PWSTR, s, w},
};

use crate::imp::{
    camera_format::{CameraInfo, Choice, NativeMode, PixelFormat, choose_mode, to_i420},
    preview::OpenGuard,
    video::{self, LOCAL_CAMERA},
};

const STREAM: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

/// Where the frames go, sent to the camera's thread once the track is up.
struct Start {
    source: NativeVideoSource,
    stop: Arc<AtomicBool>,
    key: &'static str,
}

/// A camera, open (on its own thread) and ready to stream.
pub(crate) struct Camera {
    /// The device's symbolic link.
    id: String,
    resolution: VideoResolution,
    start: mpsc::Sender<Start>,
    thread: Option<JoinHandle<()>>,
}

impl Camera {
    /// The camera chosen ([`crate::imp::choose_camera`]), else the first that
    /// opens.
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

/// The camera's thread: open, say so, wait to be told where frames go (the
/// device is let go if that never comes), then stream.
fn camera_thread(
    wanted: Option<String>,
    opened: mpsc::Sender<io::Result<(String, u32, u32)>>,
    start: mpsc::Receiver<Start>,
) {
    let _com = Com::init();
    let _mf = match MediaFoundation::start() {
        Ok(mf) => mf,
        Err(err) => {
            let _ = opened.send(Err(err));
            return;
        }
    };
    let device = match Device::open(wanted.as_deref()) {
        Ok(device) => device,
        Err(err) => {
            let _ = opened.send(Err(err));
            return;
        }
    };
    let ready = (device.id.clone(), device.width, device.height);
    if opened.send(Ok(ready)).is_err() {
        return;
    }
    let Ok(Start { source, stop, key }) = start.recv() else {
        return;
    };
    stream(device, source, &stop, key);
}

fn stream(device: Device, source: NativeVideoSource, stop: &AtomicBool, key: &'static str) {
    let (id, name) = (device.id.clone(), device.name.clone());
    let mut device = Some(device);
    let every = video::show_every();
    let mut shown: Option<Instant> = None;
    let mut looked: Option<Instant> = None;
    while !stop.load(Ordering::Relaxed) {
        let Some(open) = device.as_mut() else {
            // Gone (unplugged, or taken by another app): only this camera
            // is taken back, never another one in its place.
            std::thread::sleep(Duration::from_millis(100));
            if looked.is_none_or(|at| at.elapsed() >= Duration::from_millis(500)) {
                looked = Some(Instant::now());
                if let Ok(back) = Device::open_same(&id, &name) {
                    info!(name, "camera back");
                    device = Some(back);
                }
            }
            continue;
        };
        match open.read() {
            Ok(Some(buffer)) => {
                let frame = VideoFrame::new(VideoRotation::VideoRotation0, buffer);
                source.capture_frame(&frame);
                if shown.is_none_or(|at| at.elapsed() >= every) {
                    shown = Some(Instant::now());
                    video::keep(key, &frame.buffer);
                }
            }
            Ok(None) => {}
            Err(err) => {
                warn!(%err, name, "camera lost; waiting for it to come back");
                device = None;
                looked = Some(Instant::now());
                video::forget(key);
            }
        }
    }
    drop(device);
    video::forget(key);
}

/// COM on this thread (in the multithreaded apartment), undone on drop.
struct Com(bool);

impl Com {
    fn init() -> Self {
        // SAFETY: no pointers; undone in drop only if it succeeded.
        Self(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: matches the CoInitializeEx above.
            unsafe { CoUninitialize() };
        }
    }
}

/// Media Foundation started, stopped on drop (both are counted).
struct MediaFoundation;

impl MediaFoundation {
    fn start() -> io::Result<Self> {
        // SAFETY: no pointers.
        let api = api()?;
        unsafe { (api.startup)(MF_VERSION, MFSTARTUP_LITE) }.ok().map_err(error)?;
        Ok(Self)
    }
}

impl Drop for MediaFoundation {
    fn drop(&mut self) {
        // SAFETY: matches the MFStartup above.
        if let Ok(api) = api() {
            let _ = unsafe { (api.shutdown)() };
        }
    }
}

/// A Windows error as an I/O one, of the kind the preview words for a
/// person (no camera, not allowed, busy).
fn error(err: windows::core::Error) -> io::Error {
    let code = err.code();
    let win32 = |e: windows::Win32::Foundation::WIN32_ERROR| HRESULT::from_win32(e.0);
    let kind = if code == E_ACCESSDENIED {
        io::ErrorKind::PermissionDenied
    } else if [
        MF_E_HW_MFT_FAILED_START_STREAMING,
        MF_E_VIDEO_RECORDING_DEVICE_PREEMPTED,
        win32(ERROR_SHARING_VIOLATION),
        win32(ERROR_BUSY),
    ]
    .contains(&code)
    {
        io::ErrorKind::ResourceBusy
    } else if code == MF_E_VIDEO_RECORDING_DEVICE_INVALIDATED {
        io::ErrorKind::NotFound
    } else {
        io::ErrorKind::Other
    };
    io::Error::new(kind, format!("{} ({:#010x})", err.message(), code.0 as u32))
}

fn attributes(size: u32) -> io::Result<IMFAttributes> {
    let mut attributes = std::ptr::null_mut();
    // SAFETY: a valid out pointer; the object comes back with a reference
    // that `from_raw` takes over.
    unsafe {
        (api()?.create_attributes)(&mut attributes, size).ok().map_err(error)?;
        Ok(IMFAttributes::from_raw(attributes))
    }
}

fn media_type() -> io::Result<IMFMediaType> {
    let mut media = std::ptr::null_mut();
    // SAFETY: as in `attributes`.
    unsafe {
        (api()?.create_media_type)(&mut media).ok().map_err(error)?;
        Ok(IMFMediaType::from_raw(media))
    }
}

/// Media Foundation's few plain functions, looked up when first needed
/// rather than linked: Windows N editions without the Media Feature Pack
/// (and some Server installs) have no mf.dll, and a linked one would stop
/// the app from starting at all. Without them there is no camera.
struct Api {
    startup: Startup,
    shutdown: Shutdown,
    create_attributes: CreateAttributes,
    create_media_type: CreateMediaType,
    enum_device_sources: EnumDeviceSources,
    create_source_reader: CreateSourceReader,
}

type Startup = unsafe extern "system" fn(u32, u32) -> HRESULT;
type Shutdown = unsafe extern "system" fn() -> HRESULT;
type CreateAttributes = unsafe extern "system" fn(*mut *mut c_void, u32) -> HRESULT;
type CreateMediaType = unsafe extern "system" fn(*mut *mut c_void) -> HRESULT;
type EnumDeviceSources = unsafe extern "system" fn(*mut c_void, *mut *mut *mut c_void, *mut u32) -> HRESULT;
type CreateSourceReader = unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> HRESULT;

fn api() -> io::Result<&'static Api> {
    static API: OnceLock<Result<Api, String>> = OnceLock::new();
    API.get_or_init(load)
        .as_ref()
        .map_err(|why| io::Error::new(io::ErrorKind::Unsupported, why.clone()))
}

/// Whether Media Foundation is there, for the self-test's report.
pub(crate) fn status() -> String {
    match api() {
        Ok(_) => "Media Foundation".to_owned(),
        Err(why) => format!("Media Foundation unavailable: {why}"),
    }
}

fn load() -> Result<Api, String> {
    let library = |name: PCWSTR| {
        // SAFETY: a constant library name, from System32 only; the module
        // is never freed (the functions are kept for the app's life).
        unsafe { LoadLibraryExW(name, None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
            .map_err(|err| format!("{} not found ({err}); on Windows N, install the Media Feature Pack", unsafe { name.display() }))
    };
    let (plat, mf, readwrite) = (library(w!("mfplat.dll"))?, library(w!("mf.dll"))?, library(w!("mfreadwrite.dll"))?);
    let find = |module: HMODULE, name: PCSTR| {
        // SAFETY: a constant, NUL-terminated name.
        unsafe { GetProcAddress(module, name) }
            .ok_or_else(|| format!("{} missing", String::from_utf8_lossy(unsafe { name.as_bytes() })))
    };
    // SAFETY: each pointer is the named export, whose signature is the one
    // it is given here (as declared in mfapi.h, mfidl.h, mfreadwrite.h).
    unsafe {
        Ok(Api {
            startup: std::mem::transmute::<FarProc, Startup>(find(plat, s!("MFStartup"))?),
            shutdown: std::mem::transmute::<FarProc, Shutdown>(find(plat, s!("MFShutdown"))?),
            create_attributes: std::mem::transmute::<FarProc, CreateAttributes>(find(plat, s!("MFCreateAttributes"))?),
            create_media_type: std::mem::transmute::<FarProc, CreateMediaType>(find(plat, s!("MFCreateMediaType"))?),
            enum_device_sources: std::mem::transmute::<FarProc, EnumDeviceSources>(find(mf, s!("MFEnumDeviceSources"))?),
            create_source_reader: std::mem::transmute::<FarProc, CreateSourceReader>(find(
                readwrite,
                s!("MFCreateSourceReaderFromMediaSource"),
            )?),
        })
    }
}

type FarProc = unsafe extern "system" fn() -> isize;

/// The system's video capture devices.
fn devices() -> io::Result<Vec<IMFActivate>> {
    let filter = attributes(1)?;
    let mut list: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    // SAFETY: valid GUIDs and out pointers; the array Media Foundation
    // allocates is read within its count, its entries taken (so dropping
    // them releases them) and the array freed with CoTaskMemFree.
    unsafe {
        filter
            .SetGUID(&MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID)
            .map_err(error)?;
        (api()?.enum_device_sources)(filter.as_raw(), (&raw mut list).cast(), &mut count)
            .ok()
            .map_err(error)?;
        let mut out = Vec::with_capacity(count as usize);
        if !list.is_null() {
            for n in 0..count as usize {
                if let Some(device) = (*list.add(n)).take() {
                    out.push(device);
                }
            }
            CoTaskMemFree(Some(list as *const std::ffi::c_void));
        }
        Ok(out)
    }
}

/// A string attribute of a device (its name, its link).
fn string(device: &IMFActivate, key: &GUID) -> Option<String> {
    let mut text = PWSTR::null();
    let mut len = 0u32;
    // SAFETY: out pointers are valid; the string Media Foundation allocates
    // is read once and freed with CoTaskMemFree.
    unsafe {
        device.GetAllocatedString(key, &mut text, &mut len).ok()?;
        let value = text.to_string().ok();
        CoTaskMemFree(Some(text.0 as *const std::ffi::c_void));
        value
    }
}

fn format_of(subtype: &GUID) -> Option<PixelFormat> {
    [
        (MFVideoFormat_NV12, PixelFormat::Nv12),
        (MFVideoFormat_I420, PixelFormat::I420),
        (MFVideoFormat_YUY2, PixelFormat::Yuy2),
        (MFVideoFormat_MJPG, PixelFormat::Mjpg),
    ]
    .into_iter()
    .find_map(|(guid, format)| (guid == *subtype).then_some(format))
}

fn subtype_of(format: PixelFormat) -> GUID {
    match format {
        PixelFormat::Nv12 => MFVideoFormat_NV12,
        PixelFormat::I420 => MFVideoFormat_I420,
        PixelFormat::Yuy2 => MFVideoFormat_YUY2,
        PixelFormat::Mjpg => MFVideoFormat_MJPG,
    }
}

/// Two numbers packed in one attribute (a size, a rate): high, low.
fn pair(media: &IMFMediaType, key: &GUID) -> Option<(u32, u32)> {
    // SAFETY: a plain read of an attribute.
    let packed = unsafe { media.GetUINT64(key) }.ok()?;
    Some(((packed >> 32) as u32, packed as u32))
}

/// A camera open on this thread, reading in its chosen mode.
struct Device {
    id: String,
    name: String,
    source: IMFMediaSource,
    reader: Option<IMFSourceReader>,
    output: PixelFormat,
    width: u32,
    height: u32,
    _open: OpenGuard,
}

impl Device {
    /// The camera with link `wanted` if it opens, else the first that does.
    fn open(wanted: Option<&str>) -> io::Result<Self> {
        let mut all = devices()?;
        if all.is_empty() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no camera found"));
        }
        if let Some(wanted) = wanted
            && let Some(at) = all
                .iter()
                .position(|d| string(d, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK).as_deref() == Some(wanted))
        {
            all[..=at].rotate_right(1);
        }
        let mut first_error = None;
        for device in &all {
            match Self::activate(device) {
                Ok(open) => return Ok(open),
                Err(err) => {
                    warn!(%err, "a camera did not open");
                    first_error.get_or_insert(err);
                }
            }
        }
        Err(first_error.unwrap_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no camera found")))
    }

    /// This camera again (by its link, or its name if it came back on
    /// another port), and no other.
    fn open_same(id: &str, name: &str) -> io::Result<Self> {
        let all = devices()?;
        let link = |d: &IMFActivate| string(d, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK);
        let same = all
            .iter()
            .find(|d| link(d).as_deref() == Some(id))
            .or_else(|| {
                all.iter()
                    .find(|d| string(d, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME).as_deref() == Some(name))
            })
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "camera not back"))?;
        Self::activate(same)
    }

    fn activate(device: &IMFActivate) -> io::Result<Self> {
        let id = string(device, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK).unwrap_or_default();
        let name = string(device, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME).unwrap_or_else(|| "Camera".into());
        // SAFETY: a COM call with no pointers of ours.
        let source: IMFMediaSource = unsafe { device.ActivateObject() }.map_err(error)?;
        match Self::read_from(&source, &name) {
            Ok((reader, choice, output, width, height)) => {
                let info = CameraInfo {
                    name: name.clone(),
                    native: choice.native,
                    output,
                    width,
                    height,
                    fps: choice.fps,
                    backend: "Media Foundation",
                };
                info!(camera = %info, "camera open");
                crate::diag::note_camera(Some(info.to_string()));
                Ok(Self {
                    id,
                    name,
                    source,
                    reader: Some(reader),
                    output,
                    width,
                    height,
                    _open: OpenGuard::new(),
                })
            }
            Err(err) => {
                // SAFETY: the source made above, not used after this.
                let _ = unsafe { source.Shutdown() };
                Err(err)
            }
        }
    }

    /// A source reader on `source` in the mode [`choose_mode`] takes.
    fn read_from(
        source: &IMFMediaSource,
        name: &str,
    ) -> io::Result<(IMFSourceReader, Choice, PixelFormat, u32, u32)> {
        let settings = attributes(1)?;
        // SAFETY: COM calls on objects made here, with valid GUIDs; every
        // media type read is owned by its `IMFMediaType`.
        unsafe {
            // A converter where one is needed (a JPEG camera's decoder).
            settings
                .SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)
                .map_err(error)?;
            let mut reader = std::ptr::null_mut();
            (api()?.create_source_reader)(source.as_raw(), settings.as_raw(), &mut reader)
                .ok()
                .map_err(error)?;
            let reader = IMFSourceReader::from_raw(reader);
            let _ = reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false);
            reader.SetStreamSelection(STREAM, true).map_err(error)?;
            let mut types = Vec::new();
            let mut modes = Vec::new();
            for index in 0..1000u32 {
                let Ok(media) = reader.GetNativeMediaType(STREAM, index) else {
                    break;
                };
                let format = media.GetGUID(&MF_MT_SUBTYPE).ok().as_ref().and_then(format_of);
                let size = pair(&media, &MF_MT_FRAME_SIZE);
                let rate = pair(&media, &MF_MT_FRAME_RATE);
                if let (Some(format), Some((width, height))) = (format, size) {
                    let fps = rate.filter(|r| r.1 > 0).map_or(30.0, |(n, d)| n as f32 / d as f32);
                    modes.push(NativeMode { index, format, width, height, fps });
                }
                types.push(media);
            }
            let choice = choose_mode(&modes).ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, format!("{name} gives no picture format Morpheus reads"))
            })?;
            let native = &types[choice.index as usize];
            // The device in that mode, then its pictures in the format read
            // (NV12 if the one wanted cannot be had).
            reader.SetCurrentMediaType(STREAM, None, native).map_err(error)?;
            let mut output = choice.native;
            if choice.output != choice.native {
                let mut set = Err(io::Error::other("no output format"));
                for want in [choice.output, PixelFormat::Nv12, PixelFormat::Yuy2] {
                    let media = media_type()?;
                    media.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(error)?;
                    media.SetGUID(&MF_MT_SUBTYPE, &subtype_of(want)).map_err(error)?;
                    for key in [&MF_MT_FRAME_SIZE, &MF_MT_FRAME_RATE] {
                        if let Ok(value) = native.GetUINT64(key) {
                            media.SetUINT64(key, value).map_err(error)?;
                        }
                    }
                    set = reader.SetCurrentMediaType(STREAM, None, &media).map_err(error);
                    if set.is_ok() {
                        output = want;
                        break;
                    }
                }
                set?;
            }
            let current = reader.GetCurrentMediaType(STREAM).map_err(error)?;
            let (width, height) = pair(&current, &MF_MT_FRAME_SIZE).unwrap_or((choice.width, choice.height));
            Ok((reader, choice, output, width, height))
        }
    }

    /// The next picture as I420 (`None` for a gap in the stream).
    fn read(&mut self) -> io::Result<Option<I420Buffer>> {
        let reader = self.reader.as_ref().ok_or_else(|| io::Error::other("closed"))?;
        let mut flags = 0u32;
        let mut sample = None;
        // SAFETY: valid out pointers; the buffer is read only while locked,
        // within the length Media Foundation gives.
        unsafe {
            reader
                .ReadSample(STREAM, 0, None, Some(&mut flags), None, Some(&mut sample))
                .map_err(error)?;
            let flags = flags as i32;
            if flags & (MF_SOURCE_READERF_ERROR.0 | MF_SOURCE_READERF_ENDOFSTREAM.0) != 0 {
                return Err(io::Error::new(io::ErrorKind::NotFound, "the camera stopped"));
            }
            if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 != 0
                && let Ok(current) = reader.GetCurrentMediaType(STREAM)
                && let Some((width, height)) = pair(&current, &MF_MT_FRAME_SIZE)
            {
                (self.width, self.height) = (width, height);
            }
            let Some(sample) = sample else {
                return Ok(None);
            };
            let buffer = sample.ConvertToContiguousBuffer().map_err(error)?;
            let mut data = std::ptr::null_mut();
            let mut len = 0u32;
            buffer.Lock(&mut data, None, Some(&mut len)).map_err(error)?;
            let picture = if data.is_null() {
                None
            } else {
                let bytes = std::slice::from_raw_parts(data, len as usize);
                to_i420(self.output, bytes, self.width, self.height)
            };
            let _ = buffer.Unlock();
            Ok(picture)
        }
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        drop(self.reader.take());
        // SAFETY: the source this device made; nothing uses it after this.
        // Shutting it down is what lets the camera go (its light off).
        let _ = unsafe { self.source.Shutdown() };
        crate::diag::note_camera(None);
    }
}

/// The cameras, by symbolic link and name. Listed on a thread of its own
/// (the caller's may have COM set up otherwise).
pub(crate) fn list() -> Vec<crate::imp::CameraDevice> {
    let listed = std::thread::Builder::new()
        .name("morpheus-cameras".into())
        .spawn(|| {
            let _com = Com::init();
            let Ok(_mf) = MediaFoundation::start() else {
                return Vec::new();
            };
            devices()
                .unwrap_or_default()
                .iter()
                .filter_map(|device| {
                    let id = string(device, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK)?;
                    let name = string(device, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME).unwrap_or_else(|| id.clone());
                    Some(crate::imp::CameraDevice { id, name })
                })
                .collect()
        });
    listed.ok().and_then(|thread| thread.join().ok()).unwrap_or_default()
}
