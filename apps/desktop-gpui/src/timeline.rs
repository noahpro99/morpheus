//! A room's messages (`crates/ui/src/components/timeline.rs`): the core's
//! live timeline as a virtualised list that opens at the newest message,
//! stays there while new ones arrive, loads older history when scrolled
//! near the top, and offers "Jump to present" once scrolled away.
//!
//! Data flow: the core's batches of `VectorDiff<TimelineEntry>` are applied
//! to a vector of entries; [`rows::group_rows`] makes rows from it once per
//! batch; only what differs from the rows before is spliced into the list
//! element's state, so the whole list is never rebuilt, and each frame only
//! builds the rows in or near the view.

use std::{collections::HashMap, rc::Rc, sync::Arc, time::Duration};

use gpui::{
    AnyElement, App, Context, EventEmitter, FontWeight, Image, IntoElement, ListAlignment,
    ListOffset, ListState, Render, Window, div, list, point, prelude::*, px, relative, rgb,
};
use morpheus_core::{
    MediaRef, Message, MessageKind, OpenTimeline, OwnedRoomId, Session, TimelineEntry, TimelineHandle,
    Vector,
};

use crate::{
    e2e::{E2eExt as _, ScrollState, Scrollable},
    media::{self, Avatars},
    message::{self, ActionHandler, LinkHandler, Pictures, RowAction},
    theme,
    view_model::rows::{
        MessageRow, Row, changed_range, first_unread_after_marker, first_unread_since, group_rows,
    },
};

/// Start loading older history when the first row in view is this close to the top.
const NEAR_TOP_ROWS: usize = 8;
/// ... or when what is loaded is shorter than the view plus this (px).
const FILL_PX: f32 = 400.0;
/// How far from the newest message (px) before the jump button appears.
const JUMP_THRESHOLD_PX: f32 = 240.0;
const PAGE_SIZE: u16 = 40;
/// Quiet time after the last update before checking whether the column needs
/// more history to fill it.
const AUTO_FILL_SETTLE: Duration = Duration::from_millis(400);
/// Most automatic history pages per room open (rooms full of hidden events).
const AUTO_FILL_MAX_PAGES: u32 = 15;
/// Rows beyond the view that are measured ahead, in px.
const OVERDRAW: f32 = 600.0;

/// A link in a message was clicked.
pub struct LinkClicked(pub String);

impl EventEmitter<LinkClicked> for TimelineView {}

/// What the room's composer needs to hear from its timeline.
pub enum TimelineEvent {
    /// The timeline is open: its handle sends messages.
    Opened(TimelineHandle),
    /// "Reply" on a message.
    Reply(Message),
    /// "Edit" on a message.
    Edit(Message),
    /// Your newest message that can be edited changed.
    LastOwn(Option<Message>),
}

impl EventEmitter<TimelineEvent> for TimelineView {}

pub struct TimelineView {
    #[allow(dead_code)]
    room_id: OwnedRoomId,
    session: Option<Session>,
    handle: Option<TimelineHandle>,
    entries: Vector<TimelineEntry>,
    rows: Vec<Row>,
    list: ListState,
    reached_start: bool,
    paginating: bool,
    auto_pages: u32,
    last_update: std::time::Instant,
    load_error: Option<String>,
    /// The message the NEW divider sits above, fixed once found.
    new_marker: Option<String>,
    caught_up: bool,
    read_at: Option<u64>,
    images: Avatars,
    links: LinkHandler,
    actions: ActionHandler,
    last_own: Option<Message>,
    /// The newest own message still sending that the view jumped down to.
    echoed: Option<String>,
}

/// Scroll numbers of a list, in px: what a test and the jump button read.
fn metrics(list: &ListState) -> ScrollState {
    let max = f32::from(list.max_offset_for_scrollbar().height);
    let offset = -f32::from(list.scroll_px_offset_for_scrollbar().y);
    let top = offset.clamp(0.0, max);
    ScrollState {
        from_bottom: max - top,
        range: max,
        viewport: f32::from(list.viewport_bounds().size.height),
    }
}

