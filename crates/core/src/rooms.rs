use std::collections::HashMap;

use futures_util::{StreamExt, pin_mut};
use matrix_sdk_ui::timeline::RoomExt as _;

use matrix_sdk::{
    Room, RoomState,
    notification_settings::{IsEncrypted, IsOneToOne, NotificationSettings, RoomNotificationMode},
    room::ParentSpace,
    ruma::{
        api::client::room::create_room::{self, v3::RoomPreset},
        room::RoomType,
        serde::Raw,
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

/// How many rooms the room list fetches at a time (more pages follow while it is full).
const PAGE_SIZE: usize = 200;

/// How often the room list looks for call memberships that ran out.
const CALL_EXPIRY_CHECK: std::time::Duration = std::time::Duration::from_secs(10);

/// Remember who the call of each room in `diff` lists.
fn note_calls(diff: &VectorDiff<RoomSummary>, in_calls: &mut HashMap<OwnedRoomId, Vec<OwnedUserId>>) {
    let mut note = |room: &RoomSummary| {
        in_calls.insert(room.id.clone(), room.call_participants.clone());
    };
    match diff {
        VectorDiff::Append { values } | VectorDiff::Reset { values } => values.iter().for_each(note),
        VectorDiff::PushFront { value }
        | VectorDiff::PushBack { value }
        | VectorDiff::Insert { value, .. }
        | VectorDiff::Set { value, .. } => note(value),
        _ => {}
    }
}

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
    /// The room's call membership events (left ones too): when the call began is
    /// worked out from them, at the time it is shown.
    pub call_memberships: Vec<crate::CallMembership>,
    /// What each person in the call has on (muted, deafened, camera, screen),
    /// as Morpheus publishes it; people in other apps are missing.
    pub call_media: Vec<(OwnedUserId, crate::CallMedia)>,
    /// In your favorites (`m.favourite`).
    pub favorite: bool,
    /// A voice channel: a room that is a call (Element's video rooms,
    /// `org.matrix.msc3417.call`, or its older Jitsi ones).
    pub is_voice: bool,
    /// End-to-end encrypted (as last synced).
    pub is_encrypted: bool,
    /// When the newest message came (milliseconds since the epoch; 0 unknown).
    pub latest_ms: u64,
    /// For a space: the rooms it lists (`m.space.child`), in its order.
    /// They belong to it even when they don't name it back.
    pub space_children: Vec<OwnedRoomId>,
    /// For a space: you may change its children (and so their order).
    pub can_edit_children: bool,
    /// The network this room is bridged to (a mautrix or other bridge's
    /// `m.bridge` state), when it is.
    pub bridge: Option<Bridge>,
}

/// A bridge a room is part of, from its `m.bridge` (or the older
/// `uk.half-shot.bridge`) state event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bridge {
    /// The network's id as the bridge names it (`discord`, `whatsapp`, ...), lower case.
    pub protocol: String,
    /// What the bridge calls the network (`Discord`).
    pub name: String,
    /// The bridge's bot user, when the event names it.
    pub bot: Option<String>,
}

/// What a bridge state event's content says about its network.
pub fn bridge_of_content(content: &serde_json::Value) -> Option<Bridge> {
    let protocol = content.get("protocol")?;
    let id = protocol.get("id")?.as_str()?.trim().to_lowercase();
    if id.is_empty() {
        return None;
    }
    let name = protocol
        .get("displayname")
        .and_then(serde_json::Value::as_str)
        .filter(|n| !n.is_empty())
        .unwrap_or(&id)
        .to_owned();
    let bot = content
        .get("bridgebot")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    Some(Bridge { protocol: id, name, bot })
}

