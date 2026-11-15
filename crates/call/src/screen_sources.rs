//! What a screen share can show, for a picker of the app's own: the screens
//! and windows here, each with a name and a small picture of it now. Linux
//! has the desktop portal's picker and macOS ScreenCaptureKit's
//! (SCContentSharingPicker, macOS 14 and later), so only Windows lists them
//! ([`platform_source_provider`]); tests bring a provider of their own.
//! Plain data and a trait, so every platform and the browser build have it.

use std::sync::{Arc, Mutex};

/// A whole screen or one window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScreenSourceKind {
    Screen,
    Window,
}

impl ScreenSourceKind {
    /// The word a test and a key use.
    pub fn key(self) -> &'static str {
        match self {
            Self::Screen => "screen",
            Self::Window => "window",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "screen" => Some(Self::Screen),
            "window" => Some(Self::Window),
            _ => None,
        }
    }
}

/// Something to share: libwebrtc's desktop capturer's id for it (a screen's
/// display index, a window's handle on Windows) and what to call it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScreenSource {
    pub kind: ScreenSourceKind,
    pub id: u64,
    pub name: String,
}

impl ScreenSource {
    /// "screen:1", "window:4567": unique among the sources.
    pub fn key(&self) -> String {
        format!("{}:{}", self.kind.key(), self.id)
    }
}

/// A small picture of a source, B, G, R, A, top row first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// Lists what can be shared and pictures it. Called off the interface's
/// thread, about once a second while the picker is open.
pub trait ScreenSourceProvider: Send + Sync {
    /// The screens and windows now, screens first.
    fn sources(&self) -> Vec<ScreenSource>;
    /// `source` now, fitted in `max_width` x `max_height`; nothing when it
    /// cannot be pictured (a minimised window).
    fn thumbnail(
        &self,
        source: &ScreenSource,
        max_width: u32,
        max_height: u32,
    ) -> Option<Thumbnail>;
}

/// This platform's sources, where the app picks them itself: Windows. Linux
/// and macOS ask the system's picker when sharing starts instead.
pub fn platform_source_provider() -> Option<Arc<dyn ScreenSourceProvider>> {
    #[cfg(target_os = "windows")]
    {
        Some(Arc::new(crate::imp::WindowsSources::default()))
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// The largest size within `max_width` x `max_height` with the shape of
/// `width` x `height` (never enlarged, at least 1 x 1).
pub fn fit_thumbnail(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    let (width, height) = (width.max(1), height.max(1));
    let scale = (max_width as f64 / width as f64)
        .min(max_height as f64 / height as f64)
        .min(1.0);
    (
        ((width as f64 * scale).round() as u32).max(1),
        ((height as f64 * scale).round() as u32).max(1),
    )
}

static SHARED: Mutex<Option<ScreenSource>> = Mutex::new(None);

/// What this device's screen share was started from, when it was picked in
/// the app (nothing for the system's pickers, or no share).
pub fn shared_screen_source() -> Option<ScreenSource> {
    SHARED.lock().ok()?.clone()
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn note_shared(source: Option<ScreenSource>) {
    if let Ok(mut now) = SHARED.lock() {
        *now = source;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thumbnail_keeps_the_shape_and_is_never_enlarged() {
        assert_eq!(fit_thumbnail(2560, 1440, 320, 180), (320, 180));
        assert_eq!(fit_thumbnail(1000, 1000, 320, 180), (180, 180));
        assert_eq!(fit_thumbnail(100, 50, 320, 180), (100, 50));
        assert_eq!(fit_thumbnail(0, 0, 320, 180), (1, 1));
    }

    #[test]
    fn keys_name_the_kind_and_id() {
        let s = ScreenSource {
            kind: ScreenSourceKind::Window,
            id: 42,
            name: "x".into(),
        };
        assert_eq!(s.key(), "window:42");
        assert_eq!(
            ScreenSourceKind::from_key("screen"),
            Some(ScreenSourceKind::Screen)
        );
    }
}
