//! What I am doing, as a status (#329): activities pushed by sources (Steam
//! built in; any other program or a browser extension over the local socket,
//! see docs/activity-ipc.md), the rule that picks the one shown, when the
//! status is set and put back, the socket's line protocol, and Chrome's
//! native-messaging framing. No I/O besides the settings files and `native_read`.

use std::io::Read;
use std::path::Path;

use serde_json::{Value, json};

pub const PROTOCOL_VERSION: u32 = 1;

/// What the source is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Playing,
    Listening,
    Watching,
    /// The name is shown as it is.
    Custom,
}

impl Kind {
    pub fn key(self) -> &'static str {
        match self {
            Kind::Playing => "playing",
            Kind::Listening => "listening",
            Kind::Watching => "watching",
            Kind::Custom => "custom",
        }
    }

    pub fn from_key(key: &str) -> Option<Kind> {
        [Kind::Playing, Kind::Listening, Kind::Watching, Kind::Custom]
            .into_iter()
            .find(|k| k.key() == key)
    }

    /// Lower wins when several sources are active.
    fn rank(self) -> u8 {
        match self {
            Kind::Playing => 0,
            Kind::Watching => 1,
            Kind::Listening => 2,
            Kind::Custom => 3,
        }
    }
}

/// One thing a source says I am doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    /// Who says so: `steam`, or `ext:<name>` for a program on the socket.
    pub source: String,
    pub kind: Kind,
    pub name: String,
    pub details: Option<String>,
    pub url: Option<String>,
}

const MAX_TEXT: usize = 120;

impl Activity {
    /// The status text: "Playing Hades", "Listening to Song - Artist".
    pub fn text(&self) -> String {
        let base = match self.kind {
            Kind::Playing => format!("Playing {}", self.name),
            Kind::Listening => format!("Listening to {}", self.name),
            Kind::Watching => format!("Watching {}", self.name),
            Kind::Custom => self.name.clone(),
        };
        let full = match self.details.as_deref().filter(|d| !d.is_empty()) {
            Some(details) => format!("{base} - {details}"),
            None => base,
        };
        if full.chars().count() > MAX_TEXT {
            let cut: String = full.chars().take(MAX_TEXT - 1).collect();
            format!("{cut}\u{2026}")
        } else {
            full
        }
    }
}

/// The start of what the kinds write, to tell a status left by an earlier run.
const OUR_PREFIXES: [&str; 3] = ["Playing ", "Listening to ", "Watching "];

// ---- the sources ---------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Entry {
    activity: Activity,
    /// Milliseconds (the caller's clock) when it lapses; None: until cleared.
    expires: Option<u64>,
    seq: u64,
}

/// The activities now active, one per source.
#[derive(Debug, Clone, Default)]
pub struct Sources {
    entries: Vec<Entry>,
    seq: u64,
}

/// A source that does not repeat itself lapses after this.
pub const DEFAULT_TTL_MS: u64 = 60_000;

impl Sources {
    /// A source says what it does now (replacing what it said); a source that
    /// says it again before `ttl_ms` is up keeps it alive.
    pub fn set(&mut self, activity: Activity, ttl_ms: Option<u64>, now_ms: u64) {
        self.entries.retain(|e| e.activity.source != activity.source);
        self.seq += 1;
        self.entries.push(Entry {
            activity,
            expires: ttl_ms.map(|t| now_ms.saturating_add(t)),
            seq: self.seq,
        });
    }

    pub fn clear(&mut self, source: &str) {
        self.entries.retain(|e| e.activity.source != source);
    }

    /// Drops what has lapsed.
    pub fn expire(&mut self, now_ms: u64) {
        self.entries
            .retain(|e| e.expires.is_none_or(|at| at > now_ms));
    }

    /// The activity to show: of the sources that are switched on, a game before a
    /// video before music before a custom line, the newest first among equals.
    pub fn shown(&self, now_ms: u64, on: impl Fn(&str) -> bool) -> Option<&Activity> {
        self.entries
            .iter()
            .filter(|e| e.expires.is_none_or(|at| at > now_ms))
            .filter(|e| on(&e.activity.source))
            .min_by_key(|e| (e.activity.kind.rank(), std::cmp::Reverse(e.seq)))
            .map(|e| &e.activity)
    }

