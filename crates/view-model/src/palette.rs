//! The command palette's actions (Ctrl+K): what the palette offers besides
//! rooms and people, each with an icon and its shortcut as key caps, and the
//! fuzzy match that finds them. Shared by the desktop and mobile apps; what an
//! action does is the app's.

use crate::settings::SettingsPage;

/// What an action asks the app to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteAction {
    CreateServer,
    Discover,
    /// Settings on its first page.
    Settings,
    /// Settings on one page.
    SettingsPage(SettingsPage),
    StartDm,
    /// Join by address: a room alias, ID or link.
    Join,
    ToggleTheme,
    ToggleStreamer,
    ToggleMute,
    ToggleDeafen,
    LeaveCall,
    Shortcuts,
    Inbox,
    CheckForUpdates,
    Acknowledgements,
}

/// What the labels depend on.
#[derive(Clone, Copy, Debug, Default)]
pub struct PaletteState {
    pub in_call: bool,
    pub muted: bool,
    pub deafened: bool,
    pub streamer: bool,
    /// The light theme is on (the toggle goes to dark, else to light).
    pub light: bool,
    /// No window of its own (a phone): no Startup page.
    pub no_window: bool,
}

/// One row of the palette.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteEntry {
    pub action: PaletteAction,
    pub label: String,
    pub icon: &'static str,
    /// The shortcut, key by key ("Ctrl", "Shift", "N"); empty when it has none.
    pub keys: Vec<&'static str>,
    /// Other words that find it.
    pub words: &'static str,
}

/// The actions to offer now, in the order shown when nothing is typed.
pub fn actions(state: PaletteState) -> Vec<PaletteEntry> {
    let entry = |action, label: &str, icon, keys: &[&'static str], words| PaletteEntry {
        action,
        label: label.to_owned(),
        icon,
        keys: keys.to_vec(),
        words,
    };
    let mut list = vec![
        entry(PaletteAction::CreateServer, "Create a server", "plus", &["Ctrl", "Shift", "N"], "new community space add"),
        entry(PaletteAction::Discover, "Discover servers", "compass", &[], "browse explore public directory find"),
        entry(PaletteAction::StartDm, "Start a DM", "message-circle", &[], "dm direct message chat friend person"),
        entry(PaletteAction::Join, "Join by address", "log-in", &[], "join room server alias link matrix.to id knock"),
        entry(PaletteAction::Settings, "Settings", "settings", &["Ctrl", ","], "preferences options"),
    ];
    list.extend(SettingsPage::listed(!state.no_window).into_iter().map(|page| PaletteEntry {
        action: PaletteAction::SettingsPage(page),
        label: format!("Settings: {}", page.label()),
        icon: page.glyph(),
        keys: Vec::new(),
        words: "preferences options",
    }));
    list.push(entry(
        PaletteAction::ToggleTheme,
        if state.light { "Switch to the dark theme" } else { "Switch to the light theme" },
        "palette",
        &[],
        "toggle theme dark light appearance night day mode",
    ));
    list.push(entry(
        PaletteAction::ToggleStreamer,
        if state.streamer { "Turn off streamer mode" } else { "Turn on streamer mode" },
        "eye-off",
        &[],
        "toggle streamer mode hide privacy stream",
    ));
    list.push(entry(
        PaletteAction::ToggleMute,
        if state.muted || state.deafened { "Unmute" } else { "Mute" },
        if state.muted || state.deafened { "mic-off" } else { "mic" },
        &["Ctrl", "Shift", "M"],
        "microphone mic toggle voice",
    ));
    list.push(entry(
        PaletteAction::ToggleDeafen,
        if state.deafened { "Undeafen" } else { "Deafen" },
        "headphones",
        &["Ctrl", "Shift", "D"],
        "headphones sound toggle voice",
    ));
    if state.in_call {
        list.push(entry(PaletteAction::LeaveCall, "Leave call", "phone-off", &[], "hang up disconnect end call"));
    }
    list.extend([
        entry(PaletteAction::Inbox, "Inbox", "inbox", &["Ctrl", "I"], "mentions notifications invites"),
        entry(PaletteAction::Shortcuts, "Keyboard shortcuts", "circle-help", &["Ctrl", "/"], "hotkeys keys help"),
        entry(PaletteAction::CheckForUpdates, "Check for updates", "refresh-cw", &[], "update upgrade version"),
        entry(PaletteAction::Acknowledgements, "Acknowledgements", "heart", &[], "about credits licenses thanks"),
    ]);
    list
}

/// How well `needle` matches `text` as a fuzzy (in-order, not necessarily
/// adjacent) match, case aside: `None` when it does not match; higher is
/// better (prefixes, word starts and runs score more).
pub fn fuzzy_score(needle: &str, text: &str) -> Option<i32> {
    let hay: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    let want: Vec<char> = needle
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if want.is_empty() {
        return Some(0);
    }
    // The best score of matching the first `i + 1` letters with the last one
    // at each place of the text.
    let mut best: Vec<Option<i32>> = vec![None; hay.len()];
    for (i, letter) in want.iter().enumerate() {
        let mut next: Vec<Option<i32>> = vec![None; hay.len()];
        for j in 0..hay.len() {
            if hay[j] != *letter {
                continue;
            }
            let here = 10
                + if j == 0 {
                    30
                } else if !hay[j - 1].is_alphanumeric() {
                    20
                } else {
                    0
                };
            let before = if i == 0 {
                Some(-(j.min(10) as i32))
            } else {
                (0..j)
                    .filter_map(|k| {
                        best[k].map(|score| {
                            score + if k + 1 == j { 15 } else { -((j - k - 1).min(10) as i32) }
                        })
                    })
                    .max()
            };
            next[j] = before.map(|b| b + here);
        }
        best = next;
    }
    best.into_iter().flatten().max()
}

