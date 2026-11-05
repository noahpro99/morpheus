//! Calls over MatrixRTC, the way Element Call does them: the homeserver
//! names a LiveKit service (MSC4143 transports, else the client
//! well-known), which swaps a Matrix OpenID token for a LiveKit token, and
//! each device in the call publishes an `m.call.member` state event so
//! everyone in the room sees who is there. The media itself is LiveKit's
//! business (`morpheus-call`).

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::Duration,
};

use matrix_sdk::ruma::{
    MilliSecondsSinceUnixEpoch, OwnedEventId, OwnedRoomId, OwnedUserId,
    api::client::{
        account::request_openid_token,
        delayed_events::{
            DelayParameters, delayed_state_event,
            update_delayed_event::{self, UpdateAction},
        },
        state::get_state_event_for_key,
    },
    events::{
        Mentions, StateEventType,
        call::member::{
            ActiveFocus, ActiveLivekitFocus, Application, CallApplicationContent,
            CallMemberEventContent, CallMemberStateKey, CallScope, Focus, LivekitFocus,
        },
        relation::Reference,
        rtc::{
            decline::RtcDeclineEventContent,
            notification::{NotificationType, RtcNotificationEventContent},
        },
    },
};
use matrix_sdk::{
    deserialized_responses::EncryptionInfo,
    event_handler::{EventHandlerDropGuard, RawEvent},
    ruma::events::AnySyncTimelineEvent,
};
use serde_json::{Value, json};
use tracing::debug;

use crate::{CoreFuture, Error, Session};

/// The MatrixRTC slot for a room's call (application `m.call`, room scope).
const CALL_SLOT: &str = "m.call#ROOM";

const TRANSPORTS_PATH: &str = "/_matrix/client/unstable/org.matrix.msc4143/rtc/transports";

/// What a device needs to connect to a room's call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallCredentials {
    /// The LiveKit server (a `ws(s)://` URL).
    pub livekit_url: String,
    /// The LiveKit access token for this device in this room's call.
    pub jwt: String,
    /// The service that issued it (advertised in the membership event).
    pub service_url: String,
    /// In an encrypted room, the media is too: this device's fresh key,
    /// to share with the room ([`Session::send_call_key`]).
    pub media_key: Option<Vec<u8>>,
}

