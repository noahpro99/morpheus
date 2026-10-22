//! What the profile card shows about another user: rooms in common, your
//! private note, their role here, and reporting.

use matrix_sdk::ruma::{
    OwnedRoomId, OwnedUserId,
    api::client::reporting::report_user,
    events::{AnyGlobalAccountDataEventContent, GlobalAccountDataEventType},
    serde::Raw,
};
use serde_json::Value;

use crate::{CoreFuture, Error, MemberRole, Session};

const NOTES_TYPE: &str = "dev.morpheus.notes";
const NICKNAMES_TYPE: &str = "dev.morpheus.nicknames";
const FRIENDS_TYPE: &str = "dev.morpheus.friends";

impl Session {
    /// Joined rooms (not spaces) that both of you are in, by name.
    pub fn mutual_rooms(&self, user_id: OwnedUserId) -> CoreFuture<Vec<(OwnedRoomId, String)>> {
        let client = self.client.clone();
        self.core.run(async move {
            let mut out = Vec::new();
            for room in client.joined_rooms() {
                if room.is_space() {
                    continue;
                }
                if let Ok(Some(member)) = room.get_member_no_sync(&user_id).await
                    && member.membership()
                        == &matrix_sdk::ruma::events::room::member::MembershipState::Join
                {
                    let name = room
                        .cached_display_name()
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| room.room_id().to_string());
                    out.push((room.room_id().to_owned(), name));
                }
            }
            out.sort_by_key(|entry| entry.1.to_lowercase());
            Ok(out)
        })
    }

    /// The user's role in a room, if they are a member.
    pub fn member_role(
        &self,
        room_id: OwnedRoomId,
        user_id: OwnedUserId,
    ) -> CoreFuture<Option<MemberRole>> {
        let room = self.room(&room_id);
        self.core.run(async move {
            let member = room?.get_member_no_sync(&user_id).await?;
            Ok(member.map(|m| MemberRole::from_sdk(m.suggested_role_for_power_level())))
        })
    }

    /// Your private note about a user (account data, synced across devices).
    pub fn note(&self, user_id: OwnedUserId) -> CoreFuture<String> {
        let client = self.client.clone();
        self.core.run(async move {
            let notes = read_notes(&client).await?;
            Ok(notes
                .get(user_id.as_str())
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned())
        })
    }

    pub fn set_note(&self, user_id: OwnedUserId, note: String) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let mut notes = read_notes(&client).await?;
            let note = note.trim();
            if note.is_empty() {
                notes.remove(user_id.as_str());
            } else {
                notes.insert(user_id.to_string(), Value::String(note.to_owned()));
            }
            let raw: Raw<AnyGlobalAccountDataEventContent> =
                Raw::from_json(serde_json::value::to_raw_value(&Value::Object(notes))?);
            client
                .account()
                .set_account_data_raw(GlobalAccountDataEventType::from(NOTES_TYPE), raw)
                .await?;
            Ok(())
        })
    }

    /// Report a user to the homeserver admins (MSC4260).
    pub fn report_user(&self, user_id: OwnedUserId, reason: String) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            client
                .send(report_user::v3::Request::new(
                    user_id,
                    reason.trim().to_owned(),
                ))
                .await?;
            Ok(())
        })
    }

    /// Whether you have blocked (ignored) this user.
    pub fn is_blocked(&self, user_id: OwnedUserId) -> CoreFuture<bool> {
        let client = self.client.clone();
        self.core
            .run(async move { Ok(crate::members::is_ignored(&client, &user_id).await) })
    }
}

async fn read_notes(client: &matrix_sdk::Client) -> crate::Result<serde_json::Map<String, Value>> {
    read_map(client, NOTES_TYPE).await
}

/// Account data of `kind`, from the server: the local copy only catches up
/// when sync delivers a change, so right after a write (or a restart before
/// the first sync) it is stale, and a read-modify-write would drop entries.
/// Offline, the local copy.
pub(crate) async fn read_account_data(
    client: &matrix_sdk::Client,
    kind: &str,
) -> crate::Result<Option<Raw<AnyGlobalAccountDataEventContent>>> {
    let kind = GlobalAccountDataEventType::from(kind);
    match client.account().fetch_account_data(kind.clone()).await {
        Ok(raw) => Ok(raw),
        Err(err) => {
            tracing::debug!(%err, %kind, "account data from the local store");
            Ok(client.account().account_data_raw(kind).await?)
        }
    }
}

