//! Mod view, after Discord's: one sheet for a person, from their profile
//! card. Every room you share is listed with where they stand there (role,
//! invited, banned, gone), and whatever moderation you may do in each: a
//! role, kick, ban, unban. Moderation only works downwards, so rooms where
//! they outrank you list them without controls.

use std::future::Future;

use dioxus::prelude::*;
use morpheus_core::{MemberRole, ModRoom, OwnedRoomId, OwnedUserId, Session, Standing};

use super::room_dialogs::{OpenRoomDialog, RoomDialog};
use super::shell::MembersVersion;
use super::toast::report;
use crate::media::Avatar;

/// The roles offered, highest first, with the name each goes by.
const ROLES: [(MemberRole, &str); 3] = [
    (MemberRole::Admin, "Admin"),
    (MemberRole::Moderator, "Moderator"),
    (MemberRole::Member, "Member"),
];

fn role_name(role: MemberRole) -> &'static str {
    ROLES
        .iter()
        .find(|(r, _)| *r == role)
        .map_or("Member", |(_, name)| name)
}

#[component]
pub fn ModViewModal(user_id: OwnedUserId) -> Element {
    let session = use_context::<Session>();
    let mut dialog = use_context::<OpenRoomDialog>();
    let mut close = move || dialog.set(RoomDialog::None);
    let mut reason = use_signal(String::new);
    // Bumped after each action: the rooms are fetched again.
    let mut changed = use_signal(|| 0u32);
    let mut busy = use_signal(|| None::<OwnedRoomId>);
    let profile = use_resource({
        let (session, user_id) = (session.clone(), user_id.clone());
        move || {
            let (session, user_id) = (session.clone(), user_id.clone());
            async move { session.profile(user_id).await.ok() }
        }
    });
    let rooms = use_resource({
        let (session, user_id) = (session.clone(), user_id.clone());
        move || {
            let _ = changed();
            let (session, user_id) = (session.clone(), user_id.clone());
            async move {
                session
                    .mod_overview(user_id)
                    .await
                    .map_err(|err| err.to_string())
            }
        }
    });
    let (name, avatar) = match profile().flatten() {
        Some(profile) => (profile.name, profile.avatar),
        None => (user_id.localpart().to_owned(), None),
    };
    let shown = try_use_context::<super::nicknames::Nicknames>()
        .map(|map| super::nicknames::shown_name(&map(), user_id.as_str(), &name))
        .unwrap_or_else(|| name.clone());

    // One moderation action in one room, then a fresh look at every room.
    let mut act =
        move |room_id: OwnedRoomId,
              what: &'static str,
              action: std::pin::Pin<Box<dyn Future<Output = morpheus_core::Result<()>>>>| {
            busy.set(Some(room_id));
            spawn(async move {
                if let Err(err) = action.await {
                    report(what, &err);
                }
                busy.set(None);
                changed += 1;
                if let Some(mut version) = try_consume_context::<MembersVersion>() {
                    version.with_mut(|v| v.0 += 1);
                }
            });
        };

    rsx! {
        div { class: "overlay", onclick: move |_| close(),
            div {
                class: "message-dialog mod-view",
                tabindex: "-1",
                onclick: move |e| e.stop_propagation(),
                header { class: "mod-view-header",
                    Avatar { media: avatar, fallback: shown.clone(), size: 48 }
                    div { class: "mod-view-who",
                        h2 { "{shown}" }
                        span { class: "mod-view-id", "{user_id}" }
                    }
                }
                label { class: "mod-view-reason",
                    "Reason (for kicks and bans)"
                    input {
                        name: "reason",
                        placeholder: "Optional",
                        value: "{reason}",
                        oninput: move |e| reason.set(e.value()),
                    }
                }
                div { class: "mod-rooms",
                    match rooms() {
                        None => rsx! { p { class: "muted", "Looking through your rooms…" } },
                        Some(Err(err)) => rsx! { p { class: "error", "{err}" } },
                        Some(Ok(list)) if list.is_empty() => rsx! {
                            p { class: "muted", "You share no rooms with them." }
                        },
                        Some(Ok(list)) => rsx! {
                            for room in list {
                                ModRoomRow {
                                    key: "{room.room_id}",
                                    busy: busy().as_ref() == Some(&room.room_id),
                                    on_role: {
                                        let (session, user_id, room_id) = (session.clone(), user_id.clone(), room.room_id.clone());
                                        move |role: MemberRole| act(room_id.clone(), "changing the role", Box::pin(session.set_role(room_id.clone(), user_id.clone(), role)))
                                    },
                                    on_kick: {
                                        let (session, user_id, room_id) = (session.clone(), user_id.clone(), room.room_id.clone());
                                        move |()| act(room_id.clone(), "kicking", Box::pin(session.kick(room_id.clone(), user_id.clone(), given(&reason()))))
                                    },
                                    on_ban: {
                                        let (session, user_id, room_id) = (session.clone(), user_id.clone(), room.room_id.clone());
                                        move |()| act(room_id.clone(), "banning", Box::pin(session.ban(room_id.clone(), user_id.clone(), given(&reason()))))
                                    },
                                    on_unban: {
                                        let (session, user_id, room_id) = (session.clone(), user_id.clone(), room.room_id.clone());
                                        move |()| act(room_id.clone(), "unbanning", Box::pin(session.unban(room_id.clone(), user_id.clone())))
                                    },
                                    room,
                                }
                            }
                        },
                    }
                }
                div { class: "settings-actions",
                    button { class: "secondary", onclick: move |_| close(), "Close" }
                }
            }
        }
    }
}

