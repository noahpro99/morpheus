//! How the screen is shared and what is published, chosen up front like
//! Discord's Go Live: a resolution (720p, 1080p, or the screen's own), a
//! frame rate, and the encoding that goes with them. Plain data and sums, so
//! every platform and the browser build have it.

use std::{
    collections::HashMap,
    sync::Mutex,
};

/// How large the shared screen is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScreenResolution {
    /// At most 1280x720.
    P720,
    /// At most 1920x1080.
    #[default]
    P1080,
    /// The screen's own size.
    Source,
}

impl ScreenResolution {
    pub const ALL: [Self; 3] = [Self::P720, Self::P1080, Self::Source];

    /// The word a saved choice and a test use.
    pub fn key(self) -> &'static str {
        match self {
            Self::P720 => "720",
            Self::P1080 => "1080",
            Self::Source => "source",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.key() == key)
    }

    /// What a button says.
    pub fn label(self) -> &'static str {
        match self {
            Self::P720 => "720p",
            Self::P1080 => "1080p",
            Self::Source => "Source",
        }
    }

    /// The box a picture is fitted in (never enlarged), if there is one.
    fn limit(self) -> Option<(u32, u32)> {
        match self {
            Self::P720 => Some((1280, 720)),
            Self::P1080 => Some((1920, 1080)),
            Self::Source => None,
        }
    }
}

/// Frame rates offered.
pub const SCREEN_FPS: [u32; 3] = [15, 30, 60];

/// The screen share's quality: what the popover offers and keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenQuality {
    pub resolution: ScreenResolution,
    pub fps: u32,
}

impl Default for ScreenQuality {
    fn default() -> Self {
        Self {
            resolution: ScreenResolution::P1080,
            fps: 30,
        }
    }
}

impl ScreenQuality {
    /// The size a `width` x `height` screen is sent at: fitted in the
    /// resolution's box, never enlarged, even (video wants it).
    pub fn fit(self, width: u32, height: u32) -> (u32, u32) {
        let (width, height) = (width.max(2), height.max(2));
        let Some((max_w, max_h)) = self.resolution.limit() else {
            return (width & !1, height & !1);
        };
        if width <= max_w && height <= max_h {
            return (width & !1, height & !1);
        }
        let scale = (max_w as f64 / width as f64).min(max_h as f64 / height as f64);
        let even = |side: u32| (((side as f64 * scale).round() as u32).max(2)) & !1;
        (even(width), even(height))
    }

    /// The most the encoder may spend: bits per pixel for the speed (text
    /// needs more per frame at 30 or less than motion needs at 60), 1 to 20
    /// Mbps. 1080p30 is 5, 1080p60 8, 720p30 2.2, 1440p30 8.8.
    pub fn max_bitrate(self, width: u32, height: u32) -> u64 {
        bitrate_for(width, height, self.fps)
    }

    /// "detail" (keep text sharp) up to 30 frames a second, "motion" above.
    pub fn content_hint(self) -> &'static str {
        if self.fps <= 30 { "detail" } else { "motion" }
    }

    /// Whether the encoder treats this as a screen (sharp, few frames
    /// dropped), which is what "detail" asks of it.
    pub fn is_screencast(self) -> bool {
        self.fps <= 30
    }

    /// Keeps a hand-edited choice sane.
    pub fn normalized(self) -> Self {
        let fps = SCREEN_FPS
            .into_iter()
            .min_by_key(|f| f.abs_diff(self.fps))
            .unwrap_or(30);
        Self { fps, ..self }
    }

    /// The saved form.
    pub fn to_json(self) -> String {
        format!(
            "{{\"resolution\":\"{}\",\"fps\":{}}}",
            self.resolution.key(),
            self.fps
        )
    }

    /// From the saved form; anything unreadable is the default.
    pub fn from_json(json: &str) -> Self {
        let find = |name: &str| -> Option<String> {
            let at = json.find(&format!("\"{name}\""))?;
            let rest = json[at + name.len() + 2..].trim_start().strip_prefix(':')?.trim_start();
            let rest = rest.strip_prefix('"').unwrap_or(rest);
            let end = rest.find(['"', ',', '}']).unwrap_or(rest.len());
            Some(rest[..end].trim().to_owned())
        };
        let resolution = find("resolution")
            .and_then(|k| ScreenResolution::from_key(&k))
            .unwrap_or_default();
        let fps = find("fps").and_then(|f| f.parse().ok()).unwrap_or(30);
        Self { resolution, fps }.normalized()
    }
}