    /// Applies a request from the socket.
    pub fn apply(&mut self, request: Request, now_ms: u64) -> Result<(), String> {
        match request {
            Request::Hello { .. } => Ok(()),
            Request::Set { source, kind, name, details, url, ttl_ms } => {
                let activity = Activity { source: format!("ext:{source}"), kind, name, details, url };
                self.set(activity, Some(ttl_ms), now_ms);
                Ok(())
            }
            Request::Clear { source } => {
                self.clear(&format!("ext:{source}"));
                Ok(())
            }
        }
    }
}

// ---- settings ----------------------------------------------------------------------------

pub const STEAM_LABEL: &str = "Steam games";
pub const STEAM_HINT: &str = "While a Steam game or app is running, your status says \"Playing <name>\" and goes back to what it was when it closes.";
pub const EXTERNAL_LABEL: &str = "Other apps and browser extensions";
pub const EXTERNAL_HINT: &str = "Programs on this computer can set your status (\"Listening to ...\") through a local socket, for example a browser extension for your music. See docs/activity-ipc.md.";
pub const SECTION_NOTE: &str = "A status you type yourself while one of these is showing is never replaced.";

/// Which switch a source answers to: `steam` or `external`.
pub fn setting_group(source: &str) -> &str {
    if source.starts_with("ext:") { "external" } else { source }
}

fn off_marker(dir: &Path, group: &str) -> std::path::PathBuf {
    dir.join(format!("activity-off-{group}"))
}

/// Whether a group of sources may set the status (on unless turned off).
pub fn group_enabled(dir: &Path, group: &str) -> bool {
    !off_marker(dir, group).exists()
}

pub fn set_group_enabled(dir: &Path, group: &str, on: bool) -> std::io::Result<()> {
    let marker = off_marker(dir, group);
    if on {
        match std::fs::remove_file(&marker) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        }
    } else {
        std::fs::write(marker, "")
    }
}

/// Whether the source `source` may set the status.
pub fn source_enabled(dir: &Path, source: &str) -> bool {
    group_enabled(dir, setting_group(source))
}

// ---- when to set and restore --------------------------------------------------------------

/// Set this status (None clears it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change(pub Option<String>);

/// What the status was set to, and what it was before.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tracker {
    wanted: Option<String>,
    ours: Option<String>,
    before: Option<String>,
}

impl Tracker {
    /// Whether `wanted` differs from what was last acted on: only then is the
    /// current status needed for [`Tracker::step`].
    pub fn changed(&self, wanted: Option<&str>) -> bool {
        self.wanted.as_deref() != wanted
    }

    /// The status wanted (None: no activity shows) and the status now. A status
    /// that is not the one set here was typed by hand and is left alone, now and
    /// when the activity ends.
    pub fn step(&mut self, wanted: Option<&str>, current: Option<&str>) -> Option<Change> {
        self.wanted = wanted.map(str::to_owned);
        let current = current.map(str::trim).filter(|s| !s.is_empty());
        match wanted {
            Some(text) => match self.ours.clone() {
                Some(ours) if current == Some(ours.as_str()) => {
                    self.ours = Some(text.to_owned());
                    Some(Change(Some(text.to_owned())))
                }
                Some(_) => {
                    self.ours = None;
                    self.before = None;
                    None
                }
                None => {
                    self.before = current
                        .filter(|c| !OUR_PREFIXES.iter().any(|p| c.starts_with(p)))
                        .map(str::to_owned);
                    self.ours = Some(text.to_owned());
                    Some(Change(Some(text.to_owned())))
                }
            },
            None => {
                let ours = self.ours.take()?;
                let before = self.before.take();
                (current == Some(ours.as_str())).then_some(Change(before))
            }
        }
    }
}

// ---- the socket's protocol ---------------------------------------------------------------------

