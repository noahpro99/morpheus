//! Discover (the compass under + in the server list): which directory to
//! browse and how a public room reads in the list.
//!
//! The directory is a Matrix server's own public room directory
//! (`/publicRooms`), your own server's first. Servers other than yours can be
//! typed in; a few well-known ones are offered.

use morpheus_core::PublicRoom;

/// Servers offered besides your own, whose directories are worth browsing.
pub const SUGGESTED_SERVERS: &[&str] = &[
    "matrix.org",
    "mozilla.org",
    "gnome.org",
    "kde.org",
    "fosdem.org",
];

/// The server name typed into the box, cleaned: a pasted address
/// (`https://matrix.org/`) or `@user:server` / `#room:server` gives its
/// server. `None` when it is not a server name.
pub fn normalise_server(input: &str) -> Option<String> {
    let mut s = input.trim();
    if s.starts_with(['@', '#', '!', '+']) {
        // An id or alias: its server is after the first colon.
        s = s.split_once(':')?.1;
    } else {
        if let Some((_, rest)) = s.split_once("://") {
            s = rest;
        }
        s = s.split(['/', '?', '#']).next().unwrap_or(s);
    }
    let s = s.trim_matches('.').to_lowercase();
    if s.is_empty() {
        return None;
    }
    let (host, port) = match s.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() => {
            (host, Some(port))
        }
        Some(_) => return None,
        None => (s.as_str(), None),
    };
    let ok = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && !host.starts_with('-')
        && !host.contains("..");
    if !ok {
        return None;
    }
    Some(match port {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    })
}

/// The room's name; its alias or id when it has none.
pub fn name(room: &PublicRoom) -> String {
    room.name
        .clone()
        .filter(|n| !n.trim().is_empty())
        .or_else(|| room.alias.clone())
        .unwrap_or_else(|| room.room_id.to_string())
}

/// The topic's first line, unless it is blank.
pub fn topic(room: &PublicRoom) -> Option<String> {
    room.topic
        .as_deref()
        .and_then(|t| t.lines().map(str::trim).find(|l| !l.is_empty()))
        .map(str::to_owned)
}

/// "1 member", "12 members".
pub fn members_line(room: &PublicRoom) -> String {
    crate::links::members_line(room.members)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(name: Option<&str>, topic: Option<&str>) -> PublicRoom {
        PublicRoom {
            room_id: "!r:x".try_into().expect("id"),
            name: name.map(str::to_owned),
            topic: topic.map(str::to_owned),
            alias: Some("#alias:x".to_owned()),
            avatar: None,
            members: 2,
            is_space: false,
        }
    }

    #[test]
    fn server_names_are_cleaned() {
        assert_eq!(normalise_server(" Matrix.org "), Some("matrix.org".into()));
        assert_eq!(
            normalise_server("https://matrix.org/"),
            Some("matrix.org".into())
        );
        assert_eq!(
            normalise_server("https://chat.example.com:8448/_matrix"),
            Some("chat.example.com:8448".into())
        );
        assert_eq!(normalise_server("#room:kde.org"), Some("kde.org".into()));
        assert_eq!(normalise_server("@me:mozilla.org"), Some("mozilla.org".into()));
        assert_eq!(normalise_server("localhost:8008"), Some("localhost:8008".into()));
    }

    #[test]
    fn what_is_not_a_server_is_refused() {
        assert_eq!(normalise_server(""), None);
        assert_eq!(normalise_server("   "), None);
        assert_eq!(normalise_server("not a server"), None);
        assert_eq!(normalise_server("a..b"), None);
    }

    #[test]
    fn rooms_read_as_name_topic_and_size() {
        assert_eq!(name(&room(Some("Rust"), None)), "Rust");
        assert_eq!(name(&room(Some(" "), None)), "#alias:x");
        assert_eq!(topic(&room(None, Some("\n\n first line \nsecond"))), Some("first line".into()));
        assert_eq!(topic(&room(None, Some("  "))), None);
        assert_eq!(members_line(&room(None, None)), "2 members");
    }
}
