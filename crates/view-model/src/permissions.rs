//! Who may do what in a channel (#175): the room's power levels read as
//! three choices (Everyone, Moderators and admins, Admins only) for sending
//! messages, reacting, pinning and starting calls, and the new power levels
//! content when one choice changes.

use serde_json::{Map, Value, json};

/// The roles' power levels (the same as `MemberRole::power_level`).
pub const MEMBER: i64 = 0;
pub const MODERATOR: i64 = 50;
pub const ADMIN: i64 = 100;

/// What a channel asks of someone to do a thing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Everyone,
    Moderators,
    Admins,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Everyone, Level::Moderators, Level::Admins];

    pub fn label(self) -> &'static str {
        match self {
            Level::Everyone => "Everyone",
            Level::Moderators => "Moderators and admins",
            Level::Admins => "Admins only",
        }
    }

    /// The power level that grants it.
    pub fn power(self) -> i64 {
        match self {
            Level::Everyone => MEMBER,
            Level::Moderators => MODERATOR,
            Level::Admins => ADMIN,
        }
    }

    /// The choice that a required power level amounts to (a level in
    /// between asks at least the one below it).
    pub fn from_required(required: i64) -> Level {
        if required <= MEMBER {
            Level::Everyone
        } else if required <= MODERATOR {
            Level::Moderators
        } else {
            Level::Admins
        }
    }
}

/// The things a channel can restrict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Send,
    React,
    Pin,
    Call,
}

impl Action {
    pub const ALL: [Action; 4] = [Action::Send, Action::React, Action::Pin, Action::Call];

    pub fn label(self) -> &'static str {
        match self {
            Action::Send => "Send messages",
            Action::React => "Add reactions",
            Action::Pin => "Pin messages",
            Action::Call => "Start calls",
        }
    }

    /// A short id for tests and element names.
    pub fn id(self) -> &'static str {
        match self {
            Action::Send => "send",
            Action::React => "react",
            Action::Pin => "pin",
            Action::Call => "call",
        }
    }

    /// Whether the power is set on events (`events_default` otherwise
    /// `state_default` is the fallback).
    fn is_state(self) -> bool {
        matches!(self, Action::Pin | Action::Call)
    }

    /// The event types whose power levels this choice sets.
    fn event_types(self) -> &'static [&'static str] {
        match self {
            // Encrypted rooms send messages as m.room.encrypted.
            Action::Send => &["m.room.message", "m.room.encrypted"],
            Action::React => &["m.reaction"],
            Action::Pin => &["m.room.pinned_events"],
            Action::Call => &["org.matrix.msc3401.call.member", "m.call.member"],
        }
    }
}

/// A room's `m.room.power_levels` content, with what the signed-in person
/// has: their own power level and whether they may change the levels.
#[derive(Clone, Debug, PartialEq)]
pub struct Powers {
    pub content: Value,
    pub own: i64,
    pub can_change: bool,
}

fn number(value: Option<&Value>) -> Option<i64> {
    value.and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
}

impl Powers {
    /// The power level `action` needs now.
    pub fn required(&self, action: Action) -> i64 {
        let events = self.content.get("events");
        let default = if action.is_state() {
            number(self.content.get("state_default")).unwrap_or(MODERATOR)
        } else {
            number(self.content.get("events_default")).unwrap_or(MEMBER)
        };
        // The first listed type that is set decides.
        action
            .event_types()
            .iter()
            .find_map(|kind| number(events.and_then(|e| e.get(*kind))))
            .unwrap_or(default)
    }

    /// The choice the channel is on for `action`.
    pub fn level(&self, action: Action) -> Level {
        Level::from_required(self.required(action))
    }

    /// Whether the signed-in person may do `action`.
    pub fn allows(&self, action: Action) -> bool {
        self.own >= self.required(action)
    }

    /// The content after choosing `level` for `action`: every other value
    /// kept, the event types of the action set (to 0 for Everyone, so a
    /// higher default does not apply).
    pub fn with_level(&self, action: Action, level: Level) -> Value {
        let mut content = match &self.content {
            Value::Object(map) => map.clone(),
            _ => Map::new(),
        };
        let mut events = match content.remove("events") {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        for kind in action.event_types() {
            events.insert((*kind).to_owned(), json!(level.power()));
        }
        content.insert("events".to_owned(), Value::Object(events));
        Value::Object(content)
    }
}

/// The reason shown in place of the message box.
pub const CANNOT_SEND: &str = "You don't have permission to send messages in this channel";

/// Said in the settings to someone who may not change the levels.
pub const CANNOT_CHANGE: &str = "Only admins can change who may do what in this channel.";

#[cfg(test)]
mod tests {
    use super::*;

    fn powers(content: Value, own: i64) -> Powers {
        Powers { content, own, can_change: own >= ADMIN }
    }

    #[test]
    fn defaults_let_members_send_and_react_but_not_pin() {
        let p = powers(json!({}), MEMBER);
        assert!(p.allows(Action::Send));
        assert!(p.allows(Action::React));
        assert!(!p.allows(Action::Pin), "state_default is 50");
        assert_eq!(p.level(Action::Send), Level::Everyone);
        assert_eq!(p.level(Action::Pin), Level::Moderators);
    }

