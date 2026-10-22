//! Friends: a private list of people (account data; Matrix has no
//! friendship). Added from a person's menu; the Friends page, opened from the
//! top of the DMs, lists them with a way to message them, and starts a direct message with
//! anyone by username (New Message).

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
    NewMessage,
}

#[component]
pub fn FriendsPage() -> Element {
    let friends = use_context::<Friends>();
    let narrow = try_use_context::<super::shell::NarrowWindow>();
    // With no friends yet, the way to add one comes first.
    let mut tab = use_signal(|| {
        if friends.peek().0.is_empty() {
            FriendsTab::NewMessage
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
                    class: "friends-tab new-message-tab",
                    class: if tab() == FriendsTab::NewMessage { "active" },
                    onclick: move |_| tab.set(FriendsTab::NewMessage),
                    "New Message"
                }
            }
            if tab() == FriendsTab::NewMessage {
                NewMessage {}
            } else if friends().0.is_empty() {
                div { class: "empty-state",
                    h2 { "No friends yet" }
                    p { class: "muted", "Add someone from their menu: right-click (or press and hold) their name." }
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

/// New Message: type who, and the direct message with them opens (made if
/// there is none; the invite is how they hear of it, as Matrix has no
/// friend requests).
#[component]
fn NewMessage() -> Element {
    let session = use_context::<Session>();
    let mut typed = use_signal(String::new);
    let mut failed = use_signal(|| None::<String>);
    let mut busy = use_signal(|| false);
    let submit = move |e: Event<FormData>| {
        e.prevent_default();
        let text = typed();
        if text.trim().is_empty() || busy() {
            return;
        }
        let session = session.clone();
        let mut scope = consume_context::<SelectedSpace>();
        busy.set(true);
        spawn(async move {
            let opened = async {
                let user_id = session.find_user(text).await?;
                if session.user_id().as_ref() == Some(&user_id) {
                    return Err(morpheus_core::Error::Other("That is you.".to_owned()));
                }
                session.dm_with(user_id).await
            }
            .await;
            busy.set(false);
            match opened {
                Ok(dm) => {
                    typed.set(String::new());
                    scope.set(Scope::Dms);
                    go_to(dm);
                }
                Err(err) => failed.set(Some(err.to_string())),
            }
        });
    };
    rsx! {
        div { class: "new-message",
            h3 { "New Message" }
            p { class: "muted", "Message someone by their username, or their full Matrix ID for someone on another server." }
            form { class: "new-message", onsubmit: submit,
                input {
                    r#type: "text",
                    placeholder: "Username or @name:server",
                    autocomplete: "off",
                    spellcheck: "false",
                    value: "{typed}",
                    oninput: move |e| {
                        typed.set(e.value());
                        failed.set(None);
                    },
                }
                button {
                    r#type: "submit",
                    class: "primary",
                    disabled: typed().trim().is_empty() || busy(),
                    if busy() { "Opening…" } else { "Message" }
                }
            }
            if let Some(text) = failed() {
                p { class: "new-message-result error", "{text}" }
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
