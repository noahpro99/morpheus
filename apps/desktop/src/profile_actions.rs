//! Opening the profile card and what its menu and buttons do for the shell.

use gpui::{AppContext as _, Context, Pixels, Point};
use morpheus_core::{MediaRef, OwnedUserId, RoomSummary};

use crate::{
    menu::OpenMenu,
    model::Scope,
    profile::{ProfileCard, ProfileEvent},
    shell::Shell,
    view_model::{menu::MenuTarget, profile::more_menu, room_menu::invite_targets},
};

impl Shell {
    /// The card of `user`, popping up beside `at`. `seen`: their name and
    /// picture as last seen on screen, shown until the profile arrives.
    pub fn open_profile(
        &mut self,
        user: OwnedUserId,
        at: Option<Point<Pixels>>,
        seen: Option<(String, Option<MediaRef>)>,
        cx: &mut Context<Self>,
    ) {
        let servers: Vec<RoomSummary> = self
            .rooms
            .iter()
            .filter(|r| r.is_space && !r.invited)
            .cloned()
            .collect();
        let session = self.session.clone();
        let room = self.selected.clone();
        let nickname = self.nicknames.get(user.as_str()).cloned();
        let card = cx.new(|cx| ProfileCard::new(session, user, room, at, seen, servers, cx));
        card.update(cx, |card, _| card.nickname = nickname);
        cx.subscribe(&card, |this: &mut Self, card, event: &ProfileEvent, cx| match event {
            ProfileEvent::Closed => {
                this.card = None;
                cx.notify();
            }
            ProfileEvent::OpenServer(space) => {
                this.card = None;
                this.set_scope(Scope::Space(space.clone()), cx);
            }
            ProfileEvent::OpenDm(room) => {
                this.card = None;
                this.set_scope(Scope::Dms, cx);
                this.open_room(room.clone(), cx);
            }
            ProfileEvent::Copied(link) => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(link.clone()));
                this.toast("Link copied", cx);
            }
            ProfileEvent::ModView(user) => {
                let shown = this.card.as_ref().map(|card| card.read(cx).name());
                this.card = None;
                this.open_mod_view(user.clone(), shown, cx);
            }
            ProfileEvent::More(at) => {
                let (user, own, blocked) = {
                    let card = card.read(cx);
                    (card.user.clone(), card.is_own(), card.is_blocked())
                };
                let items = more_menu(&user, own, blocked, &invite_targets(&this.rooms));
                this.open_menu(OpenMenu::new(*at, items, false, MenuTarget::Other), cx);
            }
        })
        .detach();
        self.card = Some(card);
        cx.notify();
    }

    /// Swaps the open card for the person's full profile.
    pub fn show_full_profile(&mut self, cx: &mut Context<Self>) {
        if let Some(card) = &self.card {
            card.update(cx, |card, cx| card.show_full(cx));
        }
    }

    /// Reads whether the person on the open card is blocked again.
    pub fn refresh_profile_block(&mut self, cx: &mut Context<Self>) {
        if let Some(card) = &self.card {
            card.update(cx, |card, cx| card.refresh_blocked(cx));
        }
    }
}
