//! Streamer mode, as Discord has it: while the screen is shared (or when
//! turned on by hand) the app hides what could give people away. The choices
//! and the rule for what is shown; no toolkit in here.
//!
//! The rule: a person's name shows as its first letter and three dots
//! ("Alice" is "A•••"), so rows stay tellable apart by their first letter and
//! their pictures; user IDs, room IDs, device IDs, e-mail addresses, phone
//! numbers and links show as dots only. Channel names are not people and stay.

use std::path::Path;

use serde_json::{Value, json};

use crate::rich::{Block, Para, Span};

/// The dots that stand for what is hidden.
pub const DOTS: &str = "\u{2022}\u{2022}\u{2022}";

/// What streamer mode does, and when. Kept on this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamerSettings {
    /// Turn on by itself while this device shares its screen.
    pub auto: bool,
    /// Turned on by hand.
    pub now: bool,
    /// Abbreviate people's names.
    pub names: bool,
    /// Notifications say nothing of who or what.
    pub notifications: bool,
    /// Hide user, room and device IDs, e-mail addresses, phone numbers, links.
    pub ids: bool,
    /// Ask before another channel opens while sharing.
    pub ask: bool,
}

impl Default for StreamerSettings {
    fn default() -> Self {
        Self { auto: true, now: false, names: true, notifications: true, ids: true, ask: true }
    }
}

const FILE: &str = "streamer-mode.json";

impl StreamerSettings {
    /// Whether streamer mode is on: by hand, or because the screen is shared.
    pub fn active(&self, sharing: bool) -> bool {
        self.now || (self.auto && sharing)
    }

    /// Read from `dir` (the defaults where missing or unreadable; a key not
    /// there keeps its default).
    pub fn load(dir: &Path) -> Self {
        let kept: Value = std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or(Value::Null);
        let d = Self::default();
        let get = |key: &str, default: bool| kept.get(key).and_then(Value::as_bool).unwrap_or(default);
        Self {
            auto: get("auto", d.auto),
            now: get("now", d.now),
            names: get("names", d.names),
            notifications: get("notifications", d.notifications),
            ids: get("ids", d.ids),
            ask: get("ask", d.ask),
        }
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        let json = json!({
            "auto": self.auto, "now": self.now, "names": self.names,
            "notifications": self.notifications, "ids": self.ids, "ask": self.ask,
        });
        std::fs::write(dir.join(FILE), json.to_string())
    }
}

/// A name abbreviated: its first letter, then dots. Nothing stays nothing.
pub fn mask_name(name: &str) -> String {
    match name.trim_start_matches('@').chars().next() {
        Some(first) => format!("{first}{DOTS}"),
        None => String::new(),
    }
}

/// An ID, e-mail address, phone number or link hidden: dots alone.
pub fn mask_id(id: &str) -> String {
    if id.is_empty() { String::new() } else { format!("{DOTS}{DOTS}") }
}

/// A state line ("bob joined the room") with the name it starts with swapped
/// for `shown`; as it was when it does not start with `who`.
pub fn swap_who(text: &str, who: &str, shown: &str) -> String {
    match text.strip_prefix(who) {
        Some(rest) if !who.is_empty() => format!("{shown}{rest}"),
        _ => text.to_owned(),
    }
}

/// Whether a link target is a person (a matrix.to link to a user).
fn is_person_link(href: &str) -> bool {
    href.strip_prefix("https://matrix.to/#/")
        .is_some_and(|rest| rest.starts_with('@') || rest.starts_with("%40"))
}

/// `blocks` with each person's mention pill shown as `@` and what `mask`
/// makes of its name (a room's pill and everything else stay as they were).
pub fn mask_pills(blocks: &[Block], mask: &dyn Fn(&str) -> String) -> Vec<Block> {
    blocks
        .iter()
        .map(|block| match block {
            Block::Para(para) => Block::Para(mask_para(para, mask)),
            Block::Quote(inner) => Block::Quote(mask_pills(inner, mask)),
            Block::List { ordered, start, items } => Block::List {
                ordered: *ordered,
                start: *start,
                items: items.iter().map(|item| mask_pills(item, mask)).collect(),
            },
            other => other.clone(),
        })
        .collect()
}

