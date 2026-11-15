//! A changed name or picture, yours or anyone's, reaches everything that
//! shows it: the account, the member list, timelines, calls, the lists.

use gpui::Context;
use morpheus_core::{ProfileChange, Session};

use crate::shell::Shell;

impl Shell {
    /// Listens for profile changes that sync in (others' and your own made
    /// from another client) for as long as the shell lives.
    pub fn start_profile_sync(&mut self, session: &Session, cx: &mut Context<Self>) {
        let listening = session.profile_changes();
        cx.spawn(async move |this, cx| {
            let Ok(mut updates) = listening.await else {
                return;
            };
            while let Some(change) = updates.next().await {
                if this
                    .update(cx, |this, cx| this.profile_changed(&change, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    /// Redraws what shows `change.user_id`'s name and picture in `change.room_id`.
    pub fn profile_changed(&mut self, change: &ProfileChange, cx: &mut Context<Self>) {
        let own = self.session.as_ref().and_then(|s| s.user_id());
        if own.as_deref() == Some(change.user_id.as_ref()) {
            self.reload_own_profile(cx);
        }
        if self.selected.as_ref() == Some(&change.room_id) {
            self.refresh_members(cx);
            if let Some((_, timeline)) = &self.timeline {
                let (user, name, avatar) = (
                    change.user_id.to_string(),
                    change.name.clone(),
                    change.avatar.clone(),
                );
                timeline.update(cx, |timeline, cx| {
                    timeline.set_profile(user, name, avatar, cx)
                });
            }
        }
        cx.notify();
    }
}
