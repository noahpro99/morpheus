//! Push notifications through UnifiedPush (Sunup, ntfy, …). The phone's
//! push app hands the app an endpoint URL; this registers a Matrix pusher
//! so the homeserver sends notifications there, through a Matrix gateway:
//! the push server's own when it has one (ntfy), else UnifiedPush's public
//! gateway (Sunup and other Web Push servers).

use matrix_sdk::ruma::{
    api::client::push::{PusherIds, PusherInit, PusherKind, get_pushers},
    push::{HttpPusherData, PushFormat},
};

use crate::{CoreFuture, Error, Session};

/// The app id the homeserver files our pushers under.
pub const PUSH_APP_ID: &str = "dev.morpheus.mobile";

/// UnifiedPush's public Matrix gateway, for push servers without their own.
pub const DEFAULT_PUSH_GATEWAY: &str =
    "https://matrix.gateway.unifiedpush.org/_matrix/push/v1/notify";

const GATEWAY_PATH: &str = "/_matrix/push/v1/notify";

/// A pusher registered for this account (any of its devices).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushRegistration {
    /// The push endpoint the homeserver sends to.
    pub pushkey: String,
    pub app_id: String,
    /// The Matrix gateway URL, for HTTP pushers.
    pub gateway: String,
}

impl Session {
    /// Have the homeserver send notifications to `endpoint`; returns the
    /// gateway it goes through. `device_name` labels it on the server.
    pub fn register_push(&self, endpoint: String, device_name: String) -> CoreFuture<String> {
        let client = self.client.clone();
        self.core.run(async move {
            let gateway = discover_gateway(client.http_client(), &endpoint).await;
            let mut data = HttpPusherData::new(gateway.clone());
            // The push carries ids only; the message itself stays on the server.
            data.format = Some(PushFormat::EventIdOnly);
            let pusher = PusherInit {
                ids: PusherIds::new(endpoint, PUSH_APP_ID.to_owned()),
                kind: PusherKind::Http(data),
                app_display_name: "Morpheus".to_owned(),
                device_display_name: device_name,
                profile_tag: None,
                lang: "en".to_owned(),
            };
            client.pusher().set(pusher.into(), false).await?;
            Ok(gateway)
        })
    }

    /// Stop sending notifications to `endpoint`.
    pub fn unregister_push(&self, endpoint: String) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            client
                .pusher()
                .delete(PusherIds::new(endpoint, PUSH_APP_ID.to_owned()))
                .await?;
            Ok(())
        })
    }

    /// Every pusher on the account.
    pub fn push_registrations(&self) -> CoreFuture<Vec<PushRegistration>> {
        let client = self.client.clone();
        self.core.run(async move {
            let response = client
                .send(get_pushers::v3::Request::new())
                .await
                .map_err(|err| Error::Other(format!("listing pushers: {err}")))?;
            Ok(response
                .pushers
                .into_iter()
                .map(|pusher| PushRegistration {
                    gateway: match &pusher.kind {
                        PusherKind::Http(data) => data.url.clone(),
                        _ => String::new(),
                    },
                    pushkey: pusher.ids.pushkey,
                    app_id: pusher.ids.app_id,
                })
                .collect())
        })
    }
}

/// The push server's own Matrix gateway if it answers as one at the
/// standard path (`{"unifiedpush":{"gateway":"matrix"}}`), else the public one.
async fn discover_gateway(http: &matrix_sdk::reqwest::Client, endpoint: &str) -> String {
    let Some(origin) = origin_of(endpoint) else {
        return DEFAULT_PUSH_GATEWAY.to_owned();
    };
    let candidate = format!("{origin}{GATEWAY_PATH}");
    let answer = http
        .get(&candidate)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await;
    let is_gateway = match answer {
        Ok(response) if response.status().is_success() => response
            .text()
            .await
            .ok()
            .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok())
            .is_some_and(|body| {
                body.pointer("/unifiedpush/gateway")
                    .and_then(|g| g.as_str())
                    == Some("matrix")
            }),
        _ => false,
    };
    if is_gateway {
        candidate
    } else {
        DEFAULT_PUSH_GATEWAY.to_owned()
    }
}

/// `scheme://host[:port]` of a URL.
fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split('/').next().filter(|host| !host.is_empty())?;
    Some(format!("{scheme}://{host}"))
}

impl Session {
    /// `(room id, name)` for every joined room, for push notifications that
    /// arrive with ids only.
    pub fn room_names(&self) -> CoreFuture<Vec<(String, String)>> {
        let client = self.client.clone();
        self.core.run(async move {
            Ok(client
                .joined_rooms()
                .into_iter()
                .map(|room| {
                    let name = room
                        .cached_display_name()
                        .map(|name| name.to_string())
                        .unwrap_or_else(|| room.room_id().to_string());
                    (room.room_id().to_string(), name)
                })
                .collect())
        })
    }
}
#[cfg(test)]
mod tests {
    use super::origin_of;

    #[test]
    fn origins() {
        assert_eq!(
            origin_of("https://ntfy.sh/upAbc?x=1").as_deref(),
            Some("https://ntfy.sh")
        );
        assert_eq!(
            origin_of("http://127.0.0.1:8080/up").as_deref(),
            Some("http://127.0.0.1:8080")
        );
        assert_eq!(origin_of("nonsense"), None);
    }
}
