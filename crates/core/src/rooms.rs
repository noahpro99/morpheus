use futures_util::{StreamExt, pin_mut};
use matrix_sdk_ui::timeline::RoomExt as _;

use matrix_sdk::{
    Room, RoomState,
    notification_settings::{IsEncrypted, IsOneToOne, NotificationSettings, RoomNotificationMode},
    room::ParentSpace,
    ruma::{
        api::client::room::create_room::{self, v3::RoomPreset},
        events::{
            InitialStateEvent,
            space::{child::SpaceChildEventContent, parent::SpaceParentEventContent},
        },
    },
};
use matrix_sdk_ui::room_list_service::filters::new_filter_non_left;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::{CoreFuture, MediaRef, OwnedRoomId, OwnedUserId, Session, VectorDiff};

/// Room types that make a room a voice channel: Element Call's video
/// rooms and Element's older Jitsi ones.
const VOICE_ROOM_TYPES: [&str; 2] = ["org.matrix.msc3417.call", "io.element.video"];

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
    /// The other person in a DM, for showing your nickname for them.
    pub dm_user: Option<OwnedUserId>,
    pub member_count: u64,
    pub unread_messages: u64,
    pub highlight_count: u64,
    /// Flagged unread by the user (`m.marked_unread`), independent of counts.
    pub marked_unread: bool,
    /// Notifications muted for this room (push rules).
    pub muted: bool,
    /// Spaces this room lists as a parent via `m.space.parent`, filtered to
    /// relationships the room itself considers legitimate (reciprocated by
    /// the parent, or set by someone with power to do so). Nested spaces are
    /// not expanded here: a room only lists its direct parents.
    pub parent_spaces: Vec<OwnedRoomId>,
    /// A pending invite: not joined yet, so there is no timeline to show.
    pub invited: bool,
    /// Who sent the invite (display name, else user ID); only for invites.
    pub inviter: Option<String>,
    /// Who is in the room's call (MatrixRTC), oldest first; empty when none.
    pub call_participants: Vec<OwnedUserId>,
    /// In your favorites (`m.favourite`).
    pub favorite: bool,
    /// A voice channel: a room that is a call (Element's video rooms,
    /// `org.matrix.msc3417.call`, or its older Jitsi ones).
    pub is_voice: bool,
    /// When the newest message came (milliseconds since the epoch; 0 unknown).
    pub latest_ms: u64,
}

impl RoomSummary {
    async fn from_room(room: Room, settings: &NotificationSettings) -> Self {
        let invited = room.state() == RoomState::Invited;
        let inviter = if invited {
            match room.invite_details().await {
                Ok(details) => Some(
                    details
                        .inviter
                        .map(|member| member.name().to_owned())
                        .unwrap_or_else(|| details.inviter_id.to_string()),
                ),
                Err(err) => {
                    debug!(room_id = %room.room_id(), %err, "invite details unavailable");
                    None
                }
            }
        } else {
            None
        };
        let muted = matches!(
            settings
                .get_user_defined_room_notification_mode(room.room_id())
                .await,
            Some(RoomNotificationMode::Mute)
        );
        let name = room
            .cached_display_name()
            .map(|n| n.to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| room.room_id().to_string());
        let counts = room.unread_notification_counts();
        let parent_spaces = parent_spaces(&room).await;
        let is_dm = room.direct_targets_length() > 0;
        // Who a DM is with (the first target, for one-to-one chats).
        let dm_user = room
            .direct_targets()
            .into_iter()
            .find_map(|target| target.into_user_id());
        // DMs rarely set a room avatar; show the other person instead.
        let mut avatar = room.avatar_url();
        if avatar.is_none() && is_dm {
            avatar = room
                .heroes()
                .await
                .into_iter()
                .find_map(|hero| hero.avatar_url);
        }

        Self {
            id: room.room_id().to_owned(),
            name,
            topic: room.topic(),
            avatar: avatar.map(MediaRef::from_mxc),
            is_space: room.is_space(),
            is_dm,
            dm_user,
            member_count: room.joined_members_count(),
            unread_messages: room.num_unread_messages(),
            highlight_count: counts.highlight_count,
            marked_unread: room.is_marked_unread(),
            muted,
            parent_spaces,
            invited,
            inviter,
            call_participants: crate::calls::call_participants(&room),
            favorite: room.is_favourite(),
            is_voice: room
                .room_type()
                .is_some_and(|kind| VOICE_ROOM_TYPES.contains(&kind.to_string().as_str())),
            latest_ms: room.latest_event_timestamp().map_or(0, |ts| ts.0.into()),
        }
    }
}

