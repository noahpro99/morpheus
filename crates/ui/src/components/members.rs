use dioxus::prelude::*;
use morpheus_core::{Member, MemberRole, OwnedRoomId, Session};

use super::context_menu::{MenuItem, copy_to_clipboard, open_menu};
use super::profile::open_profile;
use crate::media::Avatar;

/// Right column: joined members grouped by role.
#[component]
pub fn Members(room_id: OwnedRoomId) -> Element {
    let session = use_context::<Session>();

    let members = use_resource(use_reactive!(|room_id| {
        let session = session.clone();
        async move { session.members(room_id).await.map_err(|e| e.to_string()) }
    }));

    rsx! {
        aside { class: "members",
            match &*members.read() {
                None => rsx! { p { class: "muted pad", "Loading…" } },
                Some(Err(err)) => rsx! { p { class: "error pad", "{err}" } },
                Some(Ok(list)) => rsx! {
                    for role in [MemberRole::Admin, MemberRole::Moderator, MemberRole::Member] {
                        {
                            let group: Vec<&Member> = list.iter().filter(|m| m.role == role).collect();
                            rsx! {
                                if !group.is_empty() {
                                    header { class: "members-header",
                                        h3 { "{role.label()} — {group.len()}" }
                                    }
                                    for member in group {
                                        button {
                                            key: "{member.user_id}",
                                            class: "member",
                                            title: "{member.user_id}",
                                            onclick: {
                                                let user_id = member.user_id.clone();
                                                move |_| open_profile(user_id.clone())
                                            },
                                            oncontextmenu: {
                                                let user_id = member.user_id.clone();
                                                move |e| {
                                                    let (profile_id, copy_id) = (user_id.clone(), user_id.to_string());
                                                    open_menu(
                                                        &e,
                                                        vec![
                                                            MenuItem::new("View profile", move || open_profile(profile_id.clone())),
                                                            MenuItem::new("Copy user ID", move || copy_to_clipboard(&copy_id)),
                                                        ],
                                                        None,
                                                    );
                                                }
                                            },
                                            Avatar { media: member.avatar.clone(), fallback: member.name.clone(), size: 32 }
                                            span { class: "member-name", "{member.name}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
            }
        }
    }
}