/// The bitrate for `width` x `height` at `fps`.
pub fn bitrate_for(width: u32, height: u32, fps: u32) -> u64 {
    let bits_per_pixel = if fps > 30 { 0.065 } else { 0.08 };
    let rate = width as f64 * height as f64 * fps as f64 * bits_per_pixel;
    (rate as u64).clamp(1_000_000, 20_000_000)
}

/// The camera's: 720p30 is 1.7 Mbps, 1080p30 3 (LiveKit's own presets).
pub fn camera_bitrate(width: u32, height: u32) -> u64 {
    if u64::from(width) * u64::from(height) > 1280 * 720 {
        3_000_000
    } else {
        1_700_000
    }
}

/// What a video track was published with, for the interface to show and
/// the tests to read.
#[derive(Debug, Clone, PartialEq)]
pub struct PublishedVideo {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub max_bitrate: u64,
    /// Encodings sent: 1 is one full-size layer.
    pub layers: usize,
    pub hint: &'static str,
    pub degradation: &'static str,
}

impl std::fmt::Display for PublishedVideo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}x{}@{} bitrate={} simulcast={} layers={} hint={} degradation={}",
            self.width,
            self.height,
            self.fps,
            self.max_bitrate,
            self.layers > 1,
            self.layers,
            self.hint,
            self.degradation
        )
    }
}

static PUBLISHED: Mutex<Option<HashMap<String, PublishedVideo>>> = Mutex::new(None);

pub(crate) fn note_published(key: &str, video: PublishedVideo) {
    if let Ok(mut all) = PUBLISHED.lock() {
        all.get_or_insert_with(HashMap::new).insert(key.to_owned(), video);
    }
}

pub(crate) fn forget_published(key: &str) {
    if let Ok(mut all) = PUBLISHED.lock()
        && let Some(all) = all.as_mut()
    {
        all.remove(key);
    }
}

/// What this device's camera or screen ([`crate::LOCAL_CAMERA`],
/// [`crate::LOCAL_SCREEN`]) was last published with.
pub fn published_video(key: &str) -> Option<PublishedVideo> {
    PUBLISHED.lock().ok()?.as_ref()?.get(key).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(resolution: ScreenResolution, fps: u32) -> ScreenQuality {
        ScreenQuality { resolution, fps }
    }

    #[test]
    fn a_screen_is_fitted_to_the_resolution_and_never_enlarged() {
        assert_eq!(q(ScreenResolution::P720, 30).fit(2560, 1440), (1280, 720));
        assert_eq!(q(ScreenResolution::P1080, 30).fit(2560, 1440), (1920, 1080));
        assert_eq!(q(ScreenResolution::Source, 30).fit(2560, 1440), (2560, 1440));
        assert_eq!(q(ScreenResolution::P1080, 30).fit(1366, 768), (1366, 768));
        // Ultrawide: the width is what limits.
        assert_eq!(q(ScreenResolution::P1080, 30).fit(3440, 1440), (1920, 804));
        // Portrait (a rotated screen or window): the height is.
        assert_eq!(q(ScreenResolution::P720, 30).fit(1080, 1920), (404, 720));
        assert_eq!(q(ScreenResolution::P720, 30).fit(1001, 563), (1000, 562));
    }

    #[test]
    fn bitrates_follow_size_and_speed() {
        let b = |w, h, fps| bitrate_for(w, h, fps);
        assert!((4_500_000..=6_000_000).contains(&b(1920, 1080, 30)));
        assert!((7_500_000..=8_800_000).contains(&b(1920, 1080, 60)));
        assert!(b(2560, 1440, 30) > b(1920, 1080, 30));
        assert!(b(1280, 720, 30) < b(1920, 1080, 30));
        assert_eq!(b(160, 90, 15), 1_000_000);
        assert_eq!(b(7680, 4320, 60), 20_000_000);
    }

    #[test]
    fn the_hint_is_detail_up_to_30_and_motion_at_60() {
        assert_eq!(q(ScreenResolution::P1080, 15).content_hint(), "detail");
        assert_eq!(q(ScreenResolution::P1080, 30).content_hint(), "detail");
        assert_eq!(q(ScreenResolution::P1080, 60).content_hint(), "motion");
    }

    #[test]
    fn the_choice_is_kept_and_read_back() {
        for resolution in ScreenResolution::ALL {
            for fps in SCREEN_FPS {
                let quality = q(resolution, fps);
                assert_eq!(ScreenQuality::from_json(&quality.to_json()), quality);
            }
        }
        assert_eq!(ScreenQuality::from_json("nonsense"), ScreenQuality::default());
        assert_eq!(
            ScreenQuality::from_json("{\"resolution\":\"720\",\"fps\":24}").fps,
            30
        );
    }
}
