//! The profile card and the full profile (`crates/ui/src/components/profile.rs`):
//! opened by clicking a picture or name in the timeline, a member in the
//! list, or "Profile" in a menu. The card pops up beside the click with the
//! person's picture and name, their role in the open room, the servers you
//! share, a private note (saved as you write) and a box to message them;
//! its "…" opens a menu (the full profile, share, invite, block, report,
//! copy), and the shield opens the mod view.

use std::{sync::Arc, time::Duration};

use gpui::{
    AnyElement, Context, Entity, EventEmitter, FocusHandle, FontWeight, Image, IntoElement,
    MouseButton, Pixels, Point, Render, Subscription, Window, anchored, deferred, div,
    linear_color_stop, linear_gradient, prelude::*, px, relative, rgb, rgba,
};
use morpheus_core::{
    MediaRef, MemberRole, OwnedRoomId, OwnedUserId, Profile, RoomSummary, Session,
};

use crate::{
    e2e::E2eExt as _,
    icons::icon,
    input::{Changed, Submit, TextInput},
    media::{self, Avatars, avatar},
    theme,
    view_model::room_dialogs::Note,
    view_model::profile::{
        CARD_W, Area, card_max_height, mutual_heading, mutual_servers_label, place_card, role_tag,
        shared_servers,
    },
};

/// What a profile tells the shell.
pub enum ProfileEvent {
    Closed,
    /// A mutual server picked on the full profile.
    OpenServer(OwnedRoomId),
    /// A message sent from the card: the direct message it went to.
    OpenDm(OwnedRoomId),
    /// The "…" button, where: the menu is the shell's.
    More(Point<Pixels>),
    /// The shield button: this person's mod view.
    ModView(OwnedUserId),
}

impl EventEmitter<ProfileEvent> for ProfileCard {}

/// How long the note waits after the last key before it is saved.
const NOTE_QUIET: Duration = Duration::from_millis(700);

pub struct ProfileCard {
    session: Option<Session>,
    pub user: OwnedUserId,
    own: bool,
    /// Where it was opened (the click), if any.
    at: Option<Point<Pixels>>,
    /// The full-page profile instead of the card.
    full: bool,
    profile: Option<Profile>,
    /// Your nickname for them, shown over their name.
    pub nickname: Option<String>,
    /// Whether a room was open where it was opened (the mod view needs one).
    in_room: bool,
    /// "Report user profile": the inline form, its reason and answer.
    reporting: bool,
    report_reason: Entity<TextInput>,
    report_note: Option<Note>,
    report_busy: bool,
    /// As seen where it was opened from, until the profile arrives.
    seen: Option<(String, Option<MediaRef>)>,
    role: Option<MemberRole>,
    blocked: bool,
    mutual: Vec<OwnedRoomId>,
    /// Joined servers, for the names of the ones you share.
    servers: Vec<RoomSummary>,
    note: Entity<TextInput>,
    note_saved: bool,
    /// Counts edits, so only the last one's timer saves.
    note_edits: u64,
    note_dirty: bool,
    message: Entity<TextInput>,
    images: Avatars,
    focus: FocusHandle,
    take_focus: bool,
    _subscriptions: Vec<Subscription>,
}

