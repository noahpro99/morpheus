//! The check that keeps the UI translated (#240): a string literal in the desktop app that looks
//! like text for people (a capital letter and words, or a capitalised word) and is not handed to
//! `t`, `tf`, `tn` or a helper that translates what it is given fails the test, so a new
//! hardcoded English string cannot slip in. A literal that is right as it is (a brand, a key
//! name, text the app only logs) is added to [`ALLOWED`] with the reason; an entry nothing uses
//! any more fails too, so the list cannot grow stale.

use std::path::{Path, PathBuf};

/// Calls that translate their (literal) argument, or whose literal is never shown: not text
/// to translate here. `t(..)` and friends are keys (checked in `i18n::tests`); the rest are
/// ids, classes, log lines, test-bridge attributes and the like.
const NOT_SHOWN_OR_TRANSLATED: &[&str] = &[
    "t", "ts", "tf", "tn", "muted", "note", "section", "section_classed", "group", "label", "attr", "e2e", "text", "expect",
    "env", "var", "arg", "args", "join", "push_str", "name", "debug", "info", "warn", "error", "trace", "span",
    "panic", "unreachable", "assert", "assert_eq", "assert_ne", "debug_assert", "println", "eprintln", "print",
    "eprint", "write", "writeln", "include_str", "open", "get", "insert", "contains", "starts_with", "ends_with",
    "split", "strip_prefix", "strip_suffix", "replace", "find", "status", "spawn", "class", "id", "key",
    "rename", "json", "from_str", "parse", "Command::new", "button", "ElementId", "new", "from", "with_context",
    "context", "unwrap_or", "unwrap_or_else", "log", "format_args", "todo", "env!", "concat", "stringify",
    "map_err", "ok_or", "ok_or_else", "set_var", "remove_var", "from_name", "named", "eq", "ne",
];

/// Files whose text is shown another way or is not for people: diagnostics (translated on screen
/// by `view_model::diagnostics::translatable_labels`, English in the copied report).
const FILES_NOT_SCANNED: &[&str] = &["diagnostics.rs"];

/// Literals that are right in English, with why: in every file.
const ALLOWED: &[&str] = &[
    // Brand and product names.
    "Morpheus",
    // Font names.
    "Inter", "Unbounded",
    // Key caps and key names (the same on every keyboard layout we draw them for).
    "Esc", "Enter", "Tab", "Ctrl", "Shift", "Alt", "ArrowUp", "ArrowDown",
    // Environment variables and registry names, read from the system.
    "PATH", "TMPDIR", "HOME", "RssAnon:", "VmRSS:", "SystemUsesLightTheme",
];

