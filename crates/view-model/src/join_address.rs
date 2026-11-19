//! "Join by address": what a person typed or pasted to name a room (a room
//! alias, a room ID, or a matrix.to / matrix: link), read into what the app
//! looks up and joins. The lookup and the join are in core.

use morpheus_core::{MatrixLink, parse_matrix_link};

/// Why the text is not a room's address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressError {
    Empty,
    /// A link to a person, not a room.
    Person,
    /// Not an alias, room ID or link.
    Invalid,
}

impl AddressError {
    pub fn message(self) -> &'static str {
        match self {
            AddressError::Empty => "Type or paste a room address.",
            AddressError::Person => "That is a person, not a room. Start a DM from New Message.",
            AddressError::Invalid => {
                "That is not a room address. Use #room:server, !id:server or a matrix.to link."
            }
        }
    }
}

/// The room (alias or ID, with the servers to ask) that `text` names.
pub fn parse_room_address(text: &str) -> Result<MatrixLink, AddressError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(AddressError::Empty);
    }
    let link = if text.starts_with("matrix:") || text.starts_with("http") {
        // A raw `#` of an alias inside a matrix.to link is the alias's sigil.
        let fixed = text.replacen("matrix.to/#/#", "matrix.to/#/%23", 1);
        parse_matrix_link(&fixed)
    } else if let Some(sigil) = text.chars().next().filter(|c| matches!(c, '#' | '!' | '@')) {
        let encoded = match sigil {
            '#' => "%23",
            '!' => "%21",
            _ => "@",
        };
        parse_matrix_link(&format!("https://matrix.to/#/{encoded}{}", &text[1..]))
    } else {
        None
    };
    match link {
        Some(MatrixLink::User(_)) => Err(AddressError::Person),
        Some(link) => Ok(link),
        None => Err(AddressError::Invalid),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(text: &str) -> (String, Vec<String>) {
        match parse_room_address(text) {
            Ok(MatrixLink::Room { room, via, .. }) => (room.to_string(), via),
            other => panic!("{text:?} is a room address, got {other:?}"),
        }
    }

    #[test]
    fn an_alias_or_room_id_is_a_room() {
        assert_eq!(room("#room:example.org"), ("#room:example.org".into(), vec![]));
        assert_eq!(room("  !abc:example.org\n"), ("!abc:example.org".into(), vec![]));
    }

    #[test]
    fn links_name_rooms_with_their_servers() {
        assert_eq!(room("https://matrix.to/#/%23room:example.org"), ("#room:example.org".into(), vec![]));
        assert_eq!(room("https://matrix.to/#/#room:example.org"), ("#room:example.org".into(), vec![]));
        assert_eq!(
            room("https://matrix.to/#/!abc:example.org?via=a.org&via=b.org"),
            ("!abc:example.org".into(), vec!["a.org".into(), "b.org".into()])
        );
        assert_eq!(room("matrix:r/room:example.org"), ("#room:example.org".into(), vec![]));
        assert_eq!(
            room("matrix:roomid/abc:example.org?via=a.org"),
            ("!abc:example.org".into(), vec!["a.org".into()])
        );
    }

    #[test]
    fn people_and_nonsense_are_told_apart() {
        assert_eq!(parse_room_address(""), Err(AddressError::Empty));
        assert_eq!(parse_room_address("   "), Err(AddressError::Empty));
        assert_eq!(parse_room_address("@bob:example.org"), Err(AddressError::Person));
        assert_eq!(parse_room_address("https://matrix.to/#/@bob:example.org"), Err(AddressError::Person));
        assert_eq!(parse_room_address("matrix:u/bob:example.org"), Err(AddressError::Person));
        assert_eq!(parse_room_address("hello there"), Err(AddressError::Invalid));
        assert_eq!(parse_room_address("#nodomain"), Err(AddressError::Invalid));
        assert_eq!(parse_room_address("https://example.org/#/@bob:example.org"), Err(AddressError::Invalid));
    }
}