impl Session {
    /// Join the call in `room_id` (starting one if nobody is in it): get
    /// LiveKit credentials, then tell the room this device is in the call.
    pub fn join_call(&self, room_id: OwnedRoomId) -> CoreFuture<CallCredentials> {
        let session = self.clone();
        self.core.run(async move {
            let room = session.room(&room_id)?;
            let client = session.client.clone();
            let user_id = client.user_id().ok_or(Error::NoSession)?.to_owned();
            let device_id = client.device_id().ok_or(Error::NoSession)?.to_owned();

            let services = livekit_services(&client).await;
            if services.is_empty() {
                return Err(Error::Other(
                    "this homeserver has no call service (LiveKit) set up".into(),
                ));
            }
            let mut last_err = None;
            let mut credentials = None;
            for service in services {
                match livekit_token(&client, &room_id, &user_id, device_id.as_str(), &service).await
                {
                    Ok(found) => {
                        credentials = Some(found);
                        break;
                    }
                    Err(err) => {
                        debug!(%service, %err, "call service refused a token");
                        last_err = Some(err);
                    }
                }
            }
            let credentials = match (credentials, last_err) {
                (Some(credentials), _) => credentials,
                (None, Some(err)) => return Err(err),
                (None, None) => return Err(Error::Other("no call service answered".into())),
            };
            ensure_calls_allowed(&room, &user_id).await?;
            let mut credentials = credentials;
            let encrypted = room
                .latest_encryption_state()
                .await
                .map(|state| state.is_encrypted())
                .unwrap_or(false);
            if encrypted {
                use rand::RngCore as _;
                let mut key = vec![0u8; 16];
                rand::rng().fill_bytes(&mut key);
                credentials.media_key = Some(key);
            }
            let state_key = member_state_key(user_id.clone(), device_id.as_str());

            // If this device vanishes (crash, battery, network), the
            // homeserver hangs up for it: a leave is scheduled now and
            // postponed for as long as the app keeps checking in.
            stop_heartbeat(&user_id, &room_id);
            match client
                .send(delayed_state_event::unstable::Request::new(
                    room_id.clone(),
                    state_key.as_ref().to_owned(),
                    DelayParameters::Timeout {
                        timeout: LEAVE_DELAY,
                    },
                    &CallMemberEventContent::new_empty(None),
                )?)
                .await
            {
                Ok(response) => {
                    let heartbeat =
                        tokio::spawn(heartbeat(client.clone(), response.delay_id.clone()));
                    if let Ok(mut beats) = heartbeats().lock() {
                        beats.insert(
                            (user_id.clone(), room_id.clone()),
                            (heartbeat, response.delay_id),
                        );
                    }
                }
                // Without delayed events (older homeservers) the membership
                // simply expires, as it does for everyone after a few hours.
                Err(err) => debug!(%err, "no delayed leave for the call"),
            }

            let content = CallMemberEventContent::new(
                Application::Call(CallApplicationContent::new(String::new(), CallScope::Room)),
                device_id.clone(),
                ActiveFocus::Livekit(ActiveLivekitFocus::new()),
                vec![Focus::Livekit(LivekitFocus::new(
                    room_id.to_string(),
                    credentials.service_url.clone(),
                ))],
                None,
                None,
            );
            let others_in_call = call_participants(&room).iter().any(|u| *u != user_id);
            let membership = room.send_state_event_for_key(&state_key, content).await?;
            if let Err(err) =
                ring_if_direct(&room, &user_id, others_in_call, membership.event_id).await
            {
                debug!(%err, "could not ring");
            }
            Ok(credentials)
        })
    }

    /// Leave the call in `room_id`: this device's membership is cleared.
    pub fn leave_call(&self, room_id: OwnedRoomId) -> CoreFuture<()> {
        let session = self.clone();
        self.core.run(async move {
            let room = session.room(&room_id)?;
            let client = session.client.clone();
            let user_id = client.user_id().ok_or(Error::NoSession)?.to_owned();
            let device_id = client.device_id().ok_or(Error::NoSession)?.to_owned();
            // The scheduled leave, sent now: one request, so nothing is lost if
            // the app dies right after (cancelling it and then sending a leave
            // could leave a ghost for hours).
            if let Some(delay_id) = stop_heartbeat(&user_id, &room_id)
                && update_delay(&client, delay_id, UpdateAction::Send)
                    .await
                    .is_ok()
            {
                return Ok(());
            }
            room.send_state_event_for_key(
                &member_state_key(user_id, device_id.as_str()),
                CallMemberEventContent::new_empty(None),
            )
            .await?;
            Ok(())
        })
    }
}

/// Element Call's state key: `_@user:server_DEVICE_m.call`.
fn member_state_key(user_id: OwnedUserId, device_id: &str) -> CallMemberStateKey {
    CallMemberStateKey::new(user_id, Some(format!("{device_id}_m.call")), true)
}

/// Everyone in the room's call, oldest first, each person once.
pub(crate) fn call_participants(room: &matrix_sdk::Room) -> Vec<OwnedUserId> {
    let mut seen = Vec::new();
    for user in room.active_room_call_participants() {
        if !seen.contains(&user) {
            seen.push(user);
        }
    }
    seen
}

