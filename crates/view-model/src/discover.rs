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

/// Which directories Discover browses: your own server's and the suggested
/// ones, plus any typed in, each switched on or off. All are on until the user
/// switches some off; what they chose is remembered (`to_json`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Directories {
    home: String,
    /// Servers typed in by the user, beyond the defaults.
    custom: Vec<String>,
    /// Servers switched off.
    off: Vec<String>,
}

impl Directories {
    /// The defaults for an account on `home`, with what was remembered
    /// (`saved`, from `to_json`) applied.
    pub fn new(home: &str, saved: Option<&serde_json::Value>) -> Self {
        let list = |key: &str| -> Vec<String> {
            saved
                .and_then(|v| v.get(key))
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().and_then(normalise_server))
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut this = Self {
            home: home.to_owned(),
            custom: Vec::new(),
            off: list("off"),
        };
        for server in list("custom") {
            if !this.all().contains(&server) {
                this.custom.push(server);
            }
        }
        this
    }

    /// Every directory offered: your own first, then the suggested, then the typed in.
    pub fn all(&self) -> Vec<String> {
        let mut all: Vec<String> = Vec::new();
        let named = std::iter::once(self.home.as_str())
            .chain(SUGGESTED_SERVERS.iter().copied())
            .chain(self.custom.iter().map(String::as_str));
        for server in named {
            if !server.is_empty() && !all.iter().any(|s| s == server) {
                all.push(server.to_owned());
            }
        }
        all
    }

    pub fn is_on(&self, server: &str) -> bool {
        !self.off.iter().any(|s| s == server)
    }

    /// The directories to ask, in `all` order.
    pub fn selected(&self) -> Vec<String> {
        self.all().into_iter().filter(|s| self.is_on(s)).collect()
    }

    /// Switch a directory on or off.
    pub fn toggle(&mut self, server: &str) {
        if self.is_on(server) {
            self.off.push(server.to_owned());
        } else {
            self.off.retain(|s| s != server);
        }
    }

    /// A typed-in directory: offered from now on, and on.
    pub fn add(&mut self, server: &str) {
        if !self.all().iter().any(|s| s == server) {
            self.custom.push(server.to_owned());
        }
        self.off.retain(|s| s != server);
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({ "custom": self.custom, "off": self.off })
    }
}

/// How one directory's answer stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Loading,
    Loaded,
    /// Its own error; the other directories are unaffected.
    Failed(String),
}

/// One directory's share of the results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub server: String,
    pub status: Status,
    pub rooms: Vec<PublicRoom>,
    /// Pass back as `since` for the next page of this directory.
    pub next: Option<String>,
}

/// A room of the merged list and the directories that list it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub room: PublicRoom,
    /// Never empty.
    pub servers: Vec<String>,
}

impl Entry {
    /// "matrix.org", or "matrix.org, kde.org" when several list it.
    pub fn label(&self) -> String {
        self.servers.join(", ")
    }
}

/// The results of one search across the selected directories, merged. The
/// directories are asked at the same time; each answer lands with `finish`
/// whenever it arrives, and a slow or failing one never holds back the rest.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Results {
    pub listings: Vec<Listing>,
    /// The newest search; answers to older ones are dropped.
    pub generation: u64,
}

/// One directory's page, or its error.
pub type Answer = Result<(Vec<PublicRoom>, Option<String>), String>;

impl Results {
    /// A new search of `servers`: everything is loading. Returns its generation.
    pub fn start(&mut self, servers: &[String]) -> u64 {
        self.generation += 1;
        self.listings = servers
            .iter()
            .map(|server| Listing {
                server: server.clone(),
                status: Status::Loading,
                rooms: Vec::new(),
                next: None,
            })
            .collect();
        self.generation
    }

    /// The next page of every directory that has one is loading. Returns the
    /// generation and the directories to ask with their `since`.
    pub fn start_more(&mut self) -> (u64, Vec<(String, String)>) {
        self.generation += 1;
        let mut ask = Vec::new();
        for listing in &mut self.listings {
            if let (Some(next), Status::Loaded) = (listing.next.clone(), &listing.status) {
                listing.status = Status::Loading;
                ask.push((listing.server.clone(), next));
            }
        }
        (self.generation, ask)
    }

    /// A directory answered the search of `generation`. `false` when it was
    /// an old search, or the directory is not in this one.
    pub fn finish(&mut self, generation: u64, server: &str, answer: Answer) -> bool {
        if generation != self.generation {
            return false;
        }
        let Some(listing) = self.listings.iter_mut().find(|l| l.server == server) else {
            return false;
        };
        match answer {
            Ok((rooms, next)) => {
                listing.rooms.extend(rooms);
                listing.next = next;
                listing.status = Status::Loaded;
            }
            Err(err) => {
                listing.next = None;
                listing.status = Status::Failed(err);
            }
        }
        true
    }

    pub fn loading(&self) -> bool {
        self.listings.iter().any(|l| l.status == Status::Loading)
    }

    pub fn has_more(&self) -> bool {
        self.listings.iter().any(|l| l.next.is_some())
    }

    /// The directories that failed, with their errors.
    pub fn errors(&self) -> Vec<(&str, &str)> {
        self.listings
            .iter()
            .filter_map(|l| match &l.status {
                Status::Failed(e) => Some((l.server.as_str(), e.as_str())),
                _ => None,
            })
            .collect()
    }

