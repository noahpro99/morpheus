//! Your nicknames for people (Discord's friend nicknames). Kept in private
//! account data by the core; shown instead of display names in messages and
//! the member list.

use std::collections::HashMap;

use dioxus::prelude::*;
use morpheus_core::{OwnedUserId, Session};

use super::toast::report;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NickMap(pub HashMap<String, String>);
pub type Nicknames = Signal<NickMap>;

/// Whose nickname is being edited, with the name they show now.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NicknameTarget(pub Option<(OwnedUserId, String)>);
pub type EditNickname = Signal<NicknameTarget>;

/// Install in the shell: provides the map (loaded from the account) and the
/// editor's target (returned, for the shell).
pub fn provide_nicknames(session: Session) -> EditNickname {
    let mut map: Nicknames = use_context_provider(|| Signal::new(NickMap::default()));
    let target = use_context_provider(|| Signal::new(NicknameTarget::default()) as EditNickname);
    use_future(move || {
        let session = session.clone();
        async move {
            match session.nicknames().await {
                Ok(names) => map.set(NickMap(names)),
                Err(err) => report("loading nicknames", &err),
            }
        }
    });
    target
}

/// The name to show for `user_id`: your nickname for them, else `fallback`.
pub fn shown_name(map: &NickMap, user_id: &str, fallback: &str) -> String {
    map.0
        .get(user_id)
        .cloned()
        .unwrap_or_else(|| fallback.to_owned())
}

/// Set or clear a nickname (from a person's menu). The shell shows it
/// while `EditNickname` names someone.
#[component]
pub fn NicknameEditor(user_id: OwnedUserId, current: String) -> Element {
    let session = use_context::<Session>();
    let mut target = use_context::<EditNickname>();
    let mut map = use_context::<Nicknames>();
    let existing = map
        .peek()
        .0
        .get(user_id.as_str())
        .cloned()
        .unwrap_or_default();
    let mut value = use_signal(move || existing);
    let mut close = move || target.set(NicknameTarget(None));
    let save = {
        let user_id = user_id.clone();
        move |evt: FormEvent| {
            evt.prevent_default();
            let (session, user_id) = (session.clone(), user_id.clone());
            let nickname = value().trim().to_owned();
            spawn(async move {
                match session
                    .set_nickname(user_id.clone(), nickname.clone())
                    .await
                {
                    Ok(()) => {
                        map.with_mut(|names| {
                            if nickname.is_empty() {
                                names.0.remove(user_id.as_str());
                            } else {
                                names.0.insert(user_id.to_string(), nickname);
                            }
                        });
                        close();
                    }
                    Err(err) => report("saving the nickname", &err),
                }
            });
        }
    };
    rsx! {
        div { class: "overlay", onclick: move |_| close(),
            form {
                class: "message-dialog nickname-editor",
                tabindex: "-1",
                onclick: move |e| e.stop_propagation(),
                onsubmit: save,
                h2 { "Nickname" }
                p { class: "muted", "Only you see this, on all your devices. Leave it empty to use {current}'s own name." }
                input {
                    name: "nickname",
                    placeholder: "{current}",
                    autofocus: true,
                    value: "{value}",
                    oninput: move |e| value.set(e.value()),
                }
                div { class: "settings-actions",
                    button { r#type: "submit", class: "primary", "Save" }
                    button { r#type: "button", class: "secondary", onclick: move |_| close(), "Cancel" }
                }
            }
        }
    }
}
