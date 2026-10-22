//! Friends: a private list of people (account data; Matrix has no
//! friendship). Added from a person's menu or by name; the Friends page, opened from the
//! top of the DMs, lists them with a way to message them, and adds people by
//! username (Discord's "Add Friend").

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

/// The Friends page's tabs, as in Discord.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FriendsTab {
    All,
    Add,
}

#[component]
pub fn FriendsPage() -> Element {
    let friends = use_context::<Friends>();
    let narrow = try_use_context::<super::shell::NarrowWindow>();
    // With no friends yet, the way to add one comes first.
    let mut tab = use_signal(|| {
        if friends.peek().0.is_empty() {
            FriendsTab::Add
        } else {
            FriendsTab::All
        }
    });
    rsx! {
        section { class: "friends-page",
            header { class: "chat-header friends-header",
                // A phone shows one pane: back to the list, as from a chat.
                if narrow.is_some_and(|n| n().0) {
                    button {
                        class: "ghost pane-back",
                        title: "Back to rooms",
                        onclick: move |_| {
                            consume_context::<super::shell::ChatOpen>().set(super::shell::ChatInFront(false));
                        },
                        crate::icons::Icon { glyph: crate::icons::CHEVRON_LEFT, size: 18 }
                    }
                }
                crate::icons::Icon { glyph: crate::icons::USERS, size: 18 }
                h2 { "Friends" }
                span { class: "friends-divider" }
                button {
                    class: "friends-tab all-friends-tab",
                    class: if tab() == FriendsTab::All { "active" },
                    onclick: move |_| tab.set(FriendsTab::All),
                    "All"
                }
                button {
                    class: "friends-tab add-friend-tab",
                    class: if tab() == FriendsTab::Add { "active" },
                    onclick: move |_| tab.set(FriendsTab::Add),
                    "Add Friend"
                }
            }
            if tab() == FriendsTab::Add {
                AddFriend {}
            } else if friends().0.is_empty() {
                div { class: "empty-state",
                    h2 { "No friends yet" }
                    p { class: "muted", "Add someone with the Add Friend button, or from their menu: right-click (or press and hold) their name." }
                }
            } else {
                h4 { class: "section friends-count", "All friends — {friends().0.len()}" }
                ul { class: "friend-list",
                    for user_id in friends().0 {
                        FriendRow { key: "{user_id}", user_id }
                    }
                }
            }
        }
    }
}

/// What adding someone came to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Added {
    Ok(String),
    Failed(String),
}

/// Discord's Add Friend: type who, add them.
#[component]
fn AddFriend() -> Element {
    let session = use_context::<Session>();
    let mut friends = use_context::<Friends>();
    let mut typed = use_signal(String::new);
    let mut result = use_signal(|| None::<Added>);
    let mut busy = use_signal(|| false);
    let submit = move |e: Event<FormData>| {
        e.prevent_default();
        let text = typed();
        if text.trim().is_empty() || busy() {
            return;
        }
        let session = session.clone();
        busy.set(true);
        spawn(async move {
            let outcome = async {
                let user_id = session.find_user(text).await?;
                if friends.peek().0.contains(&user_id) {
                    return Ok(Added::Failed(format!("{user_id} is already your friend.")));
                }
                if session.user_id().as_ref() == Some(&user_id) {
                    return Ok(Added::Failed("That is you.".to_owned()));
                }
                session.set_friend(user_id.clone(), true).await?;
                friends.with_mut(|list| {
                    list.0.retain(|id| *id != user_id);
                    list.0.push(user_id.clone());
                });
                Ok::<_, morpheus_core::Error>(Added::Ok(format!("{user_id} is now your friend.")))
            }
            .await;
            let outcome = outcome.unwrap_or_else(|err| Added::Failed(err.to_string()));
            if matches!(outcome, Added::Ok(_)) {
                typed.set(String::new());
            }
            result.set(Some(outcome));
            busy.set(false);
        });
    };
    rsx! {
        div { class: "add-friend",
            h3 { "Add Friend" }
            p { class: "muted", "You can add friends with their username, or their full Matrix ID for someone on another server." }
            form { class: "add-friend", onsubmit: submit,
                input {
                    r#type: "text",
                    placeholder: "Username or @name:server",
                    autocomplete: "off",
                    spellcheck: "false",
                    value: "{typed}",
                    oninput: move |e| {
                        typed.set(e.value());
                        result.set(None);
                    },
                }
                button {
                    r#type: "submit",
                    class: "primary",
                    disabled: typed().trim().is_empty() || busy(),
                    "Add Friend"
                }
            }
            match result() {
                Some(Added::Ok(text)) => rsx! { p { class: "add-friend-result ok", "{text}" } },
                Some(Added::Failed(text)) => rsx! { p { class: "add-friend-result error", "{text}" } },
                None => rsx! {},
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
