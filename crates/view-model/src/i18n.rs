//! Translations. The app's text is written in English and the English text is the key:
//! `t("Theme")` is "Theme" in English and whatever `locales/<code>.json` says for "Theme"
//! in another language, and a string with no translation shows in English, so a language
//! can be added a screen at a time and nothing is ever blank.
//!
//! The crate is `rust-i18n`: the locale files are read at compile time and embedded (no
//! files to ship or find, so it works the same on desktop, the web and the phone apps
//! that reach this crate through UniFFI), lookups are a hash lookup, and the English-text
//! keys keep the code readable where Fluent-style ids (`settings-appearance-theme`)
//! would hide what is on screen. The language picked is global state (`set_language`),
//! read when something is drawn, so a change shows at the next frame.
//!
//! A number goes in with `tf("Back to %{account}", &[("account", name)])`. A plural is two
//! keys ("1 member", "%{count} members") chosen by [`plural`], since English and Spanish
//! both have one and many; a language with more forms gets its own rule here.
//!
//! Missing translations are test failures: every key the code uses must be in every
//! locale file, with the same `%{...}` placeholders, and no file may keep a key the code
//! no longer uses (see the tests).

use std::sync::RwLock;

rust_i18n::i18n!("locales", fallback = "en");

/// A language the app can be shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Language {
    /// The BCP 47 code, which is also the locale file's name (`en` has no file).
    pub code: &'static str,
    /// Its own name, as its speakers write it: what the picker shows.
    pub name: &'static str,
}

/// Every language, English first (the source; every other one is a file in `locales/`).
pub const LANGUAGES: &[Language] = &[
    Language { code: "en", name: "English" },
    Language { code: "es", name: "Español" },
];

/// The source language, shown wherever a translation is missing.
pub const SOURCE: &str = "en";

static CURRENT: RwLock<&'static str> = RwLock::new(SOURCE);

/// The language in use.
pub fn language() -> &'static str {
    CURRENT.read().map(|c| *c).unwrap_or(SOURCE)
}

/// Shows the app in `code` (an unknown one is English).
pub fn set_language(code: &str) {
    let lang = LANGUAGES.iter().find(|l| l.code == code).map(|l| l.code).unwrap_or(SOURCE);
    if let Ok(mut current) = CURRENT.write() {
        *current = lang;
    }
}

/// The text for `key` in the language in use.
pub fn t(key: &str) -> String {
    t_in(language(), key)
}

/// The text for `key` in `code`: English where that language has no entry.
pub fn t_in(code: &str, key: &str) -> String {
    if code == SOURCE {
        return key.to_owned();
    }
    let found = _rust_i18n_try_translate(code, key);
    found.map(|s| s.into_owned()).unwrap_or_else(|| key.to_owned())
}

/// [`t`] with `%{name}` placeholders filled from `args`.
pub fn tf(key: &str, args: &[(&str, &str)]) -> String {
    fill(t(key), args)
}

pub fn tf_in(code: &str, key: &str, args: &[(&str, &str)]) -> String {
    fill(t_in(code, key), args)
}

fn fill(mut text: String, args: &[(&str, &str)]) -> String {
    for (name, value) in args {
        text = text.replace(&format!("%{{{name}}}"), value);
    }
    text
}

/// `one` for 1, `other` for the rest, as the key to translate: `t(plural(n, "1 member",
/// "%{count} members"))`.
pub fn plural<'a>(n: u64, one: &'a str, other: &'a str) -> &'a str {
    if n == 1 { one } else { other }
}

/// What the person picked: the system's language (the default), or one of ours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Choice {
    #[default]
    System,
    Fixed(&'static str),
}

impl Choice {
    /// What is saved: `system` or a language code.
    pub fn key(self) -> &'static str {
        match self {
            Choice::System => "system",
            Choice::Fixed(code) => code,
        }
    }

    pub fn from_key(key: &str) -> Choice {
        LANGUAGES
            .iter()
            .find(|l| l.code == key.trim())
            .map(|l| Choice::Fixed(l.code))
            .unwrap_or_default()
    }

    /// The language to show, given the system's languages (most wanted first).
    pub fn resolve(self, system: &[String]) -> &'static str {
        match self {
            Choice::Fixed(code) => code,
            Choice::System => from_system(system),
        }
    }
}

/// The first of the system's languages that we have, by its primary subtag
/// (`es_MX.UTF-8`, `es-419` and `es` are all Spanish); English when none is.
pub fn from_system(system: &[String]) -> &'static str {
    for tag in system {
        let primary = tag
            .split(['_', '-', '.', '@'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if let Some(l) = LANGUAGES.iter().find(|l| l.code == primary) {
            return l.code;
        }
    }
    SOURCE
}

/// The system's languages, most wanted first. `MORPHEUS_LANGUAGE` (a code) overrides it,
/// for tests and for a system that is wrong about itself.
pub fn system_languages() -> Vec<String> {
    if let Ok(forced) = std::env::var("MORPHEUS_LANGUAGE")
        && !forced.trim().is_empty()
    {
        return vec![forced];
    }
    sys_locale::get_locales().collect()
}

/// Keys the code hands to `t` from a variable rather than a literal (the labels of the
/// settings pages and search entries, themes, notification choices and menus), for the
/// tests that every one is translated and none is stale.
pub fn dynamic_keys() -> Vec<&'static str> {
    use crate::settings::{NotificationContent, SETTINGS_INDEX, SettingsPage, Theme};
    let mut keys: Vec<&'static str> = Vec::new();
    keys.extend(SettingsPage::ALL.iter().map(|p| p.label()));
    keys.extend(SETTINGS_INDEX.iter().map(|e| e.label));
    keys.extend(Theme::ALL.iter().map(|t| t.label()));
    keys.extend(NotificationContent::ALL.iter().map(|c| c.label()));
    keys.extend(crate::menu::translatable_labels());
    keys.extend(crate::settings::translatable_labels());
    keys
}

