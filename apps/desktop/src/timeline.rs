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

/// Whether a thread panel is open (set by the shell each frame).
pub static THREAD_PANEL_OPEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use gpui::{
    AnyElement, App, Context, EventEmitter, FontWeight, Image, IntoElement, ListAlignment,
    ListOffset, ListState, Pixels, Point, Render, Window, div, list, point, prelude::*, px,
    relative, rgb,
};
use morpheus_core::{
    MediaRef, Message, MessageId, MessageKind, OpenTimeline, OwnedEventId, OwnedRoomId,
    OwnedUserId, Session, TimelineEntry, TimelineHandle, Vector,
};

use crate::{
    e2e::{E2eExt as _, ScrollState, Scrollable},
    icons::icon,
    media::{self, Avatars},
    message::{self, ActionHandler, LinkHandler, Pictures, RowAction},
    theme,
    view_model::keys::{
        BANNER_RECOVERY, BANNER_TEXT, BANNER_TITLE, BANNER_VERIFY, KeyAccess, SettingsFocus,
        show_banner, undecryptable,
    },
    view_model::nicknames::NickMap,
    view_model::rows::{
        MessageRow, Row, anchor_row, changed_range, first_unread_after_marker, first_unread_since,
        group_rows,
    },
};

/// How often the relative times (and the date dividers) are drawn again.
const CLOCK_REFRESH: Duration = Duration::from_secs(15);

/// Start loading older history when the first row in view is this close to the top.
/// The most pages a background fill asks for (a room flooded with hidden events).
const PREFILL_MAX_PAGES: u16 = 200;
const NEAR_TOP_ROWS: usize = 8;
/// ... or when what is loaded is shorter than the view plus this (px).
const FILL_PX: f32 = 400.0;
/// How far from the newest message (px) before the jump button appears.
const JUMP_THRESHOLD_PX: f32 = 240.0;
const PAGE_SIZE: u16 = 40;
/// Most history pages loaded looking for a message jumped to.
const JUMP_MAX_PAGES: u32 = 50;
/// How long a message jumped to stays highlighted.
const JUMP_HIGHLIGHT: Duration = Duration::from_millis(2000);
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
    /// Your newest message that can be edited changed.
    LastOwn(Option<Message>),
    /// A button of the key banner: open Settings there.
    OpenSettings(SettingsFocus),
    /// A right click on a message, at this point.
    Menu(Box<Message>, Point<Pixels>),
    /// The hover bar's "more reactions" button, at this point.
    Picker(Box<Message>, Point<Pixels>),
    /// The hover bar's bin; true when Shift skipped the question.
    Delete(Box<Message>, bool),
    /// Someone's picture or name clicked: their profile, near this point.
    Sender(Box<Message>, Point<Pixels>),
    /// Someone's picture or name right-clicked: their member menu.
    SenderMenu(Box<Message>, Point<Pixels>),
    /// A background action failed: what it was, and why (a toast).
    Failed(&'static str, String),
    /// "Reply in thread" or a thread chip: open this message's thread.
    Thread(Box<Message>),
    /// A picture clicked: the room's pictures and which one.
    OpenImage(Vec<crate::lightbox::GalleryImage>, usize),
    /// A video's full-window button.
    FullscreenVideo(gpui::Entity<crate::video::VideoPlayer>),
}

impl EventEmitter<TimelineEvent> for TimelineView {}

pub struct TimelineView {
    _alive: crate::memory::Alive,
    #[allow(dead_code)]
    room_id: OwnedRoomId,
    session: Option<Session>,
    handle: Option<TimelineHandle>,
    /// A direct message: an empty one offers "Say hi!" (set by the shell).
    say_hi: bool,
    /// The room is encrypted: no link previews unless turned on for them.
    encrypted: bool,
    entries: Vector<TimelineEntry>,
    rows: Vec<Row>,
    list: ListState,
    reached_start: bool,
    paginating: bool,
    auto_pages: u32,
    /// Messages to have loaded whether or not the list is on screen (0: none), and the pages asked for them.
    prefill: usize,
    prefill_pages: u16,
    /// Whether the jump button was showing last frame (kept through the frame in
    /// which a list that was just changed has not measured its rows yet).
    was_away: bool,
    last_update: std::time::Instant,
    /// A Page Up/Down in flight: where it is going (px from the bottom) and
    /// where the last step left the list (if the list is elsewhere, the
    /// reader scrolled and it stops).
    paging: Option<(f32, f32)>,
    /// The message being edited in place.
    edit: Option<InPlaceEdit>,
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
    /// Whether this device can read encrypted history (set by the shell).
    keys: KeyAccess,
    /// Your nicknames for people, shown over their names.
    nicknames: Arc<NickMap>,
    /// Names and pictures people changed while this timeline was open, by user:
    /// drawn over what the messages carry (the timeline itself only follows
    /// picture changes, not a new name alone).
    profiles: std::collections::HashMap<String, (Option<String>, Option<morpheus_core::MediaRef>)>,
    /// People you ignore: their messages are folded away until shown.
    ignored: std::collections::HashSet<String>,
    /// Folded messages shown on request (by unique ID).
    revealed: std::collections::HashSet<String>,
    /// Files being or done downloading, by the message's unique ID.
    downloads: std::collections::HashMap<String, crate::message::Download>,
    /// Some message here cannot be decrypted (kept up to date by `rebuild`).
    undecryptable: bool,
    /// Messages hidden while their deletion can still be undone (unique ids).
    hidden: HashSet<String>,
    /// The room was marked unread from here: no read receipts (any of them
    /// would clear the flag) until the room is opened again.
    keep_unread: bool,
    /// Not on screen (a voice channel's chat while closed): nothing is read.
    covered: bool,
    /// Set for a thread's replies: the root they belong to.
    thread_root: Option<OwnedEventId>,
    /// A message being jumped to (a search result): its event ID and the
    /// history pages loaded looking for it so far.
    jump: Option<(String, u32)>,
    /// The message just jumped to, glowing for a moment (unique ID).
    highlight: Option<String>,
    /// The message whose right-click menu is open (unique ID).
    menu_message: Option<String>,
    /// The message vim mode's selection is on (unique ID).
    vim_selected: Option<String>,
    /// The message whose avatar is held down (it sits a pixel lower).
    pressed_avatar: Option<String>,
    /// The scrollbar's state (#144).
    bar: crate::scrollbar::Bar,
    /// Set while the room is kept in the background: the newest message it
    /// had when left (what came after is new on return) and whether the view
    /// sat at the newest end.
    left_at: Option<(Option<String>, bool)>,
    /// Video messages' players, by the message's unique ID (kept while the
    /// view lives, so a video scrolled away and back is where it was).
    videos: HashMap<String, gpui::Entity<crate::video::VideoPlayer>>,
    /// The room's members, to turn a typed "" into a mention pill.
    people: crate::view_model::rich::Mentions,
}

/// Where vim mode's selection goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VimStep {
    Newest,
    Oldest,
    Older,
    Newer,
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

    fn scroll_by(&self, down: f32) {
        self.0.scroll_by(px(down));
    }
}

impl TimelineView {
    pub fn new(session: Option<Session>, room_id: OwnedRoomId, cx: &mut Context<Self>) -> Self {
        Self::open(session, room_id, None, cx)
    }

    /// The replies to `root`, in the thread panel.
    pub fn new_thread(
        session: Option<Session>,
        room_id: OwnedRoomId,
        root: OwnedEventId,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::open(session, room_id, Some(root), cx)
    }