/// The LiveKit token services the homeserver points at: the MSC4143
/// transports endpoint, else `org.matrix.msc4143.rtc_foci` in the client
/// well-known (on the server name's domain, then on the homeserver itself).
async fn livekit_services(client: &matrix_sdk::Client) -> Vec<String> {
    // Tests stand a different token service in front (one naming people as
    // matrix.org's does).
    if let Ok(service) = std::env::var("MORPHEUS_LIVEKIT_SERVICE") {
        return vec![service];
    }
    let http = client.http_client();
    let homeserver = client.homeserver().to_string();
    let homeserver = homeserver.trim_end_matches('/');
    let token = client.access_token().unwrap_or_default();

    let transports = get_json(
        http.get(format!("{homeserver}{TRANSPORTS_PATH}"))
            .bearer_auth(&token),
    )
    .await;
    let found = services_in(transports.as_ref().and_then(|v| v.get("rtc_transports")));
    if !found.is_empty() {
        return found;
    }

    let mut well_knowns = Vec::new();
    if let Some(user) = client.user_id() {
        well_knowns.push(format!(
            "https://{}/.well-known/matrix/client",
            user.server_name()
        ));
    }
    well_knowns.push(format!("{homeserver}/.well-known/matrix/client"));
    for url in well_knowns {
        let body = get_json(http.get(url)).await;
        let found = services_in(
            body.as_ref()
                .and_then(|v| v.get("org.matrix.msc4143.rtc_foci")),
        );
        if !found.is_empty() {
            return found;
        }
    }
    Vec::new()
}

fn services_in(list: Option<&Value>) -> Vec<String> {
    list.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|t| t.get("type").and_then(Value::as_str) == Some("livekit"))
        .filter_map(|t| t.get("livekit_service_url").and_then(Value::as_str))
        .map(|url| url.trim_end_matches('/').to_owned())
        .collect()
}

async fn get_json(request: matrix_sdk::reqwest::RequestBuilder) -> Option<Value> {
    let response = request.timeout(Duration::from_secs(10)).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    serde_json::from_str(&response.text().await.ok()?).ok()
}

/// Swap an OpenID token for a LiveKit one at `service` (`/get_token`, or the
/// older `/sfu/get` for services that predate it).
async fn livekit_token(
    client: &matrix_sdk::Client,
    room_id: &OwnedRoomId,
    user_id: &OwnedUserId,
    device_id: &str,
    service: &str,
) -> crate::Result<CallCredentials> {
    let openid = client
        .send(request_openid_token::v3::Request::new(user_id.clone()))
        .await?;
    let openid = json!({
        "access_token": openid.access_token,
        "token_type": openid.token_type.to_string(),
        "matrix_server_name": openid.matrix_server_name.to_string(),
        "expires_in": openid.expires_in.as_secs(),
    });
    let attempts = [
        (
            "get_token",
            json!({
                "room_id": room_id,
                "slot_id": CALL_SLOT,
                "openid_token": openid,
                "member": {
                    "id": format!("{device_id}_m.call"),
                    "claimed_user_id": user_id,
                    "claimed_device_id": device_id,
                },
            }),
        ),
        (
            "sfu/get",
            json!({ "room": room_id, "openid_token": openid, "device_id": device_id }),
        ),
    ];
    let http = client.http_client();
    let mut last = String::new();
    for (path, body) in attempts {
        let response = http
            .post(format!("{service}/{path}"))
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|err| Error::Other(format!("call service unreachable: {err}")))?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if status.is_success() {
            let body: Value = serde_json::from_str(&text)?;
            let field = |name: &str| {
                body.get(name)
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| Error::Other(format!("call service answer lacks `{name}`")))
            };
            return Ok(CallCredentials {
                livekit_url: field("url")?,
                jwt: field("jwt")?,
                service_url: service.to_owned(),
                media_key: None,
            });
        }
        last = format!("call service said {status}: {text}");
        // Only a missing endpoint is worth the older one.
        if status.as_u16() != 404 && status.as_u16() != 405 {
            break;
        }
    }
    Err(Error::Other(last))
}

/// How long after this device stops checking in the homeserver hangs up for it.
const LEAVE_DELAY: Duration = Duration::from_secs(10);
/// How often a device in a call checks in (well inside [`LEAVE_DELAY`]).
const HEARTBEAT: Duration = Duration::from_secs(4);

type Heartbeats = Mutex<HashMap<(OwnedUserId, OwnedRoomId), (tokio::task::JoinHandle<()>, String)>>;

/// The running heartbeat (and delayed leave id) per account and room.
fn heartbeats() -> &'static Heartbeats {
    static HEARTBEATS: OnceLock<Heartbeats> = OnceLock::new();
    HEARTBEATS.get_or_init(Default::default)
}

