//! The camera preview in Settings → Voice & Video: opens the camera chosen
//! and shows it under [`PREVIEW_CAMERA`] until stopped. A call with the
//! camera on has the device already, so the preview then shows the call's
//! own feed ([`crate::LOCAL_CAMERA`]) and opens nothing; a call turning its
//! camera on while the preview has it lets the preview's hold go first (the
//! device takes one user) and the preview carries on with the call's feed.
//!
//! Test runs have the test camera, and count how often it is opened
//! ([`camera_opens`]); `MORPHEUS_CALL_CAMERA_FAIL` = `none`, `denied` or
//! `busy` makes opening it fail so the messages can be seen.

use std::{
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use crate::imp::{AudioMode, runtime, video};

/// Where the preview's own camera shows (its picture is mirrored by the viewer).
pub const PREVIEW_CAMERA: &str = "preview/camera";

/// Why the camera could not be opened, in words for a person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewError {
    NoCamera,
    Denied,
    Busy,
    Other(String),
}

impl std::fmt::Display for PreviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCamera => f.write_str("No camera found. Plug one in and try again."),
            Self::Denied => f.write_str(
                "Morpheus is not allowed to use the camera. Allow it in your system's privacy settings.",
            ),
            Self::Busy => f.write_str(
                "The camera is in use by another app. Close it there and try again.",
            ),
            Self::Other(why) => write!(f, "Could not open the camera: {why}"),
        }
    }
}

impl PreviewError {
    pub(crate) fn from_io(err: &std::io::Error) -> Self {
        match err.kind() {
            std::io::ErrorKind::NotFound => Self::NoCamera,
            std::io::ErrorKind::PermissionDenied => Self::Denied,
            std::io::ErrorKind::ResourceBusy => Self::Busy,
            _ if err.raw_os_error() == Some(16) => Self::Busy,
            _ => Self::Other(err.to_string()),
        }
    }
}

static OPEN: AtomicUsize = AtomicUsize::new(0);
static OPENED: AtomicUsize = AtomicUsize::new(0);

/// A camera device held open: counted while it lives.
pub(crate) struct OpenGuard;

