//! The full emoji picker, as Discord's: every emoji by category with a tab
//! for each, a search, a skin tone, and the recently used ones first. The
//! message hover bar's "more" button opens it to react.

use std::rc::Rc;

use dioxus::prelude::*;
use emojis::{Emoji, Group, SkinTone};

use crate::icons::{Icon, SEARCH};

/// What the open picker does with the emoji picked, and where it opened.
#[derive(Clone)]
pub struct PickerTarget {
    pub x: f64,
    pub y: f64,
    pub on_pick: Rc<dyn Fn(String)>,
}

/// The picker, if open. Provided by the shell, drawn over everything.
#[derive(Clone, Default)]
pub struct PickerState(pub Option<PickerTarget>);
pub type EmojiPickerOpen = Signal<PickerState>;

/// Open the picker at a click; `on_pick` gets the emoji chosen.
pub fn open_emoji_picker(x: f64, y: f64, on_pick: impl Fn(String) + 'static) {
    consume_context::<EmojiPickerOpen>().set(PickerState(Some(PickerTarget {
        x,
        y,
        on_pick: Rc::new(on_pick),
    })));
}

const GROUPS: [(Group, &str, &str); 9] = [
    (Group::SmileysAndEmotion, "Smileys & Emotion", "😀"),
    (Group::PeopleAndBody, "People & Body", "👋"),
    (Group::AnimalsAndNature, "Animals & Nature", "🐶"),
    (Group::FoodAndDrink, "Food & Drink", "🍔"),
    (Group::TravelAndPlaces, "Travel & Places", "✈️"),
    (Group::Activities, "Activities", "⚽"),
    (Group::Objects, "Objects", "💡"),
    (Group::Symbols, "Symbols", "❤️"),
    (Group::Flags, "Flags", "🏁"),
];

/// The tones offered: (stored number, name in the page, tone).
const TONES: [(u8, &str, SkinTone); 6] = [
    (0, "Default", SkinTone::Default),
    (1, "Light", SkinTone::Light),
    (2, "MediumLight", SkinTone::MediumLight),
    (3, "Medium", SkinTone::Medium),
    (4, "MediumDark", SkinTone::MediumDark),
    (5, "Dark", SkinTone::Dark),
];

fn tone_of(stored: Option<u8>) -> SkinTone {
    TONES
        .iter()
        .find(|(n, _, _)| Some(*n) == stored)
        .map_or(SkinTone::Default, |(_, _, tone)| *tone)
}

/// `emoji` as shown and sent: in the chosen tone when it has tones.
fn toned(emoji: &'static Emoji, tone: SkinTone) -> &'static str {
    if tone == SkinTone::Default {
        return emoji.as_str();
    }
    emoji
        .with_skin_tone(tone)
        .map_or(emoji.as_str(), Emoji::as_str)
}

/// A raised hand in `tone`, standing for the tone.
fn hand(tone: SkinTone) -> &'static str {
    emojis::get("✋").map_or("✋", |e| toned(e, tone))
}

/// Emoji whose name or shortcodes match `query`: shortcodes starting with
/// it first, then names starting with it, then a word of the name, then
/// anything containing it.
fn search(query: &str) -> Vec<&'static Emoji> {
    let query = query.trim().to_lowercase().replace(' ', "_");
    let spaced = query.replace('_', " ");
    let mut found: Vec<(u8, &'static Emoji)> = emojis::iter()
        .filter_map(|emoji| {
            let name = emoji.name().to_lowercase();
            let codes: Vec<&str> = emoji.shortcodes().collect();
            let score = if codes.iter().any(|c| c.starts_with(&query)) {
                0
            } else if name.starts_with(&spaced) {
                1
            } else if name.split(' ').any(|w| w.starts_with(&spaced)) {
                2
            } else if codes.iter().any(|c| c.contains(&query)) || name.contains(&spaced) {
                3
            } else {
                return None;
            };
            Some((score, emoji))
        })
        .collect();
    found.sort_by_key(|(score, _)| *score);
    found.into_iter().map(|(_, emoji)| emoji).collect()
}