    /// The error of one directory, if it failed.
    pub fn error_of(&self, server: &str) -> Option<&str> {
        self.errors().into_iter().find(|(s, _)| *s == server).map(|(_, e)| e)
    }

    /// Every room found so far, the biggest first (ties keep the directories'
    /// order), a room several directories list once, with all of them.
    pub fn merged(&self) -> Vec<Entry> {
        let mut entries: Vec<Entry> = Vec::new();
        for listing in &self.listings {
            for room in &listing.rooms {
                match entries.iter_mut().find(|e| e.room.room_id == room.room_id) {
                    Some(entry) => {
                        if !entry.servers.contains(&listing.server) {
                            entry.servers.push(listing.server.clone());
                        }
                        entry.room.members = entry.room.members.max(room.members);
                    }
                    None => entries.push(Entry {
                        room: room.clone(),
                        servers: vec![listing.server.clone()],
                    }),
                }
            }
        }
        entries.sort_by_key(|e| std::cmp::Reverse(e.room.members));
        entries
    }
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

    fn listed(id: &str, members: u64) -> PublicRoom {
        PublicRoom {
            room_id: format!("!{id}:x").try_into().expect("id"),
            members,
            ..room(Some(id), None)
        }
    }

    fn servers(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn every_directory_is_on_by_default_own_server_first() {
        let dirs = Directories::new("chat.example", None);
        let all = dirs.all();
        assert_eq!(all[0], "chat.example");
        assert!(all.contains(&"matrix.org".to_owned()));
        assert_eq!(dirs.selected(), all);
        // Own server is the same as a suggested one: listed once.
        assert_eq!(Directories::new("matrix.org", None).all().len(), SUGGESTED_SERVERS.len());
    }

    #[test]
    fn the_choice_is_remembered() {
        let mut dirs = Directories::new("h", None);
        dirs.toggle("kde.org");
        dirs.add("example.net");
        assert!(!dirs.is_on("kde.org") && dirs.is_on("example.net"));
        let again = Directories::new("h", Some(&dirs.to_json()));
        assert_eq!(again, dirs);
        assert!(!again.selected().contains(&"kde.org".to_owned()));
        assert!(again.selected().contains(&"example.net".to_owned()));
        let mut back = again;
        back.toggle("kde.org");
        assert!(back.is_on("kde.org"));
    }

    #[test]
    fn a_typed_in_directory_switched_off_stays_off() {
        let mut dirs = Directories::new("h", None);
        dirs.add("example.net");
        dirs.toggle("example.net");
        let again = Directories::new("h", Some(&dirs.to_json()));
        assert!(again.all().contains(&"example.net".to_owned()));
        assert!(!again.is_on("example.net"));
    }

    #[test]
    fn results_merge_biggest_first_and_say_where_from() {
        let mut r = Results::default();
        let g = r.start(&servers(&["a", "b"]));
        assert!(r.loading());
        r.finish(g, "a", Ok((vec![listed("one", 5), listed("two", 50)], None)));
        // One directory answered: its rooms show while the other loads.
        assert!(r.loading());
        assert_eq!(r.merged().len(), 2);
        r.finish(g, "b", Ok((vec![listed("three", 20), listed("one", 9)], Some("n".into()))));
        assert!(!r.loading() && r.has_more());
        let merged = r.merged();
        let names: Vec<_> = merged.iter().map(|e| name(&e.room)).collect();
        assert_eq!(names, ["two", "three", "one"]);
        assert_eq!(merged[1].label(), "b");
        // The room both list is there once, with both servers.
        assert_eq!(merged[2].label(), "a, b");
        assert_eq!(merged[2].room.members, 9);
    }

    #[test]
    fn a_failing_directory_shows_its_own_error_and_blocks_nothing() {
        let mut r = Results::default();
        let g = r.start(&servers(&["a", "b"]));
        r.finish(g, "a", Err("timed out".into()));
        assert!(r.loading());
        r.finish(g, "b", Ok((vec![listed("x", 1)], None)));
        assert!(!r.loading());
        assert_eq!(r.errors(), vec![("a", "timed out")]);
        assert_eq!(r.error_of("b"), None);
        assert_eq!(r.merged().len(), 1);
    }

    #[test]
    fn answers_to_an_old_search_are_dropped() {
        let mut r = Results::default();
        let old = r.start(&servers(&["a"]));
        let new = r.start(&servers(&["a"]));
        assert!(!r.finish(old, "a", Ok((vec![listed("x", 1)], None))));
        assert!(r.merged().is_empty() && r.loading());
        assert!(r.finish(new, "a", Ok((vec![], None))));
        assert!(!r.finish(new, "elsewhere", Ok((vec![], None))));
    }

    #[test]
    fn more_asks_only_directories_with_a_next_page() {
        let mut r = Results::default();
        let g = r.start(&servers(&["a", "b"]));
        r.finish(g, "a", Ok((vec![listed("x", 1)], Some("p2".into()))));
        r.finish(g, "b", Ok((vec![], None)));
        let (g2, ask) = r.start_more();
        assert_eq!(ask, vec![("a".to_owned(), "p2".to_owned())]);
        r.finish(g2, "a", Ok((vec![listed("y", 2)], None)));
        assert_eq!(r.merged().len(), 2);
        assert!(!r.has_more());
    }
}
