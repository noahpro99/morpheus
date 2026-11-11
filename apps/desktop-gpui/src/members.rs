//! The member list (`crates/ui/src/components/members.rs`): joined members
//! of the open room grouped by role, a picture each, a click for their
//! profile and a right click for their menu. What the groups are and what
//! the menu offers is `view_model::members`; this fetches the list and
//! draws it.

use std::sync::Arc;

use gpui::{
    Context, EventEmitter, FontWeight, Image, IntoElement, MouseButton, Pixels, Point, Render,
    ScrollHandle, Window, div, prelude::*, px, rgb, rgba,
};
use morpheus_core::{Member, MemberPermissions, MediaRef, OwnedRoomId, Session};

use crate::{
    e2e::{E2eExt as _, HandleProbe},
    media::{self, Avatars, avatar},
    theme,
    view_model::{
        members::{PAGE, groups, show_more_label},
        nicknames::{NickMap, shown_name},
    },
};

/// What the member list tells the shell.
pub enum MemberEvent {
    /// A member's row clicked: their profile.
    Open(Box<Member>, Point<Pixels>),
    /// A member's row right-clicked: their menu, with what we may do here.
    Context(Box<Member>, MemberPermissions, Point<Pixels>),
}

impl EventEmitter<MemberEvent> for MembersView {}

pub struct MembersView {
    session: Option<Session>,
    room: OwnedRoomId,
    members: Option<Result<Vec<Member>, String>>,
    /// Your nicknames for people, shown over their names.
    nicknames: NickMap,
    permissions: MemberPermissions,
    /// How many rows show (the list grows by a page on request).
    shown: usize,
    images: Avatars,
    scroll: ScrollHandle,
}

impl MembersView {
    pub fn new(session: Option<Session>, room: OwnedRoomId, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            session,
            room,
            members: None,
            nicknames: NickMap::new(),
            permissions: MemberPermissions::default(),
            shown: PAGE,
            images: Avatars::default(),
            scroll: ScrollHandle::new(),
        };
        this.refresh(cx);
        this
    }

    pub fn set_nicknames(&mut self, nicknames: NickMap, cx: &mut Context<Self>) {
        if self.nicknames != nicknames {
            self.nicknames = nicknames;
            cx.notify();
        }
    }

    /// Reads the list and our powers again (someone joined or left, or a
    /// moderation action ran).
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let members = session.members(self.room.clone());
        let permissions = session.my_permissions(self.room.clone());
        cx.spawn(async move |this, cx| {
            let list = members.await.map_err(|err| err.to_string());
            let permissions = permissions.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                this.members = Some(list);
                this.permissions = permissions;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The picture for `media`, starting its fetch if new.
    fn picture(&mut self, media: &MediaRef, cx: &mut Context<Self>) -> Option<Arc<Image>> {
        const SIZE: u32 = 64;
        if let Some(image) = self.images.get(media, SIZE) {
            return Some(image);
        }
        let session = self.session.clone()?;
        if self.images.begin(media, SIZE) {
            let fetching = session.thumbnail(media.clone(), SIZE, SIZE);
            let media = media.clone();
            cx.spawn(async move |this, cx| {
                let image = fetching
                    .await
                    .ok()
                    .flatten()
                    .and_then(|bytes| media::decode(&bytes));
                this.update(cx, |this, cx| {
                    this.images.finish(&media, SIZE, image);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        None
    }
}

impl Render for MembersView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut panel = div()
            .id("members")
            .flex()
            .flex_col()
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .bg(rgb(theme::BG_SIDEBAR))
            .line_height(gpui::relative(theme::LINE_HEIGHT));
        match self.members.clone() {
            None => {}
            Some(Err(err)) => {
                panel = panel.child(
                    div()
                        .px(px(16.0))
                        .py(px(12.0))
                        .text_color(rgb(theme::DANGER))
                        .child(err.clone())
                        .e2e("p", "error pad")
                        .text(err),
                );
            }
            Some(Ok(list)) => {
                let (groups, hidden) = groups(&list, self.shown);
                for group in groups {
                    let heading = group.heading();
                    panel = panel.child(
                        div()
                            .px(px(16.0))
                            .pt(px(16.0))
                            .pb(px(4.0))
                            .child(
                                div()
                                    .text_size(px(0.75 * theme::REM))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(theme::TEXT_MUTED))
                                    .child(heading.to_uppercase())
                                    .e2e("h3", "")
                                    .text(heading),
                            )
                            .e2e("header", "members-header"),
                    );
                    for member in group.members {
                        let picture = member.avatar.clone().and_then(|m| self.picture(&m, cx));
                        let shown = shown_name(&self.nicknames, member.user_id.as_str(), &member.name);
                        panel = panel.child(member_row(&member, shown, picture, cx));
                    }
                }
                if hidden > 0 {
                    let label = show_more_label(hidden);
                    panel = panel.child(
                        div()
                            .id("show-more-members")
                            .mx(px(16.0))
                            .mt(px(8.0))
                            .mb(px(16.0))
                            .text_size(px(0.85 * theme::REM))
                            .text_color(rgb(theme::TEXT_MUTED))
                            .cursor_pointer()
                            .hover(|s| s.text_color(rgb(theme::TEXT)))
                            .child(label.clone())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.shown += PAGE;
                                cx.notify();
                            }))
                            .e2e("button", "ghost show-more-members")
                            .text(label),
                    );
                }
            }
        }
        panel
            .e2e("aside", "members")
            .scrollable(std::rc::Rc::new(HandleProbe(self.scroll.clone())))
    }
}

/// One member: picture, name, "blocked" when you blocked them.
fn member_row(
    member: &Member,
    shown: String,
    picture: Option<Arc<Image>>,
    cx: &mut Context<MembersView>,
) -> impl IntoElement {
    let (open, menu) = (Box::new(member.clone()), member.clone());
    let permissions_of = |this: &MembersView| this.permissions;
    let mut row = div()
        .id(gpui::SharedString::from(format!("member-{}", member.user_id)))
        .flex()
        .items_center()
        .gap(px(10.0))
        .mx(px(8.0))
        .px(px(8.0))
        .py(px(4.0))
        .rounded(px(theme::RADIUS_SM))
        .text_color(rgb(theme::TEXT_MUTED))
        .cursor_pointer()
        .hover(|s| s.bg(rgba(theme::BG_HOVER)).text_color(rgb(theme::TEXT)))
        .when(member.is_ignored, |d| d.opacity(0.6))
        .on_click(cx.listener(move |_, event: &gpui::ClickEvent, _, cx| {
            cx.emit(MemberEvent::Open(open.clone(), event.position()))
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                cx.emit(MemberEvent::Context(
                    Box::new(menu.clone()),
                    permissions_of(this),
                    event.position,
                ))
            }),
        )
        .child(avatar(picture, &member.name, 32.0, None))
        .child(
            div()
                .min_w_0()
                .truncate()
                .child(shown.clone())
                .e2e("span", "member-name")
                .text(shown),
        );
    if member.is_ignored {
        row = row.child(
            div()
                .ml_auto()
                .text_size(px(0.65 * theme::REM))
                .child("blocked")
                .e2e("span", "tag")
                .attr("title", "You blocked this user")
                .text("blocked"),
        );
    }
    let classes = if member.is_ignored {
        "member blocked"
    } else {
        "member"
    };
    row.e2e("button", classes)
        .attr("title", member.user_id.to_string())
}
