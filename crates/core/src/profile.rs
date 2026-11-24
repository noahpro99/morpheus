//! User profiles: display name and avatar for the profile modal and the
//! sidebar footer.

use matrix_sdk::ruma::{
    OwnedUserId,
    api::client::presence::{get_presence, set_presence},
    presence::PresenceState,
    profile::{AvatarUrl, DisplayName, ProfileFieldName, ProfileFieldValue},
};

use crate::{CoreFuture, Error, MediaRef, Session};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub user_id: OwnedUserId,
    /// Display name, falling back to the Matrix ID's local part.
    pub name: String,
    pub avatar: Option<MediaRef>,
    /// The status message: the presence one when the server keeps presence,
    /// else Morpheus's own profile field ([`STATUS_FIELD`]).
    pub status: Option<String>,
}

/// Morpheus's own profile field for the status message. Servers that turn
/// presence off (matrix.org) drop a presence status, so it is also kept here
/// (Matrix 1.16 profile fields). Other clients do not show it.
pub const STATUS_FIELD: &str = "dev.morpheus.status";

/// The status to show: the presence one when there is one, else the profile
/// field's. Blank counts as none.
pub fn combine_status(presence: Option<String>, field: Option<String>) -> Option<String> {
    let kept = |s: String| (!s.trim().is_empty()).then_some(s);
    presence.and_then(kept).or_else(|| field.and_then(kept))
}

/// Whether the server lets us set [`STATUS_FIELD`] (the `m.profile_fields`
/// capability, which the SDK keeps for the session).
async fn profile_field_supported(client: &matrix_sdk::Client) -> bool {
    client
        .homeserver_capabilities()
        .extended_profile_fields()
        .await
        .is_ok_and(|caps| caps.can_set_field(&ProfileFieldName::from(STATUS_FIELD)))
}

fn is_not_found(err: &matrix_sdk::Error) -> bool {
    err.as_client_api_error()
        .is_some_and(|e| e.status_code.as_u16() == 404)
}

impl Profile {
    fn fallback(user_id: OwnedUserId) -> Self {
        let name = user_id.localpart().to_owned();
        Self {
            user_id,
            name,
            avatar: None,
            status: None,
        }
    }
}

impl Session {
    /// The signed-in user's own profile, from the homeserver.
    pub fn own_profile(&self) -> CoreFuture<Profile> {
        match self.user_id() {
            Some(user_id) => self.profile(user_id),
            None => Box::pin(async { Err(Error::NoSession) }),
        }
    }

    /// Any user's global profile. Missing fields fall back to the Matrix ID.
    ///
    /// Asked for together, a person's profile is fetched once (#115): your
    /// own member event in every room that syncs in asks for yours again,
    /// and at startup that was dozens at once, which the server answered
    /// with 429s (Synapse's `rc_profile`) and matrix-sdk retried, so they
    /// all took a second or more and ended together. A profile fetched in
    /// the last [`PROFILE_FRESH`] answers those asking meanwhile; changing
    /// your own forgets yours.
    pub fn profile(&self, user_id: OwnedUserId) -> CoreFuture<Profile> {
        let client = self.client.clone();
        self.core.run(async move {
            let key = profile_key(&client, &user_id);
            let slot = fetched_profiles()
                .lock()
                .map(|mut known| known.entry(key).or_default().clone())
                .unwrap_or_default();
            // One fetch at a time per person: those asking meanwhile wait
            // for it and take what it found.
            let mut kept = slot.lock().await;
            if let Some((at, profile)) = kept.as_ref()
                && at.elapsed() < PROFILE_FRESH
            {
                return Ok(profile.clone());
            }
            let profile = fetch_profile(&client, user_id).await?;
            *kept = Some((crate::rt::Instant::now(), profile.clone()));
            Ok(profile)
        })
    }
}

/// How long a fetched profile answers those asking for it again.
const PROFILE_FRESH: std::time::Duration = std::time::Duration::from_secs(3);

