//! Spell check's state, readable from any message box: the dictionary (built
//! on a thread at startup, so typing never waits for it), the words the
//! person added (kept in the data directory) and the ones ignored until the
//! app closes. The words are checked by `view_model::spellcheck`.

use std::{
    ops::Range,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use crate::view_model::spellcheck::{self, Settings, Spell};

static ENABLED: AtomicBool = AtomicBool::new(true);
/// Bumped whenever an answer may have changed: the dictionary arrived, a word
/// was added or ignored, the setting flipped. Boxes recheck when it moves.
static GENERATION: AtomicU64 = AtomicU64::new(0);
static SPELL: Mutex<Option<Spell>> = Mutex::new(None);
static ADDED: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn bump() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// Reads the settings and starts building the dictionary.
pub fn load(dir: &std::path::Path) {
    let settings = spellcheck::load_settings(dir);
    ENABLED.store(settings.enabled, Ordering::Relaxed);
    *lock(&ADDED) = settings.words.clone();
    std::thread::spawn(move || {
        let mut spell = Spell::english();
        for word in &settings.words {
            spell.add_word(word);
        }
        *lock(&SPELL) = Some(spell);
        bump();
    });
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Whether the dictionary is built.
pub fn ready() -> bool {
    lock(&SPELL).is_some()
}

pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// Turn it on or off and keep the choice.
pub fn set_enabled(dir: &std::path::Path, on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
    bump();
    save(dir);
}

fn save(dir: &std::path::Path) {
    let settings = Settings {
        enabled: enabled(),
        words: lock(&ADDED).clone(),
    };
    if let Err(err) = spellcheck::save_settings(dir, &settings) {
        eprintln!("morpheus-desktop: saving the spell check settings: {err}");
    }
}

/// The misspelled words of `text` (nothing until the dictionary is built).
pub fn misspellings(text: &str) -> Vec<Range<usize>> {
    lock(&SPELL)
        .as_ref()
        .map(|spell| spell.misspellings(text))
        .unwrap_or_default()
}

pub fn suggest(word: &str) -> Vec<String> {
    lock(&SPELL)
        .as_ref()
        .map(|spell| spell.suggest(word))
        .unwrap_or_default()
}

/// "Add to dictionary": kept for this person.
pub fn add_word(dir: &std::path::Path, word: &str) {
    if let Some(spell) = lock(&SPELL).as_mut() {
        spell.add_word(word);
    }
    let mut added = lock(&ADDED);
    if !added.iter().any(|w| w == word) {
        added.push(word.to_owned());
    }
    drop(added);
    bump();
    save(dir);
}

/// "Ignore": not flagged until the app closes.
pub fn ignore(word: &str) {
    if let Some(spell) = lock(&SPELL).as_mut() {
        spell.add_word(word);
    }
    bump();
}
