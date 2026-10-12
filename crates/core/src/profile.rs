//! User profiles: display name and avatar for the profile modal and the
//! sidebar footer.

use matrix_sdk::ruma::{
    OwnedUserId,
    profile::{AvatarUrl, DisplayName},
};

use crate::{CoreFuture, Error, MediaRef, Session};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub user_id: OwnedUserId,
    /// Display name, falling back to the Matrix ID's local part.
    pub name: String,
    pub avatar: Option<MediaRef>,
}

impl Profile {
    fn fallback(user_id: OwnedUserId) -> Self {
        let name = user_id.localpart().to_owned();
        Self {
            user_id,
            name,
            avatar: None,
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
            Ok(profile)
        })
    }
}
