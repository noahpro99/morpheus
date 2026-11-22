//! Test runs can record what a call delivers (`MORPHEUS_CALL_RECORD=<prefix>`):
//! `<prefix>.pcm` is the decoded voice as it is played (48 kHz mono s16le),
//! and `<prefix>.json` what WebRTC says about it (concealed samples, jitter
//! buffer, loss) plus the arrival times of the 10 ms frames, rewritten each
//! second. The two-client voice e2e (#332) reads them.

use std::{io::Write, path::PathBuf, time::Instant};

pub struct Recorder {
    pcm: std::fs::File,
    prefix: PathBuf,
    last: Option<Instant>,
    /// Frames that came more than 25 ms after the one before (a gap).
    pub late_frames: u64,
    pub frames: u64,
    pub longest_gap_ms: f64,
}

impl Recorder {
    pub fn from_env() -> Option<Self> {
        let prefix = PathBuf::from(std::env::var_os("MORPHEUS_CALL_RECORD")?);
        let pcm = std::fs::File::create(prefix.with_extension("pcm")).ok()?;
        Some(Self { pcm, prefix, last: None, late_frames: 0, frames: 0, longest_gap_ms: 0.0 })
    }

    pub fn frame(&mut self, samples: &[i16]) {
        let now = Instant::now();
        if let Some(last) = self.last {
            let gap = now.duration_since(last).as_secs_f64() * 1000.0;
            self.longest_gap_ms = self.longest_gap_ms.max(gap);
            if gap > 25.0 {
                self.late_frames += 1;
            }
        }
        self.last = Some(now);
        self.frames += 1;
        note_frames(self.frames);
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let _ = self.pcm.write_all(&bytes);
    }

    pub fn stats(&self, webrtc: serde_json::Value) {
        let json = serde_json::json!({
            "frames": self.frames,
            "late_frames": self.late_frames,
            "longest_gap_ms": self.longest_gap_ms,
            "webrtc": webrtc,
        });
        let _ = std::fs::write(self.prefix.with_extension("json"), json.to_string());
    }
}

static FRAMES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// `measure` says how many frames of the voice it has recorded so far.
pub fn note_frames(n: u64) {
    FRAMES.store(n, std::sync::atomic::Ordering::Relaxed);
}

/// The speaking detector's verdicts, as `<frames recorded so far> <0|1>`
/// lines in `<prefix>.speaking`, when a change comes.
pub fn note_speaking(who: &str, speaking: bool) {
    use std::sync::Mutex;
    static LAST: Mutex<Option<bool>> = Mutex::new(None);
    let Some(prefix) = std::env::var_os("MORPHEUS_CALL_RECORD") else {
        return;
    };
    let _ = (who, START.get_or_init(Instant::now));
    let Ok(mut last) = LAST.lock() else { return };
    if *last == Some(speaking) {
        return;
    }
    *last = Some(speaking);
    let path = PathBuf::from(prefix).with_extension("speaking");
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{} {}", FRAMES.load(std::sync::atomic::Ordering::Relaxed), u8::from(speaking));
    }
}