type ProfileSlot = std::sync::Arc<tokio::sync::Mutex<Option<(crate::rt::Instant, Profile)>>>;

/// The last profile fetched of each person, by whose session asked
/// ([`profile_key`]), each behind the lock its fetch holds.
fn fetched_profiles() -> &'static std::sync::Mutex<std::collections::HashMap<String, ProfileSlot>> {
    static FETCHED: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, ProfileSlot>>,
    > = std::sync::OnceLock::new();
    FETCHED.get_or_init(Default::default)
}

fn profile_key(client: &matrix_sdk::Client, user_id: &OwnedUserId) -> String {
    let me = client.user_id().map(|me| me.to_string()).unwrap_or_default();
    format!("{me} {user_id}")
}

/// Your own profile is fetched afresh next time (you changed it).
fn forget_own_profile(client: &matrix_sdk::Client) {
    if let Some(me) = client.user_id()
        && let Ok(mut known) = fetched_profiles().lock()
    {
        known.remove(&profile_key(client, &me.to_owned()));
    }
}

/// `user_id`'s profile and presence status from the server.
async fn fetch_profile(client: &matrix_sdk::Client, user_id: OwnedUserId) -> crate::Result<Profile> {
    // The profile and the presence together, not one after the other:
    // each is a round trip.
    let account = client.account();
    let (response, presence) = futures_util::join!(
        account.fetch_user_profile_of(&user_id),
        std::future::IntoFuture::into_future(
            client.send(get_presence::v3::Request::new(user_id.clone()))
        ),
    );
    let response = response?;
    let mut profile = Profile::fallback(user_id);
    if let Some(name) = response
        .data
        .get_static::<DisplayName>()
        .ok()
        .flatten()
        .filter(|n| !n.trim().is_empty())
    {
        profile.name = name;
    }
    profile.avatar = response
        .data
        .get_static::<AvatarUrl>()
        .ok()
        .flatten()
        .map(MediaRef::from_mxc);
    // Presence may be disabled server-side; that is not an error here. The
    // profile field comes with the profile already fetched: no extra request.
    profile.status = combine_status(
        presence.ok().and_then(|presence| presence.status_msg),
        response
            .data
            .get(STATUS_FIELD)
            .and_then(|v| v.as_str())
            .map(str::to_owned),
    );
    Ok(profile)
}

