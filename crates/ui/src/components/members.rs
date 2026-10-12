use dioxus::prelude::*;
use morpheus_core::{Member, MemberRole, OwnedRoomId, Session};

use super::context_menu::{MenuItem, copy_to_clipboard, open_menu};
use super::profile::open_profile;
use crate::media::Avatar;

/// Rows rendered at first. Big rooms have thousands of members, and every
/// rendered row fetches an avatar, so the list grows only on request.
const MEMBERS_PAGE: usize = 100;

/// Right column: joined members grouped by role.
#[component]
pub fn Members(room_id: OwnedRoomId) -> Element {
    let session = use_context::<Session>();
    let mut shown = use_signal(|| MEMBERS_PAGE);

    let members = use_resource(use_reactive!(|room_id| {
        let session = session.clone();
        async move { session.members(room_id).await.map_err(|e| e.to_string()) }
    }));

    rsx! {
        aside { class: "members",
            match &*members.read() {
                None => rsx! { p { class: "muted pad", "Loading…" } },
                Some(Err(err)) => rsx! { p { class: "error pad", "{err}" } },
                Some(Ok(list)) => {
                    // The list is already ordered admins → moderators → members,
                    // so the window is a prefix of it.
                    let visible = &list[..list.len().min(shown())];
                    let hidden = list.len() - visible.len();
                    rsx! {
                        for role in [MemberRole::Admin, MemberRole::Moderator, MemberRole::Member] {
                            {
                                let total = list.iter().filter(|m| m.role == role).count();
                                let group: Vec<&Member> = visible.iter().filter(|m| m.role == role).collect();
                                rsx! {
                                    if total > 0 {
                                        header { class: "members-header",
                                            h3 { "{role.label()} — {total}" }
                                        }
                                        for member in group {
                                            MemberRow { key: "{member.user_id}", member: member.clone() }
                                        }
                                    }
                                }
                            }
                        }
                        if hidden > 0 {
                            button {
                                class: "ghost show-more-members",
                                onclick: move |_| shown += MEMBERS_PAGE,
                                "Show {hidden.min(MEMBERS_PAGE)} more ({hidden} hidden)"
                            }
                        }
                    }
                },
            }
        }
    }
}

#[component]
fn MemberRow(member: Member) -> Element {
    rsx! {
        button {
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
