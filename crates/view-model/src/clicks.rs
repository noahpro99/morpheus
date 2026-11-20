//! Single and double clicks (#286). The click count comes from the toolkit
//! (GPUI's `click_count`); what is here is the rule that a single click's
//! action waits out the double-click window, so it cannot move or replace the
//! row under the pointer before the second click lands, and the
//! classification a platform without a count can use.

/// How long a first click waits for a second (ms): the usual system
/// double-click interval.
pub const DOUBLE_CLICK_MS: u64 = 400;

/// How far the pointer may move between the two clicks (px).
pub const DOUBLE_CLICK_SLOP: f32 = 4.0;

/// A click at `(x, y)` at time `at` (ms).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Click {
    pub at: u64,
    pub x: f32,
    pub y: f32,
}

/// 1 for a click, 2 when it follows `previous` within the double-click window
/// without the pointer having moved.
pub fn click_count(previous: Option<Click>, now: Click) -> u32 {
    match previous {
        Some(p)
            if now.at >= p.at
                && now.at - p.at <= DOUBLE_CLICK_MS
                && (now.x - p.x).abs() <= DOUBLE_CLICK_SLOP
                && (now.y - p.y).abs() <= DOUBLE_CLICK_SLOP =>
        {
            2
        }
        _ => 1,
    }
}

/// The single click whose action is waiting for the double-click window to
/// pass. A second click, or another single click, cancels it.
#[derive(Debug, Default)]
pub struct ClickGate {
    token: u64,
    pending: bool,
}

impl ClickGate {
    /// A single click: its action waits. Returns the token to check on waking.
    pub fn single(&mut self) -> u64 {
        self.token += 1;
        self.pending = true;
        self.token
    }

    /// A double click: nothing waiting runs.
    pub fn double(&mut self) {
        self.token += 1;
        self.pending = false;
    }

    /// The window has passed for `token`: true once, when the action still
    /// stands and runs.
    pub fn due(&mut self, token: u64) -> bool {
        let run = self.pending && self.token == token;
        if run {
            self.pending = false;
        }
        run
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(at: u64, x: f32) -> Click {
        Click { at, x, y: 10.0 }
    }

    #[test]
    fn a_lone_click_is_single() {
        assert_eq!(click_count(None, click(0, 5.0)), 1);
    }

    #[test]
    fn two_quick_clicks_in_place_are_a_double() {
        assert_eq!(click_count(Some(click(0, 5.0)), click(120, 5.0)), 2);
    }

    #[test]
    fn a_slow_pair_is_two_singles() {
        assert_eq!(click_count(Some(click(0, 5.0)), click(900, 5.0)), 1);
    }

    #[test]
    fn a_moved_pointer_is_two_singles() {
        assert_eq!(click_count(Some(click(0, 5.0)), click(120, 60.0)), 1);
    }

    #[test]
    fn a_single_clicks_action_waits_and_a_double_cancels_it() {
        let mut gate = ClickGate::default();
        let first = gate.single();
        gate.double();
        assert!(!gate.due(first), "the double click replaced the single");
        let lone = gate.single();
        assert!(gate.due(lone));
        assert!(!gate.due(lone), "it runs once");
    }

    #[test]
    fn a_newer_single_click_replaces_an_older_one() {
        let mut gate = ClickGate::default();
        let a = gate.single();
        let b = gate.single();
        assert!(!gate.due(a));
        assert!(gate.due(b));
    }
}
