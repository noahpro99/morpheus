//! User profiles: display name and avatar for the profile modal and the
//! sidebar footer.

use matrix_sdk::ruma::{
    OwnedUserId,
    api::client::presence::{get_presence, set_presence},
    presence::PresenceState,
    profile::{AvatarUrl, DisplayName},
};

use crate::{CoreFuture, Error, MediaRef, Session};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub user_id: OwnedUserId,
    /// Display name, falling back to the Matrix ID's local part.
    pub name: String,
    pub avatar: Option<MediaRef>,
    /// Presence status message, when the server shares presence.
    pub status: Option<String>,
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
    pub fn profile(&self, user_id: OwnedUserId) -> CoreFuture<Profile> {
        let client = self.client.clone();
        self.core.run(async move {
            let response = client.account().fetch_user_profile_of(&user_id).await?;
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
            // Presence may be disabled server-side; that is not an error here.
            profile.status = client
                .send(get_presence::v3::Request::new(profile.user_id.clone()))
                .await
                .ok()
                .and_then(|presence| presence.status_msg)
                .filter(|s| !s.trim().is_empty());
            Ok(profile)
        })
    }
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
            Ok(())
        })
    }

    /// Upload an image file and make it the signed-in user's avatar.
    pub fn set_avatar(&self, path: std::path::PathBuf) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            let bytes = tokio::fs::read(&path).await?;
            client.account().upload_avatar(&mime, bytes).await?;
            Ok(())
        })
    }

    /// Set (or clear, with `None`) the presence status message other users
    /// see next to this account. Presence is set to online alongside it.
    pub fn set_status(&self, status: Option<String>) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let user_id = client.user_id().ok_or(Error::NoSession)?.to_owned();
            let mut request = set_presence::v3::Request::new(user_id, PresenceState::Online);
            request.status_msg = status
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty());
            client.send(request).await?;
            Ok(())
        })
    }
}
