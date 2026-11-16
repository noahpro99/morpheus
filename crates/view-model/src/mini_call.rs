//! The floating mini call window (issue #86): shown over the app while the
//! call view is out of sight and someone in your call has a camera or a
//! shared screen on. Which picture it shows ([`MiniPick`], fed the time so
//! its hold is testable) and where it sits and how big ([`MiniPlace`], kept
//! on this device). No toolkit in here.

use std::path::Path;

use morpheus_core::OwnedUserId;
use serde_json::{Value, json};

use crate::calls::VideoFeed;

/// How long a speaker's camera stays after they stop speaking, and the least
/// time it is shown before another speaker takes over: long enough that a
/// pause between words does not flicker back to the share.
pub const SPEAKER_HOLD_MS: u64 = 1_750;

/// The speaker shown, since when, and when they were last heard speaking.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Speaker {
    key: String,
    user: OwnedUserId,
    since: u64,
    last_spoke: u64,
    /// Speaking when last asked: the next ask that finds them quiet is when
    /// they stopped (the app asks again whenever someone stops speaking).
    talking: bool,
}

/// Which feed the mini window shows, as time goes by.
///
/// - a card focused in the call view stays shown (a person's card shows
///   their camera);
/// - otherwise someone with their camera on who is speaking (not you), held
///   [`SPEAKER_HOLD_MS`] after they stop;
/// - otherwise the default: someone else's shared screen, else their camera;
/// - never your own share; your own camera only when nothing else is on
///   (a focus on it included);
/// - no feed left: nothing, the window closes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MiniPick {
    speaker: Option<Speaker>,
}

impl MiniPick {
    /// The key of the feed to show at `now_ms`, given the call view's
    /// `focus`, the call's `feeds`, who is `speaking` and who `me` is.
    pub fn pick(
        &mut self,
        focus: Option<&str>,
        feeds: &[VideoFeed],
        speaking: &[OwnedUserId],
        me: Option<&OwnedUserId>,
        now_ms: u64,
    ) -> Option<String> {
        // Never your own share (sharing the whole screen, the window would
        // show the app inside itself, and streamer mode is about what the
        // share shows); your own camera only when nothing else is on.
        let mine = |f: &VideoFeed| Some(&f.user) == me;
        let others: Vec<VideoFeed> = feeds.iter().filter(|f| !mine(f)).cloned().collect();
        let feeds: Vec<VideoFeed> = if others.is_empty() {
            feeds.iter().filter(|f| !f.screen).cloned().collect()
        } else {
            others
        };
        let feeds = feeds.as_slice();
        if feeds.is_empty() {
            self.speaker = None;
            return None;
        }
        if let Some(focus) = focus {
            let focused = feeds.iter().find(|f| f.key == focus).or_else(|| {
                feeds
                    .iter()
                    .find(|f| !f.screen && f.user.as_str() == focus)
            });
            if let Some(feed) = focused {
                self.speaker = None;
                return Some(feed.key.clone());
            }
        }
        // A speaker's camera that went is no longer shown.
        if self
            .speaker
            .as_ref()
            .is_some_and(|s| !feeds.iter().any(|f| f.key == s.key))
        {
            self.speaker = None;
        }
        if let Some(s) = &mut self.speaker {
            let now_talking = speaking.contains(&s.user);
            if now_talking || s.talking {
                s.last_spoke = now_ms;
            }
            s.talking = now_talking;
        }
        let current = self.speaker.as_ref().map(|s| s.user.clone());
        let candidate = feeds.iter().find(|f| {
            !f.screen
                && Some(&f.user) != me
                && Some(&f.user) != current.as_ref()
                && speaking.contains(&f.user)
        });
        let take = |feed: &VideoFeed| Speaker {
            key: feed.key.clone(),
            user: feed.user.clone(),
            since: now_ms,
            last_spoke: now_ms,
            talking: true,
        };
        self.speaker = match (self.speaker.take(), candidate) {
            (None, Some(feed)) => Some(take(feed)),
            (None, None) => None,
            (Some(s), candidate) => {
                let talking = s.talking;
                let held = now_ms.saturating_sub(s.last_spoke) < SPEAKER_HOLD_MS;
                let settled = now_ms.saturating_sub(s.since) >= SPEAKER_HOLD_MS;
                match candidate {
                    // Someone else speaks: they take over once this one has
                    // been up a while and is quiet.
                    Some(feed) if !talking && (settled || !held) => Some(take(feed)),
                    _ if talking || held => Some(s),
                    _ => None,
                }
            }
        };
        if let Some(s) = &self.speaker {
            return Some(s.key.clone());
        }
        let others = |screen: bool| {
            feeds
                .iter()
                .find(|f| f.screen == screen && Some(&f.user) != me)
                .or_else(|| feeds.iter().find(|f| f.screen == screen))
        };
        others(true).or_else(|| others(false)).map(|f| f.key.clone())
    }

