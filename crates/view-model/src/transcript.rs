//! The text copied when a selection spans several messages (#303): a transcript
//! with each message's sender and time, then what it says. The full date shows on
//! the first line and wherever the day changes; replies say whom they answer;
//! pictures and files are placeholders. Dates and times use the language's words
//! (`format`), so every client copies the same.

use jiff::tz::TimeZone;

use crate::format::{calendar_date_in, clock_time_in, month_name, weekday_name, zoned};
use crate::i18n::tf_in;

/// What one message in the transcript has to say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    /// Its text (the selected part of it); lines after the first are kept.
    Text(String),
    /// An attachment, by its file name: shown as `[image: name]` and the like.
    Image(String),
    File(String),
    Video(String),
    Audio(String),
}

/// One message of the transcript.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub sender: String,
    pub timestamp_ms: u64,
    /// The name of whoever it replies to.
    pub reply_to: Option<String>,
    pub body: Body,
}

/// The transcript in the language in use and the device's time zone.
pub fn transcript(items: &[Item]) -> String {
    transcript_in(crate::i18n::language(), items, &TimeZone::system())
}

/// "Fri 21 Nov": the weekday, day and month, abbreviated.
fn short_date(code: &str, timestamp_ms: u64, zone: &TimeZone) -> String {
    zoned(timestamp_ms, zone).map_or_else(String::new, |at| {
        let weekday: String = weekday_name(code, &at).chars().take(3).collect();
        tf_in(
            code,
            "%{weekday} %{day} %{month}",
            &[("weekday", &weekday), ("day", &at.day().to_string()), ("month", &month_name(code, &at, false))],
        )
    })
}

pub fn transcript_in(code: &str, items: &[Item], zone: &TimeZone) -> String {
    let mut out = Vec::new();
    let mut day = String::new();
    for item in items {
        let date = calendar_date_in(item.timestamp_ms, zone);
        let clock = clock_time_in(item.timestamp_ms, zone);
        let when = if date != day {
            format!("{} {clock}", short_date(code, item.timestamp_ms, zone))
        } else {
            clock
        };
        day = date;
        let who = match &item.reply_to {
            Some(to) => format!("{} ({})", item.sender, tf_in(code, "replying to %{name}", &[("name", to)])),
            None => item.sender.clone(),
        };
        let placeholder = |key: &str, name: &str| tf_in(code, key, &[("name", name)]);
        let text = match &item.body {
            Body::Text(text) => text.clone(),
            Body::Image(name) => placeholder("[image: %{name}]", name),
            Body::File(name) => placeholder("[file: %{name}]", name),
            Body::Video(name) => placeholder("[video: %{name}]", name),
            Body::Audio(name) => placeholder("[audio: %{name}]", name),
        };
        out.push(format!("[{when}] {who}: {text}"));
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fri 21 Nov 2025 14:32 UTC.
    const T: u64 = 1_763_735_520_000;
    const MIN: u64 = 60_000;
    const DAY: u64 = 86_400_000;

    fn item(who: &str, at: u64, text: &str) -> Item {
        Item { sender: who.into(), timestamp_ms: at, reply_to: None, body: Body::Text(text.into()) }
    }

    #[test]
    fn the_first_line_has_the_date_and_the_rest_only_the_time() {
        let items = [item("alice", T, "hello"), item("bob", T + MIN, "hi there"), item("alice", T + 2 * MIN, "bye")];
        assert_eq!(
            transcript_in("en", &items, &TimeZone::UTC),
            "[Fri 21 Nov 14:32] alice: hello\n[14:33] bob: hi there\n[14:34] alice: bye"
        );
    }

    #[test]
    fn the_date_shows_again_when_the_day_changes() {
        let items = [item("alice", T, "late"), item("bob", T + DAY, "next day"), item("bob", T + DAY + MIN, "again")];
        assert_eq!(
            transcript_in("en", &items, &TimeZone::UTC),
            "[Fri 21 Nov 14:32] alice: late\n[Sat 22 Nov 14:32] bob: next day\n[14:33] bob: again"
        );
    }

    #[test]
    fn the_day_follows_the_time_zone() {
        let new_york = TimeZone::get("America/New_York").expect("zone");
        // 03:00 UTC on the 22nd is the evening of the 21st in New York.
        let at = T + 12 * 3_600_000 + 28 * MIN;
        assert_eq!(transcript_in("en", &[item("a", at, "x")], &new_york), "[Fri 21 Nov 22:00] a: x");
    }

    #[test]
    fn replies_and_attachments_are_noted() {
        let mut reply = item("bob", T + MIN, "agreed");
        reply.reply_to = Some("alice".into());
        let pic = Item { sender: "alice".into(), timestamp_ms: T + 2 * MIN, reply_to: None, body: Body::Image("cat.png".into()) };
        let file = Item { body: Body::File("notes.pdf".into()), ..pic.clone() };
        assert_eq!(
            transcript_in("en", &[item("alice", T, "ok?"), reply, pic, file], &TimeZone::UTC),
            "[Fri 21 Nov 14:32] alice: ok?\n[14:33] bob (replying to alice): agreed\n[14:34] alice: [image: cat.png]\n[14:34] alice: [file: notes.pdf]"
        );
    }

    #[test]
    fn the_words_are_the_languages() {
        let mut reply = item("bob", T + MIN, "vale");
        reply.reply_to = Some("alice".into());
        let pic = Item { sender: "alice".into(), timestamp_ms: T + 2 * MIN, reply_to: None, body: Body::Image("gato.png".into()) };
        assert_eq!(
            transcript_in("es", &[item("alice", T, "hola"), reply, pic], &TimeZone::UTC),
            "[vie 21 nov 14:32] alice: hola\n[14:33] bob (respondiendo a alice): vale\n[14:34] alice: [imagen: gato.png]"
        );
    }

    #[test]
    fn multi_line_text_keeps_its_lines() {
        assert_eq!(transcript_in("en", &[item("a", T, "one\ntwo")], &TimeZone::UTC), "[Fri 21 Nov 14:32] a: one\ntwo");
    }
}