impl Session {
    /// Accept an invite (or join a public room). The room list flips the
    /// entry from invited to joined on the next sync.
    pub fn join_room(&self, room_id: OwnedRoomId) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let room = client
                .get_room(&room_id)
                .ok_or_else(|| crate::Error::UnknownRoom(room_id.clone()))?;
            // Already in: nothing to do (a second tap, or another device).
            if room.state() == matrix_sdk::RoomState::Joined {
                return Ok(());
            }
            room.join().await?;
            Ok(())
        })
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
        let client = self.client.clone();

        self.core.run(async move {
            let list = sync.room_list_service().all_rooms().await?;
            let (tx, rx) = mpsc::unbounded_channel();

            core.spawn(async move {
                let (stream, controller) = list.entries_with_dynamic_adapters(PAGE_SIZE);
                controller.set_filter(Box::new(new_filter_non_left()));
                pin_mut!(stream);

                while let Some(diffs) = stream.next().await {
                    // Push rules can change between batches; read them once per batch.
                    let settings = client.notification_settings().await;
                    let mut mapped = Vec::with_capacity(diffs.len());
                    for diff in diffs {
                        mapped.push(
                            map_diff_async(diff, |item| {
                                RoomSummary::from_room(item.into_inner(), &settings)
                            })
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

/// How a room notifies, mirroring the push-rule modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyMode {
    AllMessages,
    MentionsOnly,
    Mute,
}

impl Session {
    /// Clear a room's unread state: send a read receipt for the latest event
    /// and drop any manual unread flag.
    pub fn mark_room_read(&self, room_id: OwnedRoomId) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let room = client
                .get_room(&room_id)
                .ok_or_else(|| crate::Error::UnknownRoom(room_id.clone()))?;
            let timeline = room.timeline().await?;
            timeline
                .mark_as_read(
                    matrix_sdk::ruma::api::client::receipt::create_receipt::v3::ReceiptType::Read,
                )
                .await?;
            room.set_unread_flag(false).await?;
            Ok(())
        })
    }

    /// Flag a room unread (`m.marked_unread`), to come back to it.
    pub fn mark_room_unread(&self, room_id: OwnedRoomId) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let room = client
                .get_room(&room_id)
                .ok_or_else(|| crate::Error::UnknownRoom(room_id.clone()))?;
            room.set_unread_flag(true).await?;
            Ok(())
        })
    }

    /// The room's effective notification mode (user override or the default).
    pub fn notify_mode(&self, room_id: OwnedRoomId) -> CoreFuture<NotifyMode> {
        let client = self.client.clone();
        self.core.run(async move {
            let room = client
                .get_room(&room_id)
                .ok_or_else(|| crate::Error::UnknownRoom(room_id.clone()))?;
            let settings = client.notification_settings().await;
            let mode = match settings
                .get_user_defined_room_notification_mode(&room_id)
                .await
            {
                Some(mode) => mode,
                None => {
                    let encrypted = room
                        .latest_encryption_state()
                        .await
                        .map(|s| s.is_encrypted())
                        .unwrap_or(false);
                    settings
                        .get_default_room_notification_mode(
                            IsEncrypted::from(encrypted),
                            IsOneToOne::from(room.active_members_count() == 2),
                        )
                        .await
                }
            };
            Ok(NotifyMode::from_sdk(mode))
        })
    }

    /// The account-wide default for direct messages (`dm`) or group rooms,
    /// as the encrypted-room rule has it (the setter keeps both in step):
    /// the same reading as the SDK's, on rules fetched from the server.
    pub fn default_notify_mode(&self, dm: bool) -> CoreFuture<NotifyMode> {
        let client = self.client.clone();
        self.core.run(async move {
            // From the server: the synced copy lags right after a change.
            let rules = client
                .send(matrix_sdk::ruma::api::client::push::get_pushrules_all::v3::Request::new())
                .await
                .map_err(|err| crate::Error::Other(format!("reading push rules: {err}")))?
                .global;
            let id = if dm {
                ".m.rule.encrypted_room_one_to_one"
            } else {
                ".m.rule.encrypted"
            };
            let notifies = rules
                .get(matrix_sdk::ruma::push::RuleKind::Underride, id)
                .is_some_and(|rule| rule.enabled() && rule.triggers_notification());
            Ok(if notifies {
                NotifyMode::AllMessages
            } else {
                NotifyMode::MentionsOnly
            })
        })
    }

    /// Set the default for direct messages (`dm`) or group rooms, encrypted
    /// or not. Only all messages or mentions make sense as a default.
    pub fn set_default_notify_mode(&self, dm: bool, mode: NotifyMode) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let settings = client.notification_settings().await;
            for encrypted in [IsEncrypted::Yes, IsEncrypted::No] {
                settings
                    .set_default_room_notification_mode(
                        encrypted,
                        IsOneToOne::from(dm),
                        mode.to_sdk(),
                    )
                    .await
                    .map_err(|err| crate::Error::Other(err.to_string()))?;
            }
            Ok(())
        })
    }

    pub fn set_notify_mode(&self, room_id: OwnedRoomId, mode: NotifyMode) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            client
                .notification_settings()
                .await
                .set_room_notification_mode(&room_id, mode.to_sdk())
                .await
                .map_err(|err| crate::Error::Other(err.to_string()))?;
            Ok(())
        })
    }

    /// Rename a room and/or change its topic (`None` leaves a field alone).
    pub fn edit_room(
        &self,
        room_id: OwnedRoomId,
        name: Option<String>,
        topic: Option<String>,
    ) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let room = client
                .get_room(&room_id)
                .ok_or_else(|| crate::Error::UnknownRoom(room_id.clone()))?;
            if let Some(name) = name.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()) {
                room.set_name(name).await?;
            }
            if let Some(topic) = topic {
                room.set_room_topic(topic.trim()).await?;
            }
            Ok(())
        })
    }

    /// Create a room. Inside a space, the room is added as a child of it and
    /// links back, so it shows up under the space's badge.
    pub fn create_room(
        &self,
        name: String,
        topic: String,
        space: Option<OwnedRoomId>,
    ) -> CoreFuture<OwnedRoomId> {
        let client = self.client.clone();
        self.core.run(async move {
            let server = client
                .user_id()
                .ok_or(crate::Error::NoSession)?
                .server_name()
                .to_owned();
            let mut request = create_room::v3::Request::new();
            request.name = Some(name.trim().to_owned());
            let topic = topic.trim();
            request.topic = (!topic.is_empty()).then(|| topic.to_owned());
            request.preset = Some(RoomPreset::PublicChat);
            if let Some(space_id) = &space {
                let parent = InitialStateEvent::new(
                    space_id.clone(),
                    SpaceParentEventContent::new(vec![server.clone()]),
                );
                request.initial_state = vec![parent.to_raw_any()];
            }
            let room = client.create_room(request).await?;
            // Anyone in the room may join its calls.
            if let Err(err) = crate::calls::allow_calls(&room).await {
                tracing::warn!(%err, "could not allow calls in the new room");
            }
            if let Some(space_id) = space
                && let Some(space_room) = client.get_room(&space_id)
            {
                space_room
                    .send_state_event_for_key(
                        room.room_id(),
                        SpaceChildEventContent::new(vec![server]),
                    )
                    .await?;
            }
            Ok(room.room_id().to_owned())
        })
    }
}

impl NotifyMode {
    fn from_sdk(mode: RoomNotificationMode) -> Self {
        match mode {
            RoomNotificationMode::AllMessages => Self::AllMessages,
            RoomNotificationMode::MentionsAndKeywordsOnly => Self::MentionsOnly,
            RoomNotificationMode::Mute => Self::Mute,
        }
    }

    fn to_sdk(self) -> RoomNotificationMode {
        match self {
            Self::AllMessages => RoomNotificationMode::AllMessages,
            Self::MentionsOnly => RoomNotificationMode::MentionsAndKeywordsOnly,
            Self::Mute => RoomNotificationMode::Mute,
        }
    }
}
