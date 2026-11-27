//! The channel list: servers (and the direct messages and rooms outside
//! them), each with its sections of rooms. `morpheus-view-model::scopes`
//! decides the content; this maps it for the app.

use std::sync::{Arc, Mutex};

use morpheus_core::{OwnedRoomId, RoomSummary};
use morpheus_view_model::{
    diffs,
    marks::Marks,
    scopes::{Scope, ScopeView, scope_views},
};

use crate::{FfiError, MediaHandle, Morpheus, TaskHandle, runtime};

/// A room in the list.
#[derive(uniffi::Record)]
pub struct RoomItem {
    pub id: String,
    pub name: String,
    pub topic: Option<String>,
    pub avatar: Option<Arc<MediaHandle>>,
    /// The picture's key (`MediaHandle::key`), also while the picture itself is not to be had (the list of the last run): the app's disk cache has it.
    pub avatar_key: Option<String>,
    pub is_dm: bool,
    pub is_voice: bool,
    pub is_encrypted: bool,
    /// Unread messages (0 for a muted room's too: the app greys it).
    pub unread: u64,
    /// Unread mentions.
    pub highlights: u64,
    pub muted: bool,
    /// A DM pinned to the top of the list (on any of my devices).
    pub pinned: bool,
    /// Who is in the room's call (a voice channel's list), with what they have on.
    pub call: Vec<crate::CallListing>,
    /// The other person of a direct message (their picture opens their profile, #488).
    pub dm_user: Option<String>,
}

/// What a scope is: the app may draw the first three with icons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ScopeKind {
    Dms,
    Rooms,
    Favorites,
    Server,
    /// The rooms bridged to one network (Discord, WhatsApp, ...).
    Network,
}

/// A group of rooms under an optional heading (a server's category).
#[derive(uniffi::Record)]
pub struct SectionItem {
    pub title: Option<String>,
    pub rooms: Vec<RoomItem>,
}

/// One entry of the rail: direct messages, rooms, favorites, or a server.
#[derive(uniffi::Record)]
pub struct ScopeItem {
    /// Stable, e.g. `dms`, `rooms`, `space:!id:server`.
    pub key: String,
    pub kind: ScopeKind,
    pub title: String,
    pub avatar: Option<Arc<MediaHandle>>,
    pub avatar_key: Option<String>,
    pub has_unread: bool,
    pub highlights: u64,
    pub sections: Vec<SectionItem>,
}

/// Everything the channel list shows, as of now.
#[derive(uniffi::Record)]
pub struct Sidebar {
    pub scopes: Vec<ScopeItem>,
    /// Rooms I am invited to, waiting for an answer.
    pub invites: Vec<crate::InviteItem>,
}

/// Gets a whole new [`Sidebar`] whenever something in it changed; the first
/// call is the list as the core has it at once.
#[uniffi::export(with_foreign)]
pub trait SidebarListener: Send + Sync {
    fn on_sidebar(&self, sidebar: Sidebar);
}

fn room_item(room: &RoomSummary, session: &morpheus_core::Session, marks: &Marks) -> RoomItem {
    RoomItem {
        id: room.id.to_string(),
        name: room.name.clone(),
        topic: room.topic.clone().filter(|topic| !topic.is_empty()),
        avatar: room.avatar.clone().map(|m| MediaHandle::new(m, session)),
        avatar_key: room.avatar.as_ref().map(|m| m.key().to_owned()),
        is_dm: room.is_dm,
        is_voice: room.is_voice,
        is_encrypted: room.is_encrypted,
        unread: room.unread_messages,
        highlights: room.highlight_count,
        muted: marks.is_muted(room),
        pinned: marks.is_pinned(room),
        call: crate::calls::listing(room),
        dm_user: room.dm_user.as_ref().map(ToString::to_string),
    }
}

fn scope_item(view: ScopeView, session: &morpheus_core::Session, marks: &Marks, hide_muted: bool) -> ScopeItem {
    let avatar_key = view.avatar.as_ref().map(|m| m.key().to_owned());
    // Muted channels are left out when the account says so (DMs are not channels).
    let items = |rooms: &[RoomSummary]| rooms.iter().filter(|r| !(hide_muted && !r.is_dm && marks.is_muted(r))).map(|r| room_item(r, session, marks)).collect();
    let mut sections = Vec::new();
    if matches!(view.scope, Scope::Dms) {
        sections.push(SectionItem {
            title: None,
            // Closed DMs stay out until a new message; pinned ones come first.
            rooms: items(&marks.visible_dms(view.sections.dms.clone(), None, false)),
        });
    } else {
        sections.push(SectionItem {
            title: None,
            rooms: items(&view.sections.channels),
        });
        for (_, name, rooms) in &view.sections.categories {
            sections.push(SectionItem {
                title: Some(name.clone()),
                rooms: items(rooms),
            });
        }
    }
    ScopeItem {
        key: view.scope.key(),
        kind: match view.scope {
            Scope::Dms => ScopeKind::Dms,
            Scope::Rooms => ScopeKind::Rooms,
            Scope::Favorites => ScopeKind::Favorites,
            Scope::Space(_) => ScopeKind::Server,
            Scope::Network(_) => ScopeKind::Network,
        },
        title: view.title,
        avatar: view.avatar.map(|m| MediaHandle::new(m, session)),
        avatar_key,
        has_unread: view.has_unread,
        highlights: view.highlights,
        sections,
    }
}

