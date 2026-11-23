//! What the call looks like from the network, for Settings > Help &
//! Feedback > Diagnostics and the F3 overlay: the connection (server, state,
//! round trip, loss, jitter, bit rate) and each video track. Filled by the
//! tasks that watch the call's stats; empty outside a call. Values not yet
//! measured are `None`.

use std::sync::{Mutex, atomic::{AtomicU8, Ordering}};

/// One video track.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackDiag {
    /// "camera", "screen" (ours) or "name/sid" (someone's).
    pub label: String,
    /// "sending" or "receiving".
    pub direction: &'static str,
    /// The implementation libwebrtc reports ("VAAPI H264 Encoder", ...).
    pub implementation: String,
    /// "vaapi", "nvenc", "nvdec" or "software".
    pub backend: Option<&'static str>,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub bitrate: Option<f64>,
    pub loss_percent: Option<f64>,
    pub jitter_ms: Option<f64>,
    /// Bytes sent so far on a "sending" track.
    pub bytes_sent: u64,
}

/// The call as measured.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CallDiag {
    /// The LiveKit server, without its query (which carries the token).
    pub url: String,
    pub state: String,
    pub rtt_ms: Option<f64>,
    pub loss_percent: Option<f64>,
    pub jitter_ms: Option<f64>,
    /// What our voice is sent at, bits per second.
    pub audio_bitrate: Option<f64>,
    pub tracks: Vec<TrackDiag>,
    /// The other SFUs listened on (people on other homeservers publish
    /// there), each with its connection state.
    pub others: Vec<OtherSfu>,
}

/// An SFU this device only listens on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OtherSfu {
    /// Its call service, as memberships name it.
    pub service: String,
    /// The LiveKit server, without its query.
    pub url: String,
    pub state: String,
}

static DIAG: Mutex<Option<CallDiag>> = Mutex::new(None);

/// The call in progress, if there is one.
pub fn call_diag() -> Option<CallDiag> {
    DIAG.lock().ok()?.clone()
}

/// A call to `url` begins; the token a URL may carry is dropped.
pub(crate) fn begin(url: &str) {
    let url = bare_url(url);
    if let Ok(mut diag) = DIAG.lock() {
        *diag = Some(CallDiag { url, state: "connecting".into(), ..CallDiag::default() });
    }
}

pub(crate) fn end() {
    if let Ok(mut diag) = DIAG.lock() {
        *diag = None;
    }
}

pub(crate) fn update(change: impl FnOnce(&mut CallDiag)) {
    if let Ok(mut diag) = DIAG.lock()
        && let Some(diag) = diag.as_mut()
    {
        change(diag);
    }
}

/// The bytes our video `label` ("camera", "screen") has sent so far (0 when
/// none, or not measured yet).
pub fn sent_bytes(label: &str) -> u64 {
    call_diag()
        .and_then(|d| d.tracks.into_iter().find(|t| t.label == label && t.direction == "sending"))
        .map_or(0, |t| t.bytes_sent)
}

/// Note a video track's numbers (replacing its earlier ones).
pub(crate) fn note_track(track: TrackDiag) {
    update(|diag| match diag.tracks.iter_mut().find(|t| t.label == track.label) {
        Some(known) => *known = track,
        None => diag.tracks.push(track),
    });
}

/// Note an SFU listened on (replacing what was known of it).
pub(crate) fn note_other(other: OtherSfu) {
    update(|diag| match diag.others.iter_mut().find(|o| o.service == other.service) {
        Some(known) => *known = other,
        None => diag.others.push(other),
    });
}

pub(crate) fn forget_other(service: &str) {
    update(|diag| diag.others.retain(|o| o.service != service));
}

/// A LiveKit URL as shown: without its query (which may carry a token).
pub(crate) fn bare_url(url: &str) -> String {
    url.split(['?', '#']).next().unwrap_or_default().to_owned()
}

pub(crate) fn forget_track(label: &str) {
    update(|diag| diag.tracks.retain(|t| t.label != label));
}

/// 0 unknown, 1 off, 2 on.
static ECHO: AtomicU8 = AtomicU8::new(0);
static GAIN: AtomicU8 = AtomicU8::new(0);

pub(crate) fn note_processing(echo: bool, gain: bool) {
    ECHO.store(1 + u8::from(echo), Ordering::Relaxed);
    GAIN.store(1 + u8::from(gain), Ordering::Relaxed);
}

/// Whether WebRTC's echo cancellation and automatic gain are on for the
/// microphone (None until the microphone has started).
pub fn processing() -> (Option<bool>, Option<bool>) {
    let read = |a: &AtomicU8| match a.load(Ordering::Relaxed) {
        0 => None,
        n => Some(n == 2),
    };
    (read(&ECHO), read(&GAIN))
}


/// The camera open now (for a call or the preview), as Diagnostics shows
/// it: its name, size, speed and format, and what reads it.
static CAMERA: Mutex<Option<String>> = Mutex::new(None);

#[cfg_attr(not(any(target_os = "linux", target_os = "windows", target_os = "macos")), allow(dead_code))]
pub(crate) fn note_camera(info: Option<String>) {
    if let Ok(mut camera) = CAMERA.lock() {
        *camera = info;
    }
}

