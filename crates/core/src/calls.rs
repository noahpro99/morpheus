//! Calls over MatrixRTC, the way Element Call does them: the homeserver
//! names a LiveKit service (MSC4143 transports, else the client
//! well-known), which swaps a Matrix OpenID token for a LiveKit token, and
//! each device in the call publishes an `m.call.member` state event so
//! everyone in the room sees who is there. The media itself is LiveKit's
//! business (`morpheus-call`).

use std::time::Duration;

use matrix_sdk::ruma::{
    OwnedRoomId, OwnedUserId,
    api::client::account::request_openid_token,
    events::call::member::{
        ActiveFocus, ActiveLivekitFocus, Application, CallApplicationContent,
        CallMemberEventContent, CallMemberStateKey, CallScope, Focus, LivekitFocus,
    },
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
            room.send_state_event_for_key(&member_state_key(user_id, device_id.as_str()), content)
                .await?;
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
