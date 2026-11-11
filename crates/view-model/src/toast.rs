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
pub fn failure_text(what: &str, err: &str) -> String {
    let mut chars = what.chars();
    let label: String = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    };
    format!("{label} failed: {err}")
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
    fn failures_start_with_a_capital() {
        assert_eq!(failure_text("pin", "forbidden"), "Pin failed: forbidden");
        assert_eq!(failure_text("", "x"), " failed: x");
    }
}
