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
        room_menu::invite_targets,
    },
};

impl Shell {
    /// The member list, shown or not (kept across rooms).
    pub fn toggle_members(&mut self, cx: &mut Context<Self>) {
        self.members_open = !self.members_open;
        cx.notify();
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
        let own = self
            .session
            .as_ref()
            .and_then(|session| session.user_id())
            .as_ref()
            == Some(&member.user_id);
        let items = member_menu(&MemberMenuInput {
            room: &room,
            member,
            permissions,
            own,
            friend: self.friends.contains(&member.user_id),
            invite_to: invite_targets(&self.rooms),
        });
        self.open_menu(OpenMenu::new(at, items, false, MenuTarget::Other), cx);
    }

    /// The member menu for whoever sent a message: the member and our
    /// powers are looked up first, so the menu opens where the pointer was
    /// once they arrive.
    pub fn open_sender_menu(&mut self, message: &Message, at: Point<Pixels>, cx: &mut Context<Self>) {
        let (Some(session), Some(room)) = (self.session.clone(), self.selected.clone()) else {
            return;
        };
        let (user, name) = (message.sender.clone(), message.sender_name.clone());
        let members = session.members(room.clone());
        let permissions = session.my_permissions(room);
        cx.spawn(async move |this, cx| {
            let members = members.await.unwrap_or_default();
            let permissions = permissions.await.unwrap_or_default();
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
            this.update(cx, |this, cx| this.open_member_menu(&member, permissions, at, cx))
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
                    this.set_scope(Scope::Dms, cx);
                    this.open_room(room, cx);
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
    pub fn open_picker(&mut self, message: morpheus_core::MessageId, at: Point<Pixels>, cx: &mut Context<Self>) {
        use crate::emoji_picker::{EmojiPicker, PickerEvent};
        use gpui::AppContext as _;
        let picker = cx.new(|cx| EmojiPicker::new(at, cx));
        cx.subscribe(&picker, |this: &mut Self, _, event: &PickerEvent, cx| {
            let Some((_, message)) = this.picker.take() else {
                return;
            };
            if let PickerEvent::Picked(emoji) = event
                && let Some((_, timeline)) = &this.timeline
            {
                timeline.update(cx, |timeline, cx| timeline.react(message, emoji.clone(), cx));
            }
            cx.notify();
        })
        .detach();
        self.picker = Some((picker, message));
        cx.notify();
    }
}
