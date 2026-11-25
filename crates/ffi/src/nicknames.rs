//! Your nicknames for people, as the phone shows them: kept by the core in private account data,
//! read into this map when the lists start and whenever one is set, and put in place of names in the
//! room list (a DM), the timeline, the members and a person's card (view_model::nicknames).

use std::sync::{Arc, Mutex};

use morpheus_core::OwnedUserId;
use morpheus_view_model::nicknames::{self, NickMap};
use tokio::sync::watch;

use crate::{FfiError, Morpheus};

/// The nicknames known now, and a counter that changes whenever they do (screens that show names redraw on it).
pub(crate) struct Nicknames {
    map: Mutex<NickMap>,
    tick: watch::Sender<u64>,
}

impl Default for Nicknames {
    fn default() -> Self {
        Self { map: Mutex::default(), tick: watch::channel(0).0 }
    }
}

impl Nicknames {
    pub(crate) fn snapshot(&self) -> NickMap {
        self.map.lock().map(|m| m.clone()).unwrap_or_default()
    }

    /// Take these as the nicknames; those who watch hear of it when they are not the same.
    pub(crate) fn replace(&self, map: NickMap) {
        let changed = match self.map.lock() {
            Ok(mut now) if *now != map => {
                *now = map;
                true
            }
            _ => false,
        };
        if changed {
            self.tick.send_modify(|n| *n += 1);
        }
    }

    pub(crate) fn watch(&self) -> watch::Receiver<u64> {
        self.tick.subscribe()
    }
}

impl Morpheus {
    /// Read the account's nicknames again.
    pub(crate) async fn refresh_nicknames(&self) {
        if let Ok(session) = self.session()
            && let Ok(map) = session.nicknames().await
        {
            self.nicks.replace(map);
        }
    }
}

#[uniffi::export]
impl Morpheus {
    /// The nickname you gave this person (empty for none).
    pub async fn nickname_of(&self, user_id: String) -> String {
        self.refresh_nicknames().await;
        self.nicks.snapshot().get(&user_id).cloned().unwrap_or_default()
    }

    /// What the nickname page says under its title for the person who shows as `name`.
    pub fn nickname_blurb(&self, name: String) -> String {
        nicknames::nickname_blurb(&name)
    }

    /// Give this person a nickname (empty takes it away). Names show it at once.
    pub async fn set_nickname(&self, user_id: String, nickname: String) -> Result<(), FfiError> {
        let session = self.session()?;
        let user = OwnedUserId::try_from(user_id.as_str()).map_err(|_| FfiError::Failed { reason: format!("{user_id} is not a Matrix ID") })?;
        session.set_nickname(user.clone(), nickname.clone()).await?;
        self.nicks.replace(nicknames::with_nickname(self.nicks.snapshot(), user.as_str(), &nickname));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watchers_hear_of_a_change_and_not_of_the_same_nicknames_again() {
        let book = Nicknames::default();
        let mut heard = book.watch();
        let map = nicknames::with_nickname(NickMap::new(), "@bob:x", "Bobby");
        book.replace(map.clone());
        assert!(heard.has_changed().expect("open"));
        heard.mark_changed();
        let _ = heard.borrow_and_update();
        assert!(!heard.has_changed().expect("open"));
        book.replace(map);
        assert!(!heard.has_changed().expect("open"), "the same nicknames are no news");
        assert_eq!(book.snapshot().get("@bob:x").map(String::as_str), Some("Bobby"));
        book.replace(NickMap::new());
        assert!(heard.has_changed().expect("open"));
    }
}