/// Camera health (#351): how many threads read a camera now, what the last
/// session's frames were like, and which processes hold a device open.
static READERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Widest picture the next camera may open at; lowered when a session's
/// frames mostly came cut short or flagged as errors (a link that cannot
/// carry the mode), so a camera turned off and on does not return to the
/// mode that failed.
static MAX_WIDTH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(u32::MAX);
static LAST_FRAMES: Mutex<(u64, u64)> = Mutex::new((0, 0));
/// The device node the camera open now reads.
static DEVICE: Mutex<Option<String>> = Mutex::new(None);

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn note_camera_device(path: Option<String>) {
    if let Ok(mut device) = DEVICE.lock() {
        *device = path;
    }
}

/// A thread reading a camera, counted while it lives.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) struct ReaderGuard;

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl ReaderGuard {
    pub(crate) fn new() -> Self {
        READERS.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for ReaderGuard {
    fn drop(&mut self) {
        READERS.fetch_sub(1, Ordering::SeqCst);
    }
}

pub(crate) fn camera_readers() -> usize {
    READERS.load(Ordering::SeqCst)
}

/// Wait (up to `limit`) for earlier readers to finish: a camera turned off
/// and on again must not open while the old thread is in its last read.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn wait_for_readers(limit: std::time::Duration) -> usize {
    let end = std::time::Instant::now() + limit;
    while camera_readers() > 0 && std::time::Instant::now() < end {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    camera_readers()
}

/// The widest picture the next camera session may use.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn max_camera_width() -> u32 {
    MAX_WIDTH.load(Ordering::SeqCst)
}

/// A session ended after `total` frames of which `bad` were cut short or
/// flagged as errors. Mostly bad: the next session is capped below `width`.
/// Returns the new cap when one was set.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn note_session(bad: u64, total: u64, width: u32) -> Option<u32> {
    if let Ok(mut last) = LAST_FRAMES.lock() {
        *last = (bad, total);
    }
    if total >= 30 && bad * 2 >= total && width > 640 {
        let cap = width - 1;
        MAX_WIDTH.store(cap, Ordering::SeqCst);
        return Some(cap);
    }
    None
}

#[cfg_attr(not(any(test, target_os = "linux")), allow(dead_code))]
pub(crate) fn reset_camera_cap() {
    MAX_WIDTH.store(u32::MAX, Ordering::SeqCst);
}

/// Processes (pid, name) with `device` open, found through
/// `proc_root`/<pid>/fd (`/proc`).
pub(crate) fn holders_of(proc_root: &std::path::Path, device: &str) -> Vec<(u32, String)> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(proc_root) else {
        return found;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        let Ok(fds) = std::fs::read_dir(entry.path().join("fd")) else {
            continue;
        };
        let holds = fds
            .flatten()
            .filter_map(|fd| std::fs::read_link(fd.path()).ok())
            .any(|target| target == std::path::Path::new(device));
        if holds {
            let name = std::fs::read_to_string(entry.path().join("comm"))
                .map(|c| c.trim().to_owned())
                .unwrap_or_default();
            found.push((pid, name));
        }
    }
    found.sort();
    found
}

/// One line for Diagnostics: readers in this app, the last session's bad
/// frames, the width cap, and who holds `device` (when known).
pub fn camera_health() -> String {
    let device = DEVICE.lock().ok().and_then(|d| d.clone());
    let device = device.as_deref();
    let (bad, total) = LAST_FRAMES.lock().map(|l| *l).unwrap_or((0, 0));
    let mut line = format!("{} reader(s), {bad} of {total} frames bad", camera_readers());
    let cap = MAX_WIDTH.load(Ordering::SeqCst);
    if cap != u32::MAX {
        line.push_str(&format!(", width capped at {cap}"));
    }
    if let Some(device) = device {
        let holders = holders_of(std::path::Path::new("/proc"), device);
        let list: Vec<String> = holders.iter().map(|(p, n)| format!("{n} {p}")).collect();
        let who = if list.is_empty() { "nobody".to_owned() } else { list.join(", ") };
        line.push_str(&format!("; {device} held by {who}"));
    }
    line
}

/// The camera open now, if any: "name, 1280x720 at 30 fps, MJPG decoded to
/// NV12 (Media Foundation)".
pub fn camera_info() -> Option<String> {
    CAMERA.lock().ok()?.clone()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_of_mostly_bad_frames_lowers_the_next_width() {
        reset_camera_cap();
        assert_eq!(note_session(1, 100, 1280), None, "a few bad frames are not it");
        assert_eq!(note_session(60, 100, 1280), Some(1279));
        assert_eq!(max_camera_width(), 1279);
        assert_eq!(note_session(60, 100, 640), None, "nothing smaller than 640 to fall to");
        reset_camera_cap();
    }

    #[test]
    fn processes_holding_a_device_are_found_in_proc() {
        let root = std::env::temp_dir().join(format!("morpheus-proc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (pid, target) in [(11u32, "/dev/video0"), (12, "/dev/video2")] {
            let dir = root.join(pid.to_string());
            std::fs::create_dir_all(dir.join("fd")).expect("dir");
            std::os::unix::fs::symlink(target, dir.join("fd").join("5")).expect("link");
            std::fs::write(dir.join("comm"), "morpheus\n").expect("comm");
        }
        assert_eq!(holders_of(&root, "/dev/video0"), vec![(11, "morpheus".to_owned())]);
        assert!(holders_of(&root, "/dev/video9").is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_token_never_stays_in_the_url() {
        begin("wss://sfu.example/rtc?access_token=SECRET#x");
        assert_eq!(call_diag().expect("a call").url, "wss://sfu.example/rtc");
        end();
        assert!(call_diag().is_none());
    }
}
