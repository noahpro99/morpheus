//! The thread panel (`crates/ui/src/components/thread.rs`): in the side
//! column in place of the members, a thread's root message, a line saying
//! how many replies it has, the replies themselves (a timeline of their
//! own) and a message box that posts back into the thread.

use std::{rc::Rc, sync::Arc};

use gpui::{
    Context, Entity, EventEmitter, FontWeight, Image, IntoElement, Pixels, Point, Render, Window,
    div, prelude::*, px, rgb, rgba,
};
use morpheus_core::{Message, OwnedEventId, OwnedRoomId, Session};

use crate::{
    composer::Composer,
    e2e::E2eExt as _,
    media::{self, Avatars},
    message::{self, LinkHandler, Pictures},
    theme,
    timeline::{TimelineEvent, TimelineView},
    view_model::{nicknames::NickMap, rows::MessageRow, threads::reply_count_label},
};

/// What the panel asks of the shell.
pub enum ThreadEvent {
    /// The ✕.
    Close,
    /// Someone's picture or name clicked: their profile, near this point.
    Sender(Box<Message>, Point<Pixels>),
    /// Someone's picture or name right-clicked: their member menu.
    SenderMenu(Box<Message>, Point<Pixels>),
    /// A right click on one of the replies.
    Menu(Box<Message>, Point<Pixels>),
    /// A reply's "more reactions" button.
    Picker(Box<Message>, Point<Pixels>),
    /// A reply's bin; true when Shift skipped the question.
    Delete(Box<Message>, bool),
    /// A background action failed: what it was, and why.
    Failed(&'static str, String),
}

impl EventEmitter<ThreadEvent> for ThreadPanel {}

pub struct ThreadPanel {
    session: Option<Session>,
    pub root: Message,
    row: Arc<MessageRow>,
    timeline: Entity<TimelineView>,
    composer: Entity<Composer>,
    handle: Option<morpheus_core::TimelineHandle>,
    avatars: Avatars,
    nickname: Option<String>,
}

impl ThreadPanel {
    pub fn new(
        session: Option<Session>,
        room_id: OwnedRoomId,
        root: Message,
        root_id: OwnedEventId,
        nicknames: &NickMap,
        cx: &mut Context<Self>,
    ) -> Self {
        let timeline = cx.new(|cx| {
            TimelineView::new_thread(session.clone(), room_id.clone(), root_id, cx)
        });
        let composer = cx.new(|cx| {
            Composer::new(session.clone(), room_id.clone(), String::new(), false, cx)
                .with_placeholder("Reply in thread", cx)
        });
        let target = composer.clone();
        cx.subscribe(&timeline, move |this: &mut Self, _, event: &TimelineEvent, cx| {
            target.update(cx, |composer, cx| match event {
                TimelineEvent::Opened(handle) => composer.opened(handle.clone(), cx),
                TimelineEvent::Reply(message) => composer.reply_to(message, cx),
                TimelineEvent::Edit(message) => composer.edit(message, cx),
                TimelineEvent::LastOwn(message) => composer.set_last_own(message.clone()),
                _ => {}
            });
            match event {
                TimelineEvent::Opened(handle) => this.handle = Some(handle.clone()),
                TimelineEvent::Menu(message, at) => cx.emit(ThreadEvent::Menu(message.clone(), *at)),
                TimelineEvent::Picker(message, at) => cx.emit(ThreadEvent::Picker(message.clone(), *at)),
                TimelineEvent::Delete(message, skip) => cx.emit(ThreadEvent::Delete(message.clone(), *skip)),
                TimelineEvent::Sender(message, at) => {
                    cx.emit(ThreadEvent::Sender(message.clone(), *at))
                }
                TimelineEvent::SenderMenu(message, at) => {
                    cx.emit(ThreadEvent::SenderMenu(message.clone(), *at))
                }
                TimelineEvent::Failed(what, err) => cx.emit(ThreadEvent::Failed(what, err.clone())),
                _ => {}
            }
        })
        .detach();
        let shared = Arc::new(nicknames.clone());
        timeline.update(cx, |timeline, cx| timeline.set_nicknames(shared, cx));
        Self {
            session,
            nickname: nicknames.get(root.sender.as_str()).cloned(),
            row: Arc::new(MessageRow::new(root.clone())),
            root,
            timeline,
            composer,
            handle: None,
            avatars: Avatars::default(),
        }
    }

