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

/// What the question says when a room is about to open during a share.
pub fn open_question(name: &str, dm: bool) -> String {
    if dm {
        format!("You're sharing your screen. Open the chat with {name}?")
    } else {
        format!("You're sharing your screen. Open #{name}?")
    }
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
