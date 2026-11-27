//! Reordering a server's channels and categories (the desktop's drag in `room_list.rs`): who may change the server's children sees the
//! list as places, each moved up or down, a channel also into another category. The rules are `view_model::reorder`.

use morpheus_core::OwnedRoomId;
use morpheus_view_model::{
    reorder,
    scopes::{Scope, scope_views},
};

use crate::{FfiError, Morpheus};

fn failed(reason: impl Into<String>) -> FfiError {
    FfiError::Failed { reason: reason.into() }
}

fn room(id: &str) -> Result<OwnedRoomId, FfiError> {
    OwnedRoomId::try_from(id).map_err(|_| failed(format!("{id} is not a room id")))
}

/// One line of the order: a channel, or a category (with its channels after it).
#[derive(Clone, uniffi::Record)]
pub struct ChannelPlace {
    pub id: String,
    pub name: String,
    pub is_category: bool,
    /// The server or category it is listed in.
    pub container: String,
    /// Whether a step up or down would move it.
    pub can_up: bool,
    pub can_down: bool,
}

/// A category a channel can go to.
#[derive(Clone, uniffi::Record)]
pub struct CategoryChoice {
    pub id: String,
    pub name: String,
}

#[derive(Clone, uniffi::Record)]
pub struct ChannelLayout {
    /// The signed-in person may reorder this server's children.
    pub can_edit: bool,
    pub places: Vec<ChannelPlace>,
    /// Where a channel can be put: the server itself (`id` is the server's) and each category.
    pub containers: Vec<CategoryChoice>,
}

struct Lists {
    root: OwnedRoomId,
    /// Channels listed directly in the server, then each category with its channels, as ids.
    channels: Vec<OwnedRoomId>,
    categories: Vec<(OwnedRoomId, String, Vec<OwnedRoomId>)>,
    children: std::collections::HashMap<OwnedRoomId, Vec<OwnedRoomId>>,
    names: std::collections::HashMap<OwnedRoomId, String>,
    parents: std::collections::HashMap<OwnedRoomId, Vec<OwnedRoomId>>,
    can_edit: bool,
}

impl Morpheus {
    fn lists(&self, space: &str) -> Result<Lists, FfiError> {
        let root = room(space)?;
        let rooms = self.rooms_now.lock().map_err(|_| failed("busy"))?.clone();
        let view = scope_views(&rooms).into_iter().find(|v| v.scope == Scope::Space(root.clone())).ok_or_else(|| failed("that server is not in the list"))?;
        let ids = |list: &[morpheus_core::RoomSummary]| list.iter().map(|r| r.id.clone()).collect::<Vec<_>>();
        Ok(Lists {
            channels: ids(&view.sections.channels),
            categories: view.sections.categories.iter().map(|(id, name, members)| (id.clone(), name.clone(), ids(members))).collect(),
            children: rooms.iter().map(|r| (r.id.clone(), r.space_children.clone())).collect(),
            names: rooms.iter().map(|r| (r.id.clone(), r.name.clone())).collect(),
            parents: rooms.iter().map(|r| (r.id.clone(), r.parent_spaces.clone())).collect(),
            can_edit: rooms.iter().any(|r| r.id == root && r.can_edit_children),
            root,
        })
    }
}