/// One line from a client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Hello { version: u32, client: Option<String> },
    Set {
        source: String,
        kind: Kind,
        name: String,
        details: Option<String>,
        url: Option<String>,
        ttl_ms: u64,
    },
    Clear { source: String },
}

/// The longest line read.
pub const MAX_LINE: usize = 16 * 1024;

fn source_name(value: &Value) -> Result<String, String> {
    let name = value.get("source").and_then(Value::as_str).unwrap_or_default();
    let ok = !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok {
        Ok(name.to_owned())
    } else {
        Err("\"source\" must be 1-64 letters, digits, '.', '_' or '-'".into())
    }
}

fn text_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

pub fn parse_request(line: &str) -> Result<Request, String> {
    let value: Value = serde_json::from_str(line.trim()).map_err(|e| format!("not JSON: {e}"))?;
    match value.get("op").and_then(Value::as_str) {
        Some("hello") => {
            let version = value.get("version").and_then(Value::as_u64).unwrap_or(0) as u32;
            if version != PROTOCOL_VERSION {
                return Err(format!("protocol version {PROTOCOL_VERSION} is the one spoken"));
            }
            Ok(Request::Hello { version, client: text_field(&value, "client") })
        }
        Some("set") => {
            let source = source_name(&value)?;
            let kind = value
                .get("kind")
                .and_then(Value::as_str)
                .and_then(Kind::from_key)
                .ok_or("\"kind\" must be playing, listening, watching or custom")?;
            let name = text_field(&value, "name").ok_or("\"name\" is missing")?;
            let ttl_ms = value
                .get("ttl_ms")
                .and_then(Value::as_u64)
                .unwrap_or(DEFAULT_TTL_MS)
                .clamp(500, 86_400_000);
            Ok(Request::Set {
                source,
                kind,
                name,
                details: text_field(&value, "details"),
                url: text_field(&value, "url"),
                ttl_ms,
            })
        }
        Some("clear") => Ok(Request::Clear { source: source_name(&value)? }),
        Some(other) => Err(format!("unknown op {other:?}")),
        None => Err("\"op\" is missing".into()),
    }
}

pub fn reply_ok() -> String {
    json!({ "ok": true }).to_string()
}

pub fn reply_error(message: &str) -> String {
    json!({ "ok": false, "error": message }).to_string()
}

pub fn reply_hello() -> String {
    json!({ "ok": true, "op": "hello", "version": PROTOCOL_VERSION }).to_string()
}

// ---- Chrome native messaging -----------------------------------------------------------------------

/// A message as Chrome frames it: its length as 4 bytes (little endian), then the JSON.
pub fn native_encode(json: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + json.len());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(json.as_bytes());
    out
}

/// The most Chrome sends in one message (1 MiB for the page to the host).
pub const NATIVE_MAX: usize = 1024 * 1024;