    #[test]
    fn events_default_and_per_type_overrides_decide() {
        let p = powers(json!({ "events_default": 50, "events": { "m.reaction": 0 } }), MEMBER);
        assert_eq!(p.level(Action::Send), Level::Moderators);
        assert!(!p.allows(Action::Send));
        assert!(p.allows(Action::React), "the type override wins");
        let p = powers(json!({ "events_default": "100" }), MODERATOR);
        assert_eq!(p.level(Action::Send), Level::Admins, "levels may be strings");
        assert!(!p.allows(Action::Send));
    }

    #[test]
    fn the_first_set_type_decides() {
        let p = powers(json!({ "events": { "m.room.encrypted": 50, "m.room.message": 0 } }), MEMBER);
        assert_eq!(p.required(Action::Send), 0, "m.room.message comes first");
    }

    #[test]
    fn a_level_between_two_choices_asks_for_the_higher() {
        assert_eq!(Level::from_required(-1), Level::Everyone);
        assert_eq!(Level::from_required(25), Level::Moderators);
        assert_eq!(Level::from_required(75), Level::Admins);
        assert_eq!(Level::from_required(100), Level::Admins);
    }

    #[test]
    fn choosing_a_level_sets_only_that_actions_types_and_keeps_the_rest() {
        let p = powers(
            json!({ "events_default": 0, "users": { "@a:x": 100 }, "events": { "m.room.name": 50 } }),
            ADMIN,
        );
        let content = p.with_level(Action::Send, Level::Moderators);
        assert_eq!(content["events"]["m.room.message"], 50);
        assert_eq!(content["events"]["m.room.encrypted"], 50);
        assert_eq!(content["events"]["m.room.name"], 50, "others kept");
        assert_eq!(content["users"]["@a:x"], 100);
        assert_eq!(content["events_default"], 0);
        let after = powers(content, MEMBER);
        assert!(!after.allows(Action::Send));
        // And back: Everyone is an explicit 0.
        let back = after.with_level(Action::Send, Level::Everyone);
        assert_eq!(back["events"]["m.room.message"], 0);
        assert!(powers(back, MEMBER).allows(Action::Send));
    }

    #[test]
    fn pins_and_calls_can_be_opened_to_everyone() {
        let p = powers(json!({ "state_default": 50 }), MEMBER);
        let open = p.with_level(Action::Pin, Level::Everyone);
        assert!(powers(open.clone(), MEMBER).allows(Action::Pin));
        assert_eq!(open["events"]["m.room.pinned_events"], 0);
        let calls = p.with_level(Action::Call, Level::Admins);
        assert_eq!(calls["events"]["org.matrix.msc3401.call.member"], 100);
        assert_eq!(powers(calls, MODERATOR).level(Action::Call), Level::Admins);
    }

    #[test]
    fn labels_name_the_choices() {
        let labels: Vec<_> = Level::ALL.iter().map(|l| l.label()).collect();
        assert_eq!(labels, ["Everyone", "Moderators and admins", "Admins only"]);
        assert_eq!(Action::ALL.map(|a| a.id()), ["send", "react", "pin", "call"]);
    }
}

/// A message menu without what the person may not do here: pinning, and
/// reacting (the emoji grid).
pub fn restrict_menu(items: Vec<crate::menu::MenuItem>, powers: &Powers) -> Vec<crate::menu::MenuItem> {
    use crate::menu::MenuAction;
    items
        .into_iter()
        .filter(|item| match &item.action {
            Some(MenuAction::Pin(_)) => powers.allows(Action::Pin),
            Some(MenuAction::React(_)) => powers.allows(Action::React),
            // The "Add reaction" row only opens the grid of reactions.
            None => powers.allows(Action::React) || !item.children.iter().any(|c| matches!(c.action, Some(MenuAction::React(_)))),
            _ => true,
        })
        .collect()
}

#[cfg(test)]
mod menu_tests {
    use super::*;
    use crate::menu::{MenuAction, MenuItem};

    fn menu() -> Vec<MenuItem> {
        vec![
            MenuItem::new("Reply", MenuAction::Reply),
            MenuItem::grid("Add reaction", vec![MenuItem::new("👍", MenuAction::React("👍".into()))]),
            MenuItem::new("Pin message", MenuAction::Pin(true)),
        ]
    }

    fn labels(items: &[MenuItem]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_str()).collect()
    }

    #[test]
    fn a_member_in_a_default_room_may_react_but_not_pin() {
        let p = Powers { content: json!({}), own: MEMBER, can_change: false };
        assert_eq!(labels(&restrict_menu(menu(), &p)), ["Reply", "Add reaction"]);
    }

    #[test]
    fn moderators_only_reactions_hide_the_grid_for_members() {
        let p = Powers { content: json!({ "events": { "m.reaction": 50, "m.room.pinned_events": 0 } }), own: MEMBER, can_change: false };
        assert_eq!(labels(&restrict_menu(menu(), &p)), ["Reply", "Pin message"]);
        let admin = Powers { own: ADMIN, ..p };
        assert_eq!(labels(&restrict_menu(menu(), &admin)).len(), 3);
    }
}
