//! Toasts (`crates/ui/src/components/toast.rs`): short notices stacked in
//! the bottom-right corner for actions that failed in the background, and
//! for undoable ones ("Message deleted" with an Undo button). This is the
//! list and its wording; the front end draws it and runs the timers.

/// How long a notice stays unless dismissed.
pub const TOAST_SECS: u64 = 6;
/// How long an undoable action waits before it is done for good.
pub const UNDO_SECS: u64 = 5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toast {
    pub id: u32,
    pub text: String,
    /// Offers Undo.
    pub undo: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToastList {
    pub items: Vec<Toast>,
    next_id: u32,
}

impl ToastList {
    /// Adds a toast; returns its id.
    pub fn push(&mut self, text: impl Into<String>, undo: bool) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.items.push(Toast {
            id,
            text: text.into(),
            undo,
        });
        id
    }

    /// Takes a toast off; whether it was there.
    pub fn dismiss(&mut self, id: u32) -> bool {
        let before = self.items.len();
        self.items.retain(|toast| toast.id != id);
        self.items.len() != before
    }
}

/// "Pin failed: reason": what a failed action tells the user.
/// What can fail, as the words `failure_text` puts first (each is a key: "pin" is "fijar").
pub const WHATS: &[&str] = &[
    "accepting an invite", "declining an invite", "declining the call", "delete", "friend", "invite", "join room",
    "leave", "listening for calls", "loading your marks", "opening a DM", "opening the link", "pin",
    "saving the audio devices", "sharing the screen", "switching audio devices", "the call dropped",
    "turning the camera on", "update check", "updating", "reorder channels", "changing the role", "kicking",
    "unbanning", "banning", "forward", "mark unread", "mark read", "saving", "mute", "unmute",
    "notification mode", "favorites", "block", "role change", "kick", "ban", "save prefs", "edit", "reaction",
    "unblock",
];

pub fn failure_text(what: &str, err: &str) -> String {
    failure_text_in(crate::i18n::language(), what, err)
}

pub fn failure_text_in(code: &str, what: &str, err: &str) -> String {
    let what = crate::i18n::t_in(code, what);
    let args = [("what", what.as_str()), ("err", err)];
    let tf = |key: &str| crate::i18n::tf_in(code, key, &args);
    let text = tf("%{what} failed: %{err}");
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toasts_stack_in_order_and_dismiss_by_id() {
        let mut list = ToastList::default();
        let a = list.push("a", false);
        let b = list.push("b", true);
        assert_ne!(a, b);
        assert_eq!(list.items.len(), 2);
        assert!(list.items[1].undo);
        assert!(list.dismiss(a));
        assert!(!list.dismiss(a), "already gone");
        assert_eq!(list.items.iter().map(|t| t.id).collect::<Vec<_>>(), [b]);
        let c = list.push("c", false);
        assert!(c > b, "ids are never reused");
    }

    #[test]
    fn a_failure_is_said_in_the_language_with_the_reason_kept() {
        assert_eq!(failure_text_in("es", "pin", "forbidden"), "Error al fijar: forbidden");
        assert_eq!(failure_text_in("en", "turning the camera on", "busy"), "Turning the camera on failed: busy");
        for what in WHATS {
            assert_ne!(failure_text_in("es", what, "x"), failure_text_in("en", what, "x"), "{what}");
        }
    }

    #[test]
    fn failures_start_with_a_capital() {
        assert_eq!(failure_text("pin", "forbidden"), "Pin failed: forbidden");
        assert_eq!(failure_text("", "x"), " failed: x");
    }
}
