//! The member list (`crates/ui/src/components/members.rs`): joined members
//! of the open room grouped by role, a picture each, a click for their
//! profile and a right click for their menu. What the groups are and what
//! the menu offers is `view_model::members`; this fetches the list and
//! draws it.
//!
//! A room of thousands (#115): the members already known show at once and
//! the whole list replaces them when it is in, and only the rows on screen
//! are drawn and have their pictures fetched (a uniform list: every row is
//! [`ROW`] high, headings too).

use crate::view_model::i18n::t;
use std::{collections::HashSet, ops::Range, sync::Arc};

use gpui::{
    Context, EventEmitter, FontWeight, Image, IntoElement, MouseButton, Pixels, Point, Render,
    UniformListScrollHandle, Window, div, prelude::*, px, rgb, rgba, uniform_list,
};
use morpheus_core::{
    MediaRef, Member, MemberPermissions, MemberRole, OwnedRoomId, OwnedUserId, Session,
};

use crate::{
    e2e::{E2eExt as _, HandleProbe},
    media::{self, Avatars, avatar},
    theme,
    view_model::{
        members::{Row, rows},
        nicknames::{NickMap, shown_name},
    },
};

/// Every row's height, a heading's as a member's: the list draws only what
/// is on screen, which needs rows of one height.
const ROW: f32 = 42.0;

/// What the member list tells the shell.
pub enum MemberEvent {
    /// A member's row clicked: their profile.
    Open(Box<Member>, Point<Pixels>),
    /// A member's row right-clicked: their menu, with what we may do here.
    Context(Box<Member>, MemberPermissions, Point<Pixels>),
}

impl EventEmitter<MemberEvent> for MembersView {}

/// A list read, and whether it is everyone (else what was known then).
type Kept = (Arc<[Member]>, bool);

/// Lists already read, so a room opened again shows its members at once
/// (the skeleton is for the first read only); newest rooms kept.
fn cache() -> &'static std::sync::Mutex<std::collections::HashMap<(String, OwnedRoomId), Kept>> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<(String, OwnedRoomId), Kept>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The room's kept member lists are forgotten (it was locked).
pub fn forget_room(room: &OwnedRoomId) {
    if let Ok(mut cache) = cache().lock() {
        cache.retain(|(_, cached), _| cached != room);
    }
}

pub struct MembersView {
    _alive: crate::memory::Alive,
    session: Option<Session>,
    room: OwnedRoomId,
    /// Shared, not copied: drawn every frame, and big rooms have thousands.
    members: Option<Result<Arc<[Member]>, String>>,
    /// The list's rows (headings and members), worked out once per list.
    rows: Arc<[Row]>,
    /// `members` is everyone, not only those known before the list came.
    complete: bool,
    /// Your nicknames for people, shown over their names.
    nicknames: NickMap,
    permissions: MemberPermissions,
    images: Avatars,
    /// People whose picture this list wanted (their row was drawn).
    asked: HashSet<OwnedUserId>,
    scroll: UniformListScrollHandle,
}

impl MembersView {
    /// A member of the loaded list, by id.
    pub fn member(&self, user: &OwnedUserId) -> Option<Member> {
        let Some(Ok(members)) = &self.members else {
            return None;
        };
        members.iter().find(|m| &m.user_id == user).cloned()
    }

    /// What the signed-in user may do to the members of this room.
    pub fn permissions(&self) -> MemberPermissions {
        self.permissions
    }

    /// Drops this list's pictures from the shared copies.
    pub fn forget_pictures(&self) {
        self.images.forget_shared();
    }

    pub fn new(session: Option<Session>, room: OwnedRoomId, cx: &mut Context<Self>) -> Self {
        let known = session.as_ref().and_then(|s| s.user_id()).and_then(|user| {
            cache()
                .lock()
                .ok()?
                .get(&(user.to_string(), room.clone()))
                .cloned()
        });
        let mut this = Self {
            session,
            room,
            members: None,
            rows: Arc::from([]),
            complete: false,
            nicknames: NickMap::new(),
            permissions: MemberPermissions::default(),
            images: Avatars::default(),
            asked: HashSet::new(),
            _alive: crate::memory::Alive::new(&crate::memory::MEMBER_VIEWS),
            scroll: UniformListScrollHandle::new(),
        };
        if let Some((list, complete)) = known {
            this.show(list, complete, cx);
        }
        this.refresh(cx);
        this
    }