    fn open(
        session: Option<Session>,
        room_id: OwnedRoomId,
        thread_root: Option<OwnedEventId>,
        cx: &mut Context<Self>,
    ) -> Self {
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
            say_hi: false,
            encrypted: false,
            entries: Vector::new(),
            rows: vec![Row::Skeleton { older: false }],
            list,
            reached_start: false,
            paginating: false,
            auto_pages: 0,
            prefill: 0,
            prefill_pages: 0,
            was_away: false,
            last_update: std::time::Instant::now(),
            paging: None,
            edit: None,
            load_error: None,
            new_marker: None,
            caught_up: false,
            read_at: None,
            images: Avatars::default(),
            _alive: crate::memory::Alive::new(&crate::memory::TIMELINE_VIEWS),
            links,
            actions,
            last_own: None,
            echoed: None,
            keys: KeyAccess::default(),
            nicknames: Arc::default(),
            profiles: Default::default(),
            ignored: Default::default(),
            revealed: Default::default(),
            downloads: Default::default(),
            undecryptable: false,
            hidden: HashSet::new(),
            keep_unread: false,
            covered: false,
            // A thread is never read or marked: the room's own timeline does.
            thread_root: thread_root.clone(),
            jump: None,
            highlight: None,
            menu_message: None,
            vim_selected: None,
            pressed_avatar: None,
            bar: crate::scrollbar::Bar::default(),
            left_at: None,
            videos: HashMap::new(),
            people: Default::default(),
        };
        // "5 min ago" and "Today" keep up with the clock: draw again now and then.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CLOCK_REFRESH).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
        if let Some(session) = session.clone()
            && thread_root.is_none()
        {
            let known = session.members_known(room_id.clone());
            let room = room_id.clone();
            cx.spawn(async move |this, cx| {
                // The members known at once, then (unless that was everyone)
                // the whole list when it is in (#115), each indexed off the
                // window's thread: a big room has thousands of members, and
                // every row drawn is matched.
                let index = async |members: Vec<morpheus_core::Member>,
                                   cx: &mut gpui::AsyncApp| {
                    let people = cx
                        .background_executor()
                        .spawn(async move {
                            crate::view_model::rich::Mentions::new(
                                members
                                    .into_iter()
                                    .map(|m| crate::view_model::rich::Person {
                                        id: m.user_id.to_string(),
                                        name: m.name,
                                    })
                                    .collect(),
                            )
                        })
                        .await;
                    this.update(cx, |this, cx| {
                        this.people = people;
                        cx.notify();
                    })
                    .is_ok()
                };
                let complete = match known.await {
                    Ok(known) => {
                        let complete = known.complete;
                        if !index(known.members, cx).await {
                            return;
                        }
                        complete
                    }
                    Err(_) => false,
                };
                if !complete && let Ok(members) = session.members(room).await {
                    index(members, cx).await;
                }
            })
            .detach();
        }
        if let Some(session) = session {
            cx.spawn(async move |this, cx| {
                let opened = match thread_root.clone() {
                    Some(root) => session.thread(room_id.clone(), root).await,
                    None => session.timeline(room_id.clone()).await,
                };
                match opened {
                    Ok(OpenTimeline {
                        handle,
                        mut updates,
                    }) => {
                        let read_at = if thread_root.is_some() {
                            None
                        } else {
                            session.fully_read_at(room_id).await.ok().flatten()
                        };
                        let alive = this
                            .update(cx, |this, cx| {
                                this.handle = Some(handle.clone());
                                cx.emit(TimelineEvent::Opened(handle));
                                this.read_at = read_at;
                                this.rebuild();
                                this.continue_jump(cx);
                                this.check_history(cx);
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
            && self
                .entries
                .iter()
                .any(|e| matches!(e, TimelineEntry::Message(m) if m.is_own && m.is_local_echo))
        {
            self.caught_up = true;
            self.new_marker = None;
            if let Some(handle) = self.handle.as_ref().filter(|_| self.thread_root.is_none()) {
                drop(handle.mark_as_fully_read());
            }
        }
        if let Some(handle) = self
            .handle
            .as_ref()
            .filter(|_| !self.keep_unread && !self.covered && self.thread_root.is_none())
        {
            let receipt = handle.send_read_receipt();
            cx.spawn(async move |_, _| {
                if let Err(err) = receipt.await {
                    eprintln!("morpheus-desktop: read receipt: {err}");
                }
            })
            .detach();
        }
        self.last_update = std::time::Instant::now();
        self.rebuild();
        self.continue_jump(cx);
        self.check_prefill(cx);
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
            cx.background_executor()
                .timer(AUTO_FILL_SETTLE + Duration::from_millis(50))
                .await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
        cx.notify();
    }

    /// Which way vim mode's selection moves.
    pub fn vim_select(&mut self, step: VimStep, cx: &mut Context<Self>) {
        let messages: Vec<(usize, String)> = self
            .rows
            .iter()
            .enumerate()
            .filter_map(|(ix, row)| match row {
                Row::Message { row, .. } => Some((ix, row.message.unique_id.clone())),
                _ => None,
            })
            .collect();
        let Some(last) = messages.len().checked_sub(1) else {
            return;
        };
        let current = self
            .vim_selected
            .as_ref()
            .and_then(|id| messages.iter().position(|(_, unique)| unique == id));
        let target = match (step, current) {
            (VimStep::Newest, _) | (VimStep::Older | VimStep::Newer, None) => last,
            (VimStep::Oldest, _) => 0,
            (VimStep::Older, Some(i)) => i.saturating_sub(1),
            (VimStep::Newer, Some(i)) => (i + 1).min(last),
        };
        let (ix, unique) = messages[target].clone();
        self.vim_selected = Some(unique);
        if target == last {
            self.jump_to_present(cx);
        } else {
            self.list.scroll_to_reveal_item(ix);
        }
        cx.notify();
    }

    /// The message vim mode's selection is on.
    pub fn vim_selection(&self) -> Option<Message> {
        let id = self.vim_selected.as_deref()?;
        self.rows.iter().find_map(|row| match row {
            Row::Message { row, .. } if row.message.unique_id == id => Some(row.message.clone()),
            _ => None,
        })
    }

    /// Where the selected message is on screen.
    pub fn vim_selection_bounds(&self) -> Option<gpui::Bounds<Pixels>> {
        let id = self.vim_selected.as_deref()?;
        let ix = self.rows.iter().position(
            |row| matches!(row, Row::Message { row, .. } if row.message.unique_id == id),
        )?;
        self.list.bounds_for_item(ix)
    }

    /// Lets go of vim mode's selection; whether there was one.
    pub fn vim_clear(&mut self, cx: &mut Context<Self>) -> bool {
        let had = self.vim_selected.take().is_some();
        if had {
            cx.notify();
        }
        had
    }

    /// Page Up / Page Down: the chat scrolls by (nearly) a page.
    pub fn scroll_page(&mut self, down: bool, cx: &mut Context<Self>) {
        self.scroll_by_fraction(down, 0.9, cx);
    }

    /// Ctrl+D / Ctrl+U in vim mode: half a page, eased the same way.
    pub fn scroll_half_page(&mut self, down: bool, cx: &mut Context<Self>) {
        self.scroll_by_fraction(down, 0.5, cx);
    }

    fn scroll_by_fraction(&mut self, down: bool, fraction: f32, cx: &mut Context<Self>) {
        let now = metrics(&self.list);
        let page = now.viewport * fraction;
        // A page asked for while one goes adds to it.
        let base = self.paging.map_or(now.from_bottom, |(target, _)| target);
        let target = (base + if down { -page } else { page }).clamp(0.0, now.range);
        let running = self.paging.is_some();
        self.paging = Some((target, now.from_bottom));
        if running {
            return;
        }
        // Eased like the wheel (gpui::ease_step), a step a frame.
        cx.spawn(async move |this, cx| {
            let mut last = std::time::Instant::now();
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(8))
                    .await;
                let dt = last.elapsed();
                last = std::time::Instant::now();
                if !this
                    .update(cx, |this, cx| this.page_step(dt, cx))
                    .unwrap_or(false)
                {
                    break;
                }
            }
        })
        .detach();
    }

    /// One step of a Page Up/Down; false when it is over.
    fn page_step(&mut self, dt: Duration, cx: &mut Context<Self>) -> bool {
        let Some((target, left_at)) = self.paging else {
            return false;
        };
        let now = metrics(&self.list).from_bottom;
        // Heading for the bottom the list re-measures rows as they come into view
        // and its count of the way down shifts under the ease; that is not the
        // reader scrolling, so only a scroll elsewhere stops it.
        if target > 1.0 && (now - left_at).abs() > 40.0 {
            self.paging = None;
            return false;
        }
        let dt = dt.clamp(Duration::from_millis(1), Duration::from_millis(50));
        let (step, done) = gpui::ease_step(target - now, dt.as_secs_f32());
        let to = now + step;
        ListProbe(self.list.clone()).scroll_to(to);
        self.paging = (!done).then_some((target, to));
        if done && target <= 1.0 {
            // The list's own count of the way down is an estimate until every row
            // is measured: arriving at the bottom of it lands on the real bottom.
            self.jump_to_present(cx);
        }
        cx.notify();
        !done
    }

    /// The people you ignore changed: their messages fold away (or show).
    /// Whether the room is encrypted (link previews tell the homeserver the link).
    pub fn set_encrypted(&mut self, encrypted: bool, cx: &mut Context<Self>) {
        if self.encrypted != encrypted {
            self.encrypted = encrypted;
            cx.notify();
        }
    }

    /// A link preview arrived (or went): the row is measured again.
    fn relayout(&mut self, unique_id: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self.rows.iter().position(|r| {
            matches!(r, Row::Message { row, .. } if row.message.unique_id == unique_id)
        }) {
            self.list.splice(ix..ix + 1, 1);
        }
        cx.notify();
    }

    /// Whether this is a direct message, which offers "Say hi!" while empty.
    pub fn set_direct(&mut self, direct: bool, cx: &mut Context<Self>) {
        if self.say_hi != direct {
            self.say_hi = direct;
            self.rebuild();
            cx.notify();
        }
    }

    fn say_hi(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = self.handle.clone() else {
            return;
        };
        let sending = handle.send_text("Hi! 👋".to_owned());
        cx.spawn(async move |_, _| {
            if let Err(err) = sending.await {
                eprintln!("morpheus-desktop: sending: {err}");
            }
        })
        .detach();
    }

    pub fn set_ignored(
        &mut self,
        ignored: std::collections::HashSet<String>,
        cx: &mut Context<Self>,
    ) {
        if self.ignored == ignored {
            return;
        }
        self.ignored = ignored;
        let rows = self.rows.len();
        self.list.splice(0..rows, rows);
        cx.notify();
    }

    /// `user` changed their name or picture: their messages are drawn again with it.
    pub fn set_profile(
        &mut self,
        user: String,
        name: Option<String>,
        avatar: Option<morpheus_core::MediaRef>,
        cx: &mut Context<Self>,
    ) {
        self.profiles.insert(user, (name, avatar));
        let rows = self.rows.len();
        self.list.splice(0..rows, rows);
        cx.notify();
    }

    /// Your nicknames changed: names over messages are drawn again.
    pub fn set_nicknames(&mut self, nicknames: Arc<NickMap>, cx: &mut Context<Self>) {
        if self.nicknames == nicknames {
            return;
        }
        self.nicknames = nicknames;
        let rows = self.rows.len();
        self.list.splice(0..rows, rows);
        cx.notify();
    }

    /// What this device can read changed: rows it cannot decrypt are drawn
    /// as the new state says, so they are measured again.
    pub fn set_keys(&mut self, keys: KeyAccess, cx: &mut Context<Self>) {
        if self.keys == keys {
            return;
        }
        let was = undecryptable(self.keys);
        self.keys = keys;
        if undecryptable(keys) != was {
            for (ix, row) in self.rows.iter().enumerate() {
                if matches!(row, Row::Message { row, .. } if matches!(row.message.kind, MessageKind::UnableToDecrypt))
                {
                    self.list.splice(ix..ix + 1, 1);
                }
            }
        }
        cx.notify();
    }

    /// Whether any message here cannot be decrypted.
    pub fn has_undecryptable(&self) -> bool {
        self.undecryptable
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
        self.undecryptable = self.entries.iter().any(|entry| {
            matches!(entry, TimelineEntry::Message(m) if matches!(m.kind, MessageKind::UnableToDecrypt))
        });
        let visible: Vector<TimelineEntry>;
        let entries = if self.hidden.is_empty() {
            &self.entries
        } else {
            visible = self
                .entries
                .iter()
                .filter(|entry| {
                    !matches!(entry, TimelineEntry::Message(m) if self.hidden.contains(&m.unique_id))
                })
                .cloned()
                .collect();
            &visible
        };
        let mut rows = group_rows(entries, self.new_marker.as_deref(), &known);
        if self.entries.is_empty() {
            if self.load_error.is_none() {
                rows = vec![Row::Skeleton { older: false }];
            }
        } else if !self.at_start() && self.handle.is_some() {
            rows.insert(0, Row::Skeleton { older: true });
        } else if self.at_start()
            && !rows.iter().any(|r| matches!(r, Row::Message { .. }))
            && !rows.iter().any(|r| matches!(r, Row::Start))
        {
            rows.push(Row::Start);
        }
        let (start, removed, added) = changed_range(&self.rows, &rows);
        // A view scrolled up stays on the row it was on: the list keeps its
        // place by row number, which older rows (or a replaced run of rows)
        // arriving above or around it would otherwise move.
        let top = self.list.logical_scroll_top();
        let anchor = (top.item_ix < self.rows.len())
            .then(|| anchor_row(&self.rows, top.item_ix, &rows))
            .flatten();
        if removed > 0 || added > 0 {
            self.list.splice(start..start + removed, added);
            if let Some((was, now)) = anchor {
                let offset_in_item = if was == top.item_ix {
                    top.offset_in_item
                } else {
                    px(0.0)
                };
                self.list.scroll_to(ListOffset {
                    item_ix: now,
                    offset_in_item,
                });
            }
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
            // Test builds can hold a page back, to look at the placeholders.
            if std::env::var_os("MORPHEUS_E2E_PORT").is_some()
                && let Some(ms) = std::env::var("MORPHEUS_E2E_HISTORY_DELAY_MS")
                    .ok()
                    .and_then(|v| v.parse().ok())
            {
                cx.background_executor()
                    .timer(Duration::from_millis(ms))
                    .await;
            }
            this.update(cx, |this, cx| {
                this.paginating = false;
                match result {
                    Ok(hit_start) => this.reached_start = hit_start,
                    Err(err) => this.load_error = Some(err.to_string()),
                }
                this.last_update = std::time::Instant::now();
                this.rebuild();
                this.continue_jump(cx);
                this.check_prefill(cx);
                cx.notify();
                // A page of hidden events (joins, say) leaves the column short: look
                // again once it has settled, nothing else will draw it.
                if !this.reached_start {
                    let has_messages = this.rows.iter().any(|r| matches!(r, Row::Message { .. }));
                    let delay = if has_messages {
                        AUTO_FILL_SETTLE
                    } else {
                        Duration::from_millis(10)
                    };
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(delay).await;
                        this.update(cx, |this, cx| {
                            this.check_history(cx);
                            cx.notify();
                        })
                        .ok();
                    })
                    .detach();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Shows a message (a search result): scrolls to it, paging back through
    /// history first if it is not loaded, and lets it glow for a moment.
    pub fn jump_to(&mut self, event_id: String, cx: &mut Context<Self>) {
        self.jump = Some((event_id, 0));
        self.continue_jump(cx);
        cx.notify();
    }

    /// The row of the message with this event ID.
    fn row_of_event(&self, event_id: &str) -> Option<(usize, String)> {
        self.rows.iter().enumerate().find_map(|(ix, row)| match row {
            Row::Message { row, .. }
                if matches!(&row.message.id, MessageId::Event(id) if id.as_str() == event_id) =>
            {
                Some((ix, row.message.unique_id.clone()))
            }
            _ => None,
        })
    }

    /// Takes the pending jump a step: scroll if the message is here, else
    /// load another page back.
    fn continue_jump(&mut self, cx: &mut Context<Self>) {
        let Some((event_id, pages)) = self.jump.clone() else {
            return;
        };
        if let Some((ix, unique_id)) = self.row_of_event(&event_id) {
            self.jump = None;
            // A little context above it.
            self.list.scroll_to(ListOffset {
                item_ix: ix.saturating_sub(2),
                offset_in_item: px(0.0),
            });
            self.highlight = Some(unique_id.clone());
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(JUMP_HIGHLIGHT).await;
                this.update(cx, |this, cx| {
                    if this.highlight.as_ref() == Some(&unique_id) {
                        this.highlight = None;
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
            cx.notify();
            return;
        }
        if self.handle.is_none() || self.paginating {
            return;
        }
        if self.at_start() || pages >= JUMP_MAX_PAGES {
            self.jump = None;
            return;
        }
        self.jump = Some((event_id, pages + 1));
        self.load_older(cx);
    }

    /// Has this many messages loaded without being on screen (a voice channel
    /// joined: its chat opens full), paging back through hidden events
    /// until they are there or the start is reached.
    pub fn prefill(&mut self, messages: usize, cx: &mut Context<Self>) {
        self.prefill = messages;
        self.check_prefill(cx);
    }

    fn check_prefill(&mut self, cx: &mut Context<Self>) {
        if self.prefill == 0 || self.handle.is_none() || self.paginating || self.at_start() {
            return;
        }
        let visible = self.rows.iter().filter(|r| matches!(r, Row::Message { .. })).count();
        if visible >= self.prefill || self.prefill_pages >= PREFILL_MAX_PAGES {
            return;
        }
        self.prefill_pages += 1;
        self.load_older(cx);
    }

    /// Older history when the view is near the top of what is loaded, or
    /// what is loaded does not fill it.
    fn check_history(&mut self, cx: &mut Context<Self>) {
        if self.handle.is_none() || self.paginating || self.at_start() {
            return;
        }
        let has_messages = self.rows.iter().any(|r| matches!(r, Row::Message { .. }));
        if !has_messages {
            self.load_older(cx);
            return;
        }
        let top = self.list.logical_scroll_top().item_ix;
        if top <= NEAR_TOP_ROWS {
            self.load_older(cx);
            return;
        }
        let settled = self.last_update.elapsed() >= AUTO_FILL_SETTLE;
        let viewport = metrics(&self.list).viewport;
        let at_bottom = !self.away_from_present();
        if at_bottom
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
        size: (u32, u32),
        cx: &mut Context<Self>,
    ) -> Option<Arc<Image>> {
        self.picture_of(None, media, size, cx)
    }

    /// [`Self::picture`], of a person when `person` says whose (their
    /// picture falls back to their other one when this one is gone).
    fn picture_of(
        &mut self,
        person: Option<(OwnedUserId, Option<OwnedRoomId>)>,
        media: &MediaRef,
        (w, h): (u32, u32),
        cx: &mut Context<Self>,
    ) -> Option<Arc<Image>> {
        if let Some(image) = self.images.get(media, w) {
            return Some(image);
        }
        let session = self.session.clone()?;
        if self.images.begin(media, w) {
            let fetching = media::fetch_picture(&session, person, media, w, h);
            let media = media.clone();
            cx.spawn(async move |this, cx| {
                let image = media::fetched(cx, fetching, (w, h), false).await;
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

    /// The message whose menu is open stays highlighted while it is.
    pub fn set_menu_message(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        if self.menu_message != id {
            self.menu_message = id;
            cx.notify();
        }
    }

    /// A typed "" of a member is a mention pill, and lights the row of
    /// the one it names (you).
    fn with_name_mentions(&self, row: Arc<MessageRow>) -> Arc<MessageRow> {
        if self.people.is_empty() || row.blocks.is_empty() {
            return row;
        }
        let Some((blocks, ids)) = crate::view_model::rich::name_mentions(&row.blocks, &self.people)
        else {
            return row;
        };
        let mut message = row.message.clone();
        let me = self.session.as_ref().and_then(|s| s.user_id());
        if me.is_some_and(|me| ids.iter().any(|id| id == me.as_str())) && !message.is_own {
            message.mentions_me = true;
        }
        Arc::new(MessageRow {
            message,
            blocks,
            time: row.time.clone(),
        })
    }

    /// The card under a message with a link, if previews are allowed here:
    /// asks the homeserver the first time, and again lays the row out when
    /// the answer (and its picture) arrive.
    fn link_preview(&mut self, row: &MessageRow, pictures: &mut Pictures, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        if !matches!(
            row.message.kind,
            MessageKind::Text { .. } | MessageKind::Notice { .. }
        ) || !crate::link_previews::settings().allows(self.encrypted)
            || crate::link_previews::is_dismissed(&row.message.unique_id)
        {
            return;
        }
        let Some(url) = crate::view_model::link_previews::first_url(&row.blocks) else {
            return;
        };
        let unique_id = row.message.unique_id.clone();
        crate::link_previews::want(&session, &url, cx, move |this: &mut Self, cx| {
            this.relayout(&unique_id, cx)
        });
        if let Some(preview) = crate::link_previews::card(&url) {
            let image = preview
                .image
                .as_ref()
                .and_then(|i| crate::link_previews::picture(i.key()));
            pictures.preview = Some(crate::message::PreviewCard { preview, image });
        }
    }

    fn render_row(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(row) = self.rows.get(ix).cloned() else {
            return div().into_any_element();
        };
        let Row::Message { row, show_header } = row else {
            if matches!(row, Row::Start)
                && self.say_hi
                && !self.rows.iter().any(|r| {
                    matches!(r, Row::Message { row, .. } if !matches!(row.message.kind, MessageKind::State(_)))
                })
            {
                let weak = cx.weak_entity();
                return message::say_hi_row(move |cx| {
                    weak.update(cx, |this, cx| this.say_hi(cx)).ok();
                });
            }
            return message::other_row(&row);
        };
        // Someone you ignore: folded away, shown on request.
        if self.ignored.contains(row.message.sender.as_str())
            && !self.revealed.contains(&row.message.unique_id)
        {
            let unique_id = row.message.unique_id.clone();
            return div()
                .id(gpui::SharedString::from(format!("ignored-{unique_id}")))
                .px(px(16.0))
                .py(px(4.0))
                .child(
                    div()
                        .id(gpui::SharedString::from(format!(
                            "ignored-fold-{unique_id}"
                        )))
                        .cursor_pointer()
                        .text_size(px(0.85 * theme::REM))
                        .text_color(rgb(theme::text_faint()))
                        .child("Ignored message · Show")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.revealed.insert(unique_id.clone());
                            this.list.splice(ix..ix + 1, 1);
                            cx.notify();
                        }))
                        .e2e("button", "ignored-fold")
                        .text("Ignored message · Show"),
                )
                .e2e("div", "message ignored")
                .attr(
                    "data-event-id",
                    match &row.message.id {
                        morpheus_core::MessageId::Event(id) => id.to_string(),
                        morpheus_core::MessageId::Transaction(_) => String::new(),
                    },
                )
                .into_any_element();
        }
        let mut row = self.with_name_mentions(row);
        if let Some((name, avatar)) = self.profiles.get(row.message.sender.as_str()) {
            let mut message = row.message.clone();
            message.sender_name = name
                .clone()
                .unwrap_or_else(|| message.sender.localpart().to_owned());
            message.sender_avatar = avatar.clone();
            row = Arc::new(MessageRow {
                message,
                blocks: row.blocks.clone(),
                time: row.time.clone(),
            });
        }
        let mut pictures = Pictures {
            keys: self.keys,
            highlight: self.highlight.as_deref() == Some(row.message.unique_id.as_str()),
            menu_open: self.menu_message.as_deref() == Some(row.message.unique_id.as_str()),
            selected: self.vim_selected.as_deref() == Some(row.message.unique_id.as_str()),
            avatar_pressed: self.pressed_avatar.as_deref() == Some(row.message.unique_id.as_str()),
            after_divider: ix > 0 && matches!(self.rows.get(ix - 1), Some(Row::Date(..))),
            ..Pictures::default()
        };
        *pictures.editor.borrow_mut() = self.edit_el(&row.message.unique_id);
        self.link_preview(&row, &mut pictures, cx);
        if let Some(session) = self.session.clone() {
            // Custom emoji in the text and in reactions.
            for url in crate::view_model::rich::emote_urls(&row.blocks) {
                crate::custom_emoji::want(&session, &url, cx);
            }
            for reaction in &row.message.reactions {
                crate::custom_emoji::want(&session, &reaction.key, cx);
            }
        }
        if show_header {
            pictures.nickname = self.nicknames.get(row.message.sender.as_str()).cloned();
        }
        if show_header && let Some(avatar) = &row.message.sender_avatar {
            let person = Some((row.message.sender.clone(), Some(self.room_id.clone())));
            pictures.avatar = self.picture_of(person, avatar, (80, 80), cx);
            // A copy read elsewhere stands in; else a skeleton, not the letter.
            if pictures.avatar.is_none() {
                pictures.avatar = self.images.get_or_stand_in(avatar, 80);
            }
            pictures.avatar_waiting = pictures.avatar.is_none() && self.images.waiting(avatar, 80);
        }
        match &row.message.kind {
            MessageKind::Image { media, .. } => {
                pictures.media = self.picture(media, (800, 600), cx);
            }
            MessageKind::Video {
                media,
                thumbnail,
                body,
                width,
                height,
                duration,
            } => {
                let source = thumbnail.as_ref().unwrap_or(media).clone();
                // Without a thumbnail, the picture comes from the player itself.
                let thumb = if thumbnail.is_some() {
                    self.picture(&source, (480, 320), cx)
                } else {
                    None
                };
                let player = match self.videos.get(&row.message.unique_id) {
                    Some(player) => player.clone(),
                    None => {
                        let session = self.session.clone();
                        let (media, body, width, height, duration) =
                            (media.clone(), body.clone(), *width, *height, *duration);
                        let player = cx.new(|cx| {
                            crate::video::VideoPlayer::new(
                                session, media, body, width, height, duration, cx,
                            )
                        });
                        cx.subscribe(
                            &player,
                            |_, player, event: &crate::video::VideoEvent, cx| {
                                if matches!(event, crate::video::VideoEvent::Fullscreen) {
                                    cx.emit(TimelineEvent::FullscreenVideo(player));
                                }
                            },
                        )
                        .detach();
                        self.videos
                            .insert(row.message.unique_id.clone(), player.clone());
                        player
                    }
                };
                player.update(cx, |player, cx| player.set_thumbnail(thumb.clone(), cx));
                pictures.media = thumb;
                pictures.video = Some(player.into());
            }
            _ => {}
        }
        message::message_el(
            &row,
            show_header,
            &pictures,
            self.downloads.get(&row.message.unique_id),
            &self.links,
            Some(&self.actions),
        )
    }

    /// The room's pictures in timeline order, with their messages' unique IDs.
    fn gallery(&self) -> Vec<(String, crate::lightbox::GalleryImage)> {
        self.rows
            .iter()
            .filter_map(|row| match row {
                Row::Message { row, .. } => match &row.message.kind {
                    MessageKind::Image {
                        media,
                        body,
                        width,
                        height,
                        ..
                    } => Some((
                        row.message.unique_id.clone(),
                        crate::lightbox::GalleryImage {
                            media: media.clone(),
                            body: body.clone(),
                            width: *width,
                            height: *height,
                        },
                    )),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    /// A file card's Download: saves the file, the card saying where.
    fn download(&mut self, message: &Message, cx: &mut Context<Self>) {
        let (media, name) = match &message.kind {
            MessageKind::File { media, body, .. } | MessageKind::Audio { media, body, .. } => {
                (media.clone(), body.clone())
            }
            _ => return,
        };
        let Some(session) = self.session.clone() else {
            return;
        };
        let id = message.unique_id.clone();
        self.set_download(&id, Some(crate::message::Download::Downloading), cx);
        let saving = session.download(media, name);
        cx.spawn(async move |this, cx| {
            let state = match saving.await {
                Ok(path) => crate::message::Download::Saved(path.display().to_string()),
                Err(err) => crate::message::Download::Failed(err.to_string()),
            };
            this.update(cx, |this, cx| this.set_download(&id, Some(state), cx))
                .ok();
        })
        .detach();
    }

    /// Records a card's download state and has the row drawn again.
    fn set_download(
        &mut self,
        unique_id: &str,
        state: Option<crate::message::Download>,
        cx: &mut Context<Self>,
    ) {
        match state {
            Some(state) => self.downloads.insert(unique_id.to_owned(), state),
            None => self.downloads.remove(unique_id),
        };
        if let Some(ix) = self.rows.iter().position(
            |row| matches!(row, Row::Message { row, .. } if row.message.unique_id == unique_id),
        ) {
            self.list.splice(ix..ix + 1, 1);
        }
        cx.notify();
    }

    /// A click on a message's hover bar, or a right click on it.
    pub fn row_action(&mut self, action: RowAction, unique_id: &str, cx: &mut Context<Self>) {
        if let RowAction::AvatarPress(down) = action {
            let now = down.then(|| unique_id.to_owned());
            if self.pressed_avatar != now {
                self.pressed_avatar = now;
                cx.notify();
            }
            return;
        }
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
            RowAction::Edit => self.start_edit(message, cx),
            RowAction::Thread => cx.emit(TimelineEvent::Thread(Box::new(message))),
            RowAction::React(key) => self.react(message.id, key.to_owned(), cx),
            RowAction::Context(at) => cx.emit(TimelineEvent::Menu(Box::new(message), at)),
            RowAction::MoreReactions(at) => cx.emit(TimelineEvent::Picker(Box::new(message), at)),
            RowAction::Delete(skip_asking) => {
                cx.emit(TimelineEvent::Delete(Box::new(message), skip_asking))
            }
            RowAction::Sender(at) => cx.emit(TimelineEvent::Sender(Box::new(message), at)),
            RowAction::SenderMenu(at) => cx.emit(TimelineEvent::SenderMenu(Box::new(message), at)),
            RowAction::AvatarPress(_) => {}
            RowAction::Toggle(key) => self.react(message.id, key, cx),
            RowAction::Download => self.download(&message, cx),
            RowAction::DismissPreview => {
                crate::link_previews::dismiss(unique_id);
                self.relayout(unique_id, cx);
            }
            RowAction::OpenImage => {
                let gallery = self.gallery();
                if let Some(at) = gallery.iter().position(|(id, _)| id == unique_id) {
                    cx.emit(TimelineEvent::OpenImage(
                        gallery.into_iter().map(|(_, image)| image).collect(),
                        at,
                    ));
                }
            }
        }
    }

    /// Adds our `key` reaction to a message, or takes it back.
    pub fn react(&mut self, id: MessageId, key: String, cx: &mut Context<Self>) {
        let Some(handle) = &self.handle else { return };
        let toggling = handle.toggle_reaction(id, key);
        cx.spawn(async move |this, cx| {
            if let Err(err) = toggling.await {
                this.update(cx, |_, cx| {
                    cx.emit(TimelineEvent::Failed("reaction", err.to_string()))
                })
                .ok();
            }
        })
        .detach();
    }

    /// Takes a message off the list while its deletion can be undone.
    pub fn hide(&mut self, unique_id: &str) {
        if self.hidden.insert(unique_id.to_owned()) {
            self.rebuild();
        }
    }

    /// Puts a hidden message back.
    pub fn show(&mut self, unique_id: &str, cx: &mut Context<Self>) {
        if self.hidden.remove(unique_id) {
            self.rebuild();
            cx.notify();
        }
    }

    /// Whether this timeline sends read receipts (it is in sight, not kept
    /// unread, and not a thread).
    pub fn reads(&self) -> bool {
        !self.keep_unread && !self.covered && self.thread_root.is_none()
    }

    /// Whether the timeline is out of sight (then it reads nothing); shown
    /// again, it reads the room up to its newest message.
    pub fn set_covered(&mut self, covered: bool, cx: &mut Context<Self>) {
        self.cover(covered, true, cx);
    }

    /// `to_present`: shown again, the view goes to the newest message (a room
    /// resumed keeps its place instead).
    fn cover(&mut self, covered: bool, to_present: bool, cx: &mut Context<Self>) {
        if self.covered == covered {
            return;
        }
        self.covered = covered;
        if !covered {
            if !self.keep_unread
                && self.thread_root.is_none()
                && let Some(handle) = self.handle.as_ref()
            {
                let receipt = handle.send_read_receipt();
                cx.spawn(async move |_, _| drop(receipt.await)).detach();
            }
            if to_present {
                self.jump_to_present(cx);
            }
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                this.update(cx, |this, cx| {
                    this.check_history(cx);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    /// The newest message listed (its unique ID).
    fn newest_message(&self) -> Option<String> {
        self.entries.iter().rev().find_map(|entry| match entry {
            TimelineEntry::Message(m) => Some(m.unique_id.clone()),
            _ => None,
        })
    }

    /// The room is left but kept (see `TimelineCache`): leaving reads it up
    /// to its newest message, as dropping it would, and from here on it
    /// reads nothing, though it goes on taking in what arrives.
    pub fn leave(&mut self) {
        if self.thread_root.is_some() || self.left_at.is_some() {
            return;
        }
        if let Some(handle) = self.handle.as_ref()
            && !self.keep_unread
            && !self.covered
        {
            drop(handle.mark_as_fully_read());
        }
        self.covered = true;
        let at_end = metrics(&self.list).from_bottom < 2.0;
        self.left_at = Some((self.newest_message(), at_end));
    }

    /// A room kept by [`Self::leave`] is shown again: what came after the
    /// newest message it had is new (the NEW divider goes above it), the view
    /// is where it was, and the room is read again.
    pub fn resume(&mut self, cx: &mut Context<Self>) {
        let Some((newest, at_end)) = self.left_at.take() else {
            return;
        };
        self.keep_unread = false;
        self.paging = None;
        let first_new = newest.and_then(|newest| {
            let at = self
                .entries
                .iter()
                .position(|e| matches!(e, TimelineEntry::Message(m) if m.unique_id == newest))?;
            self.entries
                .iter()
                .skip(at + 1)
                .find_map(|entry| match entry {
                    TimelineEntry::Message(m) if !m.is_own => Some(m.unique_id.clone()),
                    _ => None,
                })
        });
        self.caught_up = first_new.is_none();
        self.new_marker = first_new;
        self.rebuild();
        if at_end {
            self.jump_to_present(cx);
        }
        if let Some(handle) = &self.handle {
            cx.emit(TimelineEvent::Opened(handle.clone()));
        }
        cx.emit(TimelineEvent::LastOwn(self.last_own.clone()));
        self.cover(false, false, cx);
        cx.notify();
    }

    /// The room's pictures leave the shared copies and the stand-ins (it was
    /// locked: the views are dropped by the caller).
    pub fn forget_pictures(&self) {
        self.images.forget_shared();
        crate::message::forget_blurs();
    }

    /// What keeping this room costs, in entries (a picture counts as one).
    pub fn weight(&self) -> usize {
        // Hidden events (a voice channel's calls coming and going) cost next to nothing.
        let shown = self
            .entries
            .iter()
            .filter(|e| !matches!(e, TimelineEntry::Message(m) if matches!(m.kind, MessageKind::CallPlumbing)))
            .count();
        shown + self.images.len()
    }

    /// The room was marked unread: leaving it must not read it again.
    pub fn keep_unread(&mut self) {
        self.keep_unread = true;
    }

    /// Whether the view is far enough from the newest message for the jump button.
    pub fn away_from_present(&self) -> bool {
        metrics(&self.list).from_bottom > JUMP_THRESHOLD_PX
    }

    /// Home: to the oldest loaded message, and more history loads.
    pub fn jump_to_oldest(&mut self, cx: &mut Context<Self>) {
        self.list.scroll_to(ListOffset {
            item_ix: 0,
            offset_in_item: px(0.0),
        });
        self.load_older(cx);
        cx.notify();
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
        // Leaving a room reads it up to its newest message (unless it was
        // marked unread from here).
        if let Some(handle) = self.handle.take()
            && !self.keep_unread
            && !self.covered
            && self.thread_root.is_none()
        {
            drop(handle.mark_as_fully_read());
        }
    }
}

impl TimelineView {
    /// The scrollbar over the list (#144), if it scrolls.
    fn scrollbar_el(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        use crate::view_model::scrollbar as vm;
        // The list is measured after its first frame: look again a few times.
        if self.bar.seen_rows != self.rows.len() {
            self.bar.seen_rows = self.rows.len();
            self.bar.retries = 0;
        }
        let Some(frame) = self.bar.frame(&self.list) else {
            if self.bar.retries < 90 {
                self.bar.retries += 1;
                crate::scrollbar::want_frame(window);
            }
            return None;
        };
        self.bar.retries = 0;
        if frame.animating {
            crate::scrollbar::want_frame(window);
        } else if let Some(wait) = frame.fade_in_ms
            && !self.bar.timer
        {
            self.bar.timer = true;
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(wait))
                    .await;
                this.update(cx, |this, cx| {
                    this.bar.timer = false;
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        let bounds = self.bar.bounds.clone();
        let thumb = (frame.opacity > 0.0).then(|| {
            div()
                .absolute()
                .top(px(frame.top))
                .right(px(3.0))
                .w(px(vm::width(frame.wide)))
                .h(px(frame.len))
                .rounded_full()
                .bg(rgb(if frame.wide { theme::text_muted() } else { theme::text_faint() }))
                .opacity(frame.opacity)
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.bar.grab_at(&this.list, f32::from(event.position.y));
                        cx.notify();
                    }),
                )
                .e2e("div", "scrollbar-thumb")
                .attr("data-opacity", format!("{:.2}", frame.opacity))
                .attr("data-wide", frame.wide.to_string())
                .attr("data-len", format!("{:.1}", frame.len))
                .attr("data-top", format!("{:.1}", frame.top))
        });
        Some(
            div()
                .id("scrollbar")
                .absolute()
                .top_0()
                .bottom_0()
                .right_0()
                .w(px(14.0))
                .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                    this.bar.over_track = *hovered;
                    this.bar.touch();
                    cx.notify();
                }))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                        this.bar.click_track(&this.list, f32::from(event.position.y));
                        cx.notify();
                    }),
                )
                .child(
                    gpui::canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {})
                        .absolute()
                        .size_full(),
                )
                .children(thumb)
                .e2e("div", "scrollbar")
                .attr("data-engaged", frame.engaged.to_string())
                .into_any_element(),
        )
    }
}

impl Render for TimelineView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.check_history(cx);
        if let Some(edit) = self.edit.as_mut().filter(|e| e.focus) {
            edit.focus = false;
            edit.input.read(cx).focus(window);
        }
        let now = metrics(&self.list);
        // Rows just spliced in are unmeasured until painted and read as no
        // range at all: that is not a jump to the newest message.
        let away = if now.range == 0.0 && self.rows.len() > 1 {
            self.was_away
        } else {
            now.from_bottom > JUMP_THRESHOLD_PX
        };
        self.was_away = away;
        let rows = list(
            self.list.clone(),
            cx.processor(|this, ix, _window, cx| this.render_row(ix, cx)),
        )
        // Pinned to the timeline by its edges, not a percentage size: under a
        // flex-sized parent (the voice channel's chat panel in a big window)
        // 100% is indefinite, and the list then took its rows' measured
        // height, a sliver at the top (#25).
        .absolute()
        .inset_0()
        .pt(px(16.0))
        .pb(px(8.0));
        let scrollbar = self.scrollbar_el(window, cx);
        let dragging = self.bar.grab.is_some();
        let mut timeline = div()
            .id("timeline-section")
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.bar.over_list = *hovered;
                this.bar.touch();
                cx.notify();
            }))
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .line_height(relative(theme::LINE_HEIGHT))
            .child(rows)
            .children(scrollbar);
        // While the thumb is held, the window's own mouse listeners (not the
        // panel's) follow the pointer wherever it goes and let go wherever the
        // button comes up; the other views see none of it (#148).
        if dragging {
            let this = cx.entity();
            timeline = timeline.child(gpui::canvas(
                |_, _, _| (),
                move |_, _, window, _| {
                    let moved = this.clone();
                    window.on_mouse_event(move |event: &gpui::MouseMoveEvent, phase, _, cx| {
                        if phase != gpui::DispatchPhase::Capture {
                            return;
                        }
                        cx.stop_propagation();
                        moved.update(cx, |this, cx| {
                            if event.pressed_button != Some(gpui::MouseButton::Left) {
                                // The button came up out of our sight.
                                this.bar.end_drag(&this.list);
                            } else {
                                this.bar.drag_to(&this.list, event);
                            }
                            cx.notify();
                        });
                    });
                    let up = this.clone();
                    window.on_mouse_event(move |_: &gpui::MouseUpEvent, phase, _, cx| {
                        if phase != gpui::DispatchPhase::Capture {
                            return;
                        }
                        cx.stop_propagation();
                        up.update(cx, |this, cx| {
                            this.bar.end_drag(&this.list);
                            cx.notify();
                        });
                    });
                },
            ).absolute().size_0());
        }
        if let Some(error) = &self.load_error {
            timeline = timeline.child(
                div()
                    .absolute()
                    .top(px(8.0))
                    .left(px(16.0))
                    .text_color(rgb(theme::danger()))
                    .child(error.clone())
                    .e2e("p", "error")
                    .text(error.clone()),
            );
        }
        // Escape does this jump where nothing nearer has the key: the main
        // timeline's pill shows the key cap unless a thread panel (whose own pill
        // it is) is open.
        let show_key = self.thread_root.is_some() || !THREAD_PANEL_OPEN.load(std::sync::atomic::Ordering::Relaxed);
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
                            .bg(rgb(theme::accent_strong()))
                            .text_color(rgb(theme::on_accent_strong()))
                            .text_size(px(0.8 * theme::REM))
                            .font_weight(FontWeight::SEMIBOLD)
                            .shadow_md()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(theme::accent())))
                            .on_click(cx.listener(|this, _, _, cx| this.jump_to_present(cx)))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child("Jump to present ↓")
                            .children(show_key.then(|| crate::dialogs::key_cap("Esc", None)))
                            .e2e("button", "jump-to-bottom")
                            .attr("title", "Jump to the newest message (Esc)")
                            .text(if show_key { "Jump to present ↓ Esc" } else { "Jump to present ↓" }),
                    ),
            );
        }
        let timeline = timeline
            .e2e("section", "timeline")
            .attr(
                "data-history-requests",
                morpheus_core::history_requests().to_string(),
            )
            .scrollable(Rc::new(ListProbe(self.list.clone())));
        let banner =
            self.thread_root.is_none() && show_banner(true, self.keys, self.has_undecryptable());
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .w_full()
            .when(banner, |d| d.child(key_banner(cx)))
            .child(timeline)
    }
}

