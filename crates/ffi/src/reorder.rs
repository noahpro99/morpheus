//! Moving a server's channels and categories about (#424): who may change a server's children sees its channel list as sections to drag
//! rows between. Where a drop lands is `view_model::reorder`, the desktop's.

use morpheus_core::OwnedRoomId;
use morpheus_view_model::{reorder, scopes::{scope_views, Scope}};

use crate::{FfiError, Morpheus};

#[derive(uniffi::Record)]
pub struct ReorderRoom {
    pub id: String,
    pub name: String,
    pub is_voice: bool,
}

/// The server's own channels (the first section, its `id` the server's) or one category's.
#[derive(uniffi::Record)]
pub struct ReorderSection {
    pub id: String,
    pub title: Option<String>,
    pub rooms: Vec<ReorderRoom>,
}

fn failed(reason: impl Into<String>) -> FfiError {
    FfiError::Failed { reason: reason.into() }
}

fn room_id(id: &str) -> Result<OwnedRoomId, FfiError> {
    OwnedRoomId::try_from(id).map_err(|_| failed(format!("{id} is not a room id")))
}

impl Morpheus {
    fn space_sections(&self, space: &OwnedRoomId) -> Option<morpheus_view_model::scopes::Sections> {
        let rooms = self.rooms_now.lock().ok()?.clone();
        scope_views(&rooms).into_iter().find(|v| v.scope == Scope::Space(space.clone())).map(|v| v.sections)
    }
}

#[uniffi::export]
impl Morpheus {
    /// The server's channels in sections, when the person may change them; `None` otherwise.
    pub fn reorder_info(&self, space_id: String) -> Option<Vec<ReorderSection>> {
        let space = room_id(&space_id).ok()?;
        let can = self.rooms_now.lock().ok()?.iter().any(|r| r.id == space && r.can_edit_children);
        if !can {
            return None;
        }
        let found = self.space_sections(&space)?;
        let item = |r: &morpheus_core::RoomSummary| ReorderRoom { id: r.id.to_string(), name: r.name.clone(), is_voice: r.is_voice };
        let mut sections = vec![ReorderSection { id: space.to_string(), title: None, rooms: found.channels.iter().map(item).collect() }];
        for (id, name, rooms) in &found.categories {
            sections.push(ReorderSection { id: id.to_string(), title: Some(name.clone()), rooms: rooms.iter().map(item).collect() });
        }
        Some(sections)
    }

    /// Drop channel `from` into `dest` (the server or one of its categories): beside `target` (before it, or after it when `after`), or last
    /// when there is no target. For everyone.
    pub async fn move_channel(&self, space_id: String, from: String, dest: String, target: Option<String>, after: bool) -> Result<(), FfiError> {
        let session = self.session()?;
        let (root, from, dest) = (room_id(&space_id)?, room_id(&from)?, room_id(&dest)?);
        let found = self.space_sections(&root).ok_or_else(|| failed("that server is not in the list"))?;
        let listed: Vec<OwnedRoomId> = if dest == root {
            found.channels.iter().map(|r| r.id.clone()).collect()
        } else {
            found.categories.iter().find(|(id, _, _)| *id == dest).map(|(_, _, m)| m.iter().map(|r| r.id.clone()).collect()).unwrap_or_default()
        };
        let (order, parents) = {
            let rooms = self.rooms_now.lock().map_err(|_| failed("busy"))?;
            (
                rooms.iter().find(|r| r.id == dest).map(|r| r.space_children.clone()).unwrap_or_default(),
                rooms.iter().find(|r| r.id == from).map(|r| r.parent_spaces.clone()).unwrap_or_default(),
            )
        };
        let new_order = match target {
            Some(target) => reorder::dropped_beside(&order, &listed, &from, &room_id(&target)?, after),
            None => Some(reorder::dropped_last(&order, &listed, &from)),
        };
        let Some(new_order) = new_order else { return Ok(()) };
        let mut containers = vec![root];
        containers.extend(found.categories.iter().map(|(id, _, _)| id.clone()));
        let leaving = reorder::leaving(&parents, &containers, &dest);
        Ok(session.move_space_child(dest, new_order, Some(from), leaving).await?)
    }

    /// Put category `from` beside category `target` in the server's order.
    pub async fn move_category(&self, space_id: String, from: String, target: String, after: bool) -> Result<(), FfiError> {
        let session = self.session()?;
        let (root, from, target) = (room_id(&space_id)?, room_id(&from)?, room_id(&target)?);
        let found = self.space_sections(&root).ok_or_else(|| failed("that server is not in the list"))?;
        let mut listed: Vec<OwnedRoomId> = found.channels.iter().map(|r| r.id.clone()).collect();
        listed.extend(found.categories.iter().map(|(id, _, _)| id.clone()));
        let order = self.rooms_now.lock().map_err(|_| failed("busy"))?.iter().find(|r| r.id == root).map(|r| r.space_children.clone()).unwrap_or_default();
        match reorder::dropped_beside(&order, &listed, &from, &target, after) {
            Some(order) => Ok(session.set_space_order(root, order).await?),
            None => Ok(()),
        }
    }
}