    pub fn set_nicknames(&mut self, nicknames: NickMap, cx: &mut Context<Self>) {
        if self.nicknames != nicknames {
            self.nicknames = nicknames;
            cx.notify();
        }
    }

    /// `list` is shown (and kept for the room), unless it is only the
    /// members known so far and everyone is shown already.
    fn show(&mut self, list: Arc<[Member]>, complete: bool, cx: &mut Context<Self>) {
        if self.complete && !complete {
            return;
        }
        if let Some(user) = self.session.as_ref().and_then(|s| s.user_id())
            && let Ok(mut cache) = cache().lock()
        {
            if cache.len() >= 40 {
                cache.clear();
            }
            cache.insert((user.to_string(), self.room.clone()), (list.clone(), complete));
        }
        self.rows = rows(&list).into();
        self.members = Some(Ok(list));
        self.complete = complete;
        cx.notify();
    }

    /// Reads the list and our powers again (someone joined or left, or a
    /// moderation action ran): the members known at once, then, unless
    /// that was everyone, the whole list (a request to the server the
    /// first time, seconds in a room of thousands).
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let known = session.members_known(self.room.clone());
        let permissions = session.my_permissions(self.room.clone());
        let first = self.members.is_none();
        let room = self.room.clone();
        cx.spawn(async move |this, cx| {
            // Test builds can hold the first read back, to look at the skeleton.
            if first
                && std::env::var_os("MORPHEUS_E2E_PORT").is_some()
                && let Some(ms) = std::env::var("MORPHEUS_E2E_MEMBERS_DELAY_MS")
                    .ok()
                    .and_then(|v| v.parse().ok())
            {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(ms))
                    .await;
            }
            let complete = match known.await {
                Ok(known) => {
                    let complete = known.complete;
                    // Nobody known yet (not even you): the skeleton stays
                    // until the list comes.
                    if !known.members.is_empty() || complete {
                        this.update(cx, |this, cx| {
                            this.show(known.members.into(), complete, cx)
                        })
                        .ok();
                    }
                    complete
                }
                Err(_) => false,
            };
            let permissions = permissions.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                this.permissions = permissions;
                cx.notify();
            })
            .ok();
            if complete {
                return;
            }
            let whole = session.members(room).await;
            this.update(cx, |this, cx| match whole {
                Ok(list) => this.show(list.into(), true, cx),
                // An error shows only over nothing: a partial list is better.
                Err(err) if this.members.is_none() => {
                    this.members = Some(Err(err.to_string()));
                    cx.notify();
                }
                Err(err) => tracing::debug!(%err, "the room's whole member list failed"),
            })
            .ok();
        })
        .detach();
    }

    /// The picture for `media`, starting its fetch if new.
    fn picture(
        &mut self,
        user: &OwnedUserId,
        media: &MediaRef,
        cx: &mut Context<Self>,
    ) -> Option<Arc<Image>> {
        const SIZE: u32 = 64;
        if self.asked.insert(user.clone()) {
            crate::memory::MEMBER_PICTURES_ASKED
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if let Some(image) = self.images.get(media, SIZE) {
            return Some(image);
        }
        let session = self.session.clone()?;
        if self.images.begin(media, SIZE) {
            let fetching = media::fetch_picture(
                &session,
                Some((user.clone(), Some(self.room.clone()))),
                media,
                SIZE,
                SIZE,
            );
            let media = media.clone();
            cx.spawn(async move |this, cx| {
                let image = media::fetched(cx, fetching, (SIZE, SIZE), false).await;
                this.update(cx, |this, cx| {
                    this.images.finish(&media, SIZE, image);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        // Meanwhile a copy already read elsewhere (the DM list, a message).
        self.images.get_or_stand_in(media, SIZE)
    }

    /// The rows in `range`, drawn (only those on screen are asked for).
    fn draw_rows(
        &mut self,
        list: &[Member],
        rows: &[Row],
        range: Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        range
            .filter_map(|ix| rows.get(ix).copied())
            .filter_map(|row| match row {
                Row::Heading { role, total } => Some(heading(role, total).into_any_element()),
                Row::Member(i) => {
                    let member = list.get(i)?;
                    let picture = member
                        .avatar
                        .clone()
                        .and_then(|m| self.picture(&member.user_id, &m, cx));
                    let shown = crate::streamer::name(&shown_name(
                        &self.nicknames,
                        member.user_id.as_str(),
                        &member.name,
                    ));
                    // Still to come: a skeleton, not the letter on blue.
                    let waiting = picture.is_none()
                        && member.avatar.as_ref().is_some_and(|m| self.images.waiting(m, 64));
                    Some(member_row(member, shown, picture, waiting, cx).into_any_element())
                }
            })
            .collect()
    }
}

impl Render for MembersView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut panel = div()
            .id("members")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .line_height(gpui::relative(theme::LINE_HEIGHT));
        match self.members.clone() {
            None => {
                // Waiting shows the shape of what is coming (the timeline's
                // grey picture and bars), not words.
                for (index, width) in [62.0, 48.0, 75.0, 40.0, 56.0, 68.0, 45.0, 58.0]
                    .into_iter()
                    .enumerate()
                {
                    panel = panel.child(skeleton_row(index, width));
                }
            }
            Some(Err(err)) => {
                panel = panel.child(
                    div()
                        .px(px(16.0))
                        .py(px(12.0))
                        .text_color(rgb(theme::danger()))
                        .child(err.clone())
                        .e2e("p", "error pad")
                        .text(err),
                );
            }
            Some(Ok(list)) => {
                let rows = self.rows.clone();
                panel = panel.child(
                    uniform_list(
                        "member-rows",
                        rows.len(),
                        cx.processor(move |this, range: Range<usize>, _window, cx| {
                            this.draw_rows(&list, &rows, range, cx)
                        }),
                    )
                    .track_scroll(self.scroll.clone())
                    .flex_1()
                    .min_h_0()
                    .px(px(8.0))
                    .pb(px(8.0)),
                );
            }
        }
        let probe = HandleProbe(self.scroll.0.borrow().base_handle.clone());
        panel
            .e2e("aside", "members")
            .scrollable(std::rc::Rc::new(probe))
    }
}

/// A role's heading: "ADMINS — 2", at the foot of its row.
fn heading(role: MemberRole, total: usize) -> impl IntoElement {
    let heading = Row::heading(role, total);
    div()
        .h(px(ROW))
        .flex()
        .flex_col()
        .justify_end()
        .px(px(8.0))
        .pb(px(4.0))
        .child(
            div()
                .text_size(px(0.75 * theme::REM))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(theme::text_muted()))
                .child(heading.to_uppercase())
                .e2e("h3", "")
                .text(heading),
        )
        .e2e("header", "members-header")
}

/// One member: picture, name, "blocked" when you blocked them.
fn member_row(
    member: &Member,
    shown: String,
    picture: Option<Arc<Image>>,
    waiting: bool,
    cx: &mut Context<MembersView>,
) -> impl IntoElement {
    let (open, menu) = (Box::new(member.clone()), member.clone());
    let permissions_of = |this: &MembersView| this.permissions;
    let mut row = div()
        .id(gpui::SharedString::from(format!(
            "member-{}",
            member.user_id
        )))
        .flex()
        .items_center()
        .gap(px(12.0))
        .w_full()
        .h(px(ROW))
        .flex_none()
        .px(px(8.0))
        .rounded(px(theme::RADIUS_SM))
        .text_color(rgb(theme::text_muted()))
        .cursor_pointer()
        .hover(|s| s.bg(rgba(theme::bg_hover())).text_color(rgb(theme::text())))
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
        .child(if waiting {
            media::avatar_waiting(32.0, None).into_any_element()
        } else {
            avatar(picture, &member.name, 32.0, None).into_any_element()
        })
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
                .child(t("blocked"))
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
        .attr("title", crate::streamer::id(member.user_id.as_str()))
}

/// A member's row, greyed: a round picture and a name's worth of bar.
fn skeleton_row(index: usize, width: f32) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(10.0))
        .mx(px(8.0))
        .px(px(8.0))
        .py(px(4.0))
        .child(
            div()
                .size(px(32.0))
                .flex_none()
                .rounded_full()
                .bg(theme::fill_elevated())
                .e2e("div", "avatar skeleton"),
        )
        .child(
            div()
                .h(px(14.0))
                .w(gpui::relative(width / 100.0))
                .rounded(px(7.0))
                .bg(theme::fill_elevated())
                .id(gpui::SharedString::from(format!("member-sk{index}")))
                .e2e("div", "skeleton skeleton-line"),
        )
        .e2e("div", "skeleton-row")
}
