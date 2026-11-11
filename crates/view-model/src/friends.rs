//! The Friends page without a toolkit (`crates/ui/src/components/friends.rs`):
//! its tabs, the list's heading and rows, and the New Message form's states.
//! Matrix has no friendship, so a friend is a name in a private list and
//! there are no requests or presence tabs: just All and New Message.

use crate::nicknames::{NickMap, shown_name};

/// The page's tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    All,
    NewMessage,
}

impl Tab {
    pub fn label(self) -> &'static str {
        match self {
            Tab::All => "All",
            Tab::NewMessage => "New Message",
        }
    }
}

/// The tab showing: the one picked, else the friends if there are any,
/// else the way to add someone comes first.
pub fn effective_tab(picked: Option<Tab>, friends: usize) -> Tab {
    picked.unwrap_or(if friends == 0 {
        Tab::NewMessage
    } else {
        Tab::All
    })
}

pub const EMPTY_TITLE: &str = "No friends yet";
pub const EMPTY_HINT: &str =
    "Add someone from their menu: right-click (or press and hold) their name.";
pub const NEW_MESSAGE_TITLE: &str = "New Message";
pub const NEW_MESSAGE_HINT: &str =
    "Message someone by their username, or their full Matrix ID for someone on another server.";
pub const NEW_MESSAGE_PLACEHOLDER: &str = "Username or @name:server";

/// The heading over the list.
pub fn count_heading(count: usize) -> String {
    format!("All friends — {count}")
}

/// What a friend's row shows: the name (your nickname for them first), and
/// the line under it (their status, else their Matrix ID).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FriendRow {
    pub name: String,
    pub subtitle: String,
}

/// A friend's row, from what is known of their profile (`None` until it has
/// loaded: the ID's local part stands in).
pub fn friend_row(
    nicknames: &NickMap,
    user_id: &str,
    profile: Option<(&str, Option<&str>)>,
) -> FriendRow {
    let local = user_id
        .trim_start_matches('@')
        .split(':')
        .next()
        .unwrap_or(user_id);
    let (name, status) = match profile {
        Some((name, status)) => (name, status),
        None => (local, None),
    };
    FriendRow {
        name: shown_name(nicknames, user_id, name),
        subtitle: status.unwrap_or(user_id).to_owned(),
    }
}

/// The New Message form.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewMessage {
    pub typed: String,
    pub busy: bool,
    pub failed: Option<String>,
}

impl NewMessage {
    /// Typing clears the last refusal.
    pub fn typed(&mut self, text: &str) {
        self.typed = text.to_owned();
        self.failed = None;
    }

    pub fn can_submit(&self) -> bool {
        !self.typed.trim().is_empty() && !self.busy
    }

    pub fn button_label(&self) -> &'static str {
        if self.busy { "Opening…" } else { "Message" }
    }

    /// Submitting: what to look up, or nothing (empty, or already going).
    pub fn begin(&mut self) -> Option<String> {
        if !self.can_submit() {
            return None;
        }
        self.busy = true;
        Some(self.typed.clone())
    }

    /// The outcome: on success the box empties; on failure the reason shows.
    pub fn finish(&mut self, outcome: Result<(), String>) {
        self.busy = false;
        match outcome {
            Ok(()) => self.typed.clear(),
            Err(reason) => self.failed = Some(reason),
        }
    }
}

/// Messaging yourself is refused.
pub fn check_target(own: Option<&str>, found: &str) -> Result<(), String> {
    if own == Some(found) {
        Err("That is you.".to_owned())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_tab_depends_on_having_friends() {
        assert_eq!(effective_tab(None, 0), Tab::NewMessage);
        assert_eq!(effective_tab(None, 3), Tab::All);
        assert_eq!(effective_tab(Some(Tab::All), 0), Tab::All);
        assert_eq!(effective_tab(Some(Tab::NewMessage), 3), Tab::NewMessage);
    }

    #[test]
    fn the_heading_counts() {
        assert_eq!(count_heading(2), "All friends — 2");
    }

    #[test]
    fn a_row_is_the_name_over_status_or_id() {
        let mut nicks = NickMap::new();
        let loading = friend_row(&nicks, "@bob:x", None);
        assert_eq!(loading.name, "bob");
        assert_eq!(loading.subtitle, "@bob:x");
        let loaded = friend_row(&nicks, "@bob:x", Some(("Bob B", Some("out to lunch"))));
        assert_eq!(loaded.name, "Bob B");
        assert_eq!(loaded.subtitle, "out to lunch");
        let no_status = friend_row(&nicks, "@bob:x", Some(("Bob B", None)));
        assert_eq!(no_status.subtitle, "@bob:x");
        nicks.insert("@bob:x".into(), "Bobby".into());
        assert_eq!(friend_row(&nicks, "@bob:x", Some(("Bob B", None))).name, "Bobby");
    }

    #[test]
    fn the_form_needs_text_and_goes_once() {
        let mut form = NewMessage::default();
        assert!(!form.can_submit());
        assert_eq!(form.begin(), None);
        form.typed("  ");
        assert!(!form.can_submit());
        form.typed("alice");
        assert_eq!(form.button_label(), "Message");
        assert_eq!(form.begin().as_deref(), Some("alice"));
        assert_eq!(form.button_label(), "Opening…");
        assert_eq!(form.begin(), None, "already going");
    }

    #[test]
    fn a_refusal_shows_until_typing_and_success_empties_the_box() {
        let mut form = NewMessage::default();
        form.typed("nobody-here");
        form.begin();
        form.finish(Err("No one is called @nobody-here:x.".into()));
        assert!(!form.busy);
        assert_eq!(form.failed.as_deref(), Some("No one is called @nobody-here:x."));
        form.typed("alice");
        assert_eq!(form.failed, None);
        form.begin();
        form.finish(Ok(()));
        assert_eq!(form.typed, "");
    }

    #[test]
    fn you_cannot_message_yourself() {
        assert!(check_target(Some("@a:x"), "@a:x").is_err());
        assert!(check_target(Some("@a:x"), "@b:x").is_ok());
        assert!(check_target(None, "@b:x").is_ok());
    }
}
