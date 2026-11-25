//! "Say hi! 👋" (#421): an empty direct message offers a button that sends a greeting.

/// What the button sends.
pub const GREETING: &str = "Hi! 👋";

/// Whether the start of a conversation offers "Say hi!": a direct message with nothing in it but
/// room events (`message_is_event` is true for each message that is only such an event).
pub fn offered(is_direct: bool, at_start: bool, message_is_event: impl IntoIterator<Item = bool>) -> bool {
    is_direct && at_start && message_is_event.into_iter().all(|event| event)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_direct_message_offers_it() {
        assert!(offered(true, true, []));
        assert!(offered(true, true, [true, true]));
    }

    #[test]
    fn a_room_or_a_conversation_with_messages_does_not() {
        assert!(!offered(false, true, []));
        assert!(!offered(true, true, [true, false]));
        assert!(!offered(true, false, []));
    }
}
