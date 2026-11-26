//! What the member list and a message's sender offer (`view_model::members`
//! decides the menu): opening it where the pointer was, and running what an
//! item asks (roles, kick, ban, a direct message, a mention).

use gpui::{Context, Pixels, Point};
use morpheus_core::{Member, MemberPermissions, MemberRole, Message};

use crate::{
    menu::OpenMenu,
    model::Scope,
    shell::Shell,
    view_model::{
        members::{MemberMenuInput, member_menu},
        menu::MenuTarget,
        room_menu::available_invite_targets,
    },
};

impl Shell {
    /// The member list, shown or not (kept across rooms).
    pub fn toggle_members(&mut self, cx: &mut Context<Self>) {
        if !self.members_fit {
            return;
        }
        self.members_open = !self.members_open;
        cx.notify();
    }

    /// The panel shows when the user wants it and the window has room (#512).
    pub fn members_shown(&self) -> bool {
        crate::view_model::members::panel_shown(self.members_open, self.members_fit)
    }

    /// Reads the member list again if the room's member count changed (the
    /// room list only tells us when someone joined or left).
    pub fn sync_members(&mut self, cx: &mut Context<Self>) {
        let count = self.current().map(|room| room.member_count);
        if let (Some(count), Some((_, members))) = (count, &self.members)
            && count != self.members_count
        {
            self.members_count = count;
            members.update(cx, |members, cx| members.refresh(cx));
        }
    }

    /// The member list, read again (a moderation action ran).
    pub fn refresh_members(&mut self, cx: &mut Context<Self>) {
        if let Some((_, members)) = &self.members {
            members.update(cx, |members, cx| members.refresh(cx));
        }
    }

    /// The menu of a member right-clicked in the list.
    pub fn open_member_menu(
        &mut self,
        member: &Member,
        permissions: MemberPermissions,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(room) = self.selected.clone() else {
            return;
        };
        self.open_member_menu_in(room, member, permissions, at, cx)
    }

    /// The same, for a member of any room.
    pub fn open_member_menu_in(
        &mut self,
        room: morpheus_core::OwnedRoomId,
        member: &Member,
        permissions: MemberPermissions,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let mutual_spaces = self
            .session
            .as_ref()
            .map(|session| session.mutual_spaces(member.user_id.clone()));
        let member = member.clone();
        cx.spawn(async move |this, cx| {
            let mutual = match mutual_spaces {
                Some(f) => f.await.unwrap_or_default(),
                None => Vec::new(),
            };
            this.update(cx, |this, cx| {
                this.open_member_menu_with_mutual(room, &member, permissions, mutual, at, cx);
            })
            .ok();
        })
        .detach();
    }

    pub fn open_member_menu_with_mutual(
        &mut self,
        room: morpheus_core::OwnedRoomId,
        member: &Member,
        permissions: MemberPermissions,
        mutual: Vec<morpheus_core::OwnedRoomId>,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.open_member_menu_extra(room, member, permissions, mutual, Vec::new(), at, cx)
    }

    /// The member menu with `extra` rows (a call card's) above it.
    #[allow(clippy::too_many_arguments)]
    pub fn open_member_menu_extra(
        &mut self,
        room: morpheus_core::OwnedRoomId,
        member: &Member,
        permissions: MemberPermissions,
        mutual: Vec<morpheus_core::OwnedRoomId>,
        extra: Vec<crate::view_model::menu::MenuItem>,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let own = self
            .session
            .as_ref()
            .and_then(|session| session.user_id())
            .as_ref()
            == Some(&member.user_id);
        let invite_to = available_invite_targets(&self.rooms, Some(&room), &mutual);
        let mut items = extra;
        items.extend(member_menu(&MemberMenuInput {
            room: &room,
            member,
            permissions,
            own,
            friend: self.friends.contains(&member.user_id),
            invite_to,
        }));
        self.open_menu(OpenMenu::new(at, items, false, MenuTarget::Other), cx);
    }