/// Literals that are right in English in one file: (file, text, why).
const ALLOWED_IN: &[(&str, &str, &str)] = &[
    ("about.rs", "Morpheus {VERSION}", "the product name and version"),
    ("badge.rs", "Update", "a D-Bus property name"),
    ("chrome.rs", "Minimize", "a match key and the test bridge's title attribute, never drawn"),
    ("chrome.rs", "Maximize", "a match key and the test bridge's title attribute, never drawn"),
    ("chrome.rs", "Close", "a match key and the test bridge's title attribute, never drawn"),
    ("call_invite.rs", "Join me in [{name}]({})", "the message sent to the room: the sender's words, not the app's"),
    ("room_dialogs.rs", "Join me in [{name}]({link})", "the message sent to the room: the sender's words, not the app's"),
    ("input.rs", "INSERT", "vim's own mode names"),
    ("input.rs", "NORMAL", "vim's own mode names"),
    ("input.rs", "VISUAL", "vim's own mode names"),
    ("log_file.rs", "Room is unknown", "matches an SDK log message, never shown"),
    ("log_file.rs", "PANIC: thread '{name}' {info}\\nbacktrace:\\n{backtrace}\\n", "the log file, which stays English"),
    ("main.rs", "Morpheus (test)", "the test build's window title"),
    ("profile.rs", "Open {}", "the test bridge's title attribute, never drawn"),
    ("status_feed.rs", "Morpheus closed the connection", "an error line for the activity client"),
    ("status_feed.rs", "Morpheus is not running", "an error line for the activity client"),
    ("prefs.rs", "New folder", "stored as the folder's own name when it is made"),
    ("timeline.rs", "Hi! \u{1f44b}", "sample text in the theme preview"),
    ("composer.rs", "Up in an empty box asks to edit", "a test name"),
    ("composer.rs", "Up with text asks for nothing", "a test name"),
    ("input.rs", "Enter leaves the text to its owner", "a test name"),
    ("input.rs", "Up with text moves the caret", "a test name"),
    ("lightbox.rs", "Fit to window", "the test bridge's title attribute, never drawn"),
    ("lightbox.rs", "Fit", "the test bridge's title attribute, never drawn"),
    ("lightbox.rs", "Next image", "the test bridge's title attribute, never drawn"),
    ("lightbox.rs", "Previous image", "the test bridge's title attribute, never drawn"),
    ("lightbox.rs", "Zoom in", "the test bridge's title attribute, never drawn"),
    ("lightbox.rs", "Zoom out", "the test bridge's title attribute, never drawn"),
    ("mac_menu.rs", "TextInput", "a class name macOS looks up"),
    ("call_ui.rs", "none: {why}", "a test-bridge attribute value, never drawn"),
    ("room_list.rs", "section {class}", "a css-like class"),
    ("screen_quality.rs", "{class} active", "a css-like class"),
    ("switcher.rs", "{classes} active", "a css-like class"),
    ("status_feed.rs", "reading the status failed: {err}", "an error line for the activity client"),
    ("tray.rs", "unsupported tray png colour type {other:?}", "a log line"),
    ("update_self_test.rs", "no update offered for this platform: {status:?}", "a self-test line, never shown"),
    ("update_self_test.rs", "updated from process {from}; now {}\\n", "a self-test line, never shown"),
    ("menu.rs", "React with {key}", "the test bridge's title attribute, never drawn"),
    ("settings/pages.rs", "USERPROFILE", "a Windows environment variable"),
];

/// Whether `lit` reads as text for people.
fn text_like(lit: &str) -> bool {
    let mut plain = String::new();
    let mut depth = 0;
    for c in lit.chars() {
        match c {
            '{' => depth += 1,
            '}' if depth > 0 => depth -= 1,
            c if depth == 0 => plain.push(c),
            _ => {}
        }
    }
    let mut run = 0;
    let mut letters = false;
    for c in plain.chars() {
        if c.is_alphabetic() {
            run += 1;
            if run >= 2 {
                letters = true;
            }
        } else {
            run = 0;
        }
    }
    if !letters || plain.contains("::") || plain.contains('/') || plain.contains('_') || plain.contains("://") {
        return false;
    }
    let first = plain.trim_start().chars().next().unwrap_or(' ');
    let words = plain.split_whitespace().count();
    // "Save", "Copy link", "A card under a message": a capital start, and either more than one
    // word or a word of three letters or more. Lowercase words are ids, css and the like.
    first.is_uppercase() && (words > 1 || plain.trim().chars().count() >= 3)
}

/// A `format!` literal that reads as words around its placeholders even without a capital start:
/// "{n} unread", "in {room}: new". Two or more words of three letters or more, or one word next to
/// a placeholder, outside what is not text (paths, ids).
fn sentence_in_format(lit: &str) -> bool {
    if lit.contains("::") || lit.contains('/') || lit.contains('_') || lit.contains('=') || lit.contains('-') {
        return false;
    }
    let mut plain = String::new();
    let mut depth = 0;
    for c in lit.chars() {
        match c {
            '{' => depth += 1,
            '}' if depth > 0 => depth -= 1,
            c if depth == 0 => plain.push(c),
            _ => {}
        }
    }
    let words = plain.split_whitespace().filter(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 3).count();
    words >= 1 && lit.contains('{') && lit.contains(' ')
}

