//! Friends: a private list of people (account data; Matrix has no
//! friendship). Added from a person's menu; the Friends page, opened from the
//! top of the DMs, lists them with a way to message them.

use dioxus::prelude::*;
use morpheus_core::{OwnedUserId, Session};

use super::nicknames::{Nicknames, shown_name};
use super::shell::{SelectedSpace, go_to};
use super::toast::report;
use crate::components::shell::Scope;
use crate::media::Avatar;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FriendList(pub Vec<OwnedUserId>);
pub type Friends = Signal<FriendList>;

/// Whether the Friends page fills the chat area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FriendsShown(pub bool);
pub type FriendsOpen = Signal<FriendsShown>;

/// Install in the shell: the list (loaded from the account) and the page
/// flag, returned for the shell.
pub fn provide_friends(session: Session) -> (Friends, FriendsOpen) {
    let mut friends: Friends = use_context_provider(|| Signal::new(FriendList::default()));
    let open: FriendsOpen = use_context_provider(|| Signal::new(FriendsShown(false)));
    use_future(move || {
        let session = session.clone();
        async move {
            match session.friends().await {
                Ok(list) => friends.set(FriendList(list)),
                Err(err) => report("loading friends", &err),
            }
        }
    });
    (friends, open)
}

/// Add or remove a friend, updating the shared list once saved.
pub fn set_friend(session: &Session, user_id: OwnedUserId, friend: bool) {
    let future = session.set_friend(user_id.clone(), friend);
    let mut friends = consume_context::<Friends>();
    spawn(async move {
        match future.await {
            Ok(()) => friends.with_mut(|list| {
                list.0.retain(|id| *id != user_id);
                if friend {
                    list.0.push(user_id);
                }
            }),
            Err(err) => report("saving friends", &err),
        }
    });
}

#[component]
pub fn FriendsPage() -> Element {
    let friends = use_context::<Friends>();
    rsx! {
        section { class: "friends-page",
            header { class: "chat-header friends-header",
                h2 { "Friends" }
            }
            if friends().0.is_empty() {
                div { class: "empty-state",
                    h2 { "No friends yet" }
                    p { class: "muted", "Add someone from their menu: right-click (or press and hold) their name." }
                }
            } else {
                ul { class: "friend-list",
                    for user_id in friends().0 {
                        FriendRow { key: "{user_id}", user_id }
                    }
                }
            }
        }
    }
}

#[component]
fn FriendRow(user_id: OwnedUserId) -> Element {
    let session = use_context::<Session>();
    let nicknames = use_context::<Nicknames>();
    let profile = use_resource({
        let (session, user_id) = (session.clone(), user_id.clone());
        move || {
            let (session, user_id) = (session.clone(), user_id.clone());
            async move { session.profile(user_id).await.ok() }
        }
    });
    let (name, avatar, status) = match profile().flatten() {
        Some(p) => (p.name, p.avatar, p.status),
        None => (user_id.localpart().to_owned(), None, None),
    };
    let shown = shown_name(&nicknames(), user_id.as_str(), &name);
    let message = {
        let (session, user_id) = (session.clone(), user_id.clone());
        move |_| {
            let (session, user_id) = (session.clone(), user_id.clone());
            let mut scope = consume_context::<SelectedSpace>();
            spawn(async move {
                match session.dm_with(user_id).await {
                    Ok(dm) => {
                        scope.set(Scope::Dms);
                        go_to(dm);
                    }
                    Err(err) => report("opening a DM", &err),
                }
            });
        }
    };
    rsx! {
        li { class: "friend",
            Avatar { media: avatar, fallback: shown.clone(), size: 40 }
            div { class: "friend-text",
                span { class: "friend-name", "{shown}" }
                span { class: "friend-status muted",
                    if let Some(status) = status { "{status}" } else { "{user_id}" }
                }
            }
            div { class: "friend-actions",
                button { class: "secondary", onclick: message, "Message" }
                button {
                    class: "ghost",
                    onclick: {
                        let (session, user_id) = (session.clone(), user_id.clone());
                        move |_| set_friend(&session, user_id.clone(), false)
                    },
                    "Remove"
                }
            }
        }
    }
}
