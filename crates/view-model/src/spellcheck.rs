//! Spell checking for the message box: which words are misspelled, what to
//! offer instead, and the person's own words. It runs locally (a Hunspell
//! dictionary, en_US from SCOWL, is part of the build; `spellbook` reads it in
//! pure Rust) and has no UI toolkit in it, so every client can use it.
//!
//! What is checked is plain prose: code spans and blocks, links, mentions and
//! room aliases, `:shortcodes:`, web addresses, words with digits, acronyms
//! and camelCase names are left alone. [`words`] finds the checkable words
//! without a dictionary; [`Spell::misspellings`] runs them through it.

use std::ops::Range;

use spellbook::Dictionary;

const AFF: &str = include_str!("../dict/en_US.aff");
const DIC: &str = include_str!("../dict/en_US.dic");

/// Longest text checked (bytes), so a pasted wall of text cannot stall typing.
const MAX_CHECKED: usize = 20_000;
/// Most suggestions offered.
pub const MAX_SUGGESTIONS: usize = 5;

/// The dictionary, with the words the person added.
pub struct Spell {
    dict: Dictionary,
}

impl Spell {
    /// The bundled English dictionary (takes a moment to build: do it off the
    /// interface thread).
    pub fn english() -> Self {
        let dict = Dictionary::new(AFF, DIC).expect("the bundled dictionary is well formed");
        Self { dict }
    }

    /// A word of the person's own (or one ignored for now): never flagged again.
    pub fn add_word(&mut self, word: &str) {
        let word = word.trim();
        if !word.is_empty() && !word.contains('/') {
            let _ = self.dict.add(word);
        }
    }

    pub fn is_correct(&self, word: &str) -> bool {
        self.dict.check(&normalise(word))
    }

    /// Up to [`MAX_SUGGESTIONS`] corrections for `word`, best first.
    pub fn suggest(&self, word: &str) -> Vec<String> {
        let mut out = Vec::new();
        self.dict.suggest(&normalise(word), &mut out);
        out.truncate(MAX_SUGGESTIONS);
        out
    }

    /// The byte ranges of the misspelled words in `text`.
    pub fn misspellings(&self, text: &str) -> Vec<Range<usize>> {
        words(text)
            .into_iter()
            .filter(|range| !self.is_correct(&text[range.clone()]))
            .collect()
    }
}

/// Curly apostrophes become straight ones, which is how the dictionary has them.
fn normalise(word: &str) -> String {
    word.replace('\u{2019}', "'")
}

/// Whether a word that ends at `end` is still being typed, with the caret at
/// `caret`: the caret is right after it with nothing typed past it.
pub fn being_typed(end: usize, caret: usize) -> bool {
    end == caret
}

fn is_apostrophe(c: char) -> bool {
    c == '\'' || c == '\u{2019}'
}