fn given(reason: &str) -> Option<String> {
    Some(reason.trim().to_owned()).filter(|r| !r.is_empty())
}

#[component]
fn ModRoomRow(
    room: ModRoom,
    busy: bool,
    on_role: EventHandler<MemberRole>,
    on_kick: EventHandler<()>,
    on_ban: EventHandler<()>,
    on_unban: EventHandler<()>,
) -> Element {
    let mine = room.mine;
    let below = room.outranked();
    let state = match room.standing {
        Standing::Member(role) => role_name(role),
        Standing::Invited => "Invited",
        Standing::Banned => "Banned",
        Standing::Gone => "Not in the room",
    };
    let present = matches!(room.standing, Standing::Member(_) | Standing::Invited);
    rsx! {
        div {
            class: "mod-room",
            class: if busy { "busy" },
            "data-name": "{room.name}",
            "data-standing": "{state}",
            div { class: "mod-room-info",
                span { class: "mod-room-name", "# {room.name}" }
                match room.standing {
                    // A role they can be given, when you may: a choice of
                    // those below yours.
                    Standing::Member(role) if below && mine.can_change_roles => rsx! {
                        select {
                            class: "mod-role",
                            disabled: busy,
                            onchange: move |e| {
                                if let Some((role, _)) = ROLES.iter().find(|(_, name)| *name == e.value()) {
                                    on_role.call(*role);
                                }
                            },
                            for (option, label) in ROLES {
                                if option == role || option.power_level() < mine.power_level {
                                    option { value: "{label}", selected: option == role, "{label}" }
                                }
                            }
                        }
                    },
                    _ => rsx! { span { class: "mod-state", "{state}" } },
                }
            }
            if below {
                div { class: "mod-actions",
                    if present && mine.can_kick {
                        button { class: "secondary mod-kick", disabled: busy, onclick: move |_| on_kick.call(()), "Kick" }
                    }
                    if room.standing == Standing::Banned && mine.can_ban {
                        button { class: "secondary mod-unban", disabled: busy, onclick: move |_| on_unban.call(()), "Unban" }
                    } else if mine.can_ban {
                        button { class: "danger mod-ban", disabled: busy, onclick: move |_| on_ban.call(()), "Ban" }
                    }
                }
            }
        }
    }
}
