//! Discover (the desktop's `discover.rs`): the public room directories of several servers at once, merged: your own server's, the
//! suggested ones and any typed in, each switched on or off (kept in the data folder), spaces included, a page at a time.
//! The rules (which directories, merging, what a failing one says) are `view_model::discover`.

use std::{path::PathBuf, sync::{Arc, Mutex}};

use morpheus_view_model::discover::{self as vm, Answer, Directories, Results};

use crate::{FfiError, MediaHandle, Morpheus};

/// One room of the merged list.
#[derive(Clone, uniffi::Record)]
pub struct DiscoverEntry {
    pub id: String,
    pub name: String,
    pub topic: Option<String>,
    pub alias: Option<String>,
    pub members: String,
    pub avatar: Option<Arc<MediaHandle>>,
    pub is_space: bool,
    /// The directories that list it, "matrix.org, mozilla.org".
    pub servers: String,
    /// The first of them (what to join through).
    pub via: String,
}

/// A directory the search can ask.
#[derive(Clone, uniffi::Record)]
pub struct DirectoryChoice {
    pub server: String,
    pub on: bool,
}

/// The search as it stands.
#[derive(Clone, uniffi::Record)]
pub struct DiscoverView {
    pub entries: Vec<DiscoverEntry>,
    /// "server: why" for each directory that did not answer.
    pub errors: Vec<String>,
    pub loading: bool,
    pub has_more: bool,
}

#[derive(uniffi::Object)]
pub struct DiscoverSession {
    session: morpheus_core::Session,
    home: String,
    file: PathBuf,
    dirs: Mutex<Directories>,
    results: Mutex<Results>,
    /// The term of the search under way (what "more" continues).
    term: Mutex<String>,
    /// The pages `begin_more` asked for, by directory.
    pending: Mutex<std::collections::HashMap<String, String>>,
}

impl DiscoverSession {
    fn view(&self) -> DiscoverView {
        let results = self.results.lock().unwrap_or_else(|e| e.into_inner());
        DiscoverView {
            entries: results
                .merged()
                .into_iter()
                .map(|entry| DiscoverEntry {
                    id: entry.room.room_id.to_string(),
                    name: vm::name(&entry.room),
                    topic: vm::topic(&entry.room),
                    alias: entry.room.alias.clone(),
                    members: vm::members_line(&entry.room),
                    avatar: entry.room.avatar.clone().map(|m| MediaHandle::new(m, &self.session)),
                    is_space: entry.room.is_space,
                    servers: entry.label(),
                    via: entry.servers.first().cloned().unwrap_or_default(),
                })
                .collect(),
            errors: results.errors().into_iter().map(|(server, why)| format!("{server}: {why}")).collect(),
            loading: results.loading(),
            has_more: results.has_more(),
        }
    }

    fn keep(&self) {
        let json = self.dirs.lock().unwrap_or_else(|e| e.into_inner()).to_json();
        let _ = std::fs::write(&self.file, json.to_string());
    }

}

#[uniffi::export]
impl Morpheus {
    /// A Discover search, with the directories as they were kept.
    pub fn discover_session(&self) -> Result<Arc<DiscoverSession>, FfiError> {
        let session = self.session()?;
        let home = session.user_id().map(|u| u.server_name().to_string()).unwrap_or_default();
        let file = self.dir.join("discover_directories.json");
        let saved = std::fs::read_to_string(&file).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
        Ok(Arc::new(DiscoverSession {
            session,
            dirs: Mutex::new(Directories::new(&home, saved.as_ref())),
            home,
            file,
            results: Mutex::new(Results::default()),
            term: Mutex::new(String::new()),
            pending: Mutex::new(Default::default()),
        }))
    }
}

#[uniffi::export]
impl DiscoverSession {
    /// Every directory offered and whether it is asked.
    pub fn directories(&self) -> Vec<DirectoryChoice> {
        let dirs = self.dirs.lock().unwrap_or_else(|e| e.into_inner());
        dirs.all().into_iter().map(|server| DirectoryChoice { on: dirs.is_on(&server), server }).collect()
    }

    pub fn toggle_directory(&self, server: String) {
        self.dirs.lock().unwrap_or_else(|e| e.into_inner()).toggle(&server);
        self.keep();
    }

    /// A server typed in: offered from now on. The reason when it is not a server name.
    pub fn add_directory(&self, typed: String) -> Result<(), FfiError> {
        let server = vm::normalise_server(&typed).ok_or_else(|| FfiError::Failed {
            reason: morpheus_view_model::i18n::tf("\"%{name}\" is not a server name", &[("name", typed.trim())]),
        })?;
        self.dirs.lock().unwrap_or_else(|e| e.into_inner()).add(&server);
        self.keep();
        Ok(())
    }

    /// Start a search of the chosen directories for `term` (everything when blank): the directories to ask, each with `ask_directory`
    /// (together, so a slow one never holds back the others).
    pub fn begin_search(&self, term: String) -> SearchStart {
        let servers = self.dirs.lock().unwrap_or_else(|e| e.into_inner()).selected();
        let generation = self.results.lock().unwrap_or_else(|e| e.into_inner()).start(&servers);
        *self.term.lock().unwrap_or_else(|e| e.into_inner()) = term;
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).clear();
        SearchStart { generation, servers }
    }

    /// Start the next page of every directory that has one.
    pub fn begin_more(&self) -> SearchStart {
        let (generation, asks) = self.results.lock().unwrap_or_else(|e| e.into_inner()).start_more();
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        pending.clear();
        let servers = asks.iter().map(|(server, _)| server.clone()).collect();
        pending.extend(asks);
        SearchStart { generation, servers }
    }

    /// Ask one directory of the search `generation` and give the merged list as it stands (an answer of an older search is dropped).
    pub async fn ask_directory(&self, generation: u64, server: String) -> DiscoverView {
        let term = self.term.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let since = self.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&server);
        // Your own server is asked as yourself (no federation hop).
        let target = (server != self.home).then(|| server.clone());
        let answer: Answer = self.session.public_rooms(target, term, since).await.map(|page| (page.rooms, page.next)).map_err(|e| e.to_string());
        self.results.lock().unwrap_or_else(|e| e.into_inner()).finish(generation, &server, answer);
        self.view()
    }

    /// The list as it stands.
    pub fn current(&self) -> DiscoverView {
        self.view()
    }
}

/// A search begun: which directories to ask, under which generation.
#[derive(Clone, uniffi::Record)]
pub struct SearchStart {
    pub generation: u64,
    pub servers: Vec<String>,
}