/// Parts of the text that are not prose: code, and the address of a link.
fn skipped(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'`' => {
                let start = i;
                while i < bytes.len() && bytes[i] == b'`' {
                    i += 1;
                }
                let ticks = i - start;
                // Up to the closing run of as many backticks; open ended, to the end.
                let mut j = i;
                let mut close = None;
                while j < bytes.len() {
                    if bytes[j] == b'`' {
                        let from = j;
                        while j < bytes.len() && bytes[j] == b'`' {
                            j += 1;
                        }
                        if j - from == ticks {
                            close = Some(j);
                            break;
                        }
                    } else {
                        j += 1;
                    }
                }
                match close {
                    Some(end) => {
                        out.push(start..end);
                        i = end;
                    }
                    // A lone backtick is just a character; a fence with no end
                    // is code to the end.
                    None if ticks >= 3 => {
                        out.push(start..bytes.len());
                        i = bytes.len();
                    }
                    None => {}
                }
            }
            b']' if bytes.get(i + 1) == Some(&b'(') => {
                if let Some(close) = text[i + 2..].find(')') {
                    out.push(i..i + 2 + close + 1);
                    i += 2 + close + 1;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    out
}

/// Whether a whitespace-separated chunk is something other than prose.
fn not_prose(chunk: &str) -> bool {
    let lower = chunk.to_ascii_lowercase();
    if lower.contains("://")
        || lower.starts_with("www.")
        || lower.starts_with("matrix:")
        || lower.starts_with("mailto:")
        || chunk.contains('@')
        || chunk.contains('/')
        || chunk.contains('\\')
        || chunk.contains('_')
    {
        return true;
    }
    // A mention, room alias or room id: `@name`, `#room`, `!id`, also in `<...>`.
    let bare = chunk.trim_start_matches(['<', '(', '[', '"', '\'']);
    if bare.starts_with(['#', '!', '@']) {
        return true;
    }
    // example.com, file.rs: a full stop between two letters or digits.
    let chars: Vec<char> = chunk.chars().collect();
    chars
        .windows(3)
        .any(|w| w[1] == '.' && w[0].is_alphanumeric() && w[2].is_alphanumeric())
}

/// Whether a word is a name or acronym the dictionary cannot be asked about.
fn odd_case(word: &str) -> bool {
    let upper = word.chars().filter(|c| c.is_uppercase()).count();
    if upper >= 2 && !word.chars().any(char::is_lowercase) {
        return true; // ACRONYM
    }
    // camelCase, iPhone: a capital after a lower case letter.
    let mut prev_lower = false;
    for c in word.chars() {
        if prev_lower && c.is_uppercase() {
            return true;
        }
        prev_lower = c.is_lowercase();
    }
    false
}

/// The byte ranges of the words worth checking in `text`.
pub fn words(text: &str) -> Vec<Range<usize>> {
    let text = if text.len() > MAX_CHECKED {
        let mut cut = MAX_CHECKED;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        &text[..cut]
    } else {
        text
    };
    let skip = skipped(text);
    let in_skip = |at: usize| skip.iter().any(|r| r.contains(&at));
    let mut out = Vec::new();
    let mut chunk_start: Option<usize> = None;
    let mut flush = |start: usize, end: usize, out: &mut Vec<Range<usize>>| {
        // `[text](url)`: the address is not prose, the text of the link is.
        let end = text[start..end].find("](").map_or(end, |at| start + at);
        let chunk = &text[start..end];
        if not_prose(chunk) {
            return;
        }
        let mut word: Option<usize> = None;
        let mut has_digit = false;
        let mut shortcode_end = 0;
        let chars: Vec<(usize, char)> = chunk.char_indices().collect();
        let mut finish = |from: usize, to: usize, digit: bool, out: &mut Vec<Range<usize>>| {
            let at = start + from;
            let w = &text[at..start + to];
            if digit || in_skip(at) || w.chars().count() < 2 || odd_case(w) {
                return;
            }
            out.push(at..start + to);
        };
        for (n, &(i, c)) in chars.iter().enumerate() {
            // :shortcode: and :custom_emoji:
            if c == ':' && i >= shortcode_end {
                let rest = &chunk[i + 1..];
                if let Some(close) = rest.find(':')
                    && close > 0
                    && rest[..close]
                        .chars()
                        .all(|c| c.is_alphanumeric() || matches!(c, '_' | '+' | '-'))
                {
                    if let Some(from) = word.take() {
                        finish(from, i, has_digit, out);
                    }
                    shortcode_end = i + 1 + close + 1;
                    has_digit = false;
                    continue;
                }
            }
            if i < shortcode_end {
                continue;
            }
            let next_letter = chars.get(n + 1).is_some_and(|&(_, d)| d.is_alphabetic());
            let inside = word.is_some() && is_apostrophe(c) && next_letter;
            if c.is_alphabetic() || c.is_ascii_digit() || inside {
                if word.is_none() {
                    word = Some(i);
                    has_digit = false;
                }
                has_digit |= c.is_ascii_digit();
            } else if let Some(from) = word.take() {
                finish(from, i, has_digit, out);
                has_digit = false;
            }
        }
        if let Some(from) = word.take() {
            finish(from, chunk.len(), has_digit, out);
        }
    };
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            if let Some(start) = chunk_start.take() {
                flush(start, i, &mut out);
            }
        } else if chunk_start.is_none() {
            chunk_start = Some(i);
        }
    }
    if let Some(start) = chunk_start {
        flush(start, text.len(), &mut out);
    }
    out
}

const FILE: &str = "spellcheck.json";

/// What is kept in the data directory: whether checking is on (it is, until
/// turned off) and the words added to the dictionary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub enabled: bool,
    pub words: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { enabled: true, words: Vec::new() }
    }
}

pub fn load_settings(dir: &std::path::Path) -> Settings {
    let value = std::fs::read_to_string(dir.join(FILE))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    let Some(value) = value else {
        return Settings::default();
    };
    Settings {
        enabled: value.get("enabled").and_then(serde_json::Value::as_bool).unwrap_or(true),
        words: value
            .get("words")
            .and_then(serde_json::Value::as_array)
            .map(|list| {
                list.iter().filter_map(|w| w.as_str().map(str::to_owned)).collect()
            })
            .unwrap_or_default(),
    }
}