/// The sidebar for a room list (`scope_views` and the mapping).
pub(crate) fn sidebar_of(rooms: &[RoomSummary], session: &morpheus_core::Session, layout: &Layout) -> Sidebar {
    let mut scopes: Vec<ScopeItem> = scope_views(rooms)
        .into_iter()
        .map(|view| scope_item(view, session, &layout.marks, layout.hide_muted))
        .collect();
    // Servers in the order the account keeps (the same layout on every device); the rest after, as listed.
    let place = |key: &str| key.strip_prefix("space:").and_then(|id| layout.rail.iter().position(|r| r == id));
    let first_server = scopes.iter().position(|s| s.kind == ScopeKind::Server).unwrap_or(scopes.len());
    scopes[first_server..].sort_by_key(|s| place(&s.key).unwrap_or(usize::MAX));
    Sidebar {
        scopes,
        invites: crate::people::invite_items(rooms, session),
    }
}

#[uniffi::export]
impl Morpheus {
    /// The channel list as of the last run, for drawing at once while the session is restored (no pictures,
    /// no calls, no invites); `None` on a first run or after sign-out.
    pub fn cached_sidebar(&self) -> Option<Sidebar> {
        crate::sidebar_cache::load(&self.dir)
    }
}

#[uniffi::export]
impl Morpheus {
    /// Start telling `listener` about the channel list.
    pub async fn watch_sidebar(
        &self,
        listener: Arc<dyn SidebarListener>,
    ) -> Result<Arc<TaskHandle>, FfiError> {
        let session = self.session()?;
        let mut updates = session.room_list().await?;
        let dir = self.dir.clone();
        let (overrides, changed) = (self.muted_now.clone(), self.layout_changed.clone());
        let rooms_now = self.rooms_now.clone();
        let nicks = self.nicks.clone();
        let mut nick_changes = nicks.watch();
        let task = runtime().spawn(async move {
            let mut rooms: Vec<RoomSummary> = Vec::new();
            let load = || async { Layout::load(&session).await.with_mutes(&overrides) };
            let mut layout = load().await;
            if let Ok(map) = session.nicknames().await {
                nicks.replace(map);
            }
            let mut ticks = tokio::time::interval(std::time::Duration::from_secs(4));
            let mut told = false;
            loop {
                tokio::select! {
                    batch = updates.next() => {
                        let Some(batch) = batch else { break };
                        for diff in batch {
                            diffs::apply(&mut rooms, diff);
                        }
                        if let Ok(mut now) = rooms_now.lock() {
                            now.clone_from(&rooms);
                        }
                        layout = load().await;
                    }
                    _ = changed.notified() => {
                        layout = load().await;
                    }
                    // A nickname was set or changed: the DMs go by it.
                    _ = nick_changes.changed() => {}
                    _ = ticks.tick() => {
                        // A timed mute that ran out ends (on every device), as on the desktop.
                        for id in layout.marks.ended_mutes(now_ms()) {
                            if let Ok(room) = OwnedRoomId::try_from(id.as_str()) {
                                let _ = session.set_notify_mode(room, morpheus_core::NotifyMode::AllMessages).await;
                            }
                            overrides.lock().map(|mut m| m.insert(id.clone(), false)).ok();
                            let _ = session.set_mark(morpheus_core::marks::MUTED_UNTIL, id, None).await;
                        }
                        // So do the nicknames.
                        if let Ok(map) = session.nicknames().await {
                            nicks.replace(map);
                        }
                        // The layout changes on another device without the room list changing.
                        let now = load().await;
                        if !told || now.signature() == layout.signature() {
                            continue;
                        }
                        layout = now;
                    }
                }
                told = true;
                // A DM goes by your nickname for the person.
                let nicknames = nicks.snapshot();
                let shown: Vec<RoomSummary> = rooms.iter().cloned().map(|mut room| { room.name = morpheus_view_model::nicknames::room_title(&nicknames, &room); room }).collect();
                let sidebar = sidebar_of(&shown, &session, &layout);
                crate::sidebar_cache::save(&dir, &sidebar);
                listener.on_sidebar(sidebar);
            }
        });
        Ok(TaskHandle::new(task))
    }
}