impl ProfileCard {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session: Option<Session>,
        user: OwnedUserId,
        open_in: Option<OwnedRoomId>,
        at: Option<Point<Pixels>>,
        seen: Option<(String, Option<MediaRef>)>,
        servers: Vec<RoomSummary>,
        cx: &mut Context<Self>,
    ) -> Self {
        let own = session
            .as_ref()
            .and_then(|s| s.user_id())
            .as_ref()
            == Some(&user);
        let name = seen
            .as_ref()
            .map_or_else(|| user.localpart().to_owned(), |(n, _)| n.clone());
        let note = cx.new(|cx| {
            TextInput::new("note", "Only you can see this", false, "", cx)
                .multiline(3)
                .bare("textarea", "note")
        });
        let message = cx.new(|cx| {
            TextInput::new("message", &format!("Message @{name}"), false, "", cx)
                .bare("input", "card-message")
        });
        let report_reason = cx.new(|cx| TextInput::new("reason", "What is wrong?", false, "", cx).framed());
        let subscriptions = vec![
            cx.subscribe(&report_reason, |this, _, _: &Submit, cx| this.send_report(cx)),
            cx.subscribe(&note, |this, _, _: &Changed, cx| this.note_changed(cx)),
            cx.subscribe(&message, |this, _, _: &Submit, cx| this.send_message(cx)),
        ];
        let mut this = Self {
            session: session.clone(),
            user: user.clone(),
            own,
            at,
            full: false,
            profile: None,
            seen,
            nickname: None,
            in_room: open_in.is_some(),
            reporting: false,
            report_reason,
            report_note: None,
            report_busy: false,
            role: None,
            blocked: false,
            mutual: Vec::new(),
            servers,
            note,
            note_saved: false,
            note_edits: 0,
            note_dirty: false,
            message,
            images: Avatars::default(),
            focus: cx.focus_handle(),
            take_focus: true,
            _subscriptions: subscriptions,
        };
        if let Some(session) = session {
            let profile = session.profile(user.clone());
            let mutual = session.mutual_spaces(user.clone());
            let note = session.note(user.clone());
            let blocked = session.is_blocked(user.clone());
            let role = open_in.map(|room| session.member_role(room, user.clone()));
            cx.spawn(async move |this, cx| {
                let profile = profile.await.ok();
                let mutual = mutual.await.unwrap_or_default();
                let stored = note.await.unwrap_or_default();
                let blocked = blocked.await.unwrap_or(false);
                let role = match role {
                    Some(role) => role.await.ok().flatten(),
                    None => None,
                };
                this.update(cx, |this, cx| {
                    this.profile = profile;
                    this.mutual = mutual;
                    this.blocked = blocked;
                    this.role = role;
                    // A late fetch must not put the old note over what was typed.
                    if !this.note_dirty {
                        this.note.update(cx, |input, cx| input.set_value(&stored, cx));
                        this.note_dirty = false;
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        this.take_focus = true;
        this
    }

    /// The full-page profile of the same person.
    pub fn show_full(&mut self, cx: &mut Context<Self>) {
        self.full = true;
        self.take_focus = true;
        cx.notify();
    }

    /// Whether you blocked them, read again (the menu changed it).
    pub fn refresh_blocked(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let blocked = session.is_blocked(self.user.clone());
        cx.spawn(async move |this, cx| {
            if let Ok(blocked) = blocked.await {
                this.update(cx, |this, cx| {
                    this.blocked = blocked;
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    pub fn is_blocked(&self) -> bool {
        self.blocked
    }

    /// "Report user profile": shows the form.
    pub fn start_report(&mut self, cx: &mut Context<Self>) {
        self.reporting = true;
        self.report_note = None;
        cx.notify();
    }

    fn send_report(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        if self.report_busy {
            return;
        }
        self.report_busy = true;
        let reporting = session.report_user(self.user.clone(), self.report_reason.read(cx).value().to_owned());
        cx.spawn(async move |this, cx| {
            let result = reporting.await;
            this.update(cx, |this, cx| {
                this.report_busy = false;
                this.report_note = Some(match result {
                    Ok(()) => Note::ok("Reported to the server admins."),
                    Err(err) => Note::error(err.to_string()),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub fn is_own(&self) -> bool {
        self.own
    }

    /// The person's name as known now.
    pub fn name(&self) -> String {
        if let Some(nickname) = &self.nickname {
            return nickname.clone();
        }
        match (&self.profile, &self.seen) {
            (Some(p), _) => p.name.clone(),
            (None, Some((name, _))) => name.clone(),
            (None, None) => self.user.localpart().to_owned(),
        }
    }

    fn picture_media(&self) -> Option<MediaRef> {
        match (&self.profile, &self.seen) {
            (Some(p), _) => p.avatar.clone(),
            (None, Some((_, avatar))) => avatar.clone(),
            (None, None) => None,
        }
    }

    fn picture(&mut self, size: u32, cx: &mut Context<Self>) -> Option<Arc<Image>> {
        let media = self.picture_media()?;
        if let Some(image) = self.images.get(&media, size) {
            return Some(image);
        }
        let session = self.session.clone()?;
        if self.images.begin(&media, size) {
            let fetching = session.thumbnail(media.clone(), size, size);
            let fetched_media = media.clone();
            cx.spawn(async move |this, cx| {
                let image = fetching.await.ok().flatten().and_then(|bytes| media::decode(&bytes));
                this.update(cx, |this, cx| {
                    this.images.finish(&fetched_media, size, image);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        // The copy already on screen (the member row's) stands in meanwhile.
        self.images.get_or_stand_in(&media, size)
    }

    /// The note changed: saved once the writing pauses.
    fn note_changed(&mut self, cx: &mut Context<Self>) {
        self.note_dirty = true;
        self.note_saved = false;
        self.note_edits += 1;
        let edit = self.note_edits;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(NOTE_QUIET).await;
            this.update(cx, |this, cx| {
                if this.note_edits == edit {
                    this.save_note(cx);
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Saves the note if it changed.
    pub fn save_note(&mut self, cx: &mut Context<Self>) {
        if !self.note_dirty {
            return;
        }
        self.note_dirty = false;
        let Some(session) = self.session.clone() else {
            return;
        };
        let saving = session.set_note(self.user.clone(), self.note.read(cx).value().to_owned());
        cx.spawn(async move |this, cx| {
            let saved = saving.await.is_ok();
            this.update(cx, |this, cx| {
                this.note_saved = saved;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Enter in the message box: it goes to the DM with them, which opens.
    fn send_message(&mut self, cx: &mut Context<Self>) {
        let body = self.message.read(cx).value().trim().to_owned();
        let Some(session) = self.session.clone() else {
            return;
        };
        if body.is_empty() {
            return;
        }
        let user = self.user.clone();
        cx.spawn(async move |this, cx| {
            let Ok(dm) = session.dm_with(user).await else {
                return;
            };
            if let Ok(opened) = session.timeline(dm.clone()).await {
                opened.handle.send_text(body).await.ok();
            }
            this.update(cx, |_, cx| cx.emit(ProfileEvent::OpenDm(dm))).ok();
        })
        .detach();
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.save_note(cx);
        cx.emit(ProfileEvent::Closed);
    }
}

fn tag(text: &str, warn: bool) -> AnyElement {
    div()
        .px(px(8.0))
        .py(px(1.0))
        .rounded(px(10.0))
        .bg(if warn {
            rgba(theme::alpha(theme::warn(), 0x26))
        } else {
            rgba(theme::bg_elevated() << 8 | 0xff)
        })
        .text_size(px(0.7 * theme::REM))
        .font_weight(FontWeight::BOLD)
        .child(text.to_uppercase())
        .e2e("span", if warn { "tag warn" } else { "tag" })
        .text(text)
        .into_any_element()
}

impl Render for ProfileCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.take_focus) {
            window.focus(&self.focus);
        }
        if self.full {
            return self.render_full(window, cx);
        }
        let name = self.name();
        let picture = self.picture(144, cx);
        let size = window.viewport_size();
        let app = Area {
            x: 0.0,
            y: 0.0,
            w: f32::from(size.width),
            h: f32::from(size.height),
        };
        let (x, y) = place_card(
            self.at.map(|p| (f32::from(p.x), f32::from(p.y))),
            (CARD_W, 0.0),
            app,
        );

        let banner = div()
            .relative()
            .h(px(64.0))
            .rounded_tl(px(theme::RADIUS_XL - 1.0))
            .rounded_tr(px(theme::RADIUS_XL - 1.0))
            .w_full()
            .flex_none()
            .bg(linear_gradient(
                135.0,
                linear_color_stop(rgb(theme::accent_strong()), 0.0),
                linear_color_stop(rgb(0x2d3a9e), 1.0),
            ))
            .child(
                div()
                    .absolute()
                    .top(px(8.0))
                    .right(px(8.0))
                    .flex()
                    .gap(px(4.0))
                    .children((!self.own && self.in_room).then(|| {
                        let user = self.user.clone();
                        div()
                            .id("card-mod-view")
                            .size(px(28.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(theme::RADIUS_SM))
                            .bg(rgba(0x00000059))
                            .hover(|s| s.bg(rgba(0x0000008c)))
                            .cursor_pointer()
                            .child(icon("shield-check", 16.0).text_color(rgb(0xffffff)))
                            .on_click(cx.listener(move |_, _, _, cx| cx.emit(ProfileEvent::ModView(user.clone()))))
                            .e2e("button", "ghost card-button card-mod-view")
                            .attr("title", "Mod view")
                    }))
                    .child(
                        div()
                            .id("card-more")
                            .size(px(28.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(theme::RADIUS_SM))
                            .bg(rgba(0x00000059))
                            .hover(|s| s.bg(rgba(0x0000008c)))
                            .cursor_pointer()
                            .child(icon("ellipsis", 16.0).text_color(rgb(0xffffff)))
                            .on_click(cx.listener(|_, event: &gpui::ClickEvent, _, cx| {
                                cx.emit(ProfileEvent::More(event.position()))
                            }))
                            .e2e("button", "ghost card-button more")
                            .attr("title", "More"),
                    )
                    .e2e("div", "card-actions"),
            )
            .e2e("div", "profile-banner");

        let mut body = div()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(6.0))
            .px(px(16.0))
            .pb(px(16.0))
            .child(
                div()
                    .id("card-avatar")
                    .mt(px(-36.0))
                    .rounded_full()
                    .border_4()
                    .border_color(rgb(theme::bg_sidebar()))
                    .cursor_pointer()
                    .child(avatar(picture, &name, 72.0, None))
                    .on_click(cx.listener(|this, _, _, cx| this.show_full(cx)))
                    .e2e("button", "card-avatar")
                    .attr("title", "View full profile"),
            )
            .child(
                div()
                    .text_size(px(1.25 * theme::REM))
                    .font_weight(FontWeight::BOLD)
                    .mt(px(8.0))
                    .child(name.clone())
                    .e2e("h2", "profile-name")
                    .text(name.clone()),
            )
            .child(
                div()
                    .text_size(px(0.85 * theme::REM))
                    .text_color(rgb(theme::text_muted()))
                    .child(self.user.to_string())
                    .e2e("p", "profile-id muted")
                    .text(self.user.to_string()),
            );
        if let Some(status) = self.profile.as_ref().and_then(|p| p.status.clone()) {
            body = body.child(div().child(status.clone()).e2e("p", "profile-status").text(status));
        }
        let mut tags = div().flex().flex_wrap().gap(px(6.0));
        if self.own {
            tags = tags.child(tag("This is you", false));
        }
        if self.blocked {
            tags = tags.child(tag("Blocked", true));
        }
        if let Some(role) = self.role {
            tags = tags.child(tag(role_tag(role), false));
        }
        body = body.child(tags.e2e("div", "card-tags"));
        if !self.own {
            let count = shared_servers(&self.servers, &self.mutual).len();
            let label = mutual_servers_label(count);
            body = body.child(
                div()
                    .id("card-mutual")
                    .mt(px(8.0))
                    .text_size(px(0.85 * theme::REM))
                    .text_color(rgb(theme::accent()))
                    .cursor_pointer()
                    .hover(|s| s.underline())
                    .child(label.clone())
                    .on_click(cx.listener(|this, _, _, cx| this.show_full(cx)))
                    .e2e("button", "mutual-servers")
                    .attr("title", "View full profile")
                    .text(label),
            );
        }
        let mut note = div()
            .w_full()
            .mt(px(10.0))
            .pt(px(10.0))
            .border_t_1()
            .border_color(rgb(theme::bg_elevated()))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .text_size(px(0.7 * theme::REM))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(theme::text_faint()))
                    .child("NOTE")
                    .e2e("h4", "")
                    .text("Note"),
            )
            .child(
                div()
                    .w_full()
                    .px(px(10.0))
                    .rounded(px(theme::RADIUS))
                    .border_1()
                    .border_color(rgb(theme::bg_elevated()))
                    .bg(rgb(theme::bg_input()))
                    .text_size(px(0.9 * theme::REM))
                    .text_color(rgb(theme::text()))
                    .child(self.note.clone()),
            );
        if self.note_saved {
            note = note.child(
                div()
                    .text_size(px(0.75 * theme::REM))
                    .text_color(rgb(theme::text_muted()))
                    .child("Saved")
                    .e2e("span", "muted note-saved")
                    .text("Saved"),
            );
        }
        body = body.child(note.e2e("section", "card-section"));
        if !self.own {
            body = body.child(
                div()
                    .w_full()
                    .mt(px(12.0))
                    .px(px(12.0))
                    .rounded(px(theme::RADIUS))
                    .bg(rgb(theme::bg_elevated()))
                    .text_color(rgb(theme::text()))
                    .child(self.message.clone()),
            );
        }

        let card = div()
            .id("profile-card")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.close(cx);
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .occlude()
            .w(px(CARD_W))
            .max_h(px(card_max_height(app)))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .rounded(px(theme::RADIUS_XL))
            .bg(rgb(theme::bg_sidebar()))
            .border_1()
            .border_color(rgba(theme::edge()))
            .shadow_lg()
            .text_color(rgb(theme::text()))
            .line_height(relative(theme::LINE_HEIGHT))
            .child(banner)
            .child(body)
            .children(self.reporting.then(|| self.report_form(cx)))
            .e2e("div", "profile-card")
            .focusable(&self.focus);
        let layer = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close(cx)))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, _, _, cx| this.close(cx)))
            .child(
                anchored()
                    .position(gpui::point(px(x), px(y)))
                    .snap_to_window_with_margin(px(8.0))
                    .child(card),
            )
            .e2e("div", "overlay panel-overlay card-overlay");
        deferred(layer).with_priority(25).into_any_element()
    }
}

impl ProfileCard {
    /// The inline form under the card: why you report them.
    fn report_form(&self, cx: &mut Context<Self>) -> AnyElement {
        let note = self.report_note.as_ref().map(|note| match note {
            Note::Ok(text) => div().text_color(rgb(theme::ok())).child(text.clone()).e2e("p", "ok").text(text.clone()).into_any_element(),
            Note::Error(text) => div().text_color(rgb(theme::danger())).child(text.clone()).e2e("p", "error").text(text.clone()).into_any_element(),
        });
        let button = |id: &'static str, label: &'static str, danger: bool, kind: &'static str, submit: bool, on_click: Box<dyn Fn(&mut Self, &mut Context<Self>)>| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(px(6.0))
                .cursor_pointer()
                .rounded(px(6.0))
                .when(danger, |d| d.bg(rgb(theme::danger())).text_color(rgb(0xffffff)).px(px(14.0)).py(px(8.0)))
                .when(!danger, |d| d.px(px(6.0)).py(px(1.0)))
                .when(danger, |d| d.child(icon("flag", 16.0).text_color(rgb(0xffffff))))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| on_click(this, cx)))
                .e2e("button", kind)
                .attr("type", if submit { "submit" } else { "button" })
                .text(label)
                .into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .px(px(16.0))
            .pb(px(16.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .text_size(px(0.85 * theme::REM))
                    .text_color(rgb(theme::text_muted()))
                    .child("Report this user")
                    .child(div().text_size(px(theme::REM)).text_color(rgb(theme::text())).child(self.report_reason.clone()))
                    .e2e("label", "")
                    .text("Report this user"),
            )
            .children(note)
            .child(
                div()
                    .flex()
                    .gap(px(8.0))
                    .child(button("report-send", "Report", true, "danger", true, Box::new(|this, cx| this.send_report(cx))))
                    .child(button("report-cancel", "Cancel", false, "secondary", false, Box::new(|this, cx| {
                        this.reporting = false;
                        cx.notify();
                    }))),
            )
            .e2e("form", "report-user")
            .into_any_element()
    }

    /// The full-page profile: the card's person larger, with the servers
    /// you share listed (each opens its server).
    fn render_full(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let name = self.name();
        let picture = self.picture(160, cx);
        let top = 0.15 * f32::from(window.viewport_size().height);
        let mut body = div()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(4.0))
            .px(px(20.0))
            .pb(px(20.0))
            .child(
                div()
                    .mt(px(-40.0))
                    .rounded_full()
                    .border_color(rgb(theme::bg_sidebar()))
                    .border_4()
                    .child(avatar(picture, &name, 80.0, None)),
            )
            .child(
                div()
                    .mt(px(8.0))
                    .text_size(px(1.25 * theme::REM))
                    .font_weight(FontWeight::BOLD)
                    .child(name.clone())
                    .e2e("h2", "profile-name")
                    .text(name.clone()),
            )
            .child(
                div()
                    .text_size(px(0.85 * theme::REM))
                    .text_color(rgb(theme::text_muted()))
                    .child(self.user.to_string())
                    .e2e("p", "profile-id muted")
                    .text(self.user.to_string()),
            );
        if let Some(status) = self.profile.as_ref().and_then(|p| p.status.clone()) {
            body = body.child(div().child(status.clone()).e2e("p", "profile-status").text(status));
        }
        if self.own {
            body = body.child(tag("This is you", false));
        }
        if !self.own {
            let servers = shared_servers(&self.servers, &self.mutual);
            let heading = mutual_heading(servers.len());
            let mut section = div()
                .w_full()
                .mt(px(12.0))
                .pt(px(10.0))
                .border_t_1()
                .border_color(rgb(theme::bg_elevated()))
                .flex()
                .flex_col()
                .gap(px(6.0))
                .max_h(px(240.0))
                .child(
                    div()
                        .text_size(px(0.7 * theme::REM))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(theme::text_faint()))
                        .child(heading.to_uppercase())
                        .e2e("h4", "")
                        .text(heading),
                );
            if servers.is_empty() {
                section = section.child(
                    div()
                        .text_color(rgb(theme::text_muted()))
                        .child("None yet.")
                        .e2e("p", "muted")
                        .text("None yet."),
                );
            }
            for server in servers {
                let id = server.id.clone();
                let picture = self.server_picture(&server, cx);
                section = section.child(
                    div()
                        .id(gpui::SharedString::from(format!("mutual-{}", server.id)))
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .w_full()
                        .px(px(6.0))
                        .py(px(4.0))
                        .rounded(px(theme::RADIUS_SM))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgba(theme::bg_hover())))
                        .child(avatar(picture, &server.name, 28.0, Some(8.0)))
                        .child(server.name.clone())
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.emit(ProfileEvent::OpenServer(id.clone()))
                        }))
                        .e2e("button", "mutual-server")
                        .attr("title", format!("Open {}", server.name))
                        .text(server.name.clone()),
                );
            }
            body = body.child(section.e2e("section", "card-section mutual profile-mutual"));
        }
        let modal = div()
            .id("profile-modal")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.close(cx);
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .relative()
            .w(px(360.0))
            .max_w(relative(0.9))
            .max_h(px((f32::from(window.viewport_size().height) - top - 24.0).max(120.0)))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(theme::RADIUS_XL))
            .bg(rgb(theme::bg_sidebar()))
            .shadow_lg()
            .text_color(rgb(theme::text()))
            .line_height(relative(theme::LINE_HEIGHT))
            // The banner and the body scroll together under the modal's
            // height limit (the avatar overlaps the banner).
            .child(
                div()
                    .id("profile-scroll")
                    .flex()
                    .flex_col()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .h(px(72.0))
                            .w_full()
                            .flex_none()
                            .rounded_tl(px(theme::RADIUS_XL))
                            .rounded_tr(px(theme::RADIUS_XL))
                            .bg(linear_gradient(
                                135.0,
                                linear_color_stop(rgb(theme::accent_strong()), 0.0),
                                linear_color_stop(rgb(0x2d3a9e), 1.0),
                            ))
                            .e2e("div", "profile-banner"),
                    )
                    .child(body.e2e("div", "profile-body")),
            )
            .child(
                div()
                    .id("profile-close")
                    .absolute()
                    .top(px(8.0))
                    .right(px(8.0))
                    .p(px(6.0))
                    .cursor_pointer()
                    .child(icon("x", 18.0).text_color(rgb(0xffffff)))
                    .on_click(cx.listener(|this, _, _, cx| this.close(cx)))
                    .e2e("button", "ghost profile-close")
                    .attr("title", "Close"),
            )
            .e2e("div", "profile-modal")
            .focusable(&self.focus);
        let layer = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .justify_center()
            .items_start()
            .pt(px(top))
            .bg(rgba(0x0000008c))
            .occlude()
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close(cx)))
            .child(modal)
            .e2e("div", "overlay");
        deferred(layer).with_priority(25).into_any_element()
    }

    /// A server's picture (its letter until pictures are fetched here).
    fn server_picture(&mut self, server: &RoomSummary, cx: &mut Context<Self>) -> Option<Arc<Image>> {
        let media = server.avatar.clone()?;
        if let Some(image) = self.images.get(&media, 56) {
            return Some(image);
        }
        let session = self.session.clone()?;
        if self.images.begin(&media, 56) {
            let fetching = session.thumbnail(media.clone(), 56, 56);
            cx.spawn(async move |this, cx| {
                let image = fetching.await.ok().flatten().and_then(|bytes| media::decode(&bytes));
                this.update(cx, |this, cx| {
                    this.images.finish(&media, 56, image);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        None
    }
}