/// The name of the call a literal is an argument of: the identifier (with a `!` for a macro)
/// before the nearest unclosed `(` or `[` going backwards.
fn enclosing_call(code: &[u8], at: usize) -> String {
    let mut depth = 0i32;
    let mut i = at;
    while i > 0 {
        i -= 1;
        match code[i] {
            b')' | b']' | b'}' => depth += 1,
            b'(' | b'[' | b'{' => {
                if depth == 0 {
                    if code[i] != b'(' {
                        return String::new();
                    }
                    let mut end = i;
                    if end > 0 && code[end - 1] == b'!' {
                        end -= 1;
                    }
                    while end > 0 && code[end - 1].is_ascii_whitespace() {
                        end -= 1;
                    }
                    let mut start = end;
                    while start > 0 && (code[start - 1].is_ascii_alphanumeric() || code[start - 1] == b'_' || code[start - 1] == b':') {
                        start -= 1;
                    }
                    let name = std::str::from_utf8(&code[start..end]).unwrap_or_default().to_owned();
                    return name.rsplit("::").next().unwrap_or_default().to_owned();
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    String::new()
}

/// Every text-like literal of the file's non-test code with the call it is an argument of.
fn literals(text: &str) -> Vec<(usize, String, String)> {
    let code = text.split("#[cfg(test)]").next().unwrap_or_default();
    // Blank out comments (keeping line numbers).
    let mut clean = String::with_capacity(code.len());
    for line in code.lines() {
        let mut in_str = false;
        let mut prev = ' ';
        let mut cut = line.len();
        for (i, c) in line.char_indices() {
            if c == '"' && prev != '\\' && prev != '\'' {
                in_str = !in_str;
            }
            if !in_str && c == '/' && prev == '/' {
                cut = i - 1;
                break;
            }
            prev = c;
        }
        clean.push_str(&line[..cut]);
        clean.push('\n');
    }
    let bytes = clean.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    let mut line_no = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => line_no += 1,
            b'\'' if bytes.get(i + 1) == Some(&b'"') && bytes.get(i + 2) == Some(&b'\'') => i += 2,
            b'\'' if bytes.get(i + 1) == Some(&b'\\') && bytes.get(i + 2) == Some(&b'"') => i += 3,
            b'r' if matches!(bytes.get(i + 1), Some(b'"') | Some(b'#'))
                && !clean[..i].ends_with(|c: char| c.is_alphanumeric() || c == '_') =>
            {
                // A raw string: skip it (never text for people here).
                let hashes = bytes[i + 1..].iter().take_while(|b| **b == b'#').count();
                if bytes.get(i + 1 + hashes) == Some(&b'"') {
                    let close = format!("\"{}", "#".repeat(hashes));
                    if let Some(end) = clean[i + 2 + hashes..].find(&close) {
                        let skipped = &clean[i..i + 2 + hashes + end + close.len()];
                        line_no += skipped.matches('\n').count();
                        i += skipped.len();
                        continue;
                    }
                }
            }
            b'"' => {
                let start = i + 1;
                let mut j = start;
                let mut value = String::new();
                while j < bytes.len() && bytes[j] != b'"' {
                    if bytes[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
                value.push_str(&clean[start..j.min(clean.len())]);
                let lines = value.matches('\n').count();
                let call = enclosing_call(bytes, i);
                if text_like(&value) || (call == "format" && sentence_in_format(&value)) {
                    found.push((line_no, value, call));
                }
                line_no += lines;
                i = j;
            }
            _ => {}
        }
        i += 1;
    }
    found
}

fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != "e2e") {
                rust_files(&path, found);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
}

/// Every hardcoded text-like literal in the desktop app: `file:line: "text" (in call)`.
pub(crate) fn hardcoded() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/src");
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    files.sort();
    let mut found = Vec::new();
    for file in files {
        let name = file.strip_prefix(&root).unwrap_or(&file).display().to_string();
        if FILES_NOT_SCANNED.iter().any(|f| name == *f) {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        for (line, lit, call) in literals(&text) {
            if NOT_SHOWN_OR_TRANSLATED.contains(&call.as_str())
                || ALLOWED.contains(&lit.as_str())
                || ALLOWED_IN.iter().any(|(f, l, _)| *f == name && *l == lit)
            {
                continue;
            }
            found.push((format!("{name}\t{lit:?}"), format!("{name}:{line}: {lit:?} (in {call}(..))")));
        }
    }
    found
}

/// Text that was already hardcoded when this check was added, one `file<TAB>"text"` per line: a
/// to-do list that only shrinks. A new hardcoded string is not on it, so it fails; a string that
/// is translated (or removed) must leave the list, so it fails until it does.
const KNOWN_GAPS: &str = include_str!("known_gaps.txt");

fn known_gaps() -> std::collections::BTreeSet<String> {
    KNOWN_GAPS.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).map(str::to_owned).collect()
}

#[test]
fn no_new_hardcoded_text_for_people_in_the_desktop_app() {
    if std::env::var_os("I18N_SEED_GAPS").is_some() {
        let all: std::collections::BTreeSet<String> = hardcoded().into_iter().map(|(k, _)| k).collect();
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/i18n/known_gaps.txt");
        std::fs::write(path, all.into_iter().collect::<Vec<_>>().join("\n") + "\n").expect("write");
        return;
    }
    let known = known_gaps();
    let new: Vec<String> = hardcoded().into_iter().filter(|(k, _)| !known.contains(k)).map(|(_, w)| w).collect();
    assert!(
        new.is_empty(),
        "text shown to people must go through t(\"..\") / tf / tn (crates/view-model/locales), or be listed in \
         ALLOWED in i18n/hardcoded.rs with a reason; found {}:\n{}",
        new.len(),
        new.join("\n")
    );
}

#[test]
fn the_known_gaps_list_has_nothing_that_was_translated_since() {
    let still: std::collections::BTreeSet<String> = hardcoded().into_iter().map(|(k, _)| k).collect();
    let stale: Vec<String> = known_gaps().into_iter().filter(|k| !still.contains(k)).collect();
    if std::env::var_os("I18N_PRUNE_GAPS").is_some() {
        let kept: Vec<String> = known_gaps().into_iter().filter(|k| still.contains(k)).collect();
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/i18n/known_gaps.txt");
        std::fs::write(path, kept.join("\n") + "\n").expect("write");
        return;
    }
    assert!(
        stale.is_empty(),
        "these are translated or gone: remove them from i18n/known_gaps.txt (I18N_PRUNE_GAPS=1 cargo test does it):\n{}",
        stale.join("\n")
    );
}


#[test]
fn the_allow_list_has_no_entry_nothing_uses() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/src");
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    let all: String = files
        .iter()
        .map(|f| std::fs::read_to_string(f).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");
    for lit in ALLOWED {
        assert!(all.contains(&format!("\"{lit}\"")), "{lit:?} is allowed but no longer appears");
    }
    for (file, lit, _) in ALLOWED_IN {
        let text = files
            .iter()
            .find(|p| p.ends_with(file))
            .map(|p| std::fs::read_to_string(p).unwrap_or_default())
            .unwrap_or_default();
        assert!(text.contains(lit), "{file}: {lit:?} is allowed but no longer appears");
    }
    for f in FILES_NOT_SCANNED {
        assert!(files.iter().any(|p| p.ends_with(f)), "{f} is not scanned but does not exist");
    }
}

#[test]
fn the_scan_tells_text_from_code() {
    assert!(text_like("Copy link"));
    assert!(text_like("Save"));
    assert!(text_like("Open {} now"));
    assert!(!text_like("settings-section"));
    assert!(!text_like("ok"));
    assert!(!text_like("a b"));
    assert!(!text_like("{shown}?"));
    assert!(!text_like("ms"));
    assert!(!text_like("src/main.rs"));
    let found = literals("fn f() {\n    x.child(\"Hello there\");\n    t(\"Hi there\");\n}\n");
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].2, "child");
    assert_eq!(found[1].2, "t");
}