/// Stop checking in for a call; returns the delayed leave it kept postponed.
fn stop_heartbeat(user_id: &OwnedUserId, room_id: &OwnedRoomId) -> Option<String> {
    let (task, delay_id) = heartbeats()
        .lock()
        .ok()?
        .remove(&(user_id.clone(), room_id.clone()))?;
    task.abort();
    Some(delay_id)
}

async fn heartbeat(client: matrix_sdk::Client, delay_id: String) {
    let mut tick = tokio::time::interval(HEARTBEAT);
    tick.tick().await;
    loop {
        tick.tick().await;
        if let Err(err) = update_delay(&client, delay_id.clone(), UpdateAction::Restart).await {
            debug!(%err, "call heartbeat");
        }
    }
}

/// Restart, send or cancel a delayed event (both unstable MSC4140 shapes).
async fn update_delay(
    client: &matrix_sdk::Client,
    delay_id: String,
    action: UpdateAction,
) -> crate::Result<()> {
    let newer = client
        .send(update_delayed_event::unstable_v2::Request::new(
            delay_id.clone(),
            action.clone(),
        ))
        .await;
    if newer.is_ok() {
        return Ok(());
    }
    client
        .send(update_delayed_event::unstable_v1::Request::new(
            delay_id, action,
        ))
        .await?;
    Ok(())
}

/// Members need permission to post their call membership (state). Rooms
/// Morpheus creates allow it; in others an admin joining turns it on, and
/// anyone else is told why they can't.
async fn ensure_calls_allowed(room: &matrix_sdk::Room, user_id: &OwnedUserId) -> crate::Result<()> {
    let levels = room
        .power_levels()
        .await
        .map_err(|err| Error::Other(format!("reading power levels: {err}")))?;
    let needed = levels
        .events
        .get(&StateEventType::CallMember.to_string().into())
        .copied()
        .unwrap_or(levels.state_default);
    if levels.users_default >= needed {
        return Ok(());
    }
    // Members can't join; whoever may change that turns calls on for all.
    if levels.user_can_send_state(user_id, StateEventType::RoomPowerLevels) {
        return allow_calls(room).await;
    }
    if levels.user_can_send_state(user_id, StateEventType::CallMember) {
        return Ok(());
    }
    Err(Error::Other(
        "calls aren't allowed in this room; an admin can turn them on by joining one".into(),
    ))
}

/// Let every member join calls in `room` (power level 0 for the membership).
pub(crate) async fn allow_calls(room: &matrix_sdk::Room) -> crate::Result<()> {
    // From the server, not the local store: a fresh room may not have
    // synced yet, and writing back a partial event would wipe the rest.
    let current = room
        .client()
        .send(get_state_event_for_key::v3::Request::new(
            room.room_id().to_owned(),
            StateEventType::RoomPowerLevels,
            String::new(),
        ))
        .await?;
    let mut content: Value = serde_json::from_str(current.event_or_content.get())?;
    if !content.is_object() {
        return Err(Error::Other("unexpected power levels".into()));
    }
    if content["events"][StateEventType::CallMember.to_string()] == json!(0) {
        return Ok(());
    }
    if !content["events"].is_object() {
        content["events"] = json!({});
    }
    content["events"][StateEventType::CallMember.to_string()] = json!(0);
    room.send_state_event_raw("m.room.power_levels", "", content)
        .await?;
    Ok(())
}

/// One device in a room's call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallMember {
    pub user_id: OwnedUserId,
    pub device_id: String,
    /// The LiveKit identities this device can show up as (the token
    /// service derives them from the user and device), for matching
    /// LiveKit's speaking reports to people.
    pub livekit_identities: Vec<String>,
}