/// What the ghost users of each network's bridge (mautrix and others) are named, by network:
/// `@discord_123:server` is a Discord person. A room with one of them is bridged to that network.
pub const GHOST_PREFIXES: &[(&str, &str)] = &[
    ("whatsapp", "whatsapp_"),
    ("signal", "signal_"),
    ("discord", "discord_"),
    ("telegram", "telegram_"),
    ("slack", "slack_"),
    ("instagram", "instagram_"),
    ("messenger", "meta_"),
    ("googlechat", "googlechat_"),
    ("gmessages", "gmessages_"),
    ("imessage", "imessage_"),
    ("twitter", "twitter_"),
    ("bluesky", "bluesky_"),
    ("linkedin", "linkedin_"),
];

/// The network a bridge's ghost user stands for, from its name.
pub fn network_of_ghost(user: &matrix_sdk::ruma::UserId) -> Option<&'static str> {
    let local = user.localpart();
    GHOST_PREFIXES
        .iter()
        .find(|(_, prefix)| local.starts_with(prefix))
        .map(|(id, _)| *id)
}

/// Rooms already looked at for ghosts, by how many people were in them then.
static GHOSTS: std::sync::Mutex<Option<HashMap<OwnedRoomId, (u64, Option<String>, std::time::Instant)>>> =
    std::sync::Mutex::new(None);

/// The network of the first ghost user among the room's members the device knows.
async fn ghost_network(room: &Room) -> Option<String> {
    let count = room.joined_members_count();
    if count > 300 {
        return None;
    }
    let key = room.room_id().to_owned();
    if let Ok(cache) = GHOSTS.lock()
        && let Some((was, found, at)) = cache.as_ref().and_then(|c| c.get(&key))
        && *was == count
        // A room with no ghost found is looked at again now and then: people's names arrive late.
        && (found.is_some() || at.elapsed() < std::time::Duration::from_secs(20))
    {
        return found.clone();
    }
    let found = room
        .members_no_sync(matrix_sdk::RoomMemberships::JOIN)
        .await
        .ok()?
        .iter()
        .find_map(|member| network_of_ghost(member.user_id()))
        .map(str::to_owned);
    if let Ok(mut cache) = GHOSTS.lock() {
        cache.get_or_insert_with(HashMap::new).insert(key, (count, found.clone(), std::time::Instant::now()));
    }
    found
}