    /// The member menu for whoever sent a message: the member and our
    /// powers are looked up first, so the menu opens where the pointer was
    /// once they arrive.
    pub fn open_sender_menu(
        &mut self,
        message: &Message,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(room) = self.selected.clone() else {
            return;
        };
        let (user, name) = (message.sender.clone(), message.sender_name.clone());
        self.open_user_menu(room, user, name, at, cx)
    }

    /// The member menu for a user of `room` (a call's, under its channel).
    pub fn open_user_menu(
        &mut self,
        room: morpheus_core::OwnedRoomId,
        user: morpheus_core::OwnedUserId,
        name: String,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.open_user_menu_extra(room, user, name, Vec::new(), at, cx)
    }

    /// [`Self::open_user_menu`] with `extra` rows above it.
    pub fn open_user_menu_extra(
        &mut self,
        room: morpheus_core::OwnedRoomId,
        user: morpheus_core::OwnedUserId,
        name: String,
        extra: Vec<crate::view_model::menu::MenuItem>,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let members = session.members(room.clone());
        let permissions = session.my_permissions(room.clone());
        let mutual = session.mutual_spaces(user.clone());
        cx.spawn(async move |this, cx| {
            let members = members.await.unwrap_or_default();
            let permissions = permissions.await.unwrap_or_default();
            let mutual = mutual.await.unwrap_or_default();
            // Someone who has left still gets a menu (profile, message, block).
            let member = members
                .into_iter()
                .find(|member| member.user_id == user)
                .unwrap_or(Member {
                    user_id: user,
                    name,
                    avatar: None,
                    role: MemberRole::Member,
                    power_level: 0,
                    is_ignored: false,
                });
            this.update(cx, |this, cx| {
                this.open_member_menu_extra(room, &member, permissions, mutual, extra, at, cx);
            })
            .ok();
        })
        .detach();
    }

    /// "Message": opens the direct message with them, making it if need be.
    pub fn message_user(&mut self, user: morpheus_core::OwnedUserId, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let opening = session.dm_with(user);
        cx.spawn(async move |this, cx| {
            let result = opening.await;
            this.update(cx, |this, cx| match result {
                Ok(room) => {
                    this.open_room_in(Scope::Dms, room, cx);
                }
                Err(err) => this.fail("opening a DM", &err.to_string(), cx),
            })
            .ok();
        })
        .detach();
    }

    /// A moderation call; the member list is read again after it.
    pub fn moderate(
        &mut self,
        what: &'static str,
        action: morpheus_core::CoreFuture<()>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let result = action.await;
            this.update(cx, |this, cx| {
                if let Err(err) = result {
                    this.fail(what, &err.to_string(), cx);
                }
                this.refresh_members(cx);
            })
            .ok();
        })
        .detach();
    }
}

impl Shell {
    /// The full emoji picker beside the hover bar's "more" button; what is
    /// chosen reacts to `message`.
    pub fn open_picker(
        &mut self,
        message: morpheus_core::MessageId,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        use crate::emoji_picker::{EmojiPicker, PickerEvent};
        use gpui::AppContext as _;
        self.refresh_emotes(cx);
        let (packs, session) = (self.emote_packs.clone(), self.session.clone());
        let picker = cx.new(|cx| EmojiPicker::new(at, packs, session, cx));
        cx.subscribe(&picker, |this: &mut Self, _, event: &PickerEvent, cx| {
            let Some((_, message)) = this.picker.take() else {
                return;
            };
            if let PickerEvent::Picked(emoji) | PickerEvent::PickedCustom(emoji) = event
                && let Some((timeline, _)) = this.active_timeline(cx)
            {
                timeline.update(cx, |timeline, cx| {
                    timeline.react(message, emoji.clone(), cx)
                });
            }
            cx.notify();
        })
        .detach();
        self.picker = Some((picker, message));
        cx.notify();
    }
}