impl OpenGuard {
    pub(crate) fn new() -> Self {
        OPEN.fetch_add(1, Ordering::SeqCst);
        OPENED.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for OpenGuard {
    fn drop(&mut self) {
        OPEN.fetch_sub(1, Ordering::SeqCst);
    }
}

/// How many times the camera is open now, and how many times it has been
/// opened since the app started (a preview or a call each open it).
pub fn camera_opens() -> (usize, usize) {
    (OPEN.load(Ordering::SeqCst), OPENED.load(Ordering::SeqCst))
}

struct Own {
    stop: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<()>,
    camera: String,
}

enum State {
    Off,
    Own(Own),
    /// Showing the call's camera.
    Call,
}

static STATE: Mutex<State> = Mutex::new(State::Off);
static CALL_CAMERA: AtomicBool = AtomicBool::new(false);
static IN_USE: Mutex<Option<String>> = Mutex::new(None);

/// A call's hold on the camera (made before the device is opened); the
/// camera is the call's until this drops.
pub(crate) struct CallClaim;

impl CallClaim {
    /// Takes the camera for a call: a preview that has it lets go first.
    pub(crate) async fn take() -> Self {
        CALL_CAMERA.store(true, Ordering::SeqCst);
        let own = {
            let Ok(mut state) = STATE.lock() else {
                return Self;
            };
            match std::mem::replace(&mut *state, State::Call) {
                State::Own(own) => Some(own),
                State::Off => {
                    *state = State::Off;
                    None
                }
                State::Call => None,
            }
        };
        if let Some(own) = own {
            own.stop.store(true, Ordering::SeqCst);
            let _ = own.task.await;
        }
        Self
    }

    /// Which camera the call opened.
    pub(crate) fn using(&self, camera: String) {
        if let Ok(mut used) = IN_USE.lock() {
            *used = Some(camera);
        }
    }
}

impl Drop for CallClaim {
    fn drop(&mut self) {
        CALL_CAMERA.store(false, Ordering::SeqCst);
        if let Ok(mut used) = IN_USE.lock() {
            *used = None;
        }
    }
}

/// The camera a call is sending from (a device path; in test runs
/// `test-camera-1`), while its camera is on.
pub fn camera_in_use() -> Option<String> {
    IN_USE.lock().ok().and_then(|used| used.clone())
}

/// The camera that is chosen, or the first.
pub(crate) fn test_camera_id() -> String {
    crate::imp::chosen_camera().unwrap_or_else(|| "test-camera-1".to_owned())
}

/// Where the preview's picture is, if it is showing: its own feed, or the
/// call's while the call's camera is on.
pub fn camera_preview_key() -> Option<&'static str> {
    let state = STATE.lock().ok()?;
    match &*state {
        State::Own(_) => Some(PREVIEW_CAMERA),
        State::Call if CALL_CAMERA.load(Ordering::SeqCst) => Some(video::LOCAL_CAMERA),
        _ => None,
    }
}

/// The camera the preview is showing (`None` when it is the call's: then
/// [`camera_in_use`]).
pub fn camera_preview_device() -> Option<String> {
    match &*STATE.lock().ok()? {
        State::Own(own) => Some(own.camera.clone()),
        State::Call => camera_in_use(),
        State::Off => None,
    }
}

fn pretend_failure() -> Option<PreviewError> {
    match std::env::var("MORPHEUS_CALL_CAMERA_FAIL").ok()?.as_str() {
        "none" => Some(PreviewError::NoCamera),
        "denied" => Some(PreviewError::Denied),
        "busy" => Some(PreviewError::Busy),
        _ => None,
    }
}

/// Open the chosen camera and show it (the key it shows under comes back;
/// see [`camera_preview_key`]). With a call's camera on, shows that.
pub fn camera_preview_start() -> impl Future<Output = Result<&'static str, PreviewError>> + 'static {
    async move {
        runtime()
            .spawn(async move {
                if CALL_CAMERA.load(Ordering::SeqCst) {
                    if let Ok(mut state) = STATE.lock() {
                        *state = State::Call;
                    }
                    return Ok(video::LOCAL_CAMERA);
                }
                if matches!(STATE.lock().as_deref(), Ok(State::Own(_))) {
                    return Ok(PREVIEW_CAMERA);
                }
                let own = open().await?;
                if let Ok(mut state) = STATE.lock() {
                    *state = State::Own(own);
                }
                Ok(PREVIEW_CAMERA)
            })
            .await
            .map_err(|err| PreviewError::Other(err.to_string()))?
    }
}

async fn open() -> Result<Own, PreviewError> {
    use livekit::webrtc::video_source::{VideoResolution, native::NativeVideoSource};
    let stop = Arc::new(AtomicBool::new(false));
    if AudioMode::from_env() == AudioMode::Tone {
        if let Some(why) = pretend_failure() {
            return Err(why);
        }
        let (width, height) =
            video::test_size("MORPHEUS_CALL_CAMERA_SIZE", video::CAMERA_PATTERN);
        let source = NativeVideoSource::new(VideoResolution { width, height }, false);
        let pattern = video::CAMERA_PATTERN.shown_as(PREVIEW_CAMERA);
        let task = tokio::spawn(video::play_test_pattern(
            pattern,
            (width, height, video::FPS as u32),
            source,
            stop.clone(),
        ));
        return Ok(Own {
            stop,
            task,
            camera: test_camera_id(),
        });
    }
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
    {
        let device = tokio::task::spawn_blocking(crate::imp::camera::Camera::open)
            .await
            .map_err(|err| PreviewError::Other(err.to_string()))?
            .map_err(|err| PreviewError::from_io(&err))?;
        let camera = device.id().to_owned();
        let source = NativeVideoSource::new(device.resolution(), false);
        let flag = stop.clone();
        let task = tokio::task::spawn_blocking(move || {
            device.run_blocking(source, flag, PREVIEW_CAMERA);
        });
        Ok(Own { stop, task, camera })
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        let _ = stop;
        Err(PreviewError::NoCamera)
    }
}

/// Stop the preview; the camera is let go when this is done. (A call's
/// camera is not touched.)
pub fn camera_preview_stop() -> impl Future<Output = ()> + 'static {
    async move {
        let _ = runtime()
            .spawn(async move {
                let own = {
                    let Ok(mut state) = STATE.lock() else { return };
                    match std::mem::replace(&mut *state, State::Off) {
                        State::Own(own) => Some(own),
                        _ => None,
                    }
                };
                if let Some(own) = own {
                    own.stop.store(true, Ordering::SeqCst);
                    let _ = own.task.await;
                    video::forget(PREVIEW_CAMERA);
                }
            })
            .await;
    }
}

/// The camera path checked without a window (`MORPHEUS_CAMERA_SELF_TEST`;
/// CI's Windows runner, or by hand on a laptop): the cameras are listed,
/// and with one, it is opened as the preview opens it, pictures are waited
/// for for `seconds`, and it must be let go when stopped. No camera is a
/// pass (there is nothing to open); a camera giving no pictures, or still
/// held after stopping, is a failure.
pub fn camera_self_test(seconds: f32) -> Result<String, String> {
    use std::time::{Duration, Instant};
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
    let backend = crate::imp::camera::status();
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    let backend = "no camera on this system yet".to_owned();
    let cameras = crate::imp::cameras(AudioMode::from_env());
    let mut report = format!("{backend}: {} camera(s)", cameras.len());
    for camera in &cameras {
        report.push_str(&format!("; {} [{}]", camera.name, camera.id));
    }
    if cameras.is_empty() {
        return Ok(report);
    }
    let started = runtime().block_on(camera_preview_start());
    let key = started.map_err(|why| format!("{report}; the preview did not open: {why}"))?;
    let info = crate::diag::camera_info().unwrap_or_else(|| "no camera noted".to_owned());
    let deadline = Instant::now() + Duration::from_secs_f32(seconds.max(0.5));
    let (mut first, mut last, mut size) = (None, 0, (0, 0));
    while Instant::now() < deadline {
        if let Some(frame) = video::latest_frame(key) {
            first.get_or_insert(frame.seq);
            last = frame.seq;
            size = (frame.width, frame.height);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    runtime().block_on(camera_preview_stop());
    let pictures = first.map_or(0, |first| last - first + 1);
    let report = format!(
        "{report}; open: {info}; {pictures} picture(s) shown, {}x{}",
        size.0, size.1
    );
    if camera_opens().0 != 0 {
        return Err(format!("{report}; the camera was not let go"));
    }
    if pictures < 3 {
        return Err(format!("{report}; too few pictures"));
    }
    Ok(format!("{report}; let go"))
}
