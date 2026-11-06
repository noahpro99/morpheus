use std::rc::Rc;

use super::toast::report;
use dioxus::prelude::*;
use morpheus_core::{Member, MemberPermissions, MemberRole, OwnedRoomId, Session};

use super::context_menu::{MenuItem, copy_to_clipboard, open_menu, open_menu_at};
use super::profile::{open_profile, open_profile_at};
use super::shell::{MembersVersion, Mention, MentionRequest, Scope, SelectedSpace, go_to};
use crate::media::Avatar;

/// Rows rendered at first. Big rooms have thousands of members, and every
/// rendered row fetches an avatar, so the list grows only on request.
const MEMBERS_PAGE: usize = 100;

/// Bumped after a moderation action so the list refetches (shared with the
/// profile card, which bumps it too).
type Refresh = MembersVersion;

/// Right column: joined members grouped by role.
///
/// `member_count` comes from the room summary so the list refetches when
/// someone joins or leaves.
#[component]
pub fn Members(room_id: OwnedRoomId, member_count: u64) -> Element {
    let session = use_context::<Session>();
    let session2 = session.clone();
    let mut shown = use_signal(|| MEMBERS_PAGE);
    let refresh: Refresh = use_context::<MembersVersion>();

    let members = use_resource(use_reactive!(|room_id, member_count| {
        let session = session.clone();
        let _ = (refresh(), member_count);
        async move { session.members(room_id).await.map_err(|e| e.to_string()) }
    }));
    let permissions = use_resource(use_reactive!(|room_id| {
        let session = session2.clone();
        let _ = refresh();
        async move { session.my_permissions(room_id).await.unwrap_or_default() }
    }));
    let permissions = permissions().unwrap_or_default();

    rsx! {
        aside { class: "members",
            match &*members.read() {
                None => rsx! { crate::media::Skeleton { rows: 6, pictures: true } },
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
                                            MemberRow {
                                                key: "{member.user_id}",
                                                member: member.clone(),
                                                room_id: room_id.clone(),
                                                permissions,
                                                refresh,
                                            }
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
fn MemberRow(
    member: Member,
    room_id: OwnedRoomId,
    permissions: MemberPermissions,
    refresh: Refresh,
) -> Element {
    let session = use_context::<Session>();
    let own = session.user_id().as_ref() == Some(&member.user_id);
    let shown = try_use_context::<super::nicknames::Nicknames>()
        .map(|map| super::nicknames::shown_name(&map(), member.user_id.as_str(), &member.name))
        .unwrap_or_else(|| member.name.clone());

    let on_context = {
        let member = Rc::new(member.clone());
        let room_id = room_id.clone();
        let session = session.clone();
        move |e: Event<MouseData>| {
            let items = member_menu(&session, &room_id, &member, permissions, own);
            open_menu(&e, items, None);
        }
    };

    rsx! {
        button {
            class: "member",
            class: if member.is_ignored { "blocked" },
            title: "{member.user_id}",
            onclick: {
                let user_id = member.user_id.clone();
                move |e| open_profile_at(user_id.clone(), &e)
            },
            oncontextmenu: on_context,
            Avatar { media: member.avatar.clone(), fallback: member.name.clone(), size: 32 }
            span { class: "member-name", "{shown}" }
            if member.is_ignored {
                span { class: "tag", title: "You blocked this user", "blocked" }
            }
        }
    }
}

/// The right-click menu for a member: everything Discord offers that Matrix
/// has an equivalent for, gated by the signed-in user's power level.
pub(crate) fn member_menu(
    session: &Session,
    room_id: &OwnedRoomId,
    member: &Rc<Member>,
    permissions: MemberPermissions,
    own: bool,
) -> Vec<MenuItem> {
    let refresh: Refresh = consume_context::<MembersVersion>();
    let user_id = member.user_id.clone();
    let mut items = vec![MenuItem::new("Profile", {
        let user_id = user_id.clone();
        move || open_profile(user_id.clone())
    })];

    if own {
        items.push(MenuItem::new("Copy user ID", {
            let id = user_id.to_string();
            move || copy_to_clipboard(&id)
        }));
        return items;
    }

    // Finishes a moderation action: log failures, refetch the list.
    let run = {
        let session = session.clone();
        move |action: fn(
            &Session,
            OwnedRoomId,
            morpheus_core::OwnedUserId,
        ) -> morpheus_core::CoreFuture<()>,
              what: &'static str,
              room_id: OwnedRoomId,
              user_id: morpheus_core::OwnedUserId| {
            let future = action(&session, room_id, user_id);
            spawn(async move {
                let mut refresh = refresh;
                if let Err(err) = future.await {
                    report(what, &err);
                }
                refresh.with_mut(|v| v.0 += 1);
            });
        }
    };

    let is_friend = try_consume_context::<super::friends::Friends>()
        .is_some_and(|friends| friends.peek().0.contains(&user_id));
    items.push(MenuItem::new(
        if is_friend {
            "Remove friend"
        } else {
            "Add friend"
        },
        {
            let (session, user_id) = (session.clone(), user_id.clone());
            move || super::friends::set_friend(&session, user_id.clone(), !is_friend)
        },
    ));
    items.push(MenuItem::new("Set nickname…", {
        let (user_id, name) = (user_id.clone(), member.name.clone());
        move || {
            consume_context::<super::nicknames::EditNickname>().set(
                super::nicknames::NicknameTarget(Some((user_id.clone(), name.clone()))),
            )
        }
    }));
    items.push(MenuItem::new("Mention", {
        let name = member.name.clone();
        move || consume_context::<MentionRequest>().set(Mention(Some(name.clone())))
    }));
    items.push(MenuItem::new("Message", {
        let session = session.clone();
        let user_id = user_id.clone();
        move || {
            let session = session.clone();
            let user_id = user_id.clone();
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
    }));

    if let Some(invite) = super::invites::invite_menu(session, &user_id, Some(room_id)) {
        items.push(invite);
    }

    items.push(MenuItem::new(
        if member.is_ignored {
            "Unblock"
        } else {
            "Block"
        },
        {
            let session = session.clone();
            let user_id = user_id.clone();
            let blocked = !member.is_ignored;
            move || {
                let future = session.block(user_id.clone(), blocked);
                spawn(async move {
                    let mut refresh = refresh;
                    if let Err(err) = future.await {
                        report("block", &err);
                    }
                    refresh.with_mut(|v| v.0 += 1);
                });
            }
        },
    ));

    if permissions.can_change_roles && permissions.outranks(member) {
        let roles = [MemberRole::Member, MemberRole::Moderator, MemberRole::Admin]
            .into_iter()
            .filter(|role| role.power_level() <= permissions.power_level)
            .map(|role| {
                MenuItem::new(role_name(role), {
                    let session = session.clone();
                    let (room_id, user_id) = (room_id.clone(), user_id.clone());
                    move || {
                        let future = session.set_role(room_id.clone(), user_id.clone(), role);
                        spawn(async move {
                            let mut refresh = refresh;
                            if let Err(err) = future.await {
                                report("role change", &err);
                            }
                            refresh.with_mut(|v| v.0 += 1);
                        });
                    }
                })
                .checked(role == member.role)
            })
            .collect();
        items.push(MenuItem::submenu("Roles", roles));
    }
    if permissions.can_kick && permissions.outranks(member) {
        items.push(
            MenuItem::new(format!("Kick {}", member.name), {
                let run = run.clone();
                let (room_id, user_id) = (room_id.clone(), user_id.clone());
                move || {
                    run(
                        |s, r, u| s.kick(r, u, None),
                        "kick",
                        room_id.clone(),
                        user_id.clone(),
                    )
                }
            })
            .danger(),
        );
    }
    if permissions.can_ban && permissions.outranks(member) {
        items.push(
            MenuItem::new(format!("Ban {}", member.name), {
                let run = run.clone();
                let (room_id, user_id) = (room_id.clone(), user_id.clone());
                move || {
                    run(
                        |s, r, u| s.ban(r, u, None),
                        "ban",
                        room_id.clone(),
                        user_id.clone(),
                    )
                }
            })
            .danger(),
        );
    }
    items.push(MenuItem::new("Copy user ID", {
        let id = user_id.to_string();
        move || copy_to_clipboard(&id)
    }));
    items
}

fn role_name(role: MemberRole) -> &'static str {
    match role {
        MemberRole::Admin => "Admin",
        MemberRole::Moderator => "Moderator",
        MemberRole::Member => "Member",
    }
}

/// Right-click on a message's avatar or sender name: the same menu as the
/// member list. The member and our permissions are looked up first, so the
/// menu opens where the pointer was once they arrive.
pub(crate) fn open_sender_menu(
    session: &Session,
    room_id: OwnedRoomId,
    user_id: morpheus_core::OwnedUserId,
    name: String,
    e: &Event<MouseData>,
) {
    e.prevent_default();
    e.stop_propagation();
    let point = e.client_coordinates();
    let session = session.clone();
    let own = session.user_id().as_ref() == Some(&user_id);
    spawn(async move {
        let members = session.members(room_id.clone()).await.unwrap_or_default();
        let permissions = session
            .my_permissions(room_id.clone())
            .await
            .unwrap_or_default();
        // Someone who has left still gets a menu (profile, message, block).
        let member = members
            .into_iter()
            .find(|member| member.user_id == user_id)
            .unwrap_or(Member {
                user_id,
                name,
                avatar: None,
                role: MemberRole::Member,
                power_level: 0,
                is_ignored: false,
            });
        let items = member_menu(&session, &room_id, &Rc::new(member), permissions, own);
        open_menu_at(point.x, point.y, items, None);
    });
}
