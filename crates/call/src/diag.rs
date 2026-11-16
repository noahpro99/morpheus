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

/// The camera open now, if any: "name, 1280x720 at 30 fps, MJPG decoded to
/// NV12 (Media Foundation)".
pub fn camera_info() -> Option<String> {
    CAMERA.lock().ok()?.clone()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_never_stays_in_the_url() {
        begin("wss://sfu.example/rtc?access_token=SECRET#x");
        assert_eq!(call_diag().expect("a call").url, "wss://sfu.example/rtc");
        end();
        assert!(call_diag().is_none());
    }
}