const FILE: &str = "language.txt";

/// The saved choice (this device's, in the data folder).
pub fn load_choice(dir: &std::path::Path) -> Choice {
    std::fs::read_to_string(dir.join(FILE))
        .map(|text| Choice::from_key(&text))
        .unwrap_or_default()
}

pub fn save_choice(dir: &std::path::Path, choice: Choice) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(FILE), choice.key())
}

/// Reads the saved choice and the system's language and shows the app in the one that
/// applies. Called once at start, before anything is drawn.
pub fn init(dir: &std::path::Path) -> Choice {
    let choice = load_choice(dir);
    set_language(choice.resolve(&system_languages()));
    choice
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    fn locale(code: &str) -> BTreeMap<String, String> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("locales").join(format!("{code}.json"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let value: serde_json::Value = serde_json::from_str(&text).expect("valid json");
        value
            .as_object()
            .expect("an object")
            .iter()
            .filter(|(k, _)| !k.starts_with('_'))
            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_else(|| panic!("{k} is not text")).to_owned()))
            .collect()
    }

    fn placeholders(text: &str) -> Vec<String> {
        let mut found: Vec<String> = text
            .split("%{")
            .skip(1)
            .filter_map(|rest| rest.split_once('}').map(|(name, _)| name.to_owned()))
            .collect();
        found.sort();
        found
    }

    fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("dir").flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n != "e2e" && n != "target") {
                    rust_files(&path, found);
                }
            } else if path.extension().is_some_and(|e| e == "rs") {
                found.push(path);
            }
        }
    }

    /// The functions whose first argument, when a string literal, is text the app shows
    /// (and so a key): `t` and `tf`, and the settings screens' helpers that translate
    /// what they are given.
    const CALLS: &[&str] = &["t", "tf", "muted", "note", "section", "section_classed", "group", "label"];

    /// Every such literal in the workspace's UI code: the keys the app uses.
    pub(crate) fn keys_used() -> Vec<(String, String)> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut files = Vec::new();
        rust_files(&root.join("apps/desktop/src"), &mut files);
        rust_files(&root.join("crates/view-model/src"), &mut files);
        rust_files(&root.join("crates/ffi/src"), &mut files);
        let mut keys = Vec::new();
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            // Test code names keys that are missing on purpose; comments show examples.
            let code: String = text
                .split("#[cfg(test)]")
                .next()
                .unwrap_or_default()
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            let bytes = code.as_bytes();
            for name in CALLS {
                let call = format!("{name}(");
                let mut from = 0;
                while let Some(found) = code[from..].find(&call) {
                    let at = from + found;
                    from = at + call.len();
                    let before = code[..at].chars().next_back();
                    // `get(`, `format(` and `.t(` are not ours; a free function call is.
                    if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                        continue;
                    }
                    let mut i = from;
                    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    if bytes.get(i) != Some(&b'"') {
                        continue;
                    }
                    let mut key = String::new();
                    let mut chars = code[i + 1..].chars();
                    while let Some(c) = chars.next() {
                        match c {
                            '\\' => match chars.next() {
                                Some('n') => key.push('\n'),
                                Some('u') => {
                                    // \u{2019}
                                    let hex: String = chars.by_ref().skip(1).take_while(|c| *c != '}').collect();
                                    key.push(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32).unwrap_or('?'));
                                }
                                Some(other) => key.push(other),
                                None => break,
                            },
                            '"' => break,
                            c => key.push(c),
                        }
                    }
                    keys.push((key, file.display().to_string()));
                }
            }
        }
        keys.extend(kotlin_keys(&root.join("apps/android/app/src/main/kotlin")));
        keys.extend(super::dynamic_keys().into_iter().map(|k| (k.to_owned(), "a label in view-model".to_owned())));
        keys
    }

    /// The literals the Android app gives to `t("...")` (apps/android: `Words.kt`): also keys.
    fn kotlin_keys(dir: &Path) -> Vec<(String, String)> {
        let mut keys = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else { return keys };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                keys.extend(kotlin_keys(&path));
            } else if path.extension().is_some_and(|e| e == "kt") {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                let mut from = 0;
                while let Some(found) = text[from..].find("t(\"") {
                    let at = from + found;
                    from = at + 3;
                    let before = text[..at].chars().next_back();
                    if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                    let Some(end) = text[from..].find('"') else { break };
                    let key = &text[from..from + end];
                    if !key.contains('$') && !key.contains('\\') {
                        keys.push((key.to_owned(), path.display().to_string()));
                    }
                }
            }
        }
        keys
    }

    #[test]
    fn a_missing_translation_shows_english_not_nothing() {
        assert_eq!(t_in("es", "A string nobody translated"), "A string nobody translated");
        assert_eq!(t_in("fr", "Theme"), "Theme", "no such language");
        assert_eq!(t_in("en", "Theme"), "Theme");
    }

    #[test]
    fn a_translated_string_shows_in_its_language() {
        assert_eq!(t_in("es", "Theme"), "Tema");
    }

    #[test]
    fn placeholders_are_filled_in_every_language() {
        assert_eq!(tf_in("en", "Back to %{account}", &[("account", "carol")]), "Back to carol");
        assert_eq!(tf_in("es", "Back to %{account}", &[("account", "carol")]), "Volver a carol");
        // A placeholder with no translation is still filled, in English.
        assert_eq!(tf_in("es", "Hello %{a} and %{b}", &[("a", "x"), ("b", "y")]), "Hello x and y");
    }

    #[test]
    fn plurals_pick_one_or_many() {
        assert_eq!(plural(1, "1 member", "%{count} members"), "1 member");
        assert_eq!(plural(0, "1 member", "%{count} members"), "%{count} members");
        assert_eq!(plural(2, "1 member", "%{count} members"), "%{count} members");
    }

    #[test]
    fn the_system_language_is_matched_by_its_first_subtag() {
        let s = |tags: &[&str]| tags.iter().map(|t| t.to_string()).collect::<Vec<_>>();
        assert_eq!(from_system(&s(&["es_MX.UTF-8"])), "es");
        assert_eq!(from_system(&s(&["es-419"])), "es");
        assert_eq!(from_system(&s(&["ES"])), "es");
        assert_eq!(from_system(&s(&["fr-FR", "es-ES", "en-US"])), "es", "the first one we have");
        assert_eq!(from_system(&s(&["fr-FR", "de"])), "en", "none we have: English");
        assert_eq!(from_system(&s(&["C"])), "en");
        assert_eq!(from_system(&[]), "en");
    }

    #[test]
    fn a_fixed_choice_beats_the_system_and_system_follows_it() {
        let spanish = vec!["es-ES".to_string()];
        assert_eq!(Choice::System.resolve(&spanish), "es");
        assert_eq!(Choice::Fixed("en").resolve(&spanish), "en");
        assert_eq!(Choice::from_key("es"), Choice::Fixed("es"));
        assert_eq!(Choice::from_key("system"), Choice::System);
        assert_eq!(Choice::from_key("klingon"), Choice::System);
        for c in [Choice::System, Choice::Fixed("en"), Choice::Fixed("es")] {
            assert_eq!(Choice::from_key(c.key()), c);
        }
    }

    #[test]
    fn the_choice_is_kept_in_the_data_folder() {
        let dir = std::env::temp_dir().join(format!("morpheus-lang-{}", std::process::id()));
        assert_eq!(load_choice(&dir), Choice::System, "nothing saved yet");
        save_choice(&dir, Choice::Fixed("es")).expect("save");
        assert_eq!(load_choice(&dir), Choice::Fixed("es"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn every_language_but_english_has_a_locale_file() {
        for l in LANGUAGES.iter().filter(|l| l.code != SOURCE) {
            assert!(!locale(l.code).is_empty(), "{} has no translations", l.name);
        }
    }

    #[test]
    fn every_key_the_code_uses_is_translated_in_every_language() {
        let used = keys_used();
        assert!(used.len() > 20, "found only {} keys: the scan is broken", used.len());
        for l in LANGUAGES.iter().filter(|l| l.code != SOURCE) {
            let have = locale(l.code);
            let missing: Vec<String> = used
                .iter()
                .filter(|(k, _)| !have.contains_key(k))
                .map(|(k, f)| format!("{k:?} ({f})"))
                .collect();
            assert!(missing.is_empty(), "missing from {}.json:\n{}", l.code, missing.join("\n"));
        }
    }

    #[test]
    fn no_locale_file_keeps_a_key_the_code_no_longer_uses() {
        let used: std::collections::BTreeSet<String> = keys_used().into_iter().map(|(k, _)| k).collect();
        for l in LANGUAGES.iter().filter(|l| l.code != SOURCE) {
            let stale: Vec<String> = locale(l.code).into_keys().filter(|k| !used.contains(k)).collect();
            assert!(stale.is_empty(), "{}.json has keys nothing uses:\n{}", l.code, stale.join("\n"));
        }
    }

    #[test]
    fn a_translation_keeps_the_placeholders_of_its_key() {
        for l in LANGUAGES.iter().filter(|l| l.code != SOURCE) {
            for (key, text) in locale(l.code) {
                assert_eq!(placeholders(&key), placeholders(&text), "{}: {key:?} -> {text:?}", l.code);
                assert!(!text.trim().is_empty(), "{}: {key:?} is empty", l.code);
            }
        }
    }
}