impl Session {
    /// Change the signed-in user's display name.
    pub fn set_display_name(&self, name: String) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let name = name.trim().to_owned();
            client
                .account()
                .set_display_name((!name.is_empty()).then_some(name.as_str()))
                .await?;
            forget_own_profile(&client);
            Ok(())
        })
    }

    /// Upload an image file and make it the signed-in user's avatar.
    pub fn set_avatar(&self, path: std::path::PathBuf) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            let bytes = crate::fs::nonblocking::read(&path).await?;
            client.account().upload_avatar(&mime, bytes).await?;
            forget_own_profile(&client);
            Ok(())
        })
    }

    /// The page where the homeserver lets this account be managed (its OIDC
    /// provider's account page), when it advertises one.
    pub fn account_management_url(&self) -> CoreFuture<Option<String>> {
        let client = self.client.clone();
        self.core.run(async move {
            Ok(client
                .oauth()
                .server_metadata()
                .await
                .ok()
                .and_then(|metadata| metadata.account_management_uri)
                .map(|url| url.to_string()))
        })
    }

    /// Remove the signed-in user's avatar.
    pub fn remove_avatar(&self) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            client.account().set_avatar_url(None).await?;
            forget_own_profile(&client);
            Ok(())
        })
    }

    /// Only the signed-in user's status message (presence, else the profile
    /// field): what the status feed needs, so a failing profile request cannot
    /// stop it. A server that answers "not found" or "forbidden" has no
    /// readable status (`None`); any other failure (network, rate limit,
    /// server error) is an error, because then a status typed by hand cannot
    /// be ruled out.
    pub fn own_status(&self) -> CoreFuture<Option<String>> {
        let client = self.client.clone();
        self.core.run(async move {
            let user_id = client.user_id().ok_or(Error::NoSession)?.to_owned();
            let presence = match client
                .send(get_presence::v3::Request::new(user_id.clone()))
                .await
            {
                Ok(presence) => presence.status_msg,
                Err(err)
                    if err
                        .as_client_api_error()
                        .is_some_and(|e| presence_unreadable(e.status_code.as_u16())) =>
                {
                    None
                }
                Err(err) => return Err(err.into()),
            };
            if presence.as_deref().is_some_and(|s| !s.trim().is_empty())
                || !profile_field_supported(&client).await
            {
                return Ok(combine_status(presence, None));
            }
            let field = match client
                .account()
                .fetch_profile_field_of(user_id, ProfileFieldName::from(STATUS_FIELD))
                .await
            {
                Ok(value) => value.and_then(|v| v.value().as_str().map(str::to_owned)),
                Err(err) if is_not_found(&err) => None,
                Err(err) => return Err(err.into()),
            };
            Ok(combine_status(presence, field))
        })
    }

    /// Set (or clear, with `None`) the status message other users see next to
    /// this account: in presence (where the server keeps it) and in the profile
    /// field (where the server has profile fields, for servers that turn
    /// presence off). Presence is set to online alongside it. When the server
    /// keeps it in neither place, that is an error ([`Error::StatusUnavailable`]).
    pub fn set_status(&self, status: Option<String>) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let user_id = client.user_id().ok_or(Error::NoSession)?.to_owned();
            let text = status
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty());
            let mut request = set_presence::v3::Request::new(user_id, PresenceState::Online);
            request.status_msg = text.clone();
            let presence = client.send(request).await;
            let in_profile = profile_field_supported(&client).await;
            let result = if in_profile {
                let account = client.account();
                let field = match &text {
                    Some(text) => {
                        account
                            .set_profile_field(ProfileFieldValue::new(
                                STATUS_FIELD,
                                serde_json::Value::String(text.clone()),
                            )?)
                            .await
                    }
                    None => match account
                        .delete_profile_field(ProfileFieldName::from(STATUS_FIELD))
                        .await
                    {
                        Err(err) if is_not_found(&err) => Ok(()),
                        other => other,
                    },
                };
                // Kept in the profile: a presence refusal does not matter.
                field.map_err(Error::from)
            } else {
                presence.map(|_| ()).map_err(Error::from)
            };
            forget_own_profile(&client);
            result?;
            // Presence alone: a server with it off accepts and drops the
            // status, so read it back.
            if let Some(text) = text
                && !in_profile
            {
                let user_id = client.user_id().ok_or(Error::NoSession)?.to_owned();
                let kept = client
                    .send(get_presence::v3::Request::new(user_id))
                    .await
                    .ok()
                    .and_then(|p| p.status_msg);
                if kept.as_deref() != Some(text.as_str()) {
                    return Err(Error::StatusUnavailable);
                }
            }
            Ok(())
        })
    }
}

/// The answers with which a server says presence is not there to read (not found:
/// never set; forbidden: switched off).
pub fn presence_unreadable(status: u16) -> bool {
    matches!(status, 403 | 404)
}

#[cfg(test)]
mod presence_tests {
    use super::{combine_status, presence_unreadable};

    #[test]
    fn the_presence_status_wins_over_the_profile_field_and_blank_is_none() {
        let some = |s: &str| Some(s.to_owned());
        assert_eq!(combine_status(some("A"), some("B")), some("A"));
        assert_eq!(combine_status(None, some("B")), some("B"));
        assert_eq!(combine_status(some("  "), some("B")), some("B"));
        assert_eq!(combine_status(some(""), some(" ")), None);
        assert_eq!(combine_status(None, None), None);
    }

    #[test]
    fn only_not_found_and_forbidden_mean_no_status() {
        assert!(presence_unreadable(404));
        assert!(presence_unreadable(403));
        assert!(!presence_unreadable(429));
        assert!(!presence_unreadable(500));
        assert!(!presence_unreadable(502));
    }
}