impl Session {
    /// The devices currently in `room_id`'s call.
    pub fn call_members(&self, room_id: OwnedRoomId) -> CoreFuture<Vec<CallMember>> {
        let session = self.clone();
        self.core.run(async move {
            let room = session.room(&room_id)?;
            let events = room
                .get_state_events_static::<CallMemberEventContent>()
                .await?;
            let mut members = Vec::new();
            for raw in events {
                let Ok(matrix_sdk::deserialized_responses::SyncOrStrippedState::Sync(event)) =
                    raw.deserialize()
                else {
                    continue;
                };
                let Some(original) = event.as_original() else {
                    continue;
                };
                for membership in original
                    .content
                    .active_memberships(Some(original.origin_server_ts))
                {
                    let device_id = membership.device_id().to_string();
                    let user_id = original.state_key.user_id().to_owned();
                    members.push(CallMember {
                        livekit_identities: livekit_identities(&user_id, &device_id),
                        user_id,
                        device_id,
                    });
                }
            }
            Ok(members)
        })
    }
}

/// What the token service names a device in LiveKit: `/get_token` hashes
/// user, device and member id (ours is `{device}_m.call`); the older
/// `/sfu/get` uses `user:device`.
fn livekit_identities(user_id: &OwnedUserId, device_id: &str) -> Vec<String> {
    use base64::Engine as _;
    use sha2::Digest as _;
    let member_id = format!("{device_id}_m.call");
    let parts = json!([user_id.as_str(), device_id, member_id]).to_string();
    let hashed = base64::engine::general_purpose::STANDARD_NO_PAD
        .encode(sha2::Sha256::digest(parts.as_bytes()));
    vec![hashed, format!("{user_id}:{device_id}")]
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn livekit_identity_matches_the_token_service() {
        // The service's own derivation (lk-jwt-service `legacy_livekit_identity_for`),
        // checked against a token it issued on the dev server.
        let user: OwnedUserId = "@alice:localhost".try_into().expect("user id");
        let ids = livekit_identities(&user, "DEV1");
        assert_eq!(ids[1], "@alice:localhost:DEV1");
        assert_eq!(ids[0], "mZs3Bdzfyu9aKeiedvidIcX5EmK9NtRPyMb9gc3ISa8");
    }
}

/// Element Call's event for sharing a device's media key with the room.
const KEYS_EVENT: &str = "io.element.call.encryption_keys";

/// Someone's media key for a room's encrypted call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallKey {
    pub user_id: OwnedUserId,
    pub device_id: String,
    pub index: u32,
    pub key: Vec<u8>,
    /// The LiveKit identities it applies to: the one its sender said it has
    /// in the call, first, then those worked out from user and device (see
    /// [`CallMember`]; apps that do not say).
    pub livekit_identities: Vec<String>,
    /// The sender's LiveKit identity as it said (`IDENTITY_FIELD`).
    pub livekit_identity: Option<String>,
}

/// Our field in the key event: the sender's own LiveKit identity, from its
/// token. Token services do not all name devices the way the dev one does
/// (matrix.org's names cannot be worked out from user and device, and its
/// tokens let nobody tell LiveKit who they are), so the name travels with
/// the key, encrypted.
const IDENTITY_FIELD: &str = "dev.morpheus.livekit_identity";

/// Media keys arriving for one room.
pub struct CallKeyUpdates {
    rx: tokio::sync::mpsc::UnboundedReceiver<CallKey>,
    _handle: EventHandlerDropGuard,
}

impl CallKeyUpdates {
    pub async fn next(&mut self) -> Option<CallKey> {
        self.rx.recv().await
    }
}

impl Session {
    /// Share this device's media key with everyone in `room_id`'s call, as
    /// an (encrypted) room event, Element Call's way.
    /// `livekit_identity` is this device's name in the call, from its token.
    pub fn send_call_key(
        &self,
        room_id: OwnedRoomId,
        key: Vec<u8>,
        livekit_identity: String,
    ) -> CoreFuture<()> {
        let session = self.clone();
        self.core.run(async move {
            use base64::Engine as _;
            let room = session.room(&room_id)?;
            let device_id = session
                .client
                .device_id()
                .ok_or(Error::NoSession)?
                .to_string();
            let sent_ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or_default();
            let content = json!({
                "keys": [{
                    "index": 0,
                    "key": base64::engine::general_purpose::STANDARD_NO_PAD.encode(&key),
                }],
                "device_id": device_id,
                "call_id": "",
                "sent_ts": sent_ts,
                IDENTITY_FIELD: livekit_identity,
            });
            room.send_raw(KEYS_EVENT, content).await?;
            Ok(())
        })
    }