    /// The replies' timeline, and its handle once open: what a menu acts on.
    pub fn timeline(&self) -> (Entity<TimelineView>, Option<morpheus_core::TimelineHandle>) {
        (self.timeline.clone(), self.handle.clone())
    }

    /// Whether the reply box has something open that Escape closes first.
    pub fn composer_busy(&self, cx: &gpui::App) -> bool {
        self.composer.read(cx).busy()
    }

    /// Your nicknames changed.
    pub fn set_nicknames(&mut self, nicknames: &NickMap, cx: &mut Context<Self>) {
        self.nickname = nicknames.get(self.root.sender.as_str()).cloned();
        let shared = Arc::new(nicknames.clone());
        self.timeline
            .update(cx, |timeline, cx| timeline.set_nicknames(shared, cx));
        let names = nicknames.clone();
        self.composer
            .update(cx, |composer, cx| composer.set_nicknames(names, cx));
        cx.notify();
    }

    /// The root's picture, starting its fetch the first time.
    fn avatar(&mut self, cx: &mut Context<Self>) -> Option<Arc<Image>> {
        let media = self.root.sender_avatar.clone()?;
        if let Some(image) = self.avatars.get(&media, 80) {
            return Some(image);
        }
        let session = self.session.clone()?;
        if self.avatars.begin(&media, 80) {
            let fetching = session.thumbnail(media.clone(), 80, 80);
            cx.spawn(async move |this, cx| {
                let image = fetching
                    .await
                    .ok()
                    .flatten()
                    .and_then(|bytes| media::decode(&bytes));
                this.update(cx, |this, cx| {
                    this.avatars.finish(&media, 80, image);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        None
    }
}

impl Render for ThreadPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pictures = Pictures {
            avatar: self.avatar(cx),
            nickname: self.nickname.clone(),
            ..Pictures::default()
        };
        let links: LinkHandler = Rc::new(|_, _| {});
        let root = message::message_el(&self.row, true, &pictures, None, &links, None);
        let label = reply_count_label(&self.root);
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .line_height(gpui::relative(theme::LINE_HEIGHT))
            .child(
                div()
                    .h(px(48.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(16.0))
                    .border_b_1()
                    .border_color(rgba(0x0000004d))
                    .child(
                        div()
                            .text_size(px(0.95 * theme::REM))
                            .font_weight(FontWeight::BOLD)
                            .child("Thread")
                            .e2e("h3", "")
                            .text("Thread"),
                    )
                    .child(
                        div()
                            .id("thread-close")
                            .px(px(8.0))
                            .py(px(4.0))
                            .rounded(px(theme::RADIUS_SM))
                            .text_color(rgb(theme::text_muted()))
                            .cursor_pointer()
                            .hover(|s| s.text_color(rgb(theme::text())))
                            .child("✕")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(ThreadEvent::Close)))
                            .e2e("button", "ghost")
                            .attr("title", "Close thread")
                            .text("✕"),
                    )
                    .e2e("header", "thread-panel-header"),
            )
            .child(
                div()
                    .flex_none()
                    .pt(px(8.0))
                    .border_b_1()
                    .border_color(rgb(theme::bg_elevated()))
                    .child(root)
                    .e2e("div", "thread-panel-root"),
            )
            .child(
                div()
                    .flex_none()
                    .mx(px(16.0))
                    .my(px(10.0))
                    .text_color(rgb(theme::text_faint()))
                    .text_size(px(0.75 * theme::REM))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(label.clone())
                    .e2e("div", "thread-panel-divider")
                    .text(label),
            )
            .child(self.timeline.clone())
            .child(self.composer.clone())
            .e2e("aside", "thread-panel")
    }
}
