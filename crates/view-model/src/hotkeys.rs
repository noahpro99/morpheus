//! Keyboard shortcuts (Discord's set, as `crates/ui/src/components/hotkeys.rs`
//! has them): which combo is which action, and the pure parts of what some
//! of them do (stepping through rooms and scopes), plus the rows of the
//! shortcuts list.

/// What a shortcut asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hotkey {
    /// Ctrl+F: search the open room.
    Search,
    /// Ctrl+E: the emoji picker of the message box.
    Emoji,
    /// Ctrl+Shift+N: create or join a server.
    CreateServer,
    /// Ctrl+/: the list of shortcuts.
    Shortcuts,
    /// Ctrl+I: the inbox.
    Inbox,
    /// Ctrl+U: show or hide the member list.
    Members,
    /// Ctrl+P: the pinned messages.
    Pins,
    /// Ctrl+,: settings.
    Settings,
    /// Alt+Up / Alt+Down: the previous / next room of the sidebar.
    Room { forward: bool },
    /// Alt+Shift+Up / Down: the previous / next room with something unread.
    Unread { forward: bool },
    /// Ctrl+Alt+Up / Down: the previous / next rail entry.
    Scope { forward: bool },
    /// Alt+Left / Alt+Right: back and forward through the rooms visited.
    History { forward: bool },
    /// Ctrl+K, Ctrl+Shift+P: the quick switcher.
    Switcher,
    /// Ctrl+Shift+M: mute, or unmute.
    ToggleMute,
    /// Ctrl+Shift+D: deafen, or undeafen.
    ToggleDeafen,
    /// Ctrl+Alt+Shift+V: the room of the call you are in.
    CurrentCall,
    /// Ctrl+': start (or join) the call of the open room.
    StartCall,
}

/// The action for a key (a DOM key name, `a`, `ArrowUp`, `,`) pressed with
/// these modifiers; `None` for keys that are no shortcut. Escape is not
/// here: what it closes depends on what is open.
pub fn classify(key: &str, ctrl: bool, alt: bool, shift: bool) -> Option<Hotkey> {
    let lower = key.to_lowercase();
    let key = if key.chars().count() == 1 { lower.as_str() } else { key };
    let direction = match key {
        "ArrowUp" | "up" => Some(false),
        "ArrowDown" | "down" => Some(true),
        _ => None,
    };
    let sideways = match key {
        "ArrowLeft" | "left" => Some(false),
        "ArrowRight" | "right" => Some(true),
        _ => None,
    };
    match (ctrl, alt, shift) {
        (true, false, false) => match key {
            "f" => Some(Hotkey::Search),
            "e" => Some(Hotkey::Emoji),
            "/" => Some(Hotkey::Shortcuts),
            "i" => Some(Hotkey::Inbox),
            "u" => Some(Hotkey::Members),
            "p" => Some(Hotkey::Pins),
            "," => Some(Hotkey::Settings),
            "k" => Some(Hotkey::Switcher),
            "'" => Some(Hotkey::StartCall),
            _ => None,
        },
        (true, false, true) => match key {
            "n" => Some(Hotkey::CreateServer),
            "p" => Some(Hotkey::Switcher),
            "m" => Some(Hotkey::ToggleMute),
            "d" => Some(Hotkey::ToggleDeafen),
            _ => None,
        },
        (true, true, true) => match key {
            "v" => Some(Hotkey::CurrentCall),
            _ => None,
        },
        (true, true, false) => direction.map(|forward| Hotkey::Scope { forward }),
        (false, true, false) => match sideways {
            Some(forward) => Some(Hotkey::History { forward }),
            None => direction.map(|forward| Hotkey::Room { forward }),
        },
        (false, true, true) => match sideways {
            Some(forward) => Some(Hotkey::History { forward }),
            None => direction.map(|forward| Hotkey::Unread { forward }),
        },
        _ => None,
    }
}

/// Where `Alt+Up/Down` goes in a list whose entries are `candidates` (true:
/// can be landed on; all true for rooms, unread ones only for the unread
/// shortcut), from `current` (the open room's place in the list, if it is in
/// it). Forward takes the next candidate after it, wrapping to the first;
/// backward the last before it, wrapping to the last; with no current place,
/// the first.
pub fn step_room(candidates: &[bool], current: Option<usize>, forward: bool) -> Option<usize> {
    let ok: Vec<usize> = (0..candidates.len()).filter(|&i| candidates[i]).collect();
    if ok.is_empty() {
        return None;
    }
    let Some(at) = current else {
        return Some(ok[0]);
    };
    if forward {
        Some(ok.iter().copied().find(|&i| i > at).unwrap_or(ok[0]))
    } else {
        Some(ok.iter().rev().copied().find(|&i| i < at).unwrap_or(ok[ok.len() - 1]))
    }
}