/// The notice above a chat with messages this device cannot read.
fn key_banner(cx: &mut Context<TimelineView>) -> impl IntoElement {
    let action = |id: &'static str,
                  label: &'static str,
                  glyph: &'static str,
                  primary: bool,
                  focus: SettingsFocus,
                  cx: &mut Context<TimelineView>| {
        div()
            .id(id)
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(12.0))
            .py(px(8.0))
            .rounded(px(theme::RADIUS))
            .cursor_pointer()
            .bg(if primary {
                rgb(theme::accent())
            } else {
                gpui::rgba(theme::bg_hover()).into()
            })
            .text_color(if primary {
                rgb(theme::on_accent())
            } else {
                rgb(theme::text())
            })
            .child(icon(glyph, 16.0).text_color(if primary {
                rgb(theme::on_accent())
            } else {
                rgb(theme::text())
            }))
            .child(label)
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(TimelineEvent::OpenSettings(focus))))
            .e2e("button", if primary { "primary" } else { "secondary" })
            .text(label)
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .flex_wrap()
        .gap(px(12.0))
        .mx(px(16.0))
        .mt(px(8.0))
        .px(px(14.0))
        .py(px(10.0))
        .rounded(px(theme::RADIUS_LG))
        .bg(rgb(theme::bg_elevated()))
        .border_1()
        .border_color(gpui::rgba(theme::alpha(theme::accent(), 0x59)))
        .child(
            div()
                .text_color(rgb(theme::accent()))
                .child(icon("lock", 18.0).text_color(rgb(theme::accent())))
                .e2e("span", "key-banner-icon"),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .flex_1()
                .min_w(px(200.0))
                .text_size(px(0.9 * theme::REM))
                .child(
                    div()
                        .font_weight(FontWeight::BOLD)
                        .child(BANNER_TITLE)
                        .e2e("strong", "")
                        .text(BANNER_TITLE),
                )
                .child(
                    div()
                        .text_color(rgb(theme::text_muted()))
                        .child(BANNER_TEXT)
                        .e2e("span", "")
                        .text(BANNER_TEXT),
                )
                .e2e("div", "key-banner-text"),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(8.0))
                .child(action(
                    "key-verify",
                    BANNER_VERIFY,
                    "shield-check",
                    true,
                    SettingsFocus::Security,
                    cx,
                ))
                .child(action(
                    "key-recovery",
                    BANNER_RECOVERY,
                    "key",
                    false,
                    SettingsFocus::RecoveryKey,
                    cx,
                ))
                .e2e("div", "key-banner-actions"),
        )
        .e2e("div", "key-banner")
        .attr("role", "status")
}