/// The next message; None when the input ended cleanly between messages.
pub fn native_read(input: &mut impl Read) -> std::io::Result<Option<String>> {
    let mut len = [0u8; 4];
    match input.read_exact(&mut len) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) => return Err(err),
    }
    let len = u32::from_le_bytes(len) as usize;
    if len > NATIVE_MAX {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "message too long"));
    }
    let mut body = vec![0u8; len];
    input.read_exact(&mut body)?;
    String::from_utf8(body)
        .map(Some)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn act(source: &str, kind: Kind, name: &str) -> Activity {
        Activity { source: source.into(), kind, name: name.into(), details: None, url: None }
    }

    fn always(_: &str) -> bool {
        true
    }

    #[test]
    fn the_text_reads_by_kind() {
        assert_eq!(act("steam", Kind::Playing, "Hades").text(), "Playing Hades");
        let mut song = act("ext:ym", Kind::Listening, "Song");
        song.details = Some("Artist".into());
        assert_eq!(song.text(), "Listening to Song - Artist");
        assert_eq!(act("ext:x", Kind::Watching, "A film").text(), "Watching A film");
        assert_eq!(act("ext:x", Kind::Custom, "Reading").text(), "Reading");
        assert_eq!(act("ext:x", Kind::Custom, &"a".repeat(500)).text().chars().count(), 120);
    }

    #[test]
    fn a_game_beats_music_and_the_newest_wins_among_equals() {
        let mut s = Sources::default();
        s.set(act("ext:ym", Kind::Listening, "Song"), None, 0);
        assert_eq!(s.shown(0, always).map(|a| a.name.as_str()), Some("Song"));
        s.set(act("steam", Kind::Playing, "Hades"), None, 0);
        assert_eq!(s.shown(0, always).map(|a| a.name.as_str()), Some("Hades"));
        s.set(act("ext:b", Kind::Listening, "Newer"), None, 0);
        s.clear("steam");
        assert_eq!(s.shown(0, always).map(|a| a.name.as_str()), Some("Newer"));
        // A source saying it again replaces its own line.
        s.set(act("ext:ym", Kind::Listening, "Song 2"), None, 0);
        assert_eq!(s.shown(0, always).map(|a| a.name.as_str()), Some("Song 2"));
    }

    #[test]
    fn a_source_that_goes_quiet_lapses() {
        let mut s = Sources::default();
        s.set(act("ext:ym", Kind::Listening, "Song"), Some(1000), 100);
        assert!(s.shown(1099, always).is_some());
        assert!(s.shown(1100, always).is_none(), "lapsed even before expire runs");
        s.set(act("ext:ym", Kind::Listening, "Song"), Some(1000), 900);
        s.expire(1100);
        assert!(s.shown(1100, always).is_some(), "said again: kept alive");
        s.expire(1900);
        assert!(s.shown(1900, always).is_none());
    }

    #[test]
    fn a_source_turned_off_is_not_shown() {
        let mut s = Sources::default();
        s.set(act("steam", Kind::Playing, "Hades"), None, 0);
        s.set(act("ext:ym", Kind::Listening, "Song"), None, 0);
        let off_steam = |src: &str| setting_group(src) != "steam";
        assert_eq!(s.shown(0, off_steam).map(|a| a.name.as_str()), Some("Song"));
        assert_eq!(s.shown(0, |_| false), None);
    }

    #[test]
    fn the_groups_are_on_until_turned_off() {
        let dir = std::env::temp_dir().join(format!("morpheus-activity-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        assert!(source_enabled(&dir, "steam") && source_enabled(&dir, "ext:ym"));
        set_group_enabled(&dir, "external", false).expect("off");
        assert!(!source_enabled(&dir, "ext:ym"));
        assert!(source_enabled(&dir, "steam"));
        set_group_enabled(&dir, "external", true).expect("on");
        set_group_enabled(&dir, "external", true).expect("on twice");
        assert!(source_enabled(&dir, "ext:ym"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_activity_sets_the_status_and_its_end_brings_the_old_one_back() {
        let mut t = Tracker::default();
        assert!(!t.changed(None));
        assert_eq!(t.step(None, Some("Busy")), None);
        assert!(t.changed(Some("Playing Hades")));
        assert_eq!(
            t.step(Some("Playing Hades"), Some("Busy")),
            Some(Change(Some("Playing Hades".into())))
        );
        assert!(!t.changed(Some("Playing Hades")));
        assert_eq!(t.step(None, Some("Playing Hades")), Some(Change(Some("Busy".into()))));
        assert_eq!(t.step(Some("Playing Hades"), None), Some(Change(Some("Playing Hades".into()))));
        assert_eq!(t.step(None, Some("Playing Hades")), Some(Change(None)));
    }

    #[test]
    fn a_status_typed_by_hand_while_it_shows_is_kept() {
        let mut t = Tracker::default();
        t.step(Some("Playing Hades"), Some("Busy"));
        assert_eq!(t.step(None, Some("In a meeting")), None);
        assert_eq!(
            t.step(Some("Listening to X"), Some("In a meeting")),
            Some(Change(Some("Listening to X".into())))
        );
        assert_eq!(t.step(None, Some("Listening to X")), Some(Change(Some("In a meeting".into()))));
    }

    #[test]
    fn one_activity_after_another_keeps_the_first_before_status() {
        let mut t = Tracker::default();
        t.step(Some("Playing A"), Some("Busy"));
        assert_eq!(
            t.step(Some("Listening to B"), Some("Playing A")),
            Some(Change(Some("Listening to B".into())))
        );
        assert_eq!(t.step(None, Some("Listening to B")), Some(Change(Some("Busy".into()))));
    }

    #[test]
    fn a_status_left_by_an_earlier_run_is_not_restored() {
        let mut t = Tracker::default();
        t.step(Some("Playing Hades"), Some("Playing Hades"));
        assert_eq!(t.step(None, Some("Playing Hades")), Some(Change(None)));
    }

    #[test]
    fn a_set_line_is_read() {
        let r = parse_request(
            r#"{"op":"set","source":"youtube-music","kind":"listening","name":"Song","details":"Artist","url":"https://music.youtube.com/watch?v=1","ttl_ms":30000}"#,
        )
        .expect("set");
        assert_eq!(
            r,
            Request::Set {
                source: "youtube-music".into(),
                kind: Kind::Listening,
                name: "Song".into(),
                details: Some("Artist".into()),
                url: Some("https://music.youtube.com/watch?v=1".into()),
                ttl_ms: 30000,
            }
        );
        let mut s = Sources::default();
        s.apply(r, 0).expect("apply");
        assert_eq!(s.shown(0, always).map(Activity::text).as_deref(), Some("Listening to Song - Artist"));
        s.apply(parse_request(r#"{"op":"clear","source":"youtube-music"}"#).expect("clear"), 1)
            .expect("apply");
        assert!(s.shown(1, always).is_none());
    }

    #[test]
    fn a_set_without_ttl_lapses_by_default_and_ttl_is_bounded() {
        let Ok(Request::Set { ttl_ms, .. }) =
            parse_request(r#"{"op":"set","source":"a","kind":"custom","name":"x"}"#)
        else {
            panic!("set")
        };
        assert_eq!(ttl_ms, DEFAULT_TTL_MS);
        let Ok(Request::Set { ttl_ms, .. }) =
            parse_request(r#"{"op":"set","source":"a","kind":"custom","name":"x","ttl_ms":1}"#)
        else {
            panic!("set")
        };
        assert_eq!(ttl_ms, 500);
    }

    #[test]
    fn bad_lines_say_what_is_wrong() {
        assert!(parse_request("nope").is_err());
        assert!(parse_request(r#"{"op":"set","source":"a b","kind":"custom","name":"x"}"#).is_err());
        assert!(parse_request(r#"{"op":"set","source":"a","kind":"dancing","name":"x"}"#).is_err());
        assert!(parse_request(r#"{"op":"set","source":"a","kind":"custom"}"#).is_err());
        assert!(parse_request(r#"{"op":"hello","version":9}"#).is_err());
        assert!(parse_request(r#"{"op":"hello","version":1}"#).is_ok());
        assert!(parse_request(r#"{"op":"zap"}"#).is_err());
    }

    #[test]
    fn native_messages_are_length_prefixed_and_read_back_in_turn() {
        let mut bytes = native_encode(r#"{"op":"clear","source":"a"}"#);
        bytes.extend(native_encode("{}"));
        assert_eq!(&bytes[..4], &27u32.to_le_bytes());
        let mut input = std::io::Cursor::new(bytes);
        assert_eq!(native_read(&mut input).expect("read").as_deref(), Some(r#"{"op":"clear","source":"a"}"#));
        assert_eq!(native_read(&mut input).expect("read").as_deref(), Some("{}"));
        assert_eq!(native_read(&mut input).expect("eof"), None);
        let mut cut = std::io::Cursor::new(vec![9, 0, 0, 0, b'{']);
        assert!(native_read(&mut cut).is_err(), "cut short in a message");
        let mut huge = std::io::Cursor::new(u32::MAX.to_le_bytes().to_vec());
        assert!(native_read(&mut huge).is_err());
    }
}