#[uniffi::export]
impl Morpheus {
    /// The server's channels and categories as one ordered list.
    pub fn channel_layout(&self, space_id: String) -> Result<ChannelLayout, FfiError> {
        let lists = self.lists(&space_id)?;
        let name = |id: &OwnedRoomId| lists.names.get(id).cloned().unwrap_or_else(|| id.to_string());
        let mut places = Vec::new();
        let last = lists.channels.len().saturating_sub(1);
        for (at, id) in lists.channels.iter().enumerate() {
            places.push(ChannelPlace { id: id.to_string(), name: name(id), is_category: false, container: lists.root.to_string(), can_up: at > 0, can_down: at < last });
        }
        let top = lists.categories.len().saturating_sub(1);
        for (at, (cat, cat_name, members)) in lists.categories.iter().enumerate() {
            places.push(ChannelPlace { id: cat.to_string(), name: cat_name.clone(), is_category: true, container: lists.root.to_string(), can_up: at > 0, can_down: at < top });
            let last = members.len().saturating_sub(1);
            for (i, id) in members.iter().enumerate() {
                places.push(ChannelPlace { id: id.to_string(), name: name(id), is_category: false, container: cat.to_string(), can_up: i > 0, can_down: i < last });
            }
        }
        let mut containers = vec![CategoryChoice { id: lists.root.to_string(), name: name(&lists.root) }];
        containers.extend(lists.categories.iter().map(|(id, n, _)| CategoryChoice { id: id.to_string(), name: n.clone() }));
        Ok(ChannelLayout { can_edit: lists.can_edit, places, containers })
    }

    /// Move a channel (or a category) one place up (`by` -1) or down (1) among its neighbours, for everyone.
    pub async fn move_channel(&self, space_id: String, id: String, by: i32) -> Result<(), FfiError> {
        let lists = self.lists(&space_id)?;
        if !lists.can_edit {
            return Err(failed("You may not change this server's order."));
        }
        let from = room(&id)?;
        let is_category = lists.categories.iter().any(|(c, _, _)| *c == from);
        // The list it is in, as ids in the order drawn, and the container whose children it orders.
        let (container, listed): (OwnedRoomId, Vec<OwnedRoomId>) = if is_category {
            let mut all = lists.channels.clone();
            all.extend(lists.categories.iter().map(|(c, _, _)| c.clone()));
            (lists.root.clone(), all)
        } else if lists.channels.contains(&from) {
            (lists.root.clone(), lists.channels.clone())
        } else {
            let (cat, _, members) = lists.categories.iter().find(|(_, _, m)| m.contains(&from)).ok_or_else(|| failed("that is not in this server"))?;
            (cat.clone(), members.clone())
        };
        // Neighbours as drawn: channels then categories for the server's own order.
        let drawn: Vec<OwnedRoomId> = if is_category { lists.categories.iter().map(|(c, _, _)| c.clone()).collect() } else { listed.clone() };
        let at = drawn.iter().position(|d| *d == from).ok_or_else(|| failed("that is not in this server"))?;
        let next = at as i64 + i64::from(by.signum());
        let Some(target) = usize::try_from(next).ok().and_then(|n| drawn.get(n)) else { return Ok(()) };
        let order = lists.children.get(&container).cloned().unwrap_or_default();
        let Some(new_order) = reorder::dropped_beside(&order, &listed, &from, target, by > 0) else { return Ok(()) };
        self.session()?.set_space_order(container, new_order).await?;
        Ok(())
    }

    /// Put a channel last in another category (or in the server itself), taking it out of the one it is in.
    pub async fn move_channel_into(&self, space_id: String, id: String, destination: String) -> Result<(), FfiError> {
        let lists = self.lists(&space_id)?;
        if !lists.can_edit {
            return Err(failed("You may not change this server's order."));
        }
        let from = room(&id)?;
        let dest = room(&destination)?;
        let listed: Vec<OwnedRoomId> = if dest == lists.root { lists.channels.clone() } else { lists.categories.iter().find(|(c, _, _)| *c == dest).map(|(_, _, m)| m.clone()).unwrap_or_default() };
        let order = lists.children.get(&dest).cloned().unwrap_or_default();
        let new_order = reorder::dropped_last(&order, &listed, &from);
        let mut containers = vec![lists.root.clone()];
        containers.extend(lists.categories.iter().map(|(c, _, _)| c.clone()));
        let leaving = reorder::leaving(&lists.parents.get(&from).cloned().unwrap_or_default(), &containers, &dest);
        self.session()?.move_space_child(dest, new_order, Some(from), leaving).await?;
        Ok(())
    }
}