/// The bridge a room is part of, from the state the device holds.
pub(crate) async fn bridge_of(room: &Room) -> Option<Bridge> {
    use matrix_sdk::deserialized_responses::RawAnySyncOrStrippedState as Raws;
    for kind in ["m.bridge", "uk.half-shot.bridge"] {
        let Ok(events) = room
            .get_state_events(matrix_sdk::ruma::events::StateEventType::from(kind))
            .await
        else {
            continue;
        };
        for event in events {
            let text = match &event {
                Raws::Sync(raw) => raw.json().get(),
                Raws::Stripped(raw) => raw.json().get(),
            };
            let Ok(json) = serde_json::from_str::<serde_json::Value>(text) else {
                continue;
            };
            if let Some(bridge) = json.get("content").and_then(bridge_of_content) {
                return Some(bridge);
            }
        }
    }
    let id = ghost_network(room).await?;
    let name = {
        let mut chars = id.chars();
        chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
    };
    Some(Bridge { protocol: id, name, bot: None })
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
        let (space_children, can_edit_children) = if room.is_space() {
            let children = space_children(&room)
                .await
                .into_iter()
                .map(|(id, _)| id)
                .collect();
            let can = match room.client().user_id() {
                Some(me) => room.power_levels().await.is_ok_and(|levels| {
                    levels.user_can_send_state(
                        me,
                        matrix_sdk::ruma::events::StateEventType::SpaceChild,
                    )
                }),
                None => false,
            };
            (children, can)
        } else {
            (Vec::new(), false)
        };
        let call_participants_now = crate::calls::call_participants(&room);
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
            // Nobody's picture in the room: the partner's own (their profile's).
            if avatar.is_none()
                && let Some(user) = &dm_user
            {
                avatar = crate::media::global_avatar(&room.client(), user)
                    .await;
            }
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
            // The server's count, or the SDK's own reckoning from the read
            // receipts (sliding sync does not always carry the former).
            highlight_count: counts.highlight_count.max(room.num_unread_mentions()),
            marked_unread: room.is_marked_unread(),
            muted,
            parent_spaces,
            invited,
            inviter,
            call_participants: call_participants_now.clone(),
            call_media: crate::calls::call_media(&room).await,
            call_memberships: if call_participants_now.is_empty() { Vec::new() } else { crate::calls::call_memberships(&room).await },
            favorite: room.is_favourite(),
            is_voice: room
                .room_type()
                .is_some_and(|kind| VOICE_ROOM_TYPES.contains(&kind.to_string().as_str())),
            is_encrypted: room.encryption_state().is_encrypted(),
            latest_ms: room.latest_event_timestamp().map_or(0, |ts| ts.0.into()),
            space_children,
            can_edit_children,
            bridge: bridge_of(&room).await,
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

                // The list holds one page of rooms until asked for another:
                // keep asking while it is full, so every room is listed.
                let mut len = 0usize;
                let mut limit = PAGE_SIZE;
                // The rooms as listed, and who each one's call listed: a
                // call membership runs out (a device that vanished without
                // hanging up, on a homeserver without delayed leaves) with
                // nothing from the server to say so, so calls are looked
                // at again now and then and the room's entry redone.
                let mut shown: imbl::Vector<Room> = imbl::Vector::new();
                let mut in_calls: HashMap<OwnedRoomId, Vec<OwnedUserId>> = HashMap::new();
                let mut next_check = crate::rt::Instant::now() + CALL_EXPIRY_CHECK;

                loop {
                    let wait = next_check.saturating_duration_since(crate::rt::Instant::now());
                    let diffs = tokio::select! {
                        diffs = stream.next() => match diffs {
                            Some(diffs) => diffs,
                            None => break,
                        },
                        () = crate::rt::sleep(wait) => {
                            next_check = crate::rt::Instant::now() + CALL_EXPIRY_CHECK;
                            let mut redone = Vec::new();
                            for (index, room) in shown.iter().enumerate() {
                                let listed = in_calls.get(room.room_id());
                                if listed.is_none_or(Vec::is_empty)
                                    || listed == Some(&crate::calls::call_participants(room))
                                {
                                    continue;
                                }
                                let settings = client.notification_settings().await;
                                let value = RoomSummary::from_room(room.clone(), &settings).await;
                                in_calls.insert(value.id.clone(), value.call_participants.clone());
                                redone.push(VectorDiff::Set { index, value });
                            }
                            if !redone.is_empty() && tx.send(redone).is_err() {
                                break;
                            }
                            continue;
                        }
                    };
                    for diff in &diffs {
                        diff.clone().map(|item| item.into_inner()).apply(&mut shown);
                    }
                    for diff in &diffs {
                        len = match diff {
                            VectorDiff::Append { values } => len + values.len(),
                            VectorDiff::Clear => 0,
                            VectorDiff::PushFront { .. }
                            | VectorDiff::PushBack { .. }
                            | VectorDiff::Insert { .. } => len + 1,
                            VectorDiff::PopFront | VectorDiff::PopBack | VectorDiff::Remove { .. } => {
                                len.saturating_sub(1)
                            }
                            VectorDiff::Set { .. } => len,
                            VectorDiff::Truncate { length } => (*length).min(len),
                            VectorDiff::Reset { values } => values.len(),
                        };
                    }
                    while len >= limit {
                        controller.add_one_page();
                        limit += PAGE_SIZE;
                    }
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
                    for diff in &mapped {
                        note_calls(diff, &mut in_calls);
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
        self.create_channel(name, topic, space, false)
    }

    /// [`Self::create_room`], as a voice channel when `voice`: a room of the
    /// type clients list with a speaker and join the call of
    /// (`org.matrix.msc3417.call`), where anyone may join the call.
    pub fn create_channel(
        &self,
        name: String,
        topic: String,
        space: Option<OwnedRoomId>,
        voice: bool,
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
            if voice {
                let mut creation = create_room::v3::CreationContent::new();
                creation.room_type = Some(RoomType::from(VOICE_ROOM_TYPES[0]));
                request.creation_content = Some(Raw::new(&creation)?);
            }
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

/// A space's children from its `m.space.child` events (those with a
/// `via`; an empty one removes the child), in the space's order: by `order`
/// (Unicode codepoints), then those without one by when they were added
/// (the spec says by the rooms' creation, which isn't known for rooms not
/// joined; Element uses this too).
async fn space_children(room: &Room) -> Vec<(OwnedRoomId, SpaceChildEventContent)> {
    use matrix_sdk::deserialized_responses::SyncOrStrippedState;
    let Ok(events) = room
        .get_state_events_static::<SpaceChildEventContent>()
        .await
    else {
        return Vec::new();
    };
    // (room, content, when it was added)
    let mut children: Vec<(OwnedRoomId, SpaceChildEventContent, u64)> = events
        .into_iter()
        .filter_map(|raw| match raw.deserialize().ok()? {
            SyncOrStrippedState::Sync(event) => {
                let event = event.as_original()?;
                Some((
                    event.state_key.clone(),
                    event.content.clone(),
                    event.origin_server_ts.0.into(),
                ))
            }
            SyncOrStrippedState::Stripped(_) => None,
        })
        .filter(|(_, content, _)| !content.via.is_empty())
        .collect();
    // As Element sorts them: by order, then (no order) when added, then id.
    children.sort_by(|(a_id, a, a_ts), (b_id, b, b_ts)| {
        let key = |c: &SpaceChildEventContent| c.order.as_ref().map(|o| o.as_str().to_owned());
        let rest = || a_ts.cmp(b_ts).then_with(|| a_id.cmp(b_id));
        match (key(a), key(b)) {
            (Some(x), Some(y)) => x.cmp(&y).then_with(rest),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => rest(),
        }
    });
    children
        .into_iter()
        .map(|(id, content, _)| (id, content))
        .collect()
}

/// A room a space lists, from the server's directory of it (`/hierarchy`):
/// what shows for the space's channels you aren't in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceRoom {
    pub id: OwnedRoomId,
    pub name: String,
    pub topic: Option<String>,
    pub avatar: Option<MediaRef>,
    pub member_count: u64,
    pub is_space: bool,
    pub is_voice: bool,
    /// Servers to join it through (from the space's `m.space.child`).
    pub via: Vec<String>,
}

impl Session {
    /// The rooms `space_id` lists, one level down, as the server's directory
    /// describes them (including ones you aren't in), in the space's order.
    pub fn space_rooms(&self, space_id: OwnedRoomId) -> CoreFuture<Vec<SpaceRoom>> {
        use matrix_sdk::ruma::api::client::space::get_hierarchy;
        let client = self.client.clone();
        self.core.run(async move {
            let mut request = get_hierarchy::v1::Request::new(space_id.clone());
            request.max_depth = Some(1u32.into());
            let response = client.send(request).await?;
            let order: Vec<(OwnedRoomId, Vec<String>)> = match client.get_room(&space_id) {
                Some(space) => space_children(&space)
                    .await
                    .into_iter()
                    .map(|(id, c)| (id, c.via.iter().map(ToString::to_string).collect()))
                    .collect(),
                None => Vec::new(),
            };
            let mut rooms: Vec<SpaceRoom> = response
                .rooms
                .into_iter()
                .filter(|chunk| chunk.summary.room_id != space_id)
                .map(|chunk| {
                    let summary = chunk.summary;
                    let id = summary.room_id.clone();
                    let via = order
                        .iter()
                        .find(|(child, _)| *child == id)
                        .map(|(_, via)| via.clone())
                        .unwrap_or_default();
                    let kind = summary.room_type.as_ref().map(ToString::to_string);
                    SpaceRoom {
                        name: summary
                            .name
                            .clone()
                            .or_else(|| summary.canonical_alias.as_ref().map(ToString::to_string))
                            .unwrap_or_else(|| id.to_string()),
                        topic: summary.topic.clone(),
                        avatar: summary.avatar_url.clone().map(MediaRef::from_mxc),
                        member_count: summary.num_joined_members.into(),
                        is_space: kind.as_deref() == Some("m.space"),
                        is_voice: kind
                            .as_deref()
                            .is_some_and(|k| VOICE_ROOM_TYPES.contains(&k)),
                        via,
                        id,
                    }
                })
                .collect();
            let place = |id: &OwnedRoomId| {
                order
                    .iter()
                    .position(|(child, _)| child == id)
                    .unwrap_or(usize::MAX)
            };
            rooms.sort_by_key(|room| place(&room.id));
            Ok(rooms)
        })
    }

    /// Join a room you may know nothing about yet (a space's channel you
    /// aren't in), through `via`.
    pub fn join_room_via(&self, room_id: OwnedRoomId, via: Vec<String>) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let servers: Vec<matrix_sdk::ruma::OwnedServerName> = via
                .iter()
                .filter_map(|s| s.as_str().try_into().ok())
                .collect();
            client
                .join_room_by_id_or_alias(
                    <&matrix_sdk::ruma::RoomOrAliasId>::from(&*room_id),
                    &servers,
                )
                .await?;
            Ok(())
        })
    }

    /// Put `space_id`'s channels in `order` (their room ids, first to last)
    /// for everyone: each child's `order` is rewritten to its place. Needs
    /// the power to change the space's children.
    pub fn set_space_order(
        &self,
        space_id: OwnedRoomId,
        order: Vec<OwnedRoomId>,
    ) -> CoreFuture<()> {
        self.move_space_child(space_id, order, None, Vec::new())
    }

    /// [`Self::set_space_order`], and in the same go `child` (a room that may
    /// not be in `space_id` yet) is added to it and taken out of each space in
    /// `leaving` (a channel dragged into or out of a category).
    pub fn move_space_child(
        &self,
        space_id: OwnedRoomId,
        order: Vec<OwnedRoomId>,
        child: Option<OwnedRoomId>,
        leaving: Vec<OwnedRoomId>,
    ) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let space = client
                .get_room(&space_id)
                .ok_or_else(|| crate::Error::UnknownRoom(space_id.clone()))?;
            let children = space_children(&space).await;
            let via: Vec<matrix_sdk::ruma::OwnedServerName> = client
                .user_id()
                .map(|me| vec![me.server_name().to_owned()])
                .unwrap_or_default();
            for (place, id) in order.iter().enumerate() {
                let existing = children.iter().find(|(known, _)| known == id);
                let mut content = match existing {
                    Some((_, content)) => content.clone(),
                    None if child.as_ref() == Some(id) => SpaceChildEventContent::new(via.clone()),
                    None => continue,
                };
                // Room for 26^2 places either side, in plain letters.
                let key = format!(
                    "{}{}",
                    char::from(b'a' + (place / 26 % 26) as u8),
                    char::from(b'a' + (place % 26) as u8)
                );
                if existing.is_some()
                    && content.order.as_ref().map(|o| o.as_str()) == Some(key.as_str())
                {
                    continue;
                }
                content.order = matrix_sdk::ruma::SpaceChildOrder::parse(&key).ok();
                space.send_state_event_for_key(id, content).await?;
            }
            // Taking it out is an empty `via`, once it is in its new place.
            if let Some(child) = &child {
                for old in leaving {
                    let Some(old) = client.get_room(&old) else {
                        continue;
                    };
                    old.send_state_event_for_key(child, SpaceChildEventContent::new(Vec::new()))
                        .await?;
                }
            }
            Ok(())
        })
    }
}