/// The open picker, if any: a backdrop that closes it on a click, and the
/// picker beside where it was opened, kept on screen.
#[component]
pub fn EmojiPickerLayer() -> Element {
    let mut open = use_context::<EmojiPickerOpen>();
    let Some(target) = open().0 else {
        return rsx! {};
    };
    rsx! {
        div {
            class: "emoji-picker-backdrop",
            onclick: move |_| open.set(PickerState::default()),
            oncontextmenu: move |e| {
                e.prevent_default();
                open.set(PickerState::default());
            },
            EmojiPicker { x: target.x, y: target.y }
        }
    }
}

#[component]
fn EmojiPicker(x: f64, y: f64) -> Element {
    let mut open = use_context::<EmojiPickerOpen>();
    let data_dir = use_hook(|| consume_context::<crate::AppConfig>().data_dir);
    let mut query = use_signal(String::new);
    let mut tone = use_signal(|| crate::device_prefs::emoji_tone(&data_dir));
    let mut tones_open = use_signal(|| false);
    let recent = use_hook({
        let data_dir = data_dir.clone();
        move || crate::device_prefs::recent_emoji(&data_dir)
    });

    let pick = {
        let data_dir = data_dir.clone();
        move |emoji: String| {
            let Some(target) = open.peek().0.clone() else {
                return;
            };
            if let Err(err) = crate::device_prefs::note_recent_emoji(&data_dir, &emoji) {
                tracing::warn!(%err, "could not save the recent emoji");
            }
            open.set(PickerState::default());
            (target.on_pick)(emoji);
        }
    };

    let skin = tone_of(tone());
    let searching = !query().trim().is_empty();
    let results: Vec<&'static str> = if searching {
        search(&query())
            .into_iter()
            .map(|e| toned(e, skin))
            .collect()
    } else {
        Vec::new()
    };
    let first = results.first().map(|e| (*e).to_owned());
    let tone_hand = hand(skin);

    // Beside the button, kept inside the window.
    let style = format!(
        "left: clamp(8px, {left}px, calc(100vw - 368px)); top: clamp(8px, {y}px, calc(100vh - 448px));",
        left = x - 368.0,
    );

    rsx! {
        div {
            class: "emoji-picker",
            role: "dialog",
            style: "{style}",
            onclick: move |e| e.stop_propagation(),
            div { class: "emoji-picker-top",
                label { class: "emoji-search-box",
                    input {
                        class: "emoji-search",
                        r#type: "text",
                        placeholder: "Find the perfect emoji",
                        value: "{query}",
                        onmounted: move |e| async move {
                            let _ = e.data().set_focus(true).await;
                        },
                        oninput: move |e| query.set(e.value()),
                        onkeydown: {
                            let mut pick = pick.clone();
                            move |e: KeyboardEvent| {
                                if e.key() == Key::Enter {
                                    e.prevent_default();
                                    if let Some(first) = first.clone() {
                                        pick(first);
                                    }
                                }
                            }
                        },
                    }
                    Icon { glyph: SEARCH, size: 16 }
                }
                div { class: "skin-tone",
                    button {
                        class: "skin-tone-button",
                        r#type: "button",
                        title: "Skin tone",
                        onclick: move |_| tones_open.toggle(),
                        "{tone_hand}"
                    }
                    if tones_open() {
                        div { class: "skin-tone-menu",
                            for (stored, name, option) in TONES {
                                button {
                                    key: "{name}",
                                    class: "skin-tone-option",
                                    class: if skin == option { "active" },
                                    r#type: "button",
                                    "data-tone": "{name}",
                                    title: "{name}",
                                    onclick: {
                                        let data_dir = data_dir.clone();
                                        move |_| {
                                            let stored = (stored > 0).then_some(stored);
                                            tone.set(stored);
                                            tones_open.set(false);
                                            if let Err(err) = crate::device_prefs::set_emoji_tone(&data_dir, stored) {
                                                tracing::warn!(%err, "could not save the skin tone");
                                            }
                                        }
                                    },
                                    {hand(option)}
                                }
                            }
                        }
                    }
                }
            }
            div { class: "emoji-body",
                if !searching {
                    nav { class: "emoji-tabs",
                        if !recent.is_empty() {
                            button {
                                class: "emoji-tab",
                                r#type: "button",
                                title: "Recent",
                                onclick: move |_| scroll_to_group("Recent"),
                                "🕘"
                            }
                        }
                        for (_, name, icon) in GROUPS {
                            button {
                                key: "{name}",
                                class: "emoji-tab",
                                r#type: "button",
                                title: "{name}",
                                onclick: move |_| scroll_to_group(name),
                                "{icon}"
                            }
                        }
                    }
                }
                div { class: "emoji-grid-scroll",
                    if searching {
                        if results.is_empty() {
                            p { class: "emoji-none muted", "No emoji found" }
                        } else {
                            section { class: "emoji-section", "data-group": "Results",
                                div { class: "emoji-cells",
                                    for emoji in results {
                                        EmojiCell { key: "{emoji}", emoji, on_pick: pick.clone() }
                                    }
                                }
                            }
                        }
                    } else {
                        if !recent.is_empty() {
                            section { class: "emoji-section", "data-group": "Recent",
                                h3 { "Recently used" }
                                div { class: "emoji-cells",
                                    for emoji in recent.clone() {
                                        EmojiCell { key: "{emoji}", emoji: emoji.clone(), on_pick: pick.clone() }
                                    }
                                }
                            }
                        }
                        for (group, name, _) in GROUPS {
                            section { key: "{name}", class: "emoji-section", "data-group": "{name}",
                                h3 { "{name}" }
                                div { class: "emoji-cells",
                                    for emoji in group.emojis() {
                                        EmojiCell { key: "{emoji.as_str()}", emoji: toned(emoji, skin), on_pick: pick.clone() }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn EmojiCell(emoji: String, on_pick: Callback<String>) -> Element {
    rsx! {
        button {
            class: "emoji-cell",
            r#type: "button",
            onclick: move |_| on_pick(emoji.clone()),
            "{emoji}"
        }
    }
}

/// Scroll the picker so `group`'s heading is at the top.
fn scroll_to_group(group: &str) {
    let group = serde_json::to_string(group).unwrap_or_default();
    document::eval(&format!(
        "const g = document.querySelector('.emoji-grid-scroll'), \
           s = [...document.querySelectorAll('.emoji-section')].find(s => s.dataset.group === {group}); \
         if (g && s) g.scrollTop = s.offsetTop;"
    ));
}

/// Close the picker if it is open (Escape); whether it was.
pub fn close_emoji_picker() -> bool {
    let Some(mut open) = try_consume_context::<EmojiPickerOpen>() else {
        return false;
    };
    if open.peek().0.is_none() {
        return false;
    }
    open.set(PickerState::default());
    true
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn search_finds_by_shortcode_and_name() {
        assert_eq!(search("rocket").first().map(|e| e.as_str()), Some("🚀"));
        assert_eq!(search("thumbsup").first().map(|e| e.as_str()), Some("👍"));
        assert_eq!(search("thumbs up").first().map(|e| e.as_str()), Some("👍"));
        assert!(search("zzqqxx").is_empty());
    }

    #[test]
    fn tones_apply_where_there_are_tones() {
        let thumbs = emojis::get("👍").expect("thumbs up");
        assert_eq!(toned(thumbs, SkinTone::Medium), "👍🏽");
        let rocket = emojis::get("🚀").expect("rocket");
        assert_eq!(toned(rocket, SkinTone::Medium), "🚀");
    }
}