/// A user-id-keyed map of strings kept in private account data.
async fn read_map(
    client: &matrix_sdk::Client,
    kind: &str,
) -> crate::Result<serde_json::Map<String, Value>> {
    let raw = read_account_data(client, kind).await?;
    Ok(raw
        .and_then(|raw| raw.deserialize_as::<Value>().ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default())
}

impl Session {
    /// Your nicknames for people, by user id (account data, synced across
    /// your devices, seen by nobody else).
    pub fn nicknames(&self) -> CoreFuture<std::collections::HashMap<String, String>> {
        let client = self.client.clone();
        self.core.run(async move {
            Ok(read_map(&client, NICKNAMES_TYPE)
                .await?
                .into_iter()
                .filter_map(|(user, name)| Some((user, name.as_str()?.to_owned())))
                .collect())
        })
    }

    /// Set (or, empty, clear) your nickname for a user.
    pub fn set_nickname(&self, user_id: OwnedUserId, nickname: String) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let mut names = read_map(&client, NICKNAMES_TYPE).await?;
            let nickname = nickname.trim();
            if nickname.is_empty() {
                names.remove(user_id.as_str());
            } else {
                names.insert(user_id.to_string(), Value::String(nickname.to_owned()));
            }
            let raw: Raw<AnyGlobalAccountDataEventContent> =
                Raw::from_json(serde_json::value::to_raw_value(&Value::Object(names))?);
            client
                .account()
                .set_account_data_raw(GlobalAccountDataEventType::from(NICKNAMES_TYPE), raw)
                .await?;
            Ok(())
        })
    }
}

impl Session {
    /// Your friends (a private list in account data; Matrix has no
    /// friendship of its own), in the order you added them.
    pub fn friends(&self) -> CoreFuture<Vec<OwnedUserId>> {
        let client = self.client.clone();
        self.core
            .run(async move { Ok(read_friends(&client).await) })
    }

    /// Who someone typed means, as Discord's "Add Friend" takes it: a bare
    /// username is someone on this account's server; `name:server` or a full
    /// Matrix ID anyone. Errors when the server knows no such person.
    pub fn find_user(&self, typed: String) -> CoreFuture<OwnedUserId> {
        let client = self.client.clone();
        self.core.run(async move {
            let typed = typed.trim().trim_start_matches('@');
            if typed.is_empty() {
                return Err(Error::Other("Type a username or a Matrix ID.".into()));
            }
            let full = if typed.contains(':') {
                format!("@{typed}")
            } else {
                let server = client.user_id().map(|me| me.server_name().to_owned());
                match server {
                    Some(server) => format!("@{typed}:{server}"),
                    None => return Err(Error::NoSession),
                }
            };
            let not_found = || Error::Other(format!("No one is called {full}."));
            let user_id = OwnedUserId::try_from(full.to_lowercase()).map_err(|_| not_found())?;
            match client.account().fetch_user_profile_of(&user_id).await {
                Ok(_) => Ok(user_id),
                Err(err)
                    if err
                        .as_client_api_error()
                        .is_some_and(|e| e.status_code.as_u16() == 404) =>
                {
                    Err(not_found())
                }
                Err(err) => Err(err.into()),
            }
        })
    }

    /// Add (`true`) or remove a friend.
    pub fn set_friend(&self, user_id: OwnedUserId, friend: bool) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let mut friends = read_friends(&client).await;
            friends.retain(|id| *id != user_id);
            if friend {
                friends.push(user_id);
            }
            let list: Vec<Value> = friends
                .iter()
                .map(|id| Value::String(id.to_string()))
                .collect();
            let raw: Raw<AnyGlobalAccountDataEventContent> = Raw::from_json(
                serde_json::value::to_raw_value(&serde_json::json!({ "users": list }))?,
            );
            client
                .account()
                .set_account_data_raw(GlobalAccountDataEventType::from(FRIENDS_TYPE), raw)
                .await?;
            Ok(())
        })
    }
}

async fn read_friends(client: &matrix_sdk::Client) -> Vec<OwnedUserId> {
    let raw = read_account_data(client, FRIENDS_TYPE).await.ok().flatten();
    raw.and_then(|raw| raw.deserialize_as::<Value>().ok())
        .and_then(|value| value.get("users")?.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|id| id.as_str()?.parse().ok())
        .collect()
}
