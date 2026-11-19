//! A server's p.map(MediaRef::from_mxc_uri)blic room directory (`/publicRooms`): what "Discover" lists.

use matrix_sdk::ruma::{
    api::client::directory::get_public_rooms_filtered, directory::Filter, room::RoomType,
};

use crate::{CoreFuture, MediaRef, OwnedRoomId, Session};

/// One room (or space) of a server's directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicRoom {
    pub room_id: OwnedRoomId,
    pub name: Option<String>,
    pub topic: Option<String>,
    /// `#alias:server`, when it has one.
    pub alias: Option<String>,
    pub avatar: Option<MediaRef>,
    pub members: u64,
    pub is_space: bool,
}

/// One page of a directory.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PublicRooms {
    pub rooms: Vec<PublicRoom>,
    /// Pass back as `since` for the next page; `None` at the end.
    pub next: Option<String>,
}

impl Session {
    /// A page of `server`'s public rooms (this account's own server when
    /// `None`) matching `term` (everything when blank), the biggest first as
    /// the server orders them. `since`: the previous page's `next`.
    pub fn public_rooms(
        &self,
        server: Option<String>,
        term: String,
        since: Option<String>,
    ) -> CoreFuture<PublicRooms> {
        let client = self.client.clone();
        self.core.run(async move {
            // Test setups: a directory reached at its own address (a second dev
            // homeserver the first cannot federate with), asked without signing in.
            if let Some(url) = server.as_deref().and_then(test_directory_url) {
                return test_directory_page(&url, &term, since).await;
            }
            let mut request = get_public_rooms_filtered::v3::Request::new();
            request.server = match server.filter(|s| !s.trim().is_empty()) {
                Some(server) => Some(
                    server
                        .trim()
                        .try_into()
                        .map_err(|_| crate::Error::Other(format!("not a server name: {server}")))?,
                ),
                None => None,
            };
            request.limit = Some(30u32.into());
            request.since = since;
            let mut filter = Filter::new();
            let term = term.trim();
            if !term.is_empty() {
                filter.generic_search_term = Some(term.to_owned());
            }
            request.filter = filter;
            let response = client.public_rooms_filtered(request).await?;
            Ok(page_of(response))
        })
    }
}

/// `MORPHEUS_TEST_DIRECTORY_URLS=name=url,name=url`: directories the e2e
/// tests reach at an address of their own.
fn test_directory_url(server: &str) -> Option<String> {
    std::env::var("MORPHEUS_TEST_DIRECTORY_URLS")
        .ok()?
        .split(',')
        .filter_map(|pair| pair.split_once('='))
        .find(|(name, _)| *name == server)
        .map(|(_, url)| url.to_owned())
}

fn page_of(response: get_public_rooms_filtered::v3::Response) -> PublicRooms {
    PublicRooms {
        rooms: response
            .chunk
            .into_iter()
            .map(|room| PublicRoom {
                room_id: room.room_id,
                name: room.name,
                topic: room.topic,
                alias: room.canonical_alias.map(|a| a.to_string()),
                avatar: room.avatar_url.map(MediaRef::from_mxc),
                members: room.num_joined_members.into(),
                is_space: room.room_type == Some(RoomType::Space),
            })
            .collect(),
        next: response.next_batch,
    }
}

/// A page of the directory at `url`, asked without signing in (the server
/// must allow that); the answer read by hand.
async fn test_directory_page(
    url: &str,
    term: &str,
    since: Option<String>,
) -> crate::Result<PublicRooms> {
    let other = |e: &dyn std::fmt::Display| crate::Error::Other(e.to_string());
    let body = serde_json::json!({
        "limit": 30,
        "since": since,
        "filter": { "generic_search_term": term.trim() },
    });
    let base = url.trim_end_matches('/');
    let http = matrix_sdk::reqwest::Client::new();
    // The POST (search) needs a login; a guest will do.
    let guest = http
        .post(format!("{base}/_matrix/client/v3/register?kind=guest"))
        .header("Content-Type", "application/json")
        .body("{}")
        .send()
        .await
        .map_err(|e| other(&e))?
        .text()
        .await
        .map_err(|e| other(&e))?;
    let guest: serde_json::Value = serde_json::from_str(&guest).map_err(|e| other(&e))?;
    let token = guest["access_token"].as_str().unwrap_or_default().to_owned();
    let answer = http
        .post(format!("{base}/_matrix/client/v3/publicRooms"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .map_err(|e| other(&e))?;
    let ok = answer.status().is_success();
    let text = answer.text().await.map_err(|e| other(&e))?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| other(&e))?;
    if !ok {
        return Err(other(&json["error"].as_str().unwrap_or("the directory refused")));
    }
    let rooms = json["chunk"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|room| {
            let text = |key: &str| room[key].as_str().map(str::to_owned);
            Some(PublicRoom {
                room_id: room["room_id"].as_str()?.try_into().ok()?,
                name: text("name"),
                topic: text("topic"),
                alias: text("canonical_alias"),
                avatar: room["avatar_url"]
                    .as_str()
                    .and_then(|u| u.try_into().ok())
                    .map(MediaRef::from_mxc),
                members: room["num_joined_members"].as_u64().unwrap_or(0),
                is_space: room["room_type"].as_str() == Some("m.space"),
            })
        })
        .collect();
    Ok(PublicRooms {
        rooms,
        next: json["next_batch"].as_str().map(str::to_owned),
    })
}
