//! The icon a state line ("bob joined the room", "alice started a call")
//! leads with, by its kind: Discord's green arrow in for a join, red out
//! for a leave, kick or ban, a green phone for a call, grey for the rest.

use morpheus_core::StateKind;

/// How strongly the icon is coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Green.
    Good,
    /// Red.
    Bad,
    /// Grey, like the line's text.
    Quiet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateIcon {
    /// A Lucide name.
    pub glyph: &'static str,
    pub tone: Tone,
}

pub fn icon(kind: Option<StateKind>) -> StateIcon {
    let (glyph, tone) = match kind {
        Some(StateKind::CallStarted) => ("phone", Tone::Good),
        Some(StateKind::CallEnded) => ("phone", Tone::Quiet),
        Some(StateKind::Joined) => ("log-in", Tone::Good),
        Some(StateKind::Left | StateKind::Removed) => ("log-out", Tone::Bad),
        Some(StateKind::Other) | None => ("info", Tone::Quiet),
    };
    StateIcon { glyph, tone }
}

impl StateIcon {
    /// The name tests and styles find it by (`state-icon call-started`).
    pub fn class(kind: Option<StateKind>) -> &'static str {
        match kind {
            Some(StateKind::CallStarted) => "call-started",
            Some(StateKind::CallEnded) => "call-ended",
            Some(StateKind::Joined) => "joined",
            Some(StateKind::Left) => "left",
            Some(StateKind::Removed) => "removed",
            Some(StateKind::Other) | None => "other",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_has_its_icon_and_colour() {
        assert_eq!(icon(Some(StateKind::CallStarted)), StateIcon { glyph: "phone", tone: Tone::Good });
        assert_eq!(icon(Some(StateKind::CallEnded)), StateIcon { glyph: "phone", tone: Tone::Quiet });
        assert_eq!(icon(Some(StateKind::Joined)), StateIcon { glyph: "log-in", tone: Tone::Good });
        for kind in [StateKind::Left, StateKind::Removed] {
            assert_eq!(icon(Some(kind)), StateIcon { glyph: "log-out", tone: Tone::Bad });
        }
        assert_eq!(icon(Some(StateKind::Other)), icon(None));
        assert_eq!(icon(None).tone, Tone::Quiet);
    }
}