    /// Media keys others share in `room_id`. Only keys that arrived
    /// encrypted count: in the clear, anyone could claim to be anyone.
    pub fn call_keys(&self, room_id: OwnedRoomId) -> CoreFuture<CallKeyUpdates> {
        let client = self.client.clone();
        self.core.run(async move {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            let handle = client.add_room_event_handler(
                &room_id,
                move |event: AnySyncTimelineEvent,
                      encryption: Option<EncryptionInfo>,
                      raw: RawEvent| {
                    let tx = tx.clone();
                    async move {
                        if event.event_type().to_string() != KEYS_EVENT || encryption.is_none() {
                            return;
                        }
                        for key in keys_in(event.sender(), &raw) {
                            let _ = tx.send(key);
                        }
                    }
                },
            );
            Ok(CallKeyUpdates {
                rx,
                _handle: client.event_handler_drop_guard(handle),
            })
        })
    }
}

fn keys_in(sender: &matrix_sdk::ruma::UserId, raw: &RawEvent) -> Vec<CallKey> {
    use base64::Engine as _;
    let Ok(event) = serde_json::from_str::<Value>(raw.0.get()) else {
        return Vec::new();
    };
    let content = &event["content"];
    let Some(device_id) = content["device_id"].as_str() else {
        return Vec::new();
    };
    let user_id = sender.to_owned();
    let said = content[IDENTITY_FIELD].as_str().map(str::to_owned);
    let mut identities: Vec<String> = said.iter().cloned().collect();
    identities.extend(livekit_identities(&user_id, device_id));
    content["keys"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let encoded = entry["key"].as_str()?;
            let key = base64::engine::general_purpose::STANDARD_NO_PAD
                .decode(encoded.trim_end_matches('='))
                .ok()?;
            Some(CallKey {
                livekit_identities: identities.clone(),
                livekit_identity: said.clone(),
                user_id: user_id.clone(),
                device_id: device_id.to_owned(),
                index: entry["index"].as_u64().unwrap_or(0) as u32,
                key,
            })
        })
        .collect()
}

/// How long a call rings before giving up (MSC4075's lifetime), in milliseconds.
pub const RING_FOR_MS: u64 = 30_000;
const RING_FOR: Duration = Duration::from_millis(RING_FOR_MS);

/// A ring, by its stable and unstable (MSC4075) names.
const RING_EVENTS: [&str; 2] = ["m.rtc.notification", "org.matrix.msc4075.rtc.notification"];
/// A declined ring (MSC4310).
const DECLINE_EVENTS: [&str; 2] = ["m.rtc.decline", "org.matrix.msc4310.rtc.decline"];

/// A call ringing this account: someone started one in a direct message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingCall {
    pub room_id: OwnedRoomId,
    pub caller: OwnedUserId,
    /// The ring (`m.rtc.notification`), which a decline answers.
    pub notification: OwnedEventId,
    /// When it stops ringing, in milliseconds since the Unix epoch.
    pub rings_until: u64,
}

/// What happens to rings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ringing {
    /// Someone is calling.
    Incoming(IncomingCall),
    /// A ring was declined: by the person called (the caller hears), or by
    /// another of this account's devices (stop ringing here too).
    Declined {
        room_id: OwnedRoomId,
        by: OwnedUserId,
        notification: OwnedEventId,
    },
}

/// Rings and declines as they arrive.
pub struct RingUpdates {
    rx: tokio::sync::mpsc::UnboundedReceiver<Ringing>,
    _handle: EventHandlerDropGuard,
}