fn mask_para(para: &Para, mask: &dyn Fn(&str) -> String) -> Para {
    let mut pills: Vec<(std::ops::Range<usize>, String)> = para
        .spans
        .iter()
        .filter(|s| s.style.mention && s.style.link.as_deref().is_some_and(is_person_link))
        .map(|s| (s.range.clone(), format!("@{}", mask(&para.text[s.range.clone()]))))
        .collect();
    if pills.is_empty() {
        return para.clone();
    }
    pills.sort_by_key(|(range, _)| range.start);
    pills.dedup_by(|b, a| b.0.start < a.0.end);
    // An old byte position as a new one.
    let moved = |at: usize| -> usize {
        let mut new = at as isize;
        for (range, shown) in &pills {
            let shift = shown.len() as isize - (range.end - range.start) as isize;
            if range.end <= at {
                new += shift;
            } else if range.start < at {
                // Inside a pill: its end.
                new += shift + (range.end - at) as isize;
            }
        }
        new as usize
    };
    let mut text = String::new();
    let mut from = 0;
    for (range, shown) in &pills {
        text.push_str(&para.text[from..range.start]);
        text.push_str(shown);
        from = range.end;
    }
    text.push_str(&para.text[from..]);
    let spans = para
        .spans
        .iter()
        .map(|s| Span { range: moved(s.range.start)..moved(s.range.end), style: s.style.clone() })
        .collect();
    Para { text, spans }
}

/// What the question says when a room is about to open during a share.
pub fn open_question(name: &str, dm: bool) -> String {
    if dm {
        format!("You're sharing your screen. Open the chat with {name}?")
    } else {
        format!("You're sharing your screen. Open #{name}?")
    }
}