    /// How long from `now_ms` until what is shown may change by itself (a
    /// speaker's hold running out), so the window can be redrawn then.
    pub fn next_change(&self, now_ms: u64) -> Option<u64> {
        let s = self.speaker.as_ref()?;
        let ends = s.last_spoke.max(s.since) + SPEAKER_HOLD_MS;
        Some(ends.saturating_sub(now_ms).max(1))
    }
}

/// Which edge of the app window the mini window sits against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Side {
    Left,
    #[default]
    Right,
}

impl Side {
    pub fn key(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
        }
    }
}

/// The mini window's shape: 16:9, like a screen or a camera.
pub const MINI_RATIO: f32 = 16.0 / 9.0;
/// Its narrowest and widest.
pub const MINI_MIN_W: f32 = 240.0;
pub const MINI_MAX_W: f32 = 720.0;
/// Its gap from the window's edges.
pub const MINI_MARGIN: f32 = 16.0;
/// Its default width and distance from the top of the window.
pub const MINI_DEFAULT_W: f32 = 320.0;
pub const MINI_DEFAULT_TOP: f32 = 96.0;

const FILE: &str = "mini-call.json";

/// Where the mini window sits and how wide it is, kept on this device.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MiniPlace {
    pub side: Side,
    pub width: f32,
    /// From the window's top edge.
    pub top: f32,
}

impl Default for MiniPlace {
    fn default() -> Self {
        Self {
            side: Side::Right,
            width: MINI_DEFAULT_W,
            top: MINI_DEFAULT_TOP,
        }
    }
}

/// The widest the window may be in an app window `window_w` wide: never
/// more than half of it (nor less than the narrowest).
fn widest(window_w: f32) -> f32 {
    (window_w / 2.0).clamp(MINI_MIN_W, MINI_MAX_W)
}

impl MiniPlace {
    pub fn height(&self) -> f32 {
        self.width / MINI_RATIO
    }

    /// As drawn in an app window `window` (width, height) big: its size kept
    /// inside it and its top on screen. Returns (left, top, width, height).
    pub fn bounds(&self, window: (f32, f32)) -> (f32, f32, f32, f32) {
        let w = self.width.clamp(MINI_MIN_W, widest(window.0));
        let h = w / MINI_RATIO;
        let top = self
            .top
            .clamp(MINI_MARGIN, (window.1 - h - MINI_MARGIN).max(MINI_MARGIN));
        let left = match self.side {
            Side::Left => MINI_MARGIN,
            Side::Right => (window.0 - w - MINI_MARGIN).max(0.0),
        };
        (left, top, w, h)
    }

    /// Let go after a drag that left its top-left corner at `left`, `top`:
    /// it snaps to the nearer side edge.
    pub fn dropped(&self, left: f32, top: f32, window: (f32, f32)) -> Self {
        let (_, _, w, h) = self.bounds(window);
        let side = if left + w / 2.0 < window.0 / 2.0 {
            Side::Left
        } else {
            Side::Right
        };
        let top = top.clamp(MINI_MARGIN, (window.1 - h - MINI_MARGIN).max(MINI_MARGIN));
        Self {
            side,
            width: w,
            top,
        }
    }

