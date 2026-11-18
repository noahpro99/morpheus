//! The channel list: servers (and the direct messages and rooms outside
//! them), each with its sections of rooms. `morpheus-view-model::scopes`
//! decides the content; this maps it for the app.

use std::sync::Arc;

use morpheus_core::RoomSummary;
use morpheus_view_model::{
    diffs,
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
    pub is_dm: bool,
    pub is_voice: bool,
    pub is_encrypted: bool,
    /// Unread messages (0 for a muted room's too: the app greys it).
    pub unread: u64,
    /// Unread mentions.
    pub highlights: u64,
    pub muted: bool,
}

/// What a scope is: the app may draw the first three with icons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ScopeKind {
    Dms,
    Rooms,
    Favorites,
    Server,
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
    pub has_unread: bool,
    pub highlights: u64,
    pub sections: Vec<SectionItem>,
}

/// Everything the channel list shows, as of now.
#[derive(uniffi::Record)]
pub struct Sidebar {
    pub scopes: Vec<ScopeItem>,
}

/// Gets a whole new [`Sidebar`] whenever something in it changed; the first
/// call is the list as the core has it at once.
#[uniffi::export(with_foreign)]
pub trait SidebarListener: Send + Sync {
    fn on_sidebar(&self, sidebar: Sidebar);
}

fn room_item(room: &RoomSummary, session: &morpheus_core::Session) -> RoomItem {
    RoomItem {
        id: room.id.to_string(),
        name: room.name.clone(),
        topic: room.topic.clone().filter(|topic| !topic.is_empty()),
        avatar: room.avatar.clone().map(|m| MediaHandle::new(m, session)),
        is_dm: room.is_dm,
        is_voice: room.is_voice,
        is_encrypted: room.is_encrypted,
        unread: room.unread_messages,
        highlights: room.highlight_count,
        muted: room.muted,
    }
}

fn scope_item(view: ScopeView, session: &morpheus_core::Session) -> ScopeItem {
    let items = |rooms: &[RoomSummary]| rooms.iter().map(|r| room_item(r, session)).collect();
    let mut sections = Vec::new();
    if matches!(view.scope, Scope::Dms) {
        sections.push(SectionItem {
            title: None,
            rooms: items(&view.sections.dms),
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
        },
        title: view.title,
        avatar: view.avatar.map(|m| MediaHandle::new(m, session)),
        has_unread: view.has_unread,
        highlights: view.highlights,
        sections,
    }
}

/// The sidebar for a room list (`scope_views` and the mapping).
pub(crate) fn sidebar_of(rooms: &[RoomSummary], session: &morpheus_core::Session) -> Sidebar {
    Sidebar {
        scopes: scope_views(rooms)
            .into_iter()
            .map(|view| scope_item(view, session))
            .collect(),
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
        let task = runtime().spawn(async move {
            let mut rooms: Vec<RoomSummary> = Vec::new();
            while let Some(batch) = updates.next().await {
                for diff in batch {
                    diffs::apply(&mut rooms, diff);
                }
                listener.on_sidebar(sidebar_of(&rooms, &session));
            }
        });
        Ok(TaskHandle::new(task))
    }
}

