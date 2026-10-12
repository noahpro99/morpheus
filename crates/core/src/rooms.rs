use futures_util::{StreamExt, pin_mut};
use matrix_sdk::{Room, room::ParentSpace};
use matrix_sdk_ui::room_list_service::filters::new_filter_non_left;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::{CoreFuture, MediaRef, OwnedRoomId, Session, VectorDiff};

/// How many rooms the room list fetches before the UI asks for more.
const PAGE_SIZE: usize = 200;

/// A UI-friendly snapshot of one room in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomSummary {
    pub id: OwnedRoomId,
    pub name: String,
    pub topic: Option<String>,
    pub avatar: Option<MediaRef>,
    pub is_space: bool,
    pub is_dm: bool,
    pub member_count: u64,
    pub unread_messages: u64,
    pub highlight_count: u64,
    /// Spaces this room lists as a parent via `m.space.parent`, filtered to
    /// relationships the room itself considers legitimate (reciprocated by
    /// the parent, or set by someone with power to do so). Nested spaces are
    /// not expanded here: a room only lists its direct parents.
    pub parent_spaces: Vec<OwnedRoomId>,
}

impl RoomSummary {
    async fn from_room(room: Room) -> Self {
        let name = room
            .cached_display_name()
            .map(|n| n.to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| room.room_id().to_string());
        let counts = room.unread_notification_counts();
        let parent_spaces = parent_spaces(&room).await;
        let is_dm = room.direct_targets_length() > 0;
        // DMs rarely set a room avatar; show the other person instead.
        let mut avatar = room.avatar_url();
        if avatar.is_none() && is_dm {
            avatar = room.heroes().await.into_iter().find_map(|hero| hero.avatar_url);
        }

        Self {
            id: room.room_id().to_owned(),
            name,
            topic: room.topic(),
            avatar: avatar.map(MediaRef::from_mxc),
            is_space: room.is_space(),
            is_dm,
            member_count: room.joined_members_count(),
            unread_messages: room.num_unread_messages(),
            highlight_count: counts.highlight_count,
            parent_spaces,
        }
    }
}

/// Resolve the `m.space.parent` relationships this room advertises, from
/// local state only (no network calls). Illegitimate relationships (the
/// parent does not recognize the child back, and the sender of the
/// `m.space.parent` event has no power to set it) are dropped, per the
/// spec's recommendation that clients ignore those when building the space
/// tree.
async fn parent_spaces(room: &Room) -> Vec<OwnedRoomId> {
    let stream = match room.parent_spaces().await {
        Ok(stream) => stream,
        Err(err) => {
            warn!(room_id = %room.room_id(), %err, "could not resolve parent spaces");
            return Vec::new();
        }
    };

    stream
        .filter_map(|result| async move {
            match result {
                Ok(ParentSpace::Reciprocal(parent) | ParentSpace::WithPowerlevel(parent)) => {
                    Some(parent.room_id().to_owned())
                }
                Ok(ParentSpace::Unverifiable(id)) => Some(id),
                Ok(ParentSpace::Illegitimate(_)) => None,
                Err(err) => {
                    warn!(room_id = %room.room_id(), %err, "could not check a parent space");
                    None
                }
            }
        })
        .collect()
        .await
}

/// Transforms a `VectorDiff<T>` into a `VectorDiff<U>` with an async mapping
/// function, applied to any contained items. Mirrors [`VectorDiff::map`],
/// which only accepts a synchronous closure.
async fn map_diff_async<T: Clone, U: Clone, F, Fut>(diff: VectorDiff<T>, f: F) -> VectorDiff<U>
where
    F: Fn(T) -> Fut,
    Fut: std::future::Future<Output = U>,
{
    async fn map_values<T: Clone, U: Clone, F, Fut>(
        values: imbl::Vector<T>,
        f: &F,
    ) -> imbl::Vector<U>
    where
        F: Fn(T) -> Fut,
        Fut: std::future::Future<Output = U>,
    {
        let mut mapped = imbl::Vector::new();
        for value in values {
            mapped.push_back(f(value).await);
        }
        mapped
    }

    match diff {
        VectorDiff::Append { values } => VectorDiff::Append {
            values: map_values(values, &f).await,
        },
        VectorDiff::Clear => VectorDiff::Clear,
        VectorDiff::PushFront { value } => VectorDiff::PushFront {
            value: f(value).await,
        },
        VectorDiff::PushBack { value } => VectorDiff::PushBack {
            value: f(value).await,
        },
        VectorDiff::PopFront => VectorDiff::PopFront,
        VectorDiff::PopBack => VectorDiff::PopBack,
        VectorDiff::Insert { index, value } => VectorDiff::Insert {
            index,
            value: f(value).await,
        },
        VectorDiff::Set { index, value } => VectorDiff::Set {
            index,
            value: f(value).await,
        },
        VectorDiff::Remove { index } => VectorDiff::Remove { index },
        VectorDiff::Truncate { length } => VectorDiff::Truncate { length },
        VectorDiff::Reset { values } => VectorDiff::Reset {
            values: map_values(values, &f).await,
        },
    }
}

/// Incremental updates to the room list. The first batch is always a
/// [`VectorDiff::Reset`] carrying the current rooms.
pub struct RoomListUpdates {
    rx: mpsc::UnboundedReceiver<Vec<VectorDiff<RoomSummary>>>,
}

impl RoomListUpdates {
    /// Next batch of diffs, or `None` once the session is gone.
    pub async fn next(&mut self) -> Option<Vec<VectorDiff<RoomSummary>>> {
        self.rx.recv().await
    }
}

impl Session {
    /// Subscribe to the list of joined and invited rooms, sorted by recency.
    pub fn room_list(&self) -> CoreFuture<RoomListUpdates> {
        let core = self.core.clone();
        let sync = self.sync.clone();

        self.core.run(async move {
            let list = sync.room_list_service().all_rooms().await?;
            let (tx, rx) = mpsc::unbounded_channel();

            core.spawn(async move {
                let (stream, controller) = list.entries_with_dynamic_adapters(PAGE_SIZE);
                controller.set_filter(Box::new(new_filter_non_left()));
                pin_mut!(stream);

                while let Some(diffs) = stream.next().await {
                    let mut mapped = Vec::with_capacity(diffs.len());
                    for diff in diffs {
                        mapped.push(
                            map_diff_async(diff, |item| RoomSummary::from_room(item.into_inner()))
                                .await,
                        );
                    }
                    if tx.send(mapped).is_err() {
                        debug!("room list consumer dropped, stopping");
                        break;
                    }
                }
            });

            Ok(RoomListUpdates { rx })
        })
    }
}

impl Session {
    /// Leave a room. The room list drops it on the next sync.
    pub fn leave_room(&self, room_id: OwnedRoomId) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let room = client
                .get_room(&room_id)
                .ok_or_else(|| crate::Error::UnknownRoom(room_id.clone()))?;
            room.leave().await?;
            Ok(())
        })
    }
}