    /// Its corner handle (the bottom corner away from its edge) dragged
    /// `dx`, `dy` from where the resize started with width `start_w`: wider
    /// the further out the handle goes, in its 16:9 shape.
    pub fn resized(&self, start_w: f32, dx: f32, dy: f32, window: (f32, f32)) -> Self {
        let outward = match self.side {
            Side::Left => dx,
            Side::Right => -dx,
        };
        // The larger of the two moves, the downward one in width terms.
        let by_height = dy * MINI_RATIO;
        let grow = if outward.abs() >= by_height.abs() {
            outward
        } else {
            by_height
        };
        Self {
            width: (start_w + grow).clamp(MINI_MIN_W, widest(window.0)),
            ..*self
        }
    }

    /// Read from `dir` (the default where missing or unreadable).
    pub fn load(dir: &Path) -> Self {
        let kept: Value = std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or(Value::Null);
        let d = Self::default();
        let num = |key: &str, default: f32| {
            kept.get(key)
                .and_then(Value::as_f64)
                .map(|v| v as f32)
                .filter(|v| v.is_finite())
                .unwrap_or(default)
        };
        Self {
            side: match kept.get("side").and_then(Value::as_str) {
                Some("left") => Side::Left,
                Some("right") => Side::Right,
                _ => d.side,
            },
            width: num("width", d.width).clamp(MINI_MIN_W, MINI_MAX_W),
            top: num("top", d.top).max(0.0),
        }
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        let json = json!({ "side": self.side.key(), "width": self.width, "top": self.top });
        std::fs::write(dir.join(FILE), json.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(name: &str) -> OwnedUserId {
        OwnedUserId::try_from(format!("@{name}:x")).expect("user id")
    }

    fn screen(name: &str) -> VideoFeed {
        VideoFeed {
            user: user(name),
            key: format!("{name}-screen"),
            screen: true,
        }
    }

    fn camera(name: &str) -> VideoFeed {
        VideoFeed {
            user: user(name),
            key: format!("{name}-camera"),
            screen: false,
        }
    }

    #[test]
    fn nothing_on_shows_nothing() {
        let mut pick = MiniPick::default();
        assert_eq!(pick.pick(None, &[], &[], None, 0), None);
        assert_eq!(pick.pick(Some("bob-screen"), &[], &[user("bob")], None, 10), None);
    }

    #[test]
    fn a_shared_screen_is_shown_by_default_before_cameras() {
        let mut pick = MiniPick::default();
        let feeds = [camera("carol"), screen("bob")];
        assert_eq!(pick.pick(None, &feeds, &[], None, 0).as_deref(), Some("bob-screen"));
    }

    #[test]
    fn someone_elses_feed_comes_before_your_own() {
        let me = user("alice");
        let mut pick = MiniPick::default();
        let feeds = [screen("alice"), screen("bob")];
        assert_eq!(pick.pick(None, &feeds, &[], Some(&me), 0).as_deref(), Some("bob-screen"));
        let feeds = [camera("alice"), camera("bob")];
        assert_eq!(pick.pick(None, &feeds, &[], Some(&me), 0).as_deref(), Some("bob-camera"));
        // Only yours: yours.
        let feeds = [camera("alice")];
        assert_eq!(pick.pick(None, &feeds, &[], Some(&me), 0).as_deref(), Some("alice-camera"));
    }

    #[test]
    fn your_own_share_is_never_shown() {
        let me = user("alice");
        let mut pick = MiniPick::default();
        // Your share alone: no window (it would show the app inside itself).
        assert_eq!(pick.pick(None, &[screen("alice")], &[], Some(&me), 0), None);
        assert_eq!(pick.pick(Some("alice-screen"), &[screen("alice")], &[], Some(&me), 0), None);
        // Focused in the call view: not here either.
        let feeds = [screen("alice"), camera("bob")];
        assert_eq!(
            pick.pick(Some("alice-screen"), &feeds, &[], Some(&me), 0).as_deref(),
            Some("bob-camera")
        );
        // With your camera: the camera.
        let feeds = [screen("alice"), camera("alice")];
        assert_eq!(pick.pick(None, &feeds, &[], Some(&me), 0).as_deref(), Some("alice-camera"));
    }

    #[test]
    fn your_own_camera_shows_only_when_nothing_else_is_on() {
        let me = user("alice");
        let mut pick = MiniPick::default();
        let feeds = [camera("alice"), camera("bob")];
        assert_eq!(
            pick.pick(Some("alice-camera"), &feeds, &[], Some(&me), 0).as_deref(),
            Some("bob-camera")
        );
        assert_eq!(
            pick.pick(Some("@alice:x"), &feeds, &[], Some(&me), 0).as_deref(),
            Some("bob-camera")
        );
        let feeds = [camera("alice")];
        assert_eq!(
            pick.pick(Some("alice-camera"), &feeds, &[], Some(&me), 0).as_deref(),
            Some("alice-camera")
        );
    }

    #[test]
    fn a_focused_tile_stays_whoever_speaks() {
        let mut pick = MiniPick::default();
        let feeds = [screen("bob"), camera("carol"), camera("dave")];
        let speaking = [user("dave")];
        assert_eq!(
            pick.pick(Some("carol-camera"), &feeds, &speaking, None, 0).as_deref(),
            Some("carol-camera")
        );
        assert_eq!(
            pick.pick(Some("carol-camera"), &feeds, &speaking, None, 5_000).as_deref(),
            Some("carol-camera")
        );
    }

    #[test]
    fn a_focused_persons_card_shows_their_camera() {
        let mut pick = MiniPick::default();
        let feeds = [screen("bob"), camera("carol")];
        assert_eq!(
            pick.pick(Some("@carol:x"), &feeds, &[], None, 0).as_deref(),
            Some("carol-camera")
        );
    }

    #[test]
    fn a_focus_on_something_gone_falls_back_to_the_default() {
        let mut pick = MiniPick::default();
        let feeds = [screen("bob")];
        assert_eq!(pick.pick(Some("@carol:x"), &feeds, &[], None, 0).as_deref(), Some("bob-screen"));
        assert_eq!(pick.pick(Some("gone"), &feeds, &[], None, 0).as_deref(), Some("bob-screen"));
    }

    #[test]
    fn a_speaker_on_camera_is_shown_then_held_then_back_to_the_share() {
        let mut pick = MiniPick::default();
        let feeds = [screen("bob"), camera("carol")];
        let carol = [user("carol")];
        assert_eq!(pick.pick(None, &feeds, &[], None, 0).as_deref(), Some("bob-screen"));
        assert_eq!(pick.pick(None, &feeds, &carol, None, 1_000).as_deref(), Some("carol-camera"));
        assert_eq!(pick.pick(None, &feeds, &carol, None, 3_000).as_deref(), Some("carol-camera"));
        // A pause between words: still her.
        assert_eq!(pick.pick(None, &feeds, &[], None, 3_200).as_deref(), Some("carol-camera"));
        assert_eq!(pick.pick(None, &feeds, &carol, None, 3_400).as_deref(), Some("carol-camera"));
        // Quiet from 3.5 s: held, then the share again.
        assert_eq!(pick.pick(None, &feeds, &[], None, 3_500).as_deref(), Some("carol-camera"));
        assert_eq!(
            pick.pick(None, &feeds, &[], None, 3_500 + SPEAKER_HOLD_MS - 1).as_deref(),
            Some("carol-camera")
        );
        assert_eq!(
            pick.pick(None, &feeds, &[], None, 3_500 + SPEAKER_HOLD_MS).as_deref(),
            Some("bob-screen")
        );
    }

    #[test]
    fn the_hold_counts_from_when_a_long_talk_stopped() {
        let mut pick = MiniPick::default();
        let feeds = [screen("bob"), camera("carol")];
        let carol = [user("carol")];
        assert_eq!(pick.pick(None, &feeds, &carol, None, 0).as_deref(), Some("carol-camera"));
        // Not asked again until she stops, ten seconds on: held from then.
        assert_eq!(pick.pick(None, &feeds, &[], None, 10_000).as_deref(), Some("carol-camera"));
        assert_eq!(
            pick.pick(None, &feeds, &[], None, 10_000 + SPEAKER_HOLD_MS).as_deref(),
            Some("bob-screen")
        );
    }

    #[test]
    fn a_speaker_without_a_camera_or_you_do_not_switch() {
        let me = user("carol");
        let mut pick = MiniPick::default();
        let feeds = [screen("bob"), camera("carol")];
        assert_eq!(
            pick.pick(None, &feeds, &[user("dave")], Some(&me), 0).as_deref(),
            Some("bob-screen")
        );
        assert_eq!(
            pick.pick(None, &feeds, &[user("carol")], Some(&me), 100).as_deref(),
            Some("bob-screen")
        );
        // Bob speaking shows his screen still (his camera is off).
        assert_eq!(
            pick.pick(None, &feeds, &[user("bob")], Some(&me), 200).as_deref(),
            Some("bob-screen")
        );
    }

    #[test]
    fn another_speaker_takes_over_only_once_the_first_is_quiet_and_has_been_up_a_while() {
        let mut pick = MiniPick::default();
        let feeds = [screen("bob"), camera("carol"), camera("dave")];
        let (carol, dave, both) = ([user("carol")], [user("dave")], [user("carol"), user("dave")]);
        assert_eq!(pick.pick(None, &feeds, &carol, None, 0).as_deref(), Some("carol-camera"));
        // Both speak: the one up stays.
        assert_eq!(pick.pick(None, &feeds, &both, None, 500).as_deref(), Some("carol-camera"));
        // Carol pauses at once: too soon to switch.
        assert_eq!(pick.pick(None, &feeds, &dave, None, 600).as_deref(), Some("carol-camera"));
        // She has been up long enough and is quiet: Dave.
        assert_eq!(
            pick.pick(None, &feeds, &dave, None, SPEAKER_HOLD_MS).as_deref(),
            Some("dave-camera")
        );
    }

    #[test]
    fn a_speakers_camera_turned_off_goes_back_to_the_share_at_once() {
        let mut pick = MiniPick::default();
        let carol = [user("carol")];
        let feeds = [screen("bob"), camera("carol")];
        assert_eq!(pick.pick(None, &feeds, &carol, None, 0).as_deref(), Some("carol-camera"));
        let feeds = [screen("bob")];
        assert_eq!(pick.pick(None, &feeds, &carol, None, 100).as_deref(), Some("bob-screen"));
    }

    #[test]
    fn the_last_feed_gone_closes_it() {
        let mut pick = MiniPick::default();
        let feeds = [screen("bob")];
        assert!(pick.pick(None, &feeds, &[], None, 0).is_some());
        assert_eq!(pick.pick(None, &[], &[], None, 100), None);
    }

    #[test]
    fn a_hold_says_when_it_runs_out() {
        let mut pick = MiniPick::default();
        let feeds = [screen("bob"), camera("carol")];
        assert_eq!(pick.next_change(0), None);
        pick.pick(None, &feeds, &[user("carol")], None, 1_000);
        assert_eq!(pick.next_change(1_000), Some(SPEAKER_HOLD_MS));
        // She stops at 1.2 s: the hold runs from then.
        pick.pick(None, &feeds, &[], None, 1_200);
        assert_eq!(pick.next_change(1_500), Some(SPEAKER_HOLD_MS - 300));
        pick.pick(None, &feeds, &[], None, 1_200 + SPEAKER_HOLD_MS);
        assert_eq!(pick.next_change(1_200 + SPEAKER_HOLD_MS), None);
    }

    const WINDOW: (f32, f32) = (1280.0, 800.0);

    #[test]
    fn it_starts_on_the_right_near_the_top() {
        let (left, top, w, h) = MiniPlace::default().bounds(WINDOW);
        assert_eq!((w, h), (MINI_DEFAULT_W, MINI_DEFAULT_W / MINI_RATIO));
        assert_eq!(left, WINDOW.0 - w - MINI_MARGIN);
        assert_eq!(top, MINI_DEFAULT_TOP);
    }

    #[test]
    fn dropped_it_snaps_to_the_nearer_side() {
        let place = MiniPlace::default();
        let left = place.dropped(300.0, 400.0, WINDOW);
        assert_eq!(left.side, Side::Left);
        assert_eq!(left.top, 400.0);
        assert_eq!(left.bounds(WINDOW).0, MINI_MARGIN);
        let right = left.dropped(800.0, 200.0, WINDOW);
        assert_eq!(right.side, Side::Right);
        assert_eq!(right.bounds(WINDOW).0, WINDOW.0 - right.width - MINI_MARGIN);
    }

    #[test]
    fn it_stays_inside_the_window() {
        let place = MiniPlace::default().dropped(0.0, 5_000.0, WINDOW);
        let (_, top, _, h) = place.bounds(WINDOW);
        assert_eq!(top, WINDOW.1 - h - MINI_MARGIN);
        let place = MiniPlace::default().dropped(0.0, -50.0, WINDOW);
        assert_eq!(place.bounds(WINDOW).1, MINI_MARGIN);
        // A window shrunk later: the window is narrower too, and on screen.
        let wide = MiniPlace { width: 700.0, top: 700.0, ..MiniPlace::default() };
        let (left, top, w, h) = wide.bounds((800.0, 500.0));
        assert_eq!(w, 400.0);
        assert!(left >= 0.0 && top + h <= 500.0);
    }

    #[test]
    fn its_corner_resizes_it_outward_in_shape_within_limits() {
        let right = MiniPlace::default();
        // On the right the handle is at its bottom left: leftward is wider.
        assert_eq!(right.resized(320.0, -80.0, 0.0, WINDOW).width, 400.0);
        assert_eq!(right.resized(320.0, 80.0, 0.0, WINDOW).width, 240.0);
        let left = MiniPlace { side: Side::Left, ..right };
        assert_eq!(left.resized(320.0, 80.0, 0.0, WINDOW).width, 400.0);
        // Down grows it by the height's share of the width.
        assert_eq!(left.resized(320.0, 0.0, 45.0, WINDOW).width, 400.0);
        assert_eq!(left.resized(320.0, -500.0, 0.0, WINDOW).width, MINI_MIN_W);
        assert_eq!(left.resized(320.0, 5_000.0, 0.0, WINDOW).width, WINDOW.0 / 2.0);
        assert_eq!(left.resized(320.0, 0.0, 45.0, WINDOW).height(), 400.0 / MINI_RATIO);
    }

    #[test]
    fn its_place_is_kept_on_the_device() {
        let dir = std::env::temp_dir().join(format!("morpheus-mini-call-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        assert_eq!(MiniPlace::load(&dir), MiniPlace::default());
        let place = MiniPlace { side: Side::Left, width: 400.0, top: 250.0 };
        place.save(&dir).expect("save");
        assert_eq!(MiniPlace::load(&dir), place);
        std::fs::write(dir.join(FILE), "{ not json").expect("write");
        assert_eq!(MiniPlace::load(&dir), MiniPlace::default());
        std::fs::remove_dir_all(&dir).ok();
    }
}