/// The question before a server's welcome screen (it lists channels) shows.
pub fn open_server_question(name: &str) -> String {
    format!("You're sharing your screen. Open the server {name}?")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_keeps_its_first_letter_and_ids_are_all_dots() {
        assert_eq!(mask_name("Alice"), format!("A{DOTS}"));
        assert_eq!(mask_name("@bob:x"), format!("b{DOTS}"));
        assert_eq!(mask_name("\u{e9}mile"), format!("\u{e9}{DOTS}"));
        assert_eq!(mask_name(""), "");
        assert_eq!(mask_id("@bob:x"), format!("{DOTS}{DOTS}"));
        assert_eq!(mask_id(""), "");
    }

    #[test]
    fn it_is_on_by_hand_or_while_sharing_when_that_is_allowed() {
        let mut s = StreamerSettings::default();
        assert!(!s.active(false) && s.active(true));
        s.auto = false;
        assert!(!s.active(true));
        s.now = true;
        assert!(s.active(false));
    }

    #[test]
    fn the_choices_are_kept_and_defaults_fill_what_is_missing() {
        let dir = std::env::temp_dir().join(format!("morpheus-streamer-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        assert_eq!(StreamerSettings::load(&dir), StreamerSettings::default());
        let s = StreamerSettings { auto: false, now: true, ids: false, ..Default::default() };
        s.save(&dir).expect("save");
        assert_eq!(StreamerSettings::load(&dir), s);
        std::fs::write(dir.join(FILE), r#"{"ask":false}"#).expect("write");
        assert_eq!(StreamerSettings::load(&dir), StreamerSettings { ask: false, ..Default::default() });
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod pill_tests {
    use super::*;
    use crate::rich::Style;

    fn pill(text: &str, link: &str) -> Para {
        let mut para = Para::default();
        para.push("hi ", Style::default());
        para.push(text, Style { mention: true, link: Some(link.into()), ..Style::default() });
        para.push(" and more", Style { bold: true, ..Style::default() });
        para
    }

    #[test]
    fn a_persons_pill_is_masked_and_the_spans_after_it_move() {
        let para = pill("Bobby Tables", "https://matrix.to/#/@bob:x");
        let masked = mask_pills(&[Block::Para(para)], &|n| mask_name(n));
        let [Block::Para(out)] = masked.as_slice() else {
            panic!("a paragraph");
        };
        assert_eq!(out.text, format!("hi @B{DOTS} and more"));
        let bold = out.spans.iter().find(|s| s.style.bold).expect("bold span");
        assert_eq!(&out.text[bold.range.clone()], " and more");
        let pill = out.spans.iter().find(|s| s.style.mention).expect("pill span");
        assert_eq!(&out.text[pill.range.clone()], format!("@B{DOTS}"));
    }

    #[test]
    fn a_rooms_pill_and_plain_text_stay() {
        let para = pill("#general", "https://matrix.to/#/#general:x");
        assert_eq!(mask_pills(&[Block::Para(para.clone())], &|n| mask_name(n)), vec![Block::Para(para)]);
    }

    #[test]
    fn a_state_line_swaps_the_name_it_starts_with() {
        assert_eq!(swap_who("bob joined the room", "bob", "b\u{2022}"), "b\u{2022} joined the room");
        assert_eq!(swap_who("someone joined", "bob", "b"), "someone joined");
        assert_eq!(swap_who("joined", "", "b"), "joined");
    }
}

/// Whether the screen is being shared, and streamer mode turned off by hand
/// while it is: this app's own share (call.screen) or another program's capture
/// (#143: Discord, OBS) both count; the hand-off stays until neither is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ShareTracker {
    own: bool,
    external: bool,
    overridden: bool,
}

impl ShareTracker {
    pub const fn new() -> Self {
        Self { own: false, external: false, overridden: false }
    }

    /// The share this app is making began or ended.
    pub fn set_own(&mut self, on: bool) {
        self.own = on;
        self.settle();
    }

    /// Another program's capture of the screen began or ended.
    pub fn set_external(&mut self, on: bool) {
        self.external = on;
        self.settle();
    }

    fn settle(&mut self) {
        if !self.sharing() {
            self.overridden = false;
        }
    }

    /// Whether the screen is being shared or captured.
    pub fn sharing(&self) -> bool {
        self.own || self.external
    }

    /// Streamer mode was turned off by hand: during a share it stays off.
    pub fn override_off(&mut self) {
        if self.sharing() {
            self.overridden = true;
        }
    }

    /// Streamer mode was turned on by hand again.
    pub fn clear_override(&mut self) {
        self.overridden = false;
    }

    /// Whether streamer mode is on under `settings`.
    pub fn active(&self, settings: &StreamerSettings) -> bool {
        settings.active(self.sharing()) && !(self.sharing() && self.overridden)
    }
}

#[cfg(test)]
mod tracker_tests {
    use super::*;

    #[test]
    fn another_programs_capture_turns_it_on_and_off_like_our_own_share() {
        let settings = StreamerSettings::default();
        let mut t = ShareTracker::new();
        assert!(!t.active(&settings));
        t.set_external(true);
        assert!(t.active(&settings), "OBS capturing");
        t.set_external(false);
        assert!(!t.active(&settings), "OBS stopped");
        t.set_own(true);
        assert!(t.active(&settings));
        t.set_external(true);
        t.set_own(false);
        assert!(t.active(&settings), "still captured by another");
        t.set_external(false);
        assert!(!t.active(&settings));
    }

    #[test]
    fn the_auto_setting_and_the_hand_override_apply_to_it_too() {
        let mut t = ShareTracker::new();
        let no_auto = StreamerSettings { auto: false, ..StreamerSettings::default() };
        t.set_external(true);
        assert!(!t.active(&no_auto), "auto-on turned off");
        assert!(t.active(&StreamerSettings { now: true, ..no_auto }), "by hand still works");
        let auto = StreamerSettings::default();
        t.override_off();
        assert!(!t.active(&auto), "turned off by hand during the capture");
        t.set_own(true);
        t.set_external(false);
        assert!(!t.active(&auto), "off until all sharing has ended");
        t.set_own(false);
        t.set_external(true);
        assert!(t.active(&auto), "a later capture turns it on again");
    }

    #[test]
    fn turning_it_off_by_hand_with_nothing_shared_is_not_an_override() {
        let mut t = ShareTracker::new();
        t.override_off();
        t.set_external(true);
        assert!(t.active(&StreamerSettings::default()));
    }
}