impl RingUpdates {
    pub async fn next(&mut self) -> Option<Ringing> {
        self.rx.recv().await
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

/// Ring the other person in a direct message, if nobody else is in its call
/// yet. `membership` is the event that put this device in the call.
async fn ring_if_direct(
    room: &matrix_sdk::Room,
    user_id: &OwnedUserId,
    others_in_call: bool,
    membership: OwnedEventId,
) -> crate::Result<()> {
    if others_in_call || room.direct_targets_length() == 0 {
        return Ok(());
    }
    let mut content = RtcNotificationEventContent::new(
        MilliSecondsSinceUnixEpoch::now(),
        RING_FOR,
        NotificationType::Ring,
    );
    let callee: Vec<OwnedUserId> = room
        .direct_targets()
        .into_iter()
        .filter_map(|target| target.into_user_id())
        .filter(|target| target != user_id)
        .collect();
    content.mentions = Some(Mentions::with_user_ids(callee));
    content.relates_to = Some(Reference::new(membership));
    room.send(content).await?;
    debug!(room_id = %room.room_id(), "ringing");
    Ok(())
}

impl Session {
    /// Calls ringing this account, and declines of rings (its own and others').
    pub fn ring_updates(&self) -> CoreFuture<RingUpdates> {
        let client = self.client.clone();
        let own_user = self.user_id();
        self.core.run(async move {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            // One raw handler: rings and declines go out under unstable names
            // (ruma sends `org.matrix.msc4075.rtc.notification`), and typed
            // handlers only hear the stable ones.
            let handle = client.add_event_handler(
                move |event: AnySyncTimelineEvent, room: matrix_sdk::Room, raw: RawEvent| {
                    let tx = tx.clone();
                    let own_user = own_user.clone();
                    async move {
                        if room.state() != matrix_sdk::RoomState::Joined {
                            return;
                        }
                        let kind = event.event_type().to_string();
                        let Some(content) = serde_json::from_str::<Value>(raw.0.get())
                            .ok()
                            .map(|event| event["content"].clone())
                        else {
                            return;
                        };
                        let sent: u64 = event.origin_server_ts().get().into();
                        if RING_EVENTS.contains(&kind.as_str()) {
                            let Ok(ring) =
                                serde_json::from_value::<RtcNotificationEventContent>(content)
                            else {
                                return;
                            };
                            let rings_until: u64 = ring
                                .expiration_ts(event.origin_server_ts(), None)
                                .get()
                                .into();
                            if own_user.as_deref() == Some(event.sender())
                                || ring.notification_type != NotificationType::Ring
                                || rings_until <= now_ms()
                            {
                                return;
                            }
                            let _ = tx.send(Ringing::Incoming(IncomingCall {
                                room_id: room.room_id().to_owned(),
                                caller: event.sender().to_owned(),
                                notification: event.event_id().to_owned(),
                                rings_until,
                            }));
                        } else if DECLINE_EVENTS.contains(&kind.as_str()) {
                            // Old declines (catching up after a while away) mean nothing now.
                            if now_ms().saturating_sub(sent) > RING_FOR.as_millis() as u64 {
                                return;
                            }
                            let Ok(decline) =
                                serde_json::from_value::<RtcDeclineEventContent>(content)
                            else {
                                return;
                            };
                            let _ = tx.send(Ringing::Declined {
                                room_id: room.room_id().to_owned(),
                                by: event.sender().to_owned(),
                                notification: decline.relates_to.event_id,
                            });
                        }
                    }
                },
            );
            Ok(RingUpdates {
                rx,
                _handle: client.event_handler_drop_guard(handle),
            })
        })
    }

    /// Decline a call ringing this account (the caller hears, and this
    /// account's other devices stop ringing).
    pub fn decline_call(&self, room_id: OwnedRoomId, notification: OwnedEventId) -> CoreFuture<()> {
        let session = self.clone();
        self.core.run(async move {
            let room = session.room(&room_id)?;
            room.send(RtcDeclineEventContent::new(notification)).await?;
            Ok(())
        })
    }
}

/// Whether an event of this type is a ring.
pub(crate) fn is_ring(event_type: &str) -> bool {
    RING_EVENTS.contains(&event_type)
}
