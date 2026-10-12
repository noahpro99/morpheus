//! Pending invites: a section at the top of the sidebar, a card in place of
//! the timeline, and a block in the inbox. Accepting joins the room; the room
//! list then flips the entry to a joined room and the chat opens as usual.

use super::toast::report;
use dioxus::prelude::*;
use morpheus_core::{OwnedRoomId, RoomSummary, Session};

use super::shell::{Rooms, Selected, go_to};
use crate::icons::{CHECK, Icon, X};
use crate::media::Avatar;

/// Rooms the user has been invited to, newest first as the list reports them.
pub fn pending_invites(rooms: &morpheus_core::Vector<RoomSummary>) -> Vec<RoomSummary> {
    rooms.iter().filter(|room| room.invited).cloned().collect()
}

/// Join the room; the invite goes away when the next sync lands.
pub fn accept(session: &Session, room_id: OwnedRoomId) {
    let future = session.join_room(room_id);
    spawn(async move {
        if let Err(err) = future.await {
            report("accepting an invite", &err);
        }
    });
}

/// Declining is leaving the invited room.
pub fn decline(session: &Session, room_id: OwnedRoomId) {
    let selected = consume_context::<Selected>();
    let future = session.leave_room(room_id.clone());
    spawn(async move {
        match future.await {
            Ok(()) => {
                let mut selected = selected;
                if selected.peek().as_ref() == Some(&room_id) {
                    selected.set(None);
                }
            }
            Err(err) => report("declining an invite", &err),
        }
    });
}

/// One invite in the sidebar: the room, who asked, accept and decline.
#[component]
pub fn InviteRow(room: RoomSummary) -> Element {
    let session = use_context::<Session>();
    let selected = use_context::<Selected>();
    let is_selected = selected().as_ref() == Some(&room.id);
    let id = room.id.clone();
    let inviter = room.inviter.clone().unwrap_or_else(|| "Someone".into());
    rsx! {
        div {
            class: "room invite-row",
            class: if is_selected { "selected" },
            role: "button",
            onclick: {
                let id = id.clone();
                move |_| go_to(id.clone())
            },
            Avatar { media: room.avatar.clone(), fallback: room.name.clone(), size: 24 }
            span { class: "invite-text",
                span { class: "room-name", "{room.name}" }
                span { class: "invite-from", "from {inviter}" }
            }
            button {
                class: "invite-accept",
                title: "Accept",
                onclick: {
                    let (session, id) = (session.clone(), id.clone());
                    move |e: Event<MouseData>| {
                        e.stop_propagation();
                        accept(&session, id.clone());
                    }
                },
                Icon { glyph: CHECK, size: 14 }
            }
            button {
                class: "invite-decline",
                title: "Decline",
                onclick: {
                    let (session, id) = (session.clone(), id.clone());
                    move |e: Event<MouseData>| {
                        e.stop_propagation();
                        decline(&session, id.clone());
                    }
                },
                Icon { glyph: X, size: 14 }
            }
        }
    }
}

/// Shown in the chat column for an invited room instead of a timeline.
#[component]
pub fn InviteCard(room: RoomSummary) -> Element {
    let session = use_context::<Session>();
    let inviter = room.inviter.clone().unwrap_or_else(|| "Someone".into());
    let kind = if room.is_dm {
        "a direct message"
    } else if room.is_space {
        "a server"
    } else {
        "a room"
    };
    let id = room.id.clone();
    rsx! {
        section { class: "invite-card",
            Avatar { media: room.avatar.clone(), fallback: room.name.clone(), size: 72 }
            h2 { "{room.name}" }
            p { class: "muted", "{inviter} invited you to {kind}." }
            if let Some(topic) = room.topic.as_deref() {
                p { class: "topic", "{topic}" }
            }
            div { class: "settings-actions",
                button {
                    class: "primary invite-accept",
                    onclick: {
                        let (session, id) = (session.clone(), id.clone());
                        move |_| accept(&session, id.clone())
                    },
                    "Accept"
                }
                button {
                    class: "secondary invite-decline",
                    onclick: {
                        let (session, id) = (session.clone(), id.clone());
                        move |_| decline(&session, id.clone())
                    },
                    "Decline"
                }
            }
        }
    }
}

/// The invites block the inbox shows above unread rooms.
#[component]
pub fn InboxInvites(on_open: Callback<RoomSummary>) -> Element {
    let rooms = use_context::<Rooms>();
    let session = use_context::<Session>();
    let invites = pending_invites(&rooms.read());
    if invites.is_empty() {
        return rsx! {};
    }
    rsx! {
        h4 { class: "section", "Invites" }
        for room in invites {
            div { key: "{room.id}", class: "inbox-item inbox-invite",
                button {
                    class: "inbox-open",
                    onclick: {
                        let room = room.clone();
                        move |_| on_open.call(room.clone())
                    },
                    span { class: "inbox-name", "{room.name}" }
                    span { class: "muted", " from {room.inviter.clone().unwrap_or_default()}" }
                }
                button {
                    class: "invite-accept",
                    title: "Accept",
                    onclick: {
                        let (session, id) = (session.clone(), room.id.clone());
                        move |_| accept(&session, id.clone())
                    },
                    Icon { glyph: CHECK, size: 14 }
                }
                button {
                    class: "invite-decline",
                    title: "Decline",
                    onclick: {
                        let (session, id) = (session.clone(), room.id.clone());
                        move |_| decline(&session, id.clone())
                    },
                    Icon { glyph: X, size: 14 }
                }
            }
        }
    }
}