/// The rail entry after (or before) the current one, wrapping round.
pub fn step_scope(len: usize, current: Option<usize>, forward: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let at = current.unwrap_or(0);
    Some(if forward {
        (at + 1) % len
    } else {
        (at + len - 1) % len
    })
}

/// Rows for the shortcuts dialog: (keys, what it does), grouped. As far as
/// this app has them.
pub const GROUPS: &[(&str, &[(&str, &str)])] = &[
    (
        "Navigation",
        &[
            (
                "Ctrl+K or Ctrl+Shift+P",
                "Command palette: rooms, actions and settings",
            ),
            ("Alt+↑ / Alt+↓", "Previous / next room"),
            ("Alt+← / Alt+→", "Back / forward through the rooms visited"),
            ("Alt+Shift+↑ / ↓", "Previous / next unread room"),
            ("Ctrl+Alt+↑ / ↓", "Previous / next server (rail)"),
            ("Ctrl+Shift+Alt+V", "Go to the call you are in"),
        ],
    ),
    (
        "Messages",
        &[
            ("Enter", "Send (Shift+Enter for a new line)"),
            ("Ctrl+E", "Emoji picker"),
            ("Esc", "Close what is open"),
            ("Right-click", "Actions for a message, room, or member"),
        ],
    ),
    (
        "Panels",
        &[
            ("Ctrl+F", "Search this room"),
            ("Ctrl+I", "Inbox"),
            ("Ctrl+P", "Pinned messages"),
            ("Ctrl+U", "Show or hide the member list"),
            ("Ctrl+,", "Settings"),
            ("Ctrl+/", "This list"),
            ("Ctrl+Shift+N", "Create or join a server"),
        ],
    ),
    (
        "Voice",
        &[
            ("Ctrl+Shift+M", "Mute or unmute"),
            ("Ctrl+Shift+D", "Deafen or undeafen (deafening mutes too)"),
            ("Ctrl+Enter", "Answer a call ringing"),
            ("Esc", "Decline a call ringing"),
            ("Ctrl+'", "Call the DM open"),
        ],
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combos_name_their_actions() {
        assert_eq!(classify("f", true, false, false), Some(Hotkey::Search));
        assert_eq!(classify("F", true, false, false), Some(Hotkey::Search));
        assert_eq!(classify("i", true, false, false), Some(Hotkey::Inbox));
        assert_eq!(classify("/", true, false, false), Some(Hotkey::Shortcuts));
        assert_eq!(classify(",", true, false, false), Some(Hotkey::Settings));
        assert_eq!(classify("N", true, false, true), Some(Hotkey::CreateServer));
        assert_eq!(
            classify("ArrowDown", false, true, false),
            Some(Hotkey::Room { forward: true })
        );
        assert_eq!(
            classify("ArrowUp", false, true, true),
            Some(Hotkey::Unread { forward: false })
        );
        assert_eq!(
            classify("ArrowDown", true, true, false),
            Some(Hotkey::Scope { forward: true })
        );
    }

    #[test]
    fn alt_arrows_go_back_and_forward() {
        assert_eq!(classify("ArrowLeft", false, true, false), Some(Hotkey::History { forward: false }));
        assert_eq!(classify("right", false, true, false), Some(Hotkey::History { forward: true }));
        assert_eq!(classify("ArrowLeft", false, false, false), None);
        assert_eq!(classify("ArrowLeft", true, false, false), None);
    }

    #[test]
    fn plain_keys_and_other_combos_are_nothing() {
        assert_eq!(classify("f", false, false, false), None);
        assert_eq!(classify("f", true, true, false), None);
        assert_eq!(classify("ArrowDown", false, false, false), None);
        assert_eq!(classify("n", true, false, false), None);
    }

    #[test]
    fn rooms_step_and_wrap() {
        let all = [true, true, true];
        assert_eq!(step_room(&all, Some(0), true), Some(1));
        assert_eq!(step_room(&all, Some(2), true), Some(0));
        assert_eq!(step_room(&all, Some(0), false), Some(2));
        assert_eq!(step_room(&all, None, true), Some(0));
        assert_eq!(step_room(&[], Some(0), true), None);
    }

    #[test]
    fn unread_stepping_skips_the_read_ones() {
        let unread = [false, true, false, true];
        assert_eq!(step_room(&unread, Some(0), true), Some(1));
        assert_eq!(step_room(&unread, Some(1), true), Some(3));
        assert_eq!(step_room(&unread, Some(3), true), Some(1));
        assert_eq!(step_room(&unread, Some(2), false), Some(1));
        assert_eq!(step_room(&[false, false], Some(0), true), None);
    }

    #[test]
    fn scopes_wrap_round() {
        assert_eq!(step_scope(4, Some(3), true), Some(0));
        assert_eq!(step_scope(4, Some(0), false), Some(3));
        assert_eq!(step_scope(4, None, true), Some(1));
        assert_eq!(step_scope(0, None, true), None);
    }
}