struct ListProbe(ListState);

impl Scrollable for ListProbe {
    fn scroll_state(&self) -> ScrollState {
        metrics(&self.0)
    }

    fn scroll_to(&self, from_bottom: f32) {
        let max = metrics(&self.0).range;
        let top = (max - from_bottom.clamp(0.0, max)).max(0.0);
        self.0.set_offset_from_scrollbar(point(px(0.0), px(-top)));
    }
}

impl TimelineView {
    pub fn new(session: Option<Session>, room_id: OwnedRoomId, cx: &mut Context<Self>) -> Self {
        let list = ListState::new(0, ListAlignment::Bottom, px(OVERDRAW));
        list.splice(0..0, 1);
        let weak = cx.weak_entity();
        let links: LinkHandler = Rc::new(move |url: &str, cx: &mut App| {
            let url = url.to_owned();
            weak.update(cx, |_, cx| cx.emit(LinkClicked(url))).ok();
        });
        let weak = cx.weak_entity();
        let actions: ActionHandler = Rc::new(move |action, unique_id: &str, _window, cx| {
            let unique_id = unique_id.to_owned();
            weak.update(cx, |this, cx| this.row_action(action, &unique_id, cx))
                .ok();
        });
        let this = Self {
            room_id: room_id.clone(),
            session: session.clone(),
            handle: None,
            entries: Vector::new(),
            rows: vec![Row::Skeleton { older: false }],
            list,
            reached_start: false,
            paginating: false,
            auto_pages: 0,
            last_update: std::time::Instant::now(),
            load_error: None,
            new_marker: None,
            caught_up: false,
            read_at: None,
            images: Avatars::default(),
            links,
            actions,
            last_own: None,
            echoed: None,
        };
        if let Some(session) = session {
            cx.spawn(async move |this, cx| {
                let opened = session.timeline(room_id.clone()).await;
                match opened {
                    Ok(OpenTimeline {
                        handle,
                        mut updates,
                    }) => {
                        let read_at = session.fully_read_at(room_id).await.ok().flatten();
                        let alive = this
                            .update(cx, |this, cx| {
                                this.handle = Some(handle.clone());
                                cx.emit(TimelineEvent::Opened(handle));
                                this.read_at = read_at;
                                this.rebuild();
                                cx.notify();
                            })
                            .is_ok();
                        while alive {
                            let Some(diffs) = updates.next().await else {
                                break;
                            };
                            if this.update(cx, |this, cx| this.apply(diffs, cx)).is_err() {
                                break;
                            }
                        }
                    }
                    Err(err) => {
                        this.update(cx, |this, cx| {
                            this.load_error = Some(err.to_string());
                            this.rebuild();
                            cx.notify();
                        })
                        .ok();
                    }
                }
            })
            .detach();
        }
        this
    }

