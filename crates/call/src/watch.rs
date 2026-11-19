//! Who is watching whose shared screen. Nobody receives a share they did
//! not ask for (the SFU forwards only what is subscribed, and the sharer's
//! encoder pauses when nobody is: dynacast), so each watcher says which
//! shares they watch, in a small data message, every few seconds and at
//! once when it changes; a sharer counts the watchers it heard from lately.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// The topic the messages go under.
pub(crate) const TOPIC: &str = "morpheus.watch";

/// A watcher is forgotten this long after their last message.
pub(crate) const STALE: Duration = Duration::from_secs(12);

/// How often the watchers say again what they watch.
pub(crate) const EVERY: Duration = Duration::from_secs(4);

/// The message: the identities of the sharers watched, comma separated.
pub(crate) fn encode(watching: &HashSet<String>) -> Vec<u8> {
    let mut names: Vec<&str> = watching.iter().map(String::as_str).collect();
    names.sort_unstable();
    names.join("\n").into_bytes()
}

pub(crate) fn decode(payload: &[u8]) -> HashSet<String> {
    String::from_utf8_lossy(payload)
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// What each watcher last said they watch.
#[derive(Default)]
pub(crate) struct Tally {
    heard: HashMap<String, (HashSet<String>, Instant)>,
}

impl Tally {
    pub(crate) fn hear(&mut self, watcher: &str, watching: HashSet<String>, now: Instant) {
        self.heard.insert(watcher.to_owned(), (watching, now));
    }

    pub(crate) fn forget(&mut self, watcher: &str) {
        self.heard.remove(watcher);
    }

    /// How many people watch `sharer`'s screen (the sharer's own preview is
    /// not counted: it says nothing).
    pub(crate) fn watching(&self, sharer: &str, now: Instant) -> usize {
        self.heard
            .iter()
            .filter(|(watcher, (set, at))| {
                watcher.as_str() != sharer
                    && set.contains(sharer)
                    && now.duration_since(*at) < STALE
            })
            .count()
    }
}


// ---- Thumbnails of shares nobody watches ---------------------------------
//
// A share nobody watches sends no video (its encoder stays paused), so a card
// would be black. Instead the sharer sends one small, low-resolution JPEG over
// the data channel when the share starts and then every few seconds while
// anyone else is in the call; watchers keep the latest of each share.

/// The topic thumbnails go under.
pub(crate) const THUMB_TOPIC: &str = "morpheus.thumb";

/// A thumbnail is sent this often while anyone else is in the call.
pub(crate) const THUMB_EVERY: Duration = Duration::from_secs(10);

/// A thumbnail is at most this wide (pixels).
pub(crate) const THUMB_WIDTH: u32 = 192;

/// A thumbnail message is at most this many bytes of JPEG (a LiveKit reliable
/// data packet may hold about 15 KB).
pub(crate) const THUMB_MAX_BYTES: usize = 12 * 1024;

/// The size a thumbnail of a `width` by `height` picture has: as wide as
/// [`THUMB_WIDTH`] allows (never larger than the picture), in its shape.
pub(crate) fn thumb_size(width: u32, height: u32) -> (u32, u32) {
    if width == 0 || height == 0 {
        return (2, 2);
    }
    let w = width.min(THUMB_WIDTH);
    let h = ((u64::from(height) * u64::from(w)) / u64::from(width)).max(2) as u32;
    (w.max(2), h)
}

/// One thumbnail as watchers keep it: the picture's mean colour (to tint a
/// card before the JPEG is drawn, and to tell blank from not) and the JPEG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumb {
    pub mean: (u8, u8, u8),
    pub jpeg: Vec<u8>,
}

/// The message: three bytes of mean colour, then the JPEG; `None` when the
/// JPEG is too big to send.
pub(crate) fn encode_thumb(mean: (u8, u8, u8), jpeg: &[u8]) -> Option<Vec<u8>> {
    if jpeg.is_empty() || jpeg.len() > THUMB_MAX_BYTES {
        return None;
    }
    let mut out = vec![mean.0, mean.1, mean.2];
    out.extend_from_slice(jpeg);
    Some(out)
}

/// A message read back; `None` when it is not a thumbnail (too short, too big,
/// not a JPEG).
pub(crate) fn decode_thumb(payload: &[u8]) -> Option<Thumb> {
    let jpeg = payload.get(3..)?;
    if jpeg.len() < 4 || jpeg.len() > THUMB_MAX_BYTES || jpeg[..2] != [0xFF, 0xD8] {
        return None;
    }
    Some(Thumb { mean: (payload[0], payload[1], payload[2]), jpeg: jpeg.to_vec() })
}

/// When the sharer sends its thumbnail.
#[derive(Default)]
pub(crate) struct ThumbClock {
    last: Option<Instant>,
}

impl ThumbClock {
    /// Whether one is due: sharing, someone else in the call, and none sent yet
    /// for this share or the last one [`THUMB_EVERY`] ago.
    pub(crate) fn due(&mut self, now: Instant, sharing: bool, others: bool) -> bool {
        if !sharing {
            // The share ended: the next one starts afresh.
            self.last = None;
            return false;
        }
        others && self.last.is_none_or(|at| now.duration_since(at) >= THUMB_EVERY)
    }

    /// One was sent.
    pub(crate) fn sent(&mut self, now: Instant) {
        self.last = Some(now);
    }
}

