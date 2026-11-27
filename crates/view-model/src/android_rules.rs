//! Our own Material 3 rules for the Android app's Kotlin (#468), checked like `i18n::hardcoded`: this
//! test reads apps/android/app/src and fails on drift from the spec, so a screen cannot slip a raw
//! font size, an off-grid gap, an unstyled `Text`, an emoji icon or a hand-made copy of a Material 3
//! component past review. What is right as it is goes in the allow lists below with the reason; an
//! entry nothing uses any more fails too, so the lists cannot grow stale.
//!
//! The rules:
//! 1. No raw `.sp` font size, and no `.dp` value off the 4 dp grid, outside the theme and token files.
//! 2. Every `Text(` takes a `MaterialTheme.typography` style, unless it sits inside a component that gives
//!    it one (a ListItem's slots, a Button, a dialog's title and text, ...).
//! 3. No emoji or symbol characters as icons (icons are drawables; [`EMOJI_ALLOWED`] lists the exceptions).
//! 4. No hand-made radio, switch, checkbox, search field, or clickable box posing as a button.
//! 6. A server's icon is always a squircle: `ServerIcon`, never `Avatar(.., square = ..)` outside Avatar.kt, never a CircleShape (#510).
//! 5. No back button: no back or arrow-left icon, no `navigationIcon`, no "Back" label (#481). Android has system back (the
//!    gesture and the button), and a screen's `BackHandler` must reach the same place; [`BACK_ALLOWED`] lists what is not a back button.

use std::path::{Path, PathBuf};

/// The files that define the tokens and the Material 3 helpers: raw sizes live here and nowhere else.
const TOKEN_FILES: &[&str] = &["ui/Theme.kt", "ui/M3.kt"];

/// `.dp` values that are not on the 4 dp grid but are right: (value, why). A value that is only right in one
/// place goes in [`OFF_GRID_IN`] instead.
const OFF_GRID_ANYWHERE: &[(&str, &str)] = &[
    ("1", "a hairline: Material 3's divider and outline are 1 dp"),
    ("2", "a stroke or indicator thickness: Material 3 draws these at 2 dp"),
];

/// Off-grid `.dp` values right in one file: (file, value, why).
const OFF_GRID_IN: &[(&str, &str, &str)] = &[];

/// Where a `Text(` without a style is fine: the lambda it is in belongs to a component (or a slot of one)
/// that sets the style the spec asks for.
const GIVES_A_TEXT_STYLE: &[&str] = &[
    // Named slots of ListItem, TextField, dialogs, bars, chips.
    "headlineContent", "supportingContent", "overlineContent", "trailingContent", "title", "text", "label", "placeholder",
    "supportingText", "prefix", "suffix", "confirmButton", "dismissButton",
    // Components that wrap their content in a text style.
    "Button", "TextButton", "OutlinedButton", "FilledTonalButton", "ElevatedButton", "DropdownMenuItem", "Tab",
    "NavigationBarItem", "NavigationRailItem", "FilterChip", "AssistChip", "InputChip", "SuggestionChip", "Snackbar",
    "Badge", "ListItem", "TopAppBar", "CenterAlignedTopAppBar", "AlertDialog", "ExtendedFloatingActionButton",
    "SegmentedButton",
];

/// `Text(` calls right without a style anywhere else: (file, the line's code, why).
const TEXT_WITHOUT_STYLE_IN: &[(&str, &str, &str)] = &[
    ("ui/RoomScreen.kt", "Text(title ?: t(\"Message\")", "the composer's label: the TextField gives it a style that animates between the field text and the floating label, and an explicit style would stop the shrink"),
];

/// Files where emoji or symbol characters are right: (file, the text, why).
const EMOJI_ALLOWED: &[(&str, &str, &str)] = &[
    ("ui/RoomScreen.kt", "Say hi! 👋", "the button's label is the greeting it sends: a message, not an icon"),
    ("ui/RoomScreen.kt", "quickReactions", "the reactions offered on a message are emoji: they are what gets sent, not icons"),
];

/// Where a back-looking icon or label is right: (file, the text, why). A back button is never right; this is for the icon used as
/// something else (a calendar's previous month, a carousel).
const BACK_ALLOWED: &[(&str, &str, &str)] = &[];

