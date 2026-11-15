//! What the app holds in memory, cache by cache (issue #10): the process's
//! resident size, the pictures GPUI keeps decoded and in its sprite atlas,
//! the app's own picture caches and the rooms whose timelines are kept. A
//! line of it goes to the log every few minutes, and the e2e bridge's
//! `memory` op returns it, so a test can hold each cache to its bound.

use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

use gpui::{App, Window};

/// Rooms whose timelines are kept (the open one and those just left).
pub static TIMELINES: AtomicUsize = AtomicUsize::new(0);
/// Their weight (entries plus pictures, see `TimelineView::weight`).
pub static TIMELINE_WEIGHT: AtomicUsize = AtomicUsize::new(0);

/// A count and the bytes it holds.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Held {
    pub entries: usize,
    pub bytes: usize,
}

#[derive(Debug, Default, Clone)]
pub struct Report {
    /// Resident set and its anonymous part (heap, GPU buffers mapped in), in KB.
    pub rss_kb: u64,
    pub anon_kb: u64,
    /// Freed pages the kernel may take back at will (MADV_FREE), still counted
    /// in the resident set, in KB.
    pub lazy_free_kb: u64,
    /// Pictures GPUI holds decoded (BGRA) in its asset cache.
    pub gpui_decoded: Held,
    /// The window's sprite atlas.
    pub atlas: gpui::AtlasStats,
    /// Pictures the app has drawn and keeps track of (decoded bytes, estimated).
    pub drawn: Held,
    /// The biggest copy of each picture, shared by every view (encoded bytes).
    pub any_size: Held,
    /// Pictures cut to a shape (encoded bytes).
    pub squared: Held,
    /// Call card colours worked out from avatars.
    pub card_colors: usize,
    /// Blurred stand-ins of message pictures.
    pub blurs: usize,
    /// Rooms whose timelines are kept, and their weight.
    pub timelines: usize,
    pub timeline_weight: usize,
    /// Pictures live videos hold.
    pub video_pictures: usize,
    /// Timeline and member list views alive.
    pub timeline_views: usize,
    pub member_views: usize,
}

/// What is held now; `window` is the main window (for its atlas).
pub fn report(window: &Window, cx: &App) -> Report {
    let (rss_kb, anon_kb) = resident();
    let (decoded, decoded_bytes) = gpui::decoded_image_stats(cx);
    let pictures = crate::media::stats();
    Report {
        rss_kb,
        anon_kb,
        lazy_free_kb: lazy_free(),
        gpui_decoded: Held {
            entries: decoded,
            bytes: decoded_bytes,
        },
        atlas: window.sprite_atlas_stats(),
        drawn: pictures.drawn,
        any_size: pictures.any_size,
        squared: pictures.squared,
        card_colors: pictures.card_colors,
        blurs: crate::message::blur_count(),
        timelines: TIMELINES.load(Ordering::Relaxed),
        timeline_weight: TIMELINE_WEIGHT.load(Ordering::Relaxed),
        video_pictures: crate::pictures::held(),
        timeline_views: TIMELINE_VIEWS.load(Ordering::Relaxed),
        member_views: MEMBER_VIEWS.load(Ordering::Relaxed),
    }
}

/// LazyFree of this process, in KB (0 where /proc is not there).
fn lazy_free() -> u64 {
    std::fs::read_to_string("/proc/self/smaps_rollup")
        .unwrap_or_default()
        .lines()
        .find_map(|line| line.strip_prefix("LazyFree:"))
        .and_then(|v| v.split_whitespace().next()?.parse().ok())
        .unwrap_or(0)
}

/// VmRSS and RssAnon of this process, in KB (0 where /proc is not there).
fn resident() -> (u64, u64) {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |key: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(key))
            .and_then(|v| v.split_whitespace().next()?.parse().ok())
            .unwrap_or(0)
    };
    (field("VmRSS:"), field("RssAnon:"))
}

fn mb(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "rss {:.0} MB (anon {:.0}, lazily freed {:.0}); gpui decoded pictures {} / {:.1} MB; \
             atlas {} textures / {:.1} MB ({} tiles, {} colour, {} retired); drawn {} / {:.1} MB; \
             shared pictures {} / {:.1} MB; cut pictures {} / {:.1} MB; card colours {}; \
             blurs {}; timelines {} (weight {}); video pictures {}; views: {} timelines, {} member lists",
            self.rss_kb as f64 / 1024.0,
            self.anon_kb as f64 / 1024.0,
            self.lazy_free_kb as f64 / 1024.0,
            self.gpui_decoded.entries,
            mb(self.gpui_decoded.bytes),
            self.atlas.textures,
            mb(self.atlas.bytes),
            self.atlas.tiles,
            self.atlas.colour_tiles,
            self.atlas.retired,
            self.drawn.entries,
            mb(self.drawn.bytes),
            self.any_size.entries,
            mb(self.any_size.bytes),
            self.squared.entries,
            mb(self.squared.bytes),
            self.card_colors,
            self.blurs,
            self.timelines,
            self.timeline_weight,
            self.video_pictures,
            self.timeline_views,
            self.member_views,
        )
    }
}

impl Report {
    pub fn json(&self) -> serde_json::Value {
        let held = |h: Held| serde_json::json!({ "entries": h.entries, "bytes": h.bytes });
        serde_json::json!({
            "rss_kb": self.rss_kb,
            "anon_kb": self.anon_kb,
            "lazy_free_kb": self.lazy_free_kb,
            "gpui_decoded": held(self.gpui_decoded),
            "atlas": {
                "tiles": self.atlas.tiles,
                "colour_tiles": self.atlas.colour_tiles,
                "textures": self.atlas.textures,
                "bytes": self.atlas.bytes,
                "retired": self.atlas.retired,
            },
            "drawn": held(self.drawn),
            "any_size": held(self.any_size),
            "squared": held(self.squared),
            "card_colors": self.card_colors,
            "blurs": self.blurs,
            "timelines": self.timelines,
            "timeline_weight": self.timeline_weight,
            "video_pictures": self.video_pictures,
            "timeline_views": self.timeline_views,
            "member_views": self.member_views,
        })
    }
}

thread_local! {
    static LOGGED: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
}

/// How often the memory line goes to the log.
const LOG_EVERY: Duration = Duration::from_secs(300);

/// Logs the report every [`LOG_EVERY`] (called from the shell's render).
pub fn log_now_and_then(window: &Window, cx: &App) {
    let now = Instant::now();
    let due = LOGGED
        .try_with(|at| {
            let due = at.get().is_none_or(|t| now.duration_since(t) >= LOG_EVERY);
            if due {
                at.set(Some(now));
            }
            due
        })
        .unwrap_or(false);
    if due {
        eprintln!("morpheus-desktop: memory: {}", report(window, cx));
    }
}

/// Views alive (to see that those of rooms left go).
pub static TIMELINE_VIEWS: AtomicUsize = AtomicUsize::new(0);
pub static MEMBER_VIEWS: AtomicUsize = AtomicUsize::new(0);

/// Counts itself in a counter while it lives.
pub struct Alive(&'static AtomicUsize);

impl Alive {
    pub fn new(counter: &'static AtomicUsize) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Self(counter)
    }
}

impl Drop for Alive {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}