#[cfg(all(test, feature = "e2e"))]
mod widget_tests {
    use super::*;
    use crate::e2e::{self, query};
    use crate::view_model::keys::{BANNER_RECOVERY, BANNER_VERIFY, LOCKED_TEXT};
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
            sender_name: sender
                .trim_start_matches('@')
                .split(':')
                .next()
                .unwrap_or("")
                .into(),
            sender_avatar: None,
            timestamp_ms: at,
            is_own: false,
            is_local_echo: false,
            is_edited: false,
            mentions_me: false,
            can_edit: false,
            reactions: Vec::new(),
            in_reply_to: None,
            thread_root: None,
            thread: None,
            kind,
            shield: None,
            state_kind: None,
            state_who: None,
        }))
    }

    fn text(body: &str) -> MessageKind {
        MessageKind::Text {
            body: body.into(),
            html: None,
        }
    }

    fn open(cx: &mut TestAppContext) -> (Entity<TimelineView>, &mut VisualTestContext) {
        let room = OwnedRoomId::try_from("!r:x").expect("room id");
        let (harness, cx) =
            cx.add_window_view(|_, cx| Harness(cx.new(|cx| TimelineView::new(None, room, cx))));
        let view = harness.read_with(cx, |h, _| h.0.clone());
        (view, cx)
    }

    fn feed(view: &Entity<TimelineView>, cx: &mut VisualTestContext, entries: Vec<TimelineEntry>) {
        view.update(cx, |t, cx| {
            t.apply(
                vec![VectorDiff::Append {
                    values: entries.into_iter().collect(),
                }],
                cx,
            )
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

    fn locked() -> KeyAccess {
        KeyAccess {
            locked: true,
            backup_missing: false,
        }
    }

    fn redraw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.draw(
            point(px(0.0), px(0.0)),
            gpui::size(px(700.0), px(500.0)),
            |_, _| div(),
        );
        cx.run_until_parked();
    }

    /// The placeholder rows shown while a room loads (and above its oldest
    /// message while older ones load) are message-shaped: a grey picture and
    /// grey lines across the row, not the pictures alone.
    #[gpui::test]
    fn placeholder_rows_have_their_grey_lines(cx: &mut TestAppContext) {
        let (_view, cx) = open(cx);
        redraw(cx);
        let rows = e2e::query_all_bounds(".skeleton-row .message-main");
        let lines = e2e::query_all_bounds(".skeleton-bar");
        assert!(!rows.is_empty() && !lines.is_empty(), "no placeholder rows");
        for row in rows {
            assert!(row.w >= 300.0, "a placeholder row's lines are {row:?}");
        }
        for line in lines {
            assert!(
                line.w >= 60.0 && line.h > 0.0,
                "a placeholder line is {line:?}"
            );
        }
    }

    /// Hidden call events cost next to nothing in the cache budget.
    #[gpui::test]
    fn hidden_call_events_do_not_count_toward_the_weight(cx: &mut TestAppContext) {
        let (view, cx) = open(cx);
        let mut entries: Vec<_> = (0..50)
            .map(|n| entry(n, "@bob:x", 1000 + n as u64, MessageKind::CallPlumbing))
            .collect();
        entries.push(entry(100, "@bob:x", 5000, text("hello")));
        feed(&view, cx, entries);
        assert_eq!(view.read_with(cx, |t, _| t.weight()), 1);
    }

    /// A channel whose loaded events are all hidden call events automatically
    /// shows the beginning of the room once start is reached, instead of being blank.
    #[gpui::test]
    fn hidden_call_events_at_start_show_beginning_row(cx: &mut TestAppContext) {
        let (view, cx) = open(cx);
        feed(
            &view,
            cx,
            vec![
                entry(0, "@alice:x", 1000, MessageKind::CallPlumbing),
                entry(1, "@bob:x", 1001, MessageKind::CallPlumbing),
            ],
        );
        view.update(cx, |t, _| {
            t.reached_start = true;
            t.rebuild();
        });
        redraw(cx);
        assert!(
            !texts(".timeline-start").is_empty(),
            "beginning of room should show"
        );
    }

    #[gpui::test]
    fn messages_without_keys_are_locked_rows_with_a_banner_never_a_skeleton(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = open(cx);
        feed(
            &view,
            cx,
            vec![
                entry(1, "@bob:x", 0, MessageKind::UnableToDecrypt),
                entry(2, "@bob:x", 1000, text("readable")),
            ],
        );
        // Keys unknown yet: a loading shimmer, and no banner.
        assert_eq!(query(".message .undecrypted.skeleton").len(), 1);
        assert!(query(".key-banner").is_empty());

        view.update(cx, |t, cx| t.set_keys(locked(), cx));
        redraw(cx);
        assert!(query(".undecrypted.skeleton").is_empty());
        assert_eq!(texts(".message .undecrypted.locked"), [LOCKED_TEXT]);
        assert_eq!(
            texts(".key-banner button"),
            [BANNER_VERIFY, BANNER_RECOVERY]
        );
        assert_eq!(texts(".message .body:not(.undecrypted)"), ["readable"]);

        // The message is decrypted later: the row becomes it, the banner goes.
        view.update(cx, |t, cx| {
            t.apply(
                vec![VectorDiff::Set {
                    index: 0,
                    value: entry(1, "@bob:x", 0, text("now readable")),
                }],
                cx,
            )
        });
        redraw(cx);
        assert!(query(".undecrypted").is_empty());
        assert!(query(".key-banner").is_empty());
        assert_eq!(texts(".message .body"), ["now readable", "readable"]);
    }

    #[gpui::test]
    fn the_banner_buttons_ask_for_settings(cx: &mut TestAppContext) {
        use std::{cell::RefCell, rc::Rc};
        let (view, cx) = open(cx);
        let asked: Rc<RefCell<Vec<SettingsFocus>>> = Rc::default();
        let log = asked.clone();
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &TimelineEvent, _| {
                if let TimelineEvent::OpenSettings(focus) = event {
                    log.borrow_mut().push(*focus);
                }
            })
            .detach();
        });
        feed(
            &view,
            cx,
            vec![entry(1, "@bob:x", 0, MessageKind::UnableToDecrypt)],
        );
        view.update(cx, |t, cx| t.set_keys(locked(), cx));
        redraw(cx);
        for label in [BANNER_VERIFY, BANNER_RECOVERY] {
            let (_, (x, y)) = query(".key-banner button")
                .into_iter()
                .find(|(text, _)| text == label)
                .expect(label);
            cx.simulate_click(point(px(x), px(y)), gpui::Modifiers::none());
            cx.run_until_parked();
        }
        assert_eq!(
            *asked.borrow(),
            [SettingsFocus::Security, SettingsFocus::RecoveryKey]
        );
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
                    VectorDiff::Set {
                        index: 0,
                        value: entry(1, "@bob:x", 0, text("one, edited")),
                    },
                    VectorDiff::PushBack {
                        value: entry(2, "@bob:x", 10, text("two")),
                    },
                ],
                cx,
            )
        });
        cx.run_until_parked();
        cx.draw(
            point(px(0.0), px(0.0)),
            gpui::size(px(700.0), px(500.0)),
            |_, _| div(),
        );
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
        assert_eq!(
            texts(".message .body a[href=\"https://example.org/a\"]"),
            ["a link"]
        );
        assert_eq!(
            texts(".message .body a[href=\"https://example.org/b\"]"),
            ["https://example.org/b"]
        );
    }

    #[gpui::test]
    fn day_dividers_state_lines_and_the_start_are_rows(cx: &mut TestAppContext) {
        let (view, cx) = open(cx);
        feed(
            &view,
            cx,
            vec![
                TimelineEntry::TimelineStart,
                TimelineEntry::DateDivider {
                    timestamp_ms: 86_400_000 * 3,
                },
                entry(1, "@bob:x", 86_400_000 * 3, text("hi")),
                entry(
                    2,
                    "@bob:x",
                    86_400_000 * 3,
                    MessageKind::State("bob joined the room".into()),
                ),
            ],
        );
        assert_eq!(
            texts(".timeline-start"),
            ["This is the beginning of the room."]
        );
        assert_eq!(query(".date-divider").len(), 1);
        assert_eq!(
            texts(".message.system .system-text"),
            ["bob joined the room"]
        );
    }

    /// The space above the date divider equals the space below it, measured to
    /// the neighbouring rows' hover-highlight areas (the `.message` boxes),
    /// whichever rows they are.
    #[gpui::test]
    fn the_date_divider_has_equal_space_above_and_below(cx: &mut TestAppContext) {
        let day = 86_400_000;
        let state = |id, at| {
            entry(
                id,
                "@bob:x",
                at,
                MessageKind::State("bob joined the room".into()),
            )
        };
        let divider = || TimelineEntry::DateDivider {
            timestamp_ms: day * 3,
        };
        let cases: [(&str, Vec<TimelineEntry>); 4] = [
            (
                "message, message",
                vec![
                    entry(1, "@bob:x", 0, text("before")),
                    divider(),
                    entry(2, "@carol:x", day * 3, text("after")),
                ],
            ),
            (
                "message, state line",
                vec![
                    entry(1, "@bob:x", 0, text("before")),
                    divider(),
                    state(2, day * 3),
                ],
            ),
            (
                "state line, message",
                vec![
                    state(1, 0),
                    divider(),
                    entry(2, "@carol:x", day * 3, text("after")),
                ],
            ),
            (
                "state line, state line",
                vec![state(1, 0), divider(), state(2, day * 3)],
            ),
        ];
        for (what, entries) in cases {
            let (view, cx) = open(cx);
            feed(&view, cx, entries);
            let divider = e2e::query_bounds(".date-divider").expect("divider");
            let rows = e2e::query_all_bounds(".message");
            assert_eq!(rows.len(), 2, "{what}: two rows");
            let (above, below) = (rows[0], rows[1]);
            let gap_above = divider.y - (above.y + above.h);
            let gap_below = below.y - (divider.y + divider.h);
            eprintln!(
                "{what}: above {gap_above} below {gap_below} divider {divider:?} rows {rows:?}"
            );
            assert!(
                (gap_above - gap_below).abs() <= 1.0,
                "{what}: {gap_above} px above the divider, {gap_below} px below"
            );
        }
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
            .map(|n| {
                entry(
                    n,
                    "@bob:x",
                    n as u64 * 1_000_000,
                    text(&format!("message {n}")),
                )
            })
            .collect();
        feed(&view, cx, entries);
        let painted = query(".message").len();
        assert!(
            painted > 3 && painted < 60,
            "{painted} rows painted of 2000"
        );
        assert!(texts(".message .body").contains(&"message 1999".to_owned()));
    }
}