/// What marks a back button in the source.
const BACK_MARKS: &[&str] = &["ic_ms_chevron_left", "ic_ms_arrow_back", "ArrowBack", "navigationIcon", "t(\"Back\")", "backTag", "onBack =", "onBack:", "-back\"", "testTag(\"back\")"];

/// Where a hand-made control is right: (file, the pattern, why).
const HAND_MADE_ALLOWED: &[(&str, &str, &str)] = &[];

fn kotlin_files() -> Vec<(String, String)> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(&path, root, out);
            } else if path.extension().is_some_and(|e| e == "kt") {
                let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                out.push((rel, std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    let root = android_src();
    let mut out = Vec::new();
    // The app's code (main, debug, release); not the tests, which draw with raw values on purpose.
    for set in ["main", "debug", "release"] {
        let dir = root.join(set).join("kotlin/dev/morpheus/app");
        walk(&dir, &dir, &mut out);
    }
    assert!(!out.is_empty(), "no Kotlin found under {}", root.display());
    out
}

/// The file with comments blanked (line numbers kept) in two forms: `code` keeps string text, `shape` also
/// blanks what is inside strings, so braces and parentheses in text do not confuse a scan.
fn lex(text: &str) -> (Vec<u8>, Vec<u8>) {
    let b = text.as_bytes();
    let mut code = b.to_vec();
    let mut shape = b.to_vec();
    let blank = |v: &mut Vec<u8>, from: usize, to: usize| {
        for byte in &mut v[from..to] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
    };
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'/' && b.get(i + 1) == Some(&b'/') {
            let end = b[i..].iter().position(|&c| c == b'\n').map_or(b.len(), |p| i + p);
            blank(&mut code, i, end);
            blank(&mut shape, i, end);
            i = end;
        } else if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
            let (mut depth, mut j) = (1, i + 2);
            while j < b.len() && depth > 0 {
                if b[j] == b'/' && b.get(j + 1) == Some(&b'*') {
                    depth += 1;
                    j += 2;
                } else if b[j] == b'*' && b.get(j + 1) == Some(&b'/') {
                    depth -= 1;
                    j += 2;
                } else {
                    j += 1;
                }
            }
            blank(&mut code, i, j);
            blank(&mut shape, i, j);
            i = j;
        } else if b[i] == b'"' {
            let raw = b[i..].starts_with(b"\"\"\"");
            let mut j = i + if raw { 3 } else { 1 };
            loop {
                if j >= b.len() {
                    break;
                }
                if raw && b[j..].starts_with(b"\"\"\"") {
                    j += 3;
                    break;
                }
                if !raw && b[j] == b'\\' {
                    j += 2;
                    continue;
                }
                if !raw && b[j] == b'"' {
                    j += 1;
                    break;
                }
                j += 1;
            }
            let open = if raw { 3 } else { 1 };
            blank(&mut shape, i + open, j.saturating_sub(open).max(i + open));
            i = j;
        } else if b[i] == b'\'' && b.get(i + 2) == Some(&b'\'') {
            i += 3;
        } else if b[i] == b'\'' && b.get(i + 1) == Some(&b'\\') {
            i += 4;
        } else {
            i += 1;
        }
    }
    (code, shape)
}

fn line_of(text: &[u8], at: usize) -> usize {
    text[..at.min(text.len())].iter().filter(|&&c| c == b'\n').count() + 1
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// The index just past the `)` that closes the `(` at `open`.
fn close_paren(shape: &[u8], open: usize) -> usize {
    let mut depth = 0;
    for (k, &c) in shape.iter().enumerate().skip(open) {
        match c {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return k + 1;
                }
            }
            _ => {}
        }
    }
    shape.len()
}