    /// One batch of the core's changes.
    pub fn apply(
        &mut self,
        diffs: Vec<morpheus_core::VectorDiff<TimelineEntry>>,
        cx: &mut Context<Self>,
    ) {
        for diff in diffs {
            diff.apply(&mut self.entries);
        }
        // The newest own message that can be edited, for "Up to edit".
        let newest = self.entries.iter().rev().find_map(|entry| match entry {
            TimelineEntry::Message(m) if m.is_own && m.can_edit => Some((**m).clone()),
            _ => None,
        });
        if newest != self.last_own {
            self.last_own = newest.clone();
            cx.emit(TimelineEvent::LastOwn(newest));
        }
        // The divider goes where the SDK placed its read marker (or where the
        // saved read position says), once, and stays.
        if self.new_marker.is_none()
            && !self.caught_up
            && let Some(marker) = first_unread_after_marker(&self.entries).or_else(|| {
                self.read_at
                    .and_then(|at| first_unread_since(&self.entries, at))
            })
        {
            self.new_marker = Some(marker);
        }
        // Sending a message means being caught up: drop the divider and move
        // the fully-read marker on.
        if !self.caught_up
            && self.entries.iter().any(
                |e| matches!(e, TimelineEntry::Message(m) if m.is_own && m.is_local_echo),
            )
        {
            self.caught_up = true;
            self.new_marker = None;
            if let Some(handle) = &self.handle {
                drop(handle.mark_as_fully_read());
            }
        }
        if let Some(handle) = &self.handle {
            let receipt = handle.send_read_receipt();
            cx.spawn(async move |_, _| {
                if let Err(err) = receipt.await {
                    eprintln!("morpheus-gpui: read receipt: {err}");
                }
            })
            .detach();
        }
        self.last_update = std::time::Instant::now();
        self.rebuild();
        // Sending a message brings the view to it, wherever it was scrolled.
        let sending = self.entries.iter().rev().find_map(|entry| match entry {
            TimelineEntry::Message(m) if m.is_own && m.is_local_echo => Some(m.unique_id.clone()),
            _ => None,
        });
        if sending.is_some() && sending != self.echoed {
            self.jump_to_present(cx);
        }
        self.echoed = sending.or(self.echoed.take());
        // Look again for a column the history does not fill yet, once updates settle.
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(AUTO_FILL_SETTLE + Duration::from_millis(50)).await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
        cx.notify();
    }

    fn at_start(&self) -> bool {
        self.reached_start
            || self
                .entries
                .iter()
                .any(|e| matches!(e, TimelineEntry::TimelineStart))
    }

    /// Rows from the entries, and the list told what changed.
    fn rebuild(&mut self) {
        let known: HashMap<String, Arc<MessageRow>> = self
            .rows
            .iter()
            .filter_map(|row| match row {
                Row::Message { row, .. } => Some((row.message.unique_id.clone(), row.clone())),
                _ => None,
            })
            .collect();
        let mut rows = group_rows(&self.entries, self.new_marker.as_deref(), &known);
        if self.entries.is_empty() {
            if self.load_error.is_none() {
                rows = vec![Row::Skeleton { older: false }];
            }
        } else if !self.at_start() && self.handle.is_some() {
            rows.insert(0, Row::Skeleton { older: true });
        }
        let (start, removed, added) = changed_range(&self.rows, &rows);
        if removed > 0 || added > 0 {
            self.list.splice(start..start + removed, added);
        }
        self.rows = rows;
    }

