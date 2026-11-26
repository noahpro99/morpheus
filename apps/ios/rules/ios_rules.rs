//! Our own Apple HIG rules for the iOS app's Swift (#521), the counterpart of android_rules.rs: a test that reads
//! apps/ios/Morpheus and fails on drift from the spec. Run: `rustc --edition 2021 --test apps/ios/rules/ios_rules.rs -o ios_rules && ./ios_rules`.
//!
//! 1. Text uses the system text styles (Dynamic Type): no `.font(.system(size:` and no fixed `.font(.custom(`.
//! 2. Colours come from the shared theme (Theme.swift): no `Color(red:`, `Color(hex`, `Color.white/black`, `.white`/`.black` foregrounds
//!    (text on a fill uses the theme's on-colours, #266).
//! 3. No back button of our own: the navigation stack's edge swipe and bar button are the system's (no `chevron.left`,
//!    no `navigationBarBackButtonHidden`).
//! 4. No bottom tab bar: a `TabView` is only the paging of the lists (`.tabViewStyle(.page`).
//! 5. Icons are SF Symbols: no emoji or symbol characters in the source's strings.
//! 6. Every `Image(systemName:` is hidden from accessibility or labelled (decorative or named, never silent).

use std::path::{Path, PathBuf};

fn swift_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            swift_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "swift") {
            out.push(path);
        }
    }
}

fn sources() -> Vec<(String, String)> {
    let dir = Path::new("apps/ios/Morpheus");
    let mut files = Vec::new();
    swift_files(dir, &mut files);
    assert!(!files.is_empty(), "no Swift files under apps/ios/Morpheus: run from the repository root");
    files
        .into_iter()
        .map(|f| (f.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read_to_string(&f).unwrap()))
        .collect()
}

fn violations(pred: impl Fn(&str, &str, &str) -> Option<&'static str>) -> Vec<String> {
    let mut found = Vec::new();
    for (name, text) in sources() {
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            // A statement may continue on the next lines (modifiers): give the rule the next eight.
            let next = lines[i + 1..lines.len().min(i + 9)].join("\n");
            if let Some(why) = pred(&name, line, &next) {
                found.push(format!("{name}:{}: {why}: {}", i + 1, line.trim()));
            }
        }
    }
    found
}

fn check(rule: &str, found: Vec<String>) {
    assert!(found.is_empty(), "{rule}\n{}", found.join("\n"));
}

#[test]
fn text_uses_dynamic_type_styles() {
    check("use a text style (.body, .headline, ...), never a fixed size", violations(|_, l, _| {
        (l.contains(".font(.system(size") || l.contains(".font(.custom(")).then_some("fixed font size")
    }));
}

#[test]
fn colours_come_from_the_theme() {
    check("colours come from Theme.swift (the shared palette)", violations(|name, l, _| {
        if name == "Theme.swift" {
            return None;
        }
        let bad = ["Color(red", "Color(hex", "Color.white", "Color.black", "Color.red", ".foregroundStyle(.white", ".foregroundStyle(.black", ".foregroundColor("];
        bad.iter().any(|b| l.contains(b)).then_some("hardcoded colour")
    }));
}

#[test]
fn no_back_button_of_our_own() {
    check("the system's back (edge swipe, bar button) is the only back", violations(|_, l, _| {
        (l.contains("chevron.left") || l.contains("navigationBarBackButtonHidden") || l.contains("arrow.left")).then_some("own back button")
    }));
}

#[test]
fn no_bottom_tab_bar() {
    check("a TabView only pages the lists", violations(|_, l, next| {
        (l.contains("TabView(") && !next.contains(".tabViewStyle(.page")).then_some("TabView without page style")
    }));
}

#[test]
fn icons_are_sf_symbols() {
    check("no emoji as icons", violations(|_, l, _| {
        l.chars().any(|c| ('\u{1F300}'..='\u{1FAFF}').contains(&c) || ('\u{2600}'..='\u{27BF}').contains(&c)).then_some("emoji")
    }));
}

#[test]
fn symbols_are_hidden_or_labelled() {
    check("an SF Symbol is accessibilityHidden or labelled", violations(|_, l, next| {
        (l.contains("Image(systemName:") && !(l.contains("accessibility") || next.contains("accessibilityHidden") || next.contains("accessibilityLabel")))
            .then_some("symbol without accessibility")
    }));
}