/// The latest thumbnail of each share, by the sharer's identity.
#[derive(Default)]
pub(crate) struct Thumbs {
    latest: HashMap<String, Thumb>,
}

impl Thumbs {
    /// Keep a message as `sharer`'s latest; whether it was one.
    pub(crate) fn keep(&mut self, sharer: &str, payload: &[u8]) -> bool {
        match decode_thumb(payload) {
            Some(thumb) => {
                self.latest.insert(sharer.to_owned(), thumb);
                true
            }
            None => false,
        }
    }

    pub(crate) fn get(&self, sharer: &str) -> Option<&Thumb> {
        self.latest.get(sharer)
    }

    pub(crate) fn forget(&mut self, sharer: &str) {
        self.latest.remove(sharer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(names: &[&str]) -> HashSet<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    #[test]
    fn messages_round_trip() {
        assert_eq!(decode(&encode(&set(&["@a:x", "@b:x"]))), set(&["@a:x", "@b:x"]));
        assert!(decode(&encode(&set(&[]))).is_empty());
    }

    #[test]
    fn a_sharer_counts_the_watchers_it_heard_from_lately() {
        let t0 = Instant::now();
        let mut tally = Tally::default();
        tally.hear("b", set(&["a"]), t0);
        tally.hear("c", set(&["a", "d"]), t0);
        tally.hear("d", set(&["c"]), t0);
        assert_eq!(tally.watching("a", t0), 2);
        assert_eq!(tally.watching("d", t0), 1);
        // Stopped watching: said so.
        tally.hear("b", set(&[]), t0);
        assert_eq!(tally.watching("a", t0), 1);
        // Gone silent for too long, or left.
        assert_eq!(tally.watching("a", t0 + STALE), 0);
        tally.forget("c");
        assert_eq!(tally.watching("d", t0), 0);
    }
}

#[cfg(test)]
mod thumbnail_tests {
    use super::*;

    #[test]
    fn a_thumbnail_is_small_and_keeps_the_pictures_shape() {
        assert_eq!(thumb_size(2560, 1440), (THUMB_WIDTH, 108));
        assert_eq!(thumb_size(1280, 720), (THUMB_WIDTH, 108));
        // Never larger than the picture, never zero.
        assert_eq!(thumb_size(100, 50), (100, 50));
        assert_eq!(thumb_size(0, 0), (2, 2));
        // A tall window keeps its shape within the width.
        let (w, h) = thumb_size(1000, 4000);
        assert!(w <= THUMB_WIDTH && h <= 4 * THUMB_WIDTH && w >= 2 && h >= 2);
    }

    #[test]
    fn messages_carry_the_mean_colour_and_the_jpeg() {
        let jpeg = [0xFF, 0xD8, 1, 2, 3, 0xFF, 0xD9];
        let message = encode_thumb((10, 20, 30), &jpeg).expect("fits");
        let thumb = decode_thumb(&message).expect("decodes");
        assert_eq!(thumb.mean, (10, 20, 30));
        assert_eq!(thumb.jpeg, jpeg);
        // Too big, or not a JPEG: not sent, not kept.
        assert!(encode_thumb((0, 0, 0), &vec![0xFF; THUMB_MAX_BYTES + 1]).is_none());
        assert!(decode_thumb(&[1, 2, 3, 0, 0, 0]).is_none());
        let mut big = vec![1, 2, 3, 0xFF, 0xD8];
        big.resize(3 + THUMB_MAX_BYTES + 10, 0);
        assert!(decode_thumb(&big).is_none());
        assert!(decode_thumb(&[]).is_none());
    }

    #[test]
    fn the_sharer_sends_one_when_it_starts_then_every_ten_seconds_while_anyone_is_there() {
        let t0 = Instant::now();
        let mut clock = ThumbClock::default();
        // Not sharing, or alone in the call: nothing.
        assert!(!clock.due(t0, false, true));
        assert!(!clock.due(t0, true, false));
        // The share starts: at once.
        assert!(clock.due(t0, true, true));
        clock.sent(t0);
        assert!(!clock.due(t0 + THUMB_EVERY - Duration::from_secs(1), true, true));
        assert!(clock.due(t0 + THUMB_EVERY, true, true));
        // Stopped and started again: at once again.
        clock.sent(t0 + THUMB_EVERY);
        assert!(!clock.due(t0 + THUMB_EVERY + Duration::from_secs(1), false, true));
        assert!(clock.due(t0 + THUMB_EVERY + Duration::from_secs(2), true, true));
    }

    #[test]
    fn watchers_keep_the_latest_thumbnail_of_each_share() {
        let mut thumbs = Thumbs::default();
        let a = encode_thumb((1, 2, 3), &[0xFF, 0xD8, 1, 0xFF, 0xD9]).expect("fits");
        let b = encode_thumb((4, 5, 6), &[0xFF, 0xD8, 2, 0xFF, 0xD9]).expect("fits");
        assert!(thumbs.keep("a", &a));
        assert!(thumbs.keep("a", &b));
        assert_eq!(thumbs.get("a").expect("kept").mean, (4, 5, 6));
        assert!(!thumbs.keep("a", b"nonsense"));
        assert_eq!(thumbs.get("a").expect("the last good one stays").mean, (4, 5, 6));
        thumbs.forget("a");
        assert!(thumbs.get("a").is_none());
    }
}