/// The entries matching `needle`, best first (all of them, in order, when it
/// is empty).
pub fn search(entries: Vec<PaletteEntry>, needle: &str) -> Vec<PaletteEntry> {
    if needle.trim().is_empty() {
        return entries;
    }
    let mut scored: Vec<(i32, usize, PaletteEntry)> = entries
        .into_iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let label = fuzzy_score(needle, &entry.label).map(|s| s + 20);
            let words = fuzzy_score(needle, entry.words);
            label.max(words).map(|score| (score, index, entry))
        })
        .collect();
    scored.sort_by_key(|(score, index, _)| (std::cmp::Reverse(*score), *index));
    scored.into_iter().map(|(_, _, entry)| entry).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(entries: &[PaletteEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.label.as_str()).collect()
    }

    #[test]
    fn the_actions_the_issue_lists_are_there_with_icons() {
        let all = actions(PaletteState::default());
        for want in [
            PaletteAction::CreateServer,
            PaletteAction::Discover,
            PaletteAction::Settings,
            PaletteAction::StartDm,
            PaletteAction::Join,
            PaletteAction::ToggleTheme,
            PaletteAction::ToggleStreamer,
            PaletteAction::ToggleMute,
            PaletteAction::ToggleDeafen,
        ] {
            assert!(all.iter().any(|e| e.action == want), "{want:?} is offered");
        }
        assert!(all.iter().all(|e| !e.icon.is_empty() && !e.label.is_empty()));
        for page in SettingsPage::listed(true) {
            assert!(
                all.iter().any(|e| e.action == PaletteAction::SettingsPage(page)),
                "a row for the {page:?} page"
            );
        }
    }

    #[test]
    fn shortcuts_come_as_key_caps() {
        let all = actions(PaletteState::default());
        let keys = |a| all.iter().find(|e| e.action == a).expect("entry").keys.clone();
        assert_eq!(keys(PaletteAction::CreateServer), ["Ctrl", "Shift", "N"]);
        assert_eq!(keys(PaletteAction::Settings), ["Ctrl", ","]);
        assert_eq!(keys(PaletteAction::ToggleMute), ["Ctrl", "Shift", "M"]);
        assert!(keys(PaletteAction::Discover).is_empty());
    }

    #[test]
    fn leave_call_is_offered_only_in_a_call_and_labels_follow_the_state() {
        let idle = actions(PaletteState::default());
        assert!(!idle.iter().any(|e| e.action == PaletteAction::LeaveCall));
        assert!(labels(&idle).contains(&"Mute"));
        assert!(labels(&idle).contains(&"Deafen"));
        let busy = actions(PaletteState { in_call: true, muted: true, deafened: true, streamer: true, light: true, no_window: false });
        assert!(busy.iter().any(|e| e.action == PaletteAction::LeaveCall));
        assert!(labels(&busy).contains(&"Unmute"));
        assert!(labels(&busy).contains(&"Undeafen"));
        assert!(labels(&busy).contains(&"Turn off streamer mode"));
        assert!(labels(&busy).contains(&"Switch to the dark theme"));
        assert!(labels(&idle).contains(&"Switch to the light theme"));
    }

    #[test]
    fn fuzzy_matching_finds_in_order_letters_and_ranks_tight_matches_first() {
        assert!(fuzzy_score("crsv", "Create a server").is_some());
        assert!(fuzzy_score("zzz", "Create a server").is_none());
        assert!(fuzzy_score("", "anything").is_some());
        // Letters must come in order.
        assert!(fuzzy_score("revres", "Create a server").is_none());
        // A prefix beats a scattered match; a word start beats the middle of one.
        assert!(fuzzy_score("set", "Settings") > fuzzy_score("set", "Create a server"));
        assert!(fuzzy_score("ser", "Create a server") > fuzzy_score("ser", "Observer"));
    }

    #[test]
    fn searching_finds_actions_by_label_or_other_words_best_first() {
        let all = actions(PaletteState::default());
        let found = search(all.clone(), "dm");
        assert_eq!(found[0].action, PaletteAction::StartDm, "{:?}", labels(&found));
        let found = search(all.clone(), "dark");
        assert!(found.iter().any(|e| e.action == PaletteAction::ToggleTheme), "words find the theme toggle");
        let found = search(all.clone(), "crsv");
        assert_eq!(found[0].action, PaletteAction::CreateServer);
        assert!(search(all.clone(), "qqqqq").is_empty());
        assert_eq!(search(all.clone(), "").len(), all.len());
        let found = search(all, "voice");
        assert!(found.iter().any(|e| e.action == PaletteAction::SettingsPage(SettingsPage::VoiceVideo)));
    }
}
