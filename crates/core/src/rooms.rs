use futures_util::{StreamExt, pin_mut};
use matrix_sdk::Room;
use matrix_sdk_ui::room_list_service::filters::new_filter_non_left;
use tokio::sync::mpsc;
use tracing::debug;

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
}

impl RoomSummary {
    fn from_room(room: Room) -> Self {
        let name = room
            .cached_display_name()
            .map(|n| n.to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| room.room_id().to_string());
        let counts = room.unread_notification_counts();

        Self {
            id: room.room_id().to_owned(),
            name,
            topic: room.topic(),
            avatar: room.avatar_url().map(MediaRef::from_mxc),
            is_space: room.is_space(),
            is_dm: room.direct_targets_length() > 0,
            member_count: room.joined_members_count(),
            unread_messages: room.num_unread_messages(),
            highlight_count: counts.highlight_count,
        }
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
                    let diffs: Vec<_> = diffs
                        .into_iter()
                        .map(|diff| diff.map(|item| RoomSummary::from_room(item.into_inner())))
                        .collect();
                    if tx.send(diffs).is_err() {
                        debug!("room list consumer dropped, stopping");
                        break;
                    }
                }
            });

            Ok(RoomListUpdates { rx })
        })
    }
}