/// The identifier ending just before `end` (skipping spaces).
fn ident_before(shape: &[u8], mut end: usize) -> String {
    while end > 0 && shape[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && is_ident(shape[start - 1]) {
        start -= 1;
    }
    String::from_utf8_lossy(&shape[start..end]).into_owned()
}

/// The component or slot names of the lambdas around `at`, innermost first: `Button { Text(..) }` gives
/// `Button`; `headlineContent = { Text(..) }` gives `headlineContent`.
fn enclosing_lambdas(shape: &[u8], at: usize) -> Vec<String> {
    let mut names = Vec::new();
    let mut depth = 0i32;
    let mut i = at;
    while i > 0 {
        i -= 1;
        match shape[i] {
            b'}' => depth += 1,
            b'{' => {
                if depth > 0 {
                    depth -= 1;
                    continue;
                }
                let mut j = i;
                while j > 0 && shape[j - 1].is_ascii_whitespace() {
                    j -= 1;
                }
                if j > 0 && shape[j - 1] == b'=' {
                    names.push(ident_before(shape, j - 1));
                } else if j > 0 && shape[j - 1] == b')' {
                    // `Name(args) { .. }`: walk back to the matching `(`.
                    let mut d = 0;
                    let mut k = j - 1;
                    loop {
                        match shape[k] {
                            b')' => d += 1,
                            b'(' => {
                                d -= 1;
                                if d == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        if k == 0 {
                            break;
                        }
                        k -= 1;
                    }
                    names.push(ident_before(shape, k));
                } else {
                    names.push(ident_before(shape, j));
                }
            }
            _ => {}
        }
    }
    names
}

#[derive(Default)]
struct Used {
    off_grid: Vec<(String, String)>,
    off_grid_anywhere: Vec<String>,
    text_style: Vec<(String, String)>,
    emoji: Vec<(String, String)>,
    hand_made: Vec<(String, String)>,
    back: Vec<(String, String)>,
}

fn find_all(hay: &[u8], needle: &str) -> Vec<usize> {
    let n = needle.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + n.len() <= hay.len() {
        if &hay[i..i + n.len()] == n {
            out.push(i);
        }
        i += 1;
    }
    out
}

fn is_emoji_or_symbol(c: char) -> bool {
    matches!(c as u32,
        0x2190..=0x21FF | 0x2300..=0x23FF | 0x2460..=0x27BF | 0x2900..=0x297F | 0x2B00..=0x2BFF | 0xFE0F | 0x1F000..=0x1FFFF)
}

/// Every violation of every rule, as `file:line: what`.
fn violations() -> (Vec<String>, Used) {
    let mut out = Vec::new();
    let mut used = Used::default();
    for (file, text) in kotlin_files() {
        let (code, shape) = lex(&text);
        let token_file = TOKEN_FILES.contains(&file.as_str());
        let code_str = String::from_utf8_lossy(&code).into_owned();
        let line_text = |at: usize| code_str.lines().nth(line_of(&code, at) - 1).unwrap_or("").trim().to_owned();

        // 1. Raw sizes. A number followed by `.sp` / `.dp`.
        if !token_file {
            for (unit, at) in find_all(&shape, ".sp").into_iter().map(|a| ("sp", a)).chain(find_all(&shape, ".dp").into_iter().map(|a| ("dp", a))) {
                let end = at + 3;
                if shape.get(end).is_some_and(|&c| is_ident(c)) {
                    continue;
                }
                let mut s = at;
                while s > 0 && (shape[s - 1].is_ascii_digit() || shape[s - 1] == b'.') {
                    s -= 1;
                }
                let num = String::from_utf8_lossy(&shape[s..at]).into_owned();
                if num.is_empty() || !num.chars().next().unwrap().is_ascii_digit() {
                    continue;
                }
                if unit == "sp" {
                    out.push(format!("{file}:{}: raw font size {num}.sp: use a MaterialTheme.typography role", line_of(&shape, at)));
                    continue;
                }
                let value: f64 = num.parse().unwrap_or(1.0);
                if value % 4.0 == 0.0 {
                    continue;
                }
                if OFF_GRID_ANYWHERE.iter().any(|(v, _)| *v == num) {
                    used.off_grid_anywhere.push(num);
                } else if OFF_GRID_IN.iter().any(|(f, v, _)| *f == file && *v == num) {
                    used.off_grid.push((file.clone(), num));
                } else {
                    out.push(format!("{file}:{}: {num}.dp is off the 4 dp grid: use a Space/Size/Radius token or a multiple of 4", line_of(&shape, at)));
                }
            }
        }

        // 2. Text without a typography style.
        for at in find_all(&shape, "Text(") {
            if at > 0 && (is_ident(shape[at - 1]) || shape[at - 1] == b'.') {
                continue;
            }
            let end = close_paren(&shape, at + 4);
            let args = String::from_utf8_lossy(&code[at..end]);
            if args.contains("MaterialTheme.typography") || args.contains("style =") || args.contains("style=") || args.contains("textStyle") {
                continue;
            }
            if enclosing_lambdas(&shape, at).iter().any(|n| GIVES_A_TEXT_STYLE.contains(&n.as_str())) {
                continue;
            }
            // A slot or component named earlier on the same line: `supportingContent = x?.let { { Text(it) } }`,
            // `DropdownMenuItem({ Text(it) }, ..)`.
            let line_start = shape[..at].iter().rposition(|&c| c == b'\n').map_or(0, |p| p + 1);
            let before = String::from_utf8_lossy(&shape[line_start..at]).into_owned();
            if GIVES_A_TEXT_STYLE.iter().any(|n| before.contains(&format!("{n} =")) || before.contains(&format!("{n}(")) || before.contains(&format!("{n} {{"))) {
                continue;
            }
            let line = line_text(at);
            if TEXT_WITHOUT_STYLE_IN.iter().any(|(f, l, _)| *f == file && line.contains(l)) {
                used.text_style.push((file.clone(), line));
                continue;
            }
            out.push(format!("{file}:{}: Text without a MaterialTheme.typography style: {line}", line_of(&shape, at)));
        }

        // 3. Emoji and symbols in strings.
        for (n, line) in code_str.lines().enumerate() {
            if !line.chars().any(is_emoji_or_symbol) {
                continue;
            }
            if let Some((_, text, _)) = EMOJI_ALLOWED.iter().find(|(f, t, _)| *f == file && line.contains(t)) {
                used.emoji.push((file.clone(), (*text).to_owned()));
                continue;
            }
            out.push(format!("{file}:{}: emoji or symbol character: icons are drawables (R.drawable.ic_ms_*): {}", n + 1, line.trim()));
        }

        // 6. Server icons.
        if file != "ui/Avatar.kt" {
            for (n, line) in code_str.lines().enumerate() {
                if line.contains("Avatar(") && line.contains("square = ") {
                    out.push(format!("{file}:{}: a server's icon is drawn with ServerIcon (a squircle), not Avatar(square = ..): {}", n + 1, line.trim()));
                }
            }
        }

        // 4b. Icons are Material Symbols Rounded (ic_ms_*, #538): no Lucide drawable is referenced any more.
        for (n, line) in text.lines().enumerate() {
            if line.contains(concat!("ic_", "lucide_")) {
                out.push(format!("{file}:{}: Lucide icon: use the Material Symbols ic_ms_<symbol> drawable (scripts/android-icons.map): {}", n + 1, line.trim()));
            }
        }

        // 5. Back buttons.
        for (n, line) in code_str.lines().enumerate() {
            let Some(mark) = BACK_MARKS.iter().find(|m| line.contains(**m)) else { continue };
            if let Some((_, text, _)) = BACK_ALLOWED.iter().find(|(f, t, _)| *f == file && line.contains(t)) {
                used.back.push((file.clone(), (*text).to_owned()));
                continue;
            }
            out.push(format!("{file}:{}: back button ({mark}): no on-screen back; system back goes back: {}", n + 1, line.trim()));
        }

        // 4. Hand-made Material 3 controls.
        let mut hand_made = |at: usize, what: &str, pattern: &str, out: &mut Vec<String>, used: &mut Used| {
            if token_file && what.starts_with("search") {
                return;
            }
            if HAND_MADE_ALLOWED.iter().any(|(f, p, _)| *f == file && *p == pattern) {
                used.hand_made.push((file.clone(), pattern.to_owned()));
                return;
            }
            out.push(format!("{file}:{}: hand-made {what}: use the Material 3 component ({pattern})", line_of(&shape, at)));
        };
        for at in find_all(&shape, "fun ") {
            let name = String::from_utf8_lossy(&shape[at + 4..]).chars().take_while(|&c| c.is_ascii_alphanumeric() || c == '_').collect::<String>();
            for word in ["Radio", "Switch", "Checkbox", "CheckBox"] {
                if name.contains(word) && !token_file && String::from_utf8_lossy(&shape[at.saturating_sub(40)..at]).contains("@Composable") {
                    hand_made(at, &format!("{} (fun {name})", word.to_lowercase()), &format!("fun {name}"), &mut out, &mut used);
                }
            }
        }
        for at in find_all(&shape, "BasicTextField(") {
            if !token_file {
                hand_made(at, "search or text field", "BasicTextField", &mut out, &mut used);
            }
        }
        for name in ["Box(", "Row(", "Column("] {
            for at in find_all(&shape, name) {
                if at > 0 && (is_ident(shape[at - 1]) || shape[at - 1] == b'.') {
                    continue;
                }
                let end = close_paren(&shape, at + name.len() - 1);
                let args = String::from_utf8_lossy(&shape[at..end]).into_owned();
                let styled = args.contains(".background(") || args.contains(".border(");
                if !args.contains(".clickable") && !args.contains(".combinedClickable") || !styled {
                    continue;
                }
                // A box with a click, a fill and a text inside: a button made by hand.
                let body_end = {
                    let mut j = end;
                    while j < shape.len() && shape[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if shape.get(j) == Some(&b'{') {
                        let mut d = 0;
                        let mut k = j;
                        while k < shape.len() {
                            match shape[k] {
                                b'{' => d += 1,
                                b'}' => {
                                    d -= 1;
                                    if d == 0 {
                                        break;
                                    }
                                }
                                _ => {}
                            }
                            k += 1;
                        }
                        k
                    } else {
                        end
                    }
                };
                let body = String::from_utf8_lossy(&shape[at..body_end.min(shape.len())]).into_owned();
                if body.contains("Text(") {
                    hand_made(at, "clickable box posing as a button", &format!("{name} clickable+background"), &mut out, &mut used);
                }
            }
        }
    }
    (out, used)
}

#[test]
fn the_android_kotlin_follows_the_material_3_rules() {
    let (found, used) = violations();
    let mut stale = Vec::new();
    for (v, why) in OFF_GRID_ANYWHERE {
        assert!(!why.is_empty());
        if !used.off_grid_anywhere.iter().any(|u| u == v) {
            stale.push(format!("OFF_GRID_ANYWHERE {v}"));
        }
    }
    for (f, v, why) in OFF_GRID_IN {
        assert!(!why.is_empty());
        if !used.off_grid.iter().any(|(uf, uv)| uf == f && uv == v) {
            stale.push(format!("OFF_GRID_IN {f} {v}"));
        }
    }
    for (f, l, why) in TEXT_WITHOUT_STYLE_IN {
        assert!(!why.is_empty());
        if !used.text_style.iter().any(|(uf, ul)| uf == f && ul.contains(l)) {
            stale.push(format!("TEXT_WITHOUT_STYLE_IN {f} {l}"));
        }
    }
    for (f, t, why) in EMOJI_ALLOWED {
        assert!(!why.is_empty());
        if !used.emoji.iter().any(|(uf, ut)| uf == f && ut == t) {
            stale.push(format!("EMOJI_ALLOWED {f} {t}"));
        }
    }
    for (f, p, why) in HAND_MADE_ALLOWED {
        assert!(!why.is_empty());
        if !used.hand_made.iter().any(|(uf, up)| uf == f && up == p) {
            stale.push(format!("HAND_MADE_ALLOWED {f} {p}"));
        }
    }
    for (f, t, why) in BACK_ALLOWED {
        assert!(!why.is_empty());
        if !used.back.iter().any(|(uf, ut)| uf == f && ut == t) {
            stale.push(format!("BACK_ALLOWED {f} {t}"));
        }
    }
    assert!(found.is_empty() && stale.is_empty(), "\n{} Material 3 rule violation(s):\n{}\nstale allow-list entries: {stale:?}\n", found.len(), found.join("\n"));
}

/// apps/android/app/src, found from the crate (`cargo test`) or from the repository root
/// (`just android-rules` builds this file alone with rustc, as it needs nothing but std).
fn android_src() -> PathBuf {
    let root = match option_env!("CARGO_MANIFEST_DIR") {
        Some(dir) => Path::new(dir).join("../.."),
        None => PathBuf::from("."),
    };
    root.join("apps/android/app/src")
}

/// What turns a list's stretch overscroll off (#516): a list that fits must not scroll but must still stretch when pulled, which
/// `userScrollEnabled = false` (and a null overscroll configuration or effect) would kill.
const OVERSCROLL_OFF: &[&str] = &["userScrollEnabled = false", "userScrollEnabled=false", "LocalOverscrollConfiguration provides null", "overscrollEffect = null", "overscrollEffect=null", ".overscroll(null)", "overScrollMode"];

/// The lines of this Kotlin that switch the overscroll of a scrolling list off.
fn overscroll_disabled(text: &str) -> Vec<usize> {
    let (code, _) = lex(text);
    let code = String::from_utf8_lossy(&code).into_owned();
    code.lines().enumerate().filter(|(_, l)| OVERSCROLL_OFF.iter().any(|p| l.contains(p))).map(|(i, _)| i + 1).collect()
}

#[test]
fn no_list_has_its_stretch_overscroll_turned_off() {
    // The check itself: each way of turning it off is seen, an ordinary list is not.
    assert_eq!(overscroll_disabled("LazyColumn(Modifier.fillMaxSize(), userScrollEnabled = false) {}"), vec![1]);
    assert_eq!(overscroll_disabled("a()\nCompositionLocalProvider(LocalOverscrollConfiguration provides null) {}"), vec![2]);
    assert!(overscroll_disabled("LazyColumn(Modifier.fillMaxSize(), state = s) { /* userScrollEnabled = false */ }").is_empty());
    let bad: Vec<String> = kotlin_files().iter().flat_map(|(f, t)| overscroll_disabled(t).into_iter().map(move |l| format!("{f}:{l}"))).collect();
    assert!(bad.is_empty(), "stretch overscroll turned off (a list that fits must still stretch, #516): {bad:?}");
}

/// The lines of a Kotlin file where a vertical list (`LazyColumn`, `verticalScroll`) does not carry the shared stretch overscroll (#516): its
/// call (or, for `verticalScroll`, the modifier chain just before it) must have `.stretch(`.
fn lists_without_stretch(text: &str) -> Vec<usize> {
    let (code, shape) = lex(text);
    let code = String::from_utf8_lossy(&code).into_owned();
    let shape = String::from_utf8_lossy(&shape).into_owned();
    let line_of = |at: usize| code[..at].bytes().filter(|b| *b == b'\n').count() + 1;
    let mut out = Vec::new();
    for (at, _) in code.match_indices("LazyColumn(") {
        // The call's arguments: to the matching parenthesis.
        let open = at + "LazyColumn".len();
        let mut depth = 0usize;
        let mut end = open;
        for (i, b) in shape.bytes().enumerate().skip(open) {
            match b {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
                _ => {}
            }
        }
        if !code[open..end].contains(".stretch(") {
            out.push(line_of(at));
        }
    }
    for (at, _) in code.match_indices(".verticalScroll(") {
        let from = at.saturating_sub(300);
        if !code[from..at].contains(".stretch(") {
            out.push(line_of(at));
        }
    }
    out.sort();
    out
}

#[test]
fn every_vertical_list_has_the_shared_stretch() {
    assert_eq!(lists_without_stretch("LazyColumn(Modifier.fillMaxSize()) {}"), vec![1]);
    assert!(lists_without_stretch("LazyColumn(Modifier.stretch(s).fillMaxSize(), state = s) {}").is_empty());
    assert_eq!(lists_without_stretch("a()\nColumn(Modifier.fillMaxSize().verticalScroll(s)) {}"), vec![2]);
    assert!(lists_without_stretch("Column(Modifier.stretch(s).fillMaxSize().verticalScroll(s)) {}").is_empty());
    let bad: Vec<String> = kotlin_files().iter().filter(|(f, _)| !f.ends_with("/Stretch.kt")).flat_map(|(f, t)| lists_without_stretch(t).into_iter().map(move |l| format!("{f}:{l}"))).collect();
    assert!(bad.is_empty(), "vertical lists without the shared stretch overscroll (Modifier.stretch, #516): {bad:?}");
}