pub fn save_settings(dir: &std::path::Path, settings: &Settings) -> std::io::Result<()> {
    std::fs::write(
        dir.join(FILE),
        serde_json::json!({ "enabled": settings.enabled, "words": settings.words }).to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Vec<&str> {
        words(text).into_iter().map(|r| &text[r]).collect()
    }

    fn wrong(spell: &Spell, text: &str) -> Vec<String> {
        spell.misspellings(text).into_iter().map(|r| text[r].to_owned()).collect()
    }

    #[test]
    fn words_are_the_letters_between_punctuation() {
        assert_eq!(found("Hello, wrold! It's fine."), ["Hello", "wrold", "It's", "fine"]);
        assert_eq!(found("don\u{2019}t stop"), ["don\u{2019}t", "stop"]);
        assert_eq!(found("well-known"), ["well", "known"]);
    }

    #[test]
    fn code_links_mentions_and_shortcodes_are_left_alone() {
        assert_eq!(found("a `cdoe span` here"), ["here"]);
        assert_eq!(found("```\nlet xyzzy = 1;\n```\nafter"), ["after"]);
        assert_eq!(found("see https://exmaple.com/pgae now"), ["see", "now"]);
        assert_eq!(found("see [the dcos](https://exmaple.org/x) now"), ["see", "the", "dcos", "now"]);
        assert_eq!(found("hi @alcie:exmaple.org and #rooom:server ok"), ["hi", "and", "ok"]);
        assert_eq!(found("hi @alcie there"), ["hi", "there"]);
        assert_eq!(found("go :smiel: now :cusotm_emoji: x"), ["go", "now"]);
        assert_eq!(found("open file.rs or example.com"), ["open", "or"]);
    }

    #[test]
    fn names_acronyms_and_numbers_are_left_alone() {
        assert_eq!(found("HTTP iPhone GitHub abc123 a I x2"), Vec::<&str>::new());
    }

    #[test]
    fn an_unclosed_backtick_is_just_a_character() {
        assert_eq!(found("it`s fine"), ["it", "fine"]);
    }

    #[test]
    fn misspellings_are_flagged_and_suggestions_offered() {
        let spell = Spell::english();
        assert!(wrong(&spell, "This is a fine sentence.").is_empty());
        let text = "teh quick brwon fox";
        assert_eq!(wrong(&spell, text), ["teh", "brwon"]);
        assert!(spell.suggest("teh").iter().any(|s| s == "the"), "{:?}", spell.suggest("teh"));
        assert!(spell.suggest("brwon").len() <= MAX_SUGGESTIONS);
        // Capitalised words and apostrophes are fine.
        assert!(wrong(&spell, "Hello don\u{2019}t You're").is_empty());
    }

    #[test]
    fn an_added_word_is_not_flagged() {
        let mut spell = Spell::english();
        assert_eq!(wrong(&spell, "morpheusly"), ["morpheusly"]);
        spell.add_word("morpheusly");
        assert!(wrong(&spell, "morpheusly").is_empty());
    }

    #[test]
    fn the_word_being_typed_waits_for_a_space_or_punctuation() {
        let text = "teh brwo";
        let spell = Spell::english();
        let flagged: Vec<_> = spell
            .misspellings(text)
            .into_iter()
            .filter(|r| !being_typed(r.end, text.len()))
            .collect();
        assert_eq!(flagged, [0..3]);
    }

    #[test]
    fn a_long_text_is_checked_quickly() {
        let spell = Spell::english();
        let text = "the quick brown fox jumps over the lazy dog ".repeat(400);
        let start = std::time::Instant::now();
        assert!(spell.misspellings(&text).is_empty());
        assert!(start.elapsed() < std::time::Duration::from_secs(2), "{:?}", start.elapsed());
    }

    #[test]
    fn settings_are_kept_and_on_by_default() {
        let dir = std::env::temp_dir().join(format!("morpheus-spell-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        assert_eq!(load_settings(&dir), Settings::default());
        let kept = Settings { enabled: false, words: vec!["morpheus".into()] };
        save_settings(&dir, &kept).expect("save");
        assert_eq!(load_settings(&dir), kept);
        std::fs::remove_dir_all(&dir).ok();
    }
}