    fn load_older(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = self.handle.clone() else {
            return;
        };
        if self.paginating || self.reached_start {
            return;
        }
        self.paginating = true;
        let loading = handle.paginate_backwards(PAGE_SIZE);
        cx.spawn(async move |this, cx| {
            let result = loading.await;
            this.update(cx, |this, cx| {
                this.paginating = false;
                match result {
                    Ok(hit_start) => this.reached_start = hit_start,
                    Err(err) => this.load_error = Some(err.to_string()),
                }
                this.last_update = std::time::Instant::now();
                this.rebuild();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Older history when the view is near the top of what is loaded, or
    /// what is loaded does not fill it.
    fn check_history(&mut self, cx: &mut Context<Self>) {
        if self.handle.is_none() || self.paginating || self.entries.is_empty() || self.at_start() {
            return;
        }
        let top = self.list.logical_scroll_top().item_ix;
        let scrolled = top < self.rows.len();
        if scrolled && top <= NEAR_TOP_ROWS {
            self.load_older(cx);
            return;
        }
        let settled = self.last_update.elapsed() >= AUTO_FILL_SETTLE;
        let viewport = metrics(&self.list).viewport;
        if !scrolled
            && settled
            && viewport > 0.0
            && metrics(&self.list).range < FILL_PX
            && self.auto_pages < AUTO_FILL_MAX_PAGES
        {
            self.auto_pages += 1;
            self.load_older(cx);
        }
    }

    /// The picture for `media` at `size`, starting its fetch if new.
    fn picture(
        &mut self,
        media: &MediaRef,
        (w, h): (u32, u32),
        cx: &mut Context<Self>,
    ) -> Option<Arc<Image>> {
        if let Some(image) = self.images.get(media, w) {
            return Some(image);
        }
        let session = self.session.clone()?;
        if self.images.begin(media, w) {
            let fetching = session.thumbnail(media.clone(), w, h);
            let media = media.clone();
            cx.spawn(async move |this, cx| {
                let image = fetching
                    .await
                    .ok()
                    .flatten()
                    .and_then(|bytes| media::decode(&bytes));
                this.update(cx, |this, cx| {
                    this.images.finish(&media, w, image);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        None
    }

    fn render_row(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(row) = self.rows.get(ix).cloned() else {
            return div().into_any_element();
        };
        let Row::Message { row, show_header } = row else {
            return message::other_row(&row);
        };
        let mut pictures = Pictures::default();
        if show_header && let Some(avatar) = &row.message.sender_avatar {
            pictures.avatar = self.picture(avatar, (80, 80), cx);
        }
        match &row.message.kind {
            MessageKind::Image { media, .. } => {
                pictures.media = self.picture(media, (800, 600), cx);
            }
            MessageKind::Video {
                media, thumbnail, ..
            } => {
                let source = thumbnail.as_ref().unwrap_or(media).clone();
                pictures.media = self.picture(&source, (480, 320), cx);
            }
            _ => {}
        }
        message::message_el(
            &row,
            show_header,
            &pictures,
            None,
            &self.links,
            Some(&self.actions),
        )
    }

    /// A click on a message's hover bar.
    fn row_action(&mut self, action: RowAction, unique_id: &str, cx: &mut Context<Self>) {
        let Some(message) = self.rows.iter().find_map(|row| match row {
            Row::Message { row, .. } if row.message.unique_id == unique_id => {
                Some(row.message.clone())
            }
            _ => None,
        }) else {
            return;
        };
        match action {
            RowAction::Reply => cx.emit(TimelineEvent::Reply(message)),
            RowAction::Edit => cx.emit(TimelineEvent::Edit(message)),
            RowAction::React(key) => {
                let Some(handle) = &self.handle else { return };
                let toggling = handle.toggle_reaction(message.id, key.to_owned());
                cx.spawn(async move |_, _| {
                    if let Err(err) = toggling.await {
                        eprintln!("morpheus-gpui: reaction: {err}");
                    }
                })
                .detach();
            }
        }
    }

    /// Scrolls to the newest message.
    pub fn jump_to_present(&mut self, cx: &mut Context<Self>) {
        self.list.scroll_to(ListOffset {
            item_ix: self.rows.len(),
            offset_in_item: px(0.0),
        });
        cx.notify();
    }

    /// The rows now listed, for tests.
    #[cfg(test)]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }
}

impl Drop for TimelineView {
    fn drop(&mut self) {
        // Leaving a room reads it up to its newest message.
        if let Some(handle) = self.handle.take() {
            drop(handle.mark_as_fully_read());
        }
    }
}

impl Render for TimelineView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.check_history(cx);
        let away = metrics(&self.list).from_bottom > JUMP_THRESHOLD_PX;
        let rows = list(
            self.list.clone(),
            cx.processor(|this, ix, _window, cx| this.render_row(ix, cx)),
        )
        .size_full()
        .pt(px(16.0))
        .pb(px(8.0));
        let mut timeline = div()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .line_height(relative(theme::LINE_HEIGHT))
            .child(rows);
        if let Some(error) = &self.load_error {
            timeline = timeline.child(
                div()
                    .absolute()
                    .top(px(8.0))
                    .left(px(16.0))
                    .text_color(rgb(theme::DANGER))
                    .child(error.clone())
                    .e2e("p", "error")
                    .text(error.clone()),
            );
        }
        if away {
            timeline = timeline.child(
                div()
                    .absolute()
                    .bottom(px(16.0))
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .id("jump-to-bottom")
                            .px(px(12.0))
                            .py(px(6.0))
                            .rounded_full()
                            .bg(rgb(theme::ACCENT_STRONG))
                            .text_color(rgb(0xffffff))
                            .text_size(px(0.8 * theme::REM))
                            .font_weight(FontWeight::SEMIBOLD)
                            .shadow_md()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(theme::ACCENT)))
                            .on_click(cx.listener(|this, _, _, cx| this.jump_to_present(cx)))
                            .child("Jump to present ↓")
                            .e2e("button", "jump-to-bottom")
                            .attr("title", "Jump to the newest message")
                            .text("Jump to present ↓"),
                    ),
            );
        }
        timeline
            .e2e("section", "timeline")
            .scrollable(Rc::new(ListProbe(self.list.clone())))
    }
}

#[cfg(all(test, feature = "e2e"))]
mod widget_tests {
    use super::*;
    use crate::e2e::{self, query};
    use gpui::{Entity, TestAppContext, VisualTestContext};
    use morpheus_core::{Message, MessageId, OwnedEventId, OwnedUserId, VectorDiff};

    struct Harness(Entity<TimelineView>);

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            e2e::frame(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .font_family("Inter")
                    .child(self.0.clone()),
            )
        }
    }

    fn entry(id: usize, sender: &str, at: u64, kind: MessageKind) -> TimelineEntry {
        TimelineEntry::Message(Box::new(Message {
            unique_id: format!("u{id}"),
            id: MessageId::Event(OwnedEventId::try_from(format!("$e{id}")).expect("event id")),
            sender: OwnedUserId::try_from(sender).expect("user id"),
            sender_name: sender.trim_start_matches('@').split(':').next().unwrap_or("").into(),
            sender_avatar: None,
            timestamp_ms: at,
            is_own: false,
            is_local_echo: false,
            is_edited: false,
            can_edit: false,
            reactions: Vec::new(),
            in_reply_to: None,
            thread_root: None,
            thread: None,
            kind,
            shield: None,
        }))
    }

    fn text(body: &str) -> MessageKind {
        MessageKind::Text {
            body: body.into(),
            html: None,
        }
    }

    fn open(
        cx: &mut TestAppContext,
    ) -> (Entity<TimelineView>, &mut VisualTestContext) {
        let room = OwnedRoomId::try_from("!r:x").expect("room id");
        let (harness, cx) = cx.add_window_view(|_, cx| {
            Harness(cx.new(|cx| TimelineView::new(None, room, cx)))
        });
        let view = harness.read_with(cx, |h, _| h.0.clone());
        (view, cx)
    }

    fn feed(
        view: &Entity<TimelineView>,
        cx: &mut VisualTestContext,
        entries: Vec<TimelineEntry>,
    ) {
        view.update(cx, |t, cx| {
            t.apply(vec![VectorDiff::Append { values: entries.into_iter().collect() }], cx)
        });
        cx.run_until_parked();
        cx.draw(
            point(px(0.0), px(0.0)),
            gpui::size(px(700.0), px(500.0)),
            |_, _| div(),
        );
        cx.run_until_parked();
    }

    fn texts(selector: &str) -> Vec<String> {
        query(selector).into_iter().map(|(t, _)| t).collect()
    }

    #[gpui::test]
    fn messages_group_under_one_header(cx: &mut TestAppContext) {
        let (view, cx) = open(cx);
        feed(
            &view,
            cx,
            vec![
                entry(1, "@bob:x", 0, text("first")),
                entry(2, "@bob:x", 1000, text("second")),
                entry(3, "@carol:x", 2000, text("third")),
            ],
        );
        assert_eq!(texts(".message .body"), ["first", "second", "third"]);
        assert_eq!(texts(".message.with-header .sender"), ["bob", "carol"]);
        assert_eq!(query(".message .time.gutter").len(), 1);
    }

    #[gpui::test]
    fn a_batch_adds_rows_and_an_edit_replaces_one(cx: &mut TestAppContext) {
        let (view, cx) = open(cx);
        feed(&view, cx, vec![entry(1, "@bob:x", 0, text("one"))]);
        view.update(cx, |t, cx| {
            t.apply(
                vec![
                    VectorDiff::Set { index: 0, value: entry(1, "@bob:x", 0, text("one, edited")) },
                    VectorDiff::PushBack { value: entry(2, "@bob:x", 10, text("two")) },
                ],
                cx,
            )
        });
        cx.run_until_parked();
        cx.draw(point(px(0.0), px(0.0)), gpui::size(px(700.0), px(500.0)), |_, _| div());
        assert_eq!(texts(".message .body"), ["one, edited", "two"]);
        view.read_with(cx, |t, _| assert_eq!(t.rows().len(), 2));
    }

    #[gpui::test]
    fn formatted_bodies_and_links_show_as_text_and_link_nodes(cx: &mut TestAppContext) {
        let (view, cx) = open(cx);
        feed(
            &view,
            cx,
            vec![
                entry(
                    1,
                    "@bob:x",
                    0,
                    MessageKind::Text {
                        body: "bold and a link".into(),
                        html: Some(
                            "<p><strong>bold</strong> and <a href=\"https://example.org/a\">a link</a></p><ul><li>x</li></ul>"
                                .into(),
                        ),
                    },
                ),
                entry(2, "@bob:x", 1, text("see https://example.org/b.")),
            ],
        );
        assert_eq!(texts(".message .body.rich"), ["bold and a linkx"]);
        assert_eq!(texts(".message .body a[href=\"https://example.org/a\"]"), ["a link"]);
        assert_eq!(texts(".message .body a[href=\"https://example.org/b\"]"), ["https://example.org/b"]);
    }

    #[gpui::test]
    fn day_dividers_state_lines_and_the_start_are_rows(cx: &mut TestAppContext) {
        let (view, cx) = open(cx);
        feed(
            &view,
            cx,
            vec![
                TimelineEntry::TimelineStart,
                TimelineEntry::DateDivider { timestamp_ms: 86_400_000 * 3 },
                entry(1, "@bob:x", 86_400_000 * 3, text("hi")),
                entry(2, "@bob:x", 86_400_000 * 3, MessageKind::State("bob joined the room".into())),
            ],
        );
        assert_eq!(texts(".timeline-start"), ["This is the beginning of the room."]);
        assert_eq!(query(".date-divider").len(), 1);
        assert_eq!(texts(".message.system .system-text"), ["bob joined the room"]);
    }

    #[gpui::test]
    fn reactions_replies_and_the_edited_marker(cx: &mut TestAppContext) {
        let (view, cx) = open(cx);
        let mut message = match entry(1, "@bob:x", 0, text("hello")) {
            TimelineEntry::Message(m) => *m,
            _ => unreachable!(),
        };
        message.is_edited = true;
        message.reactions = vec![morpheus_core::Reaction {
            key: "👍".into(),
            count: 2,
            mine: true,
            senders: Vec::new(),
        }];
        message.in_reply_to = Some(morpheus_core::ReplyPreview {
            event_id: OwnedEventId::try_from("$orig").expect("event id"),
            sender_name: Some("carol".into()),
            summary: Some("the original".into()),
        });
        feed(&view, cx, vec![TimelineEntry::Message(Box::new(message))]);
        assert_eq!(texts(".message .body"), ["hello (edited)"]);
        assert_eq!(texts(".message .reaction.mine .reaction-count"), ["2"]);
        assert_eq!(texts(".message .reaction-key"), ["👍"]);
        assert_eq!(texts(".message .reply-quote b"), ["carol"]);
        assert_eq!(texts(".message .reply-quote"), ["↩carolthe original"]);
    }

    #[gpui::test]
    fn a_long_room_paints_only_the_rows_in_view(cx: &mut TestAppContext) {
        let (view, cx) = open(cx);
        let entries = (0..2000)
            .map(|n| entry(n, "@bob:x", n as u64 * 1_000_000, text(&format!("message {n}"))))
            .collect();
        feed(&view, cx, entries);
        let painted = query(".message").len();
        assert!(painted > 3 && painted < 60, "{painted} rows painted of 2000");
        assert!(texts(".message .body").contains(&"message 1999".to_owned()));
    }
}
