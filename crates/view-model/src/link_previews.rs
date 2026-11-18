//! Link previews (cards under a message with a link): which link a message
//! previews, whether a room may have previews, and what this device keeps
//! (the settings and the cards dismissed). What a page is comes from
//! `morpheus_core::UrlPreview`; the front end draws the card.

use std::path::Path;

use crate::rich::{Block, is_pill};

const FILE: &str = "link_previews.json";
/// How many dismissed messages are remembered.
const DISMISSED_KEPT: usize = 2000;

/// The first web link of a message that is not a mention or a room link,
/// which is the one that gets the card.
pub fn first_url(blocks: &[Block]) -> Option<String> {
    for block in blocks {
        let found = match block {
            Block::Para(p) => p
                .links()
                .into_iter()
                .map(|(_, url)| url)
                .find(|url| (url.starts_with("http://") || url.starts_with("https://")) && !is_pill(url))
                .map(|url| url.to_string()),
            Block::Quote(inner) => first_url(inner),
            Block::List { items, .. } => items.iter().find_map(|item| first_url(item)),
            Block::Pre(_) | Block::Rule => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

/// What this device does about previews.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Show cards at all.
    pub enabled: bool,
    /// Also in encrypted rooms: fetching a card tells the homeserver the
    /// link, which end-to-end encryption otherwise keeps from it.
    pub in_encrypted: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            in_encrypted: false,
        }
    }
}

impl Settings {
    /// Whether a room gets cards.
    pub fn allows(&self, room_encrypted: bool) -> bool {
        self.enabled && (self.in_encrypted || !room_encrypted)
    }
}

/// Cards dismissed on this device, by message (newest last).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dismissed(pub Vec<String>);

impl Dismissed {
    pub fn contains(&self, message: &str) -> bool {
        self.0.iter().any(|m| m == message)
    }

    pub fn add(&mut self, message: &str) {
        if !self.contains(message) {
            self.0.push(message.to_owned());
            let extra = self.0.len().saturating_sub(DISMISSED_KEPT);
            self.0.drain(..extra);
        }
    }
}

pub fn load(dir: &Path) -> (Settings, Dismissed) {
    let value: serde_json::Value = std::fs::read_to_string(dir.join(FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let defaults = Settings::default();
    let flag = |key: &str, default: bool| {
        value
            .get(key)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(default)
    };
    let settings = Settings {
        enabled: flag("enabled", defaults.enabled),
        in_encrypted: flag("in_encrypted", defaults.in_encrypted),
    };
    let dismissed = value
        .get("dismissed")
        .and_then(serde_json::Value::as_array)
        .map(|list| list.iter().filter_map(|m| m.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    (settings, Dismissed(dismissed))
}

pub fn save(dir: &Path, settings: &Settings, dismissed: &Dismissed) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(
        dir.join(FILE),
        serde_json::json!({
            "enabled": settings.enabled,
            "in_encrypted": settings.in_encrypted,
            "dismissed": dismissed.0,
        })
        .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rich;

    #[test]
    fn the_first_web_link_is_previewed_not_mentions() {
        let blocks = rich::html(
            "<p>hi <a href=\"https://matrix.to/#/@bob:x\">Bob</a> and <a href=\"https://example.org/a\">a</a> then https://other.org/b</p>",
        );
        assert_eq!(first_url(&blocks).as_deref(), Some("https://example.org/a"));
        let plain = rich::plain("see https://one.example/x and https://two.example");
        assert_eq!(first_url(&plain).as_deref(), Some("https://one.example/x"));
        assert_eq!(first_url(&rich::plain("nothing here")), None);
        let code = rich::html("<pre>https://in.code/x</pre>");
        assert_eq!(first_url(&code), None, "links in code blocks are not previewed");
        let quoted = rich::html("<blockquote>https://quoted.example/q</blockquote>");
        assert_eq!(first_url(&quoted).as_deref(), Some("https://quoted.example/q"));
    }

    #[test]
    fn encrypted_rooms_get_no_cards_unless_turned_on() {
        let mut s = Settings::default();
        assert!(s.allows(false));
        assert!(!s.allows(true), "off by default in encrypted rooms");
        s.in_encrypted = true;
        assert!(s.allows(true));
        s.enabled = false;
        assert!(!s.allows(false) && !s.allows(true), "off is off");
    }

    #[test]
    fn settings_and_dismissals_are_kept() {
        let dir = std::env::temp_dir().join(format!("morpheus-previews-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load(&dir), (Settings::default(), Dismissed::default()));
        let mut dismissed = Dismissed::default();
        dismissed.add("$a");
        dismissed.add("$a");
        dismissed.add("$b");
        let settings = Settings {
            enabled: false,
            in_encrypted: true,
        };
        save(&dir, &settings, &dismissed).expect("save");
        let (back, gone) = load(&dir);
        assert_eq!(back, settings);
        assert_eq!(gone.0, ["$a", "$b"]);
        assert!(gone.contains("$b") && !gone.contains("$c"));
        let _ = std::fs::remove_dir_all(&dir);
        for n in 0..DISMISSED_KEPT + 5 {
            dismissed.add(&format!("$m{n}"));
        }
        assert_eq!(dismissed.0.len(), DISMISSED_KEPT);
        assert!(!dismissed.contains("$a"), "the oldest go first");
    }
}
