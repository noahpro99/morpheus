//! What the profile card shows about another user: rooms in common, your
//! private note, their role here, and reporting.

use matrix_sdk::ruma::{
    OwnedRoomId, OwnedUserId,
    api::client::reporting::report_user,
    events::{AnyGlobalAccountDataEventContent, GlobalAccountDataEventType},
    serde::Raw,
};
use serde_json::Value;

use crate::{CoreFuture, MemberRole, Session};

const NOTES_TYPE: &str = "dev.morpheus.notes";
const NICKNAMES_TYPE: &str = "dev.morpheus.nicknames";

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

/// A user-id-keyed map of strings kept in private account data.
async fn read_map(
    client: &matrix_sdk::Client,
    kind: &str,
) -> crate::Result<serde_json::Map<String, Value>> {
    let raw = client
        .account()
        .account_data_raw(GlobalAccountDataEventType::from(kind))
        .await?;
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