/// What the account keeps about the list's layout (sealed account data, shared
/// with the desktop app): DMs pinned, closed or muted, and the servers' order.
#[derive(Default, Clone)]
pub(crate) struct Layout {
    pub marks: Marks,
    pub rail: Vec<String>,
    /// "Hide muted channels": muted rooms stay out of the servers' lists.
    pub hide_muted: bool,
}

impl Layout {
    /// With the mutes changed here counted.
    fn with_mutes(mut self, overrides: &Mutex<std::collections::HashMap<String, bool>>) -> Self {
        self.marks.muted_now = overrides.lock().map(|m| m.clone()).unwrap_or_default();
        self
    }

    pub(crate) async fn load(session: &morpheus_core::Session) -> Self {
        let mut marks = Marks::default();
        for kind in [
            morpheus_core::marks::PINNED,
            morpheus_core::marks::CLOSED,
            morpheus_core::marks::IGNORED,
            morpheus_core::marks::MUTED_UNTIL,
        ] {
            if let Ok(map) = session.marks(kind).await {
                *marks.map_mut(kind) = map;
            }
        }
        let prefs = session.ui_prefs().await.ok();
        let hide_muted = prefs.as_ref().and_then(|p| p.get("hide_muted")).and_then(|v| v.as_bool()).unwrap_or(false);
        let rail = prefs
            .and_then(|prefs| prefs.get("rail").and_then(|r| r.as_array().cloned()))
            .map(|items| {
                items
                    .iter()
                    .flat_map(|item| {
                        let mut ids: Vec<String> = item.get("space").and_then(|v| v.as_str()).map(str::to_owned).into_iter().collect();
                        if let Some(spaces) = item.get("spaces").and_then(|v| v.as_array()) {
                            ids.extend(spaces.iter().filter_map(|v| v.as_str().map(str::to_owned)));
                        }
                        ids
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self { marks, rail, hide_muted }
    }

    /// Equal layouts have equal signatures (the maps' order does not count).
    fn signature(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        parts.extend(self.marks.muted_now.iter().map(|(k, v)| format!("n{k}={v}")));
        for (name, map) in [
            ("p", &self.marks.pinned),
            ("c", &self.marks.closed),
            ("i", &self.marks.ignored),
            ("m", &self.marks.muted_until),
        ] {
            let mut entries: Vec<_> = map.iter().map(|(k, v)| format!("{name}{k}={v}")).collect();
            entries.sort();
            parts.extend(entries);
        }
        parts.extend(self.rail.iter().map(|r| format!("r{r}")));
        parts.push(format!("h{}", self.hide_muted));
        parts.join("|")
    }
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or_default())
        .unwrap_or_default()
}

/// One way to mute a room: what the menu says and for how long.
#[derive(uniffi::Record)]
pub struct MuteChoice {
    pub label: String,
    /// Milliseconds; 0 is until it is turned back on.
    pub ms: u64,
}

/// The lengths a room can be muted for, shortest first (translated).
#[uniffi::export]
pub fn mute_choices() -> Vec<MuteChoice> {
    morpheus_view_model::marks::MUTE_CHOICES
        .iter()
        .map(|(label, ms)| MuteChoice { label: morpheus_view_model::i18n::t(label), ms: u64::try_from(*ms).unwrap_or(0) })
        .collect()
}

#[uniffi::export]
impl Morpheus {
    /// Mute a room (nothing notifies) for `ms` milliseconds, or until turned back on (0), on every device.
    pub async fn mute_room(&self, room_id: String, ms: u64) -> Result<(), FfiError> {
        let session = self.session()?;
        let id = OwnedRoomId::try_from(room_id.as_str()).map_err(|_| FfiError::Failed { reason: format!("{room_id} is not a room id") })?;
        session.set_notify_mode(id, morpheus_core::NotifyMode::Mute).await?;
        if let Ok(mut now) = self.muted_now.lock() {
            now.insert(room_id.clone(), true);
        }
        let until = if ms == 0 { 0 } else { now_ms() + i64::try_from(ms).unwrap_or(i64::MAX / 2) };
        session.set_mark(morpheus_core::marks::MUTED_UNTIL, room_id, Some(until)).await?;
        self.layout_changed.notify_one();
        Ok(())
    }

    /// Turn a room's notifications back on.
    pub async fn unmute_room(&self, room_id: String) -> Result<(), FfiError> {
        let session = self.session()?;
        let id = OwnedRoomId::try_from(room_id.as_str()).map_err(|_| FfiError::Failed { reason: format!("{room_id} is not a room id") })?;
        session.set_notify_mode(id, morpheus_core::NotifyMode::AllMessages).await?;
        if let Ok(mut now) = self.muted_now.lock() {
            now.insert(room_id.clone(), false);
        }
        session.set_mark(morpheus_core::marks::MUTED_UNTIL, room_id, None).await?;
        self.layout_changed.notify_one();
        Ok(())
    }
}