/// A message being edited where it is in the list: its text is a box.
struct InPlaceEdit {
    message: Message,
    original: String,
    input: gpui::Entity<crate::input::TextInput>,
    /// The box takes the caret on the next frame.
    focus: bool,
}

impl TimelineView {
    /// "Edit" on a message (the bin's neighbour, the menu, Up in an empty message
    /// box): the message's text becomes a box in the list, holding its text.
    pub fn start_edit(&mut self, message: Message, cx: &mut Context<Self>) {
        self.cancel_edit(cx);
        let original = message.kind.plain_body().unwrap_or_default().to_owned();
        let input = cx.new(|cx| {
            crate::input::TextInput::new("message-edit", "", false, &original, cx)
                .multiline(10)
                .spellcheck()
                .with_class("message-edit-input")
        });
        cx.subscribe(&input, |this, _, _: &crate::input::Submit, cx| {
            this.save_edit(cx)
        })
        .detach();
        cx.subscribe(&input, |this, _, _: &crate::input::Changed, cx| {
            // The box grew or shrank with its lines: the row is measured again.
            this.remeasure_edit(cx)
        })
        .detach();
        cx.subscribe(&input, |this, _, nav: &crate::input::Nav, cx| {
            if nav.0 == crate::input::NavKey::Escape {
                this.cancel_edit(cx);
            }
        })
        .detach();
        let unique_id = message.unique_id.clone();
        self.edit = Some(InPlaceEdit {
            message,
            original,
            input,
            focus: true,
        });
        self.remeasure(&unique_id);
        if let Some(ix) = self.row_of(&unique_id) {
            self.list.scroll_to_reveal_item(ix);
        }
        cx.notify();
    }

    /// Whether a message is being edited here.
    pub fn is_editing(&self) -> bool {
        self.edit.is_some()
    }

    fn row_of(&self, unique_id: &str) -> Option<usize> {
        self.rows.iter().position(
            |row| matches!(row, Row::Message { row, .. } if row.message.unique_id == unique_id),
        )
    }

    fn remeasure(&mut self, unique_id: &str) {
        if let Some(ix) = self.row_of(unique_id) {
            self.list.splice(ix..ix + 1, 1);
        }
    }

    fn remeasure_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(unique_id) = self.edit.as_ref().map(|e| e.message.unique_id.clone()) {
            self.remeasure(&unique_id);
            cx.notify();
        }
    }

    /// Esc: the message is as it was.
    pub fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(edit) = self.edit.take() {
            self.remeasure(&edit.message.unique_id);
            cx.notify();
        }
    }

    /// Enter: the new text is saved; unchanged is just closed, and empty asks
    /// whether to delete the message.
    fn save_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.edit.take() else {
            return;
        };
        self.remeasure(&edit.message.unique_id);
        cx.notify();
        let text = edit.input.read(cx).value().trim().to_owned();
        if text.is_empty() {
            cx.emit(TimelineEvent::Delete(Box::new(edit.message), false));
            return;
        }
        if text == edit.original.trim() {
            return;
        }
        let Some(handle) = self.handle.clone() else {
            return;
        };
        let saving = handle.edit(edit.message.id.clone(), text);
        cx.spawn(async move |this, cx| {
            if let Err(err) = saving.await {
                this.update(cx, |_, cx| {
                    cx.emit(TimelineEvent::Failed("edit", err.to_string()))
                })
                .ok();
            }
        })
        .detach();
    }

    /// The box and its hint, in place of the message's text.
    fn edit_el(&self, unique_id: &str) -> Option<AnyElement> {
        let edit = self.edit.as_ref().filter(|e| e.message.unique_id == unique_id)?;
        let hint = "escape to cancel \u{2022} enter to save";
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .py(px(4.0))
                .child(edit.input.clone())
                .child(
                    div()
                        .text_size(px(0.75 * theme::REM))
                        .text_color(rgb(theme::text_faint()))
                        .child(hint)
                        .e2e("div", "edit-hint")
                        .text(hint),
                )
                .e2e("div", "message-edit")
                .into_any_element(),
        )
    }
}

impl TimelineView {
    /// Whether the in-place edit box has the caret (it takes Escape, to cancel).
    pub fn edit_focused(&self, window: &Window, cx: &App) -> bool {
        self.edit
            .as_ref()
            .is_some_and(|e| e.input.read(cx).is_focused(window))
    }
}
