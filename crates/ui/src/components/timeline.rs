use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Duration};

use super::toast::report;
use dioxus::prelude::*;
use morpheus_core::{
    MediaRef, Message, MessageId, MessageKind, OpenTimeline, OwnedEventId, OwnedRoomId, Reaction,
    ReplyPreview, Session, ThreadSummary, TimelineEntry, TimelineHandle, Vector,
};

use super::{
    composer::{ActiveTimeline, ComposerMode, Draft, Mode},
    context_menu::{MenuItem, copy_to_clipboard, open_menu},
    hotkeys::{Hotkey, HotkeyRequest},
    keys::{DeviceKeys, KeyBanner},
    lightbox::{LightboxImage, LightboxImages, LightboxIndex},
    members::open_sender_menu,
    message_dialogs::{MessageDialog, OpenMessageDialog, Pinned, PinnedRefresh},
    profile::open_profile_at,
    shell::{Rooms, Selected},
    thread::ActiveThread,
};
use crate::{
    attach::open_with_system,
    format::{calendar_date, clock_time, human_duration, human_size},
    media::{Avatar, use_thumbnail},
};

/// Messages from the same sender within this window collapse under one header.
const GROUP_WINDOW_MS: u64 = 5 * 60 * 1000;
/// Start loading older history when the view is this close to the top.
const PAGINATE_THRESHOLD_PX: f64 = 400.0;
/// How far from the newest message (px) before the jump button appears.
const JUMP_THRESHOLD_PX: f64 = 240.0;
const PAGE_SIZE: u16 = 40;
/// Quiet time after the last timeline update before checking whether the
/// column needs more history to fill it.
const AUTO_FILL_SETTLE_MS: u64 = 400;
/// Most automatic history pages per room open (rooms full of hidden events).
const AUTO_FILL_MAX_PAGES: u32 = 15;
/// Rows rendered at first; the window grows as the user scrolls up, and only
/// once it covers everything loaded do we ask the server for more.
const INITIAL_WINDOW: usize = 120;
const WINDOW_STEP: usize = 100;
/// Most rows in the page at once: far back in a long room, the newest ones
/// are dropped as older ones come in (and come back on the way down).
const MAX_ROWS: usize = 300;
/// Far back in a long room, how long after a scroll that seems to reach the
/// bottom to look again before bringing the newest rows back (WebKit reports
/// the view at the bottom for a moment while rows change).
const BOTTOM_RECHECK_MS: u64 = 120;
/// History pages to load looking for a message to jump to.
const JUMP_MAX_PAGES: usize = 30;
/// Rows kept rendered beyond a jumped-to message.
const JUMP_MARGIN: usize = 20;
/// How long a jumped-to message stays highlighted.
const JUMP_HIGHLIGHT_MS: u64 = 2500;
/// One-click reactions offered on hover.
pub(crate) const QUICK_REACTIONS: [&str; 3] = ["👍", "❤️", "😂"];
/// The "Add reaction" picker.
const EMOJI_PICKER: [&str; 24] = [
    "👍", "👎", "❤️", "😂", "😮", "😢", "😡", "🎉", "🔥", "👀", "🙏", "👏", "💯", "✅", "❌", "🤔",
    "😎", "🥳", "🚀", "⭐", "💀", "🫡", "🤝", "💡",
];

/// Rooms marked unread while open: no read receipts from this visit
/// (sending one clears the flag), until the room is opened again. Process-wide,
/// not thread-local: on Android, event handlers and renders run on different
/// threads.
static KEPT_UNREAD: std::sync::Mutex<std::collections::BTreeSet<OwnedRoomId>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());

fn kept_unread(room_id: &OwnedRoomId) -> bool {
    KEPT_UNREAD
        .lock()
        .is_ok_and(|rooms| rooms.contains(room_id))
}

fn keep_unread(room_id: OwnedRoomId, keep: bool) {
    if let Ok(mut rooms) = KEPT_UNREAD.lock() {
        if keep {
            rooms.insert(room_id);
        } else {
            rooms.remove(&room_id);
        }
    }
}

/// One room's messages. The parent sets `key` to the room id, so switching
/// rooms remounts this component and drops the previous subscription.
#[component]
pub fn TimelineView(
    room_id: OwnedRoomId,
    #[props(default)] root_event_id: Option<OwnedEventId>,
) -> Element {
    let session = use_context::<Session>();
    let mut entries = use_signal(Vector::<TimelineEntry>::new);
    let mut handle = use_context::<ActiveTimeline>();
    // A fresh visit reads the room again, even if it was marked unread here.
    use_hook({
        let room_id = room_id.clone();
        move || keep_unread(room_id.clone(), false)
    });
    let mut load_error = use_signal(|| None::<String>);
    let mut reached_start = use_signal(|| false);
    let mut paginating = use_signal(|| false);
    let mut gallery = use_context::<LightboxImages>();

    // Keep the lightbox's gallery in step with the images currently loaded
    // in this room's timeline, so the arrow keys can walk between them.
    let gallery_images = use_memo(move || {
        entries
            .read()
            .iter()
            .filter_map(|entry| match entry {
                TimelineEntry::Message(message) => match &message.kind {
                    MessageKind::Image {
                        body,
                        media,
                        width,
                        height,
                        ..
                    } => Some(LightboxImage {
                        media: media.clone(),
                        body: body.clone(),
                        width: *width,
                        height: *height,
                    }),
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>()
    });
    use_effect(move || gallery.set(gallery_images()));
    // The message to draw the "NEW" divider above, taken from the SDK's
    // read-marker item the first time it shows up (the first batch may land
    // before the SDK has placed it). Once captured it stays fixed for the
    // rest of this viewing session, so the divider does not chase the user
    // down the timeline.
    let mut new_marker = use_signal(|| None::<String>);
    let mut caught_up = use_signal(|| false);
    // The newest own, editable message, for "Up to edit" in the composer.
    let last_own = use_hook(try_consume_context::<LastOwnMessage>);
    // Escape with nothing open: back to the newest message, and mark read.
    let mut hotkey = use_context::<HotkeyRequest>();
    let is_main_timeline = root_event_id.is_none();
    let device_keys = try_use_context::<DeviceKeys>();
    // Plain (non-signal) handle to the open timeline. Leaving the room (it
    // remounts on room switch) fires a fully-read receipt on cleanup, without
    // touching reactive state while the component is being torn down.
    let handle_cell = use_hook(|| Rc::new(RefCell::new(None::<TimelineHandle>)));
    use_drop({
        let handle_cell = handle_cell.clone();
        let room_id = room_id.clone();
        move || {
            // Not for a room marked unread here: any receipt clears the flag.
            let kept_unread = kept_unread(&room_id);
            if let Some(active) = handle_cell.borrow_mut().take()
                && !kept_unread
            {
                // The core spawns the task before returning; the handle is not needed.
                drop(active.mark_as_fully_read());
            }
        }
    });

    use_future(move || {
        let session = session.clone();
        let room_id = room_id.clone();
        let root_event_id = root_event_id.clone();
        let handle_cell = handle_cell.clone();
        async move {
            let is_thread = root_event_id.is_some();
            let opened = match root_event_id {
                Some(root_event_id) => session.thread(room_id.clone(), root_event_id).await,
                None => session.timeline(room_id.clone()).await,
            };
            match opened {
                Ok(OpenTimeline {
                    handle: opened,
                    mut updates,
                }) => {
                    handle.set(Some(opened.clone()));
                    *handle_cell.borrow_mut() = Some(opened.clone());
                    // Where the user read up to, for when the SDK draws no marker (it
                    // only can when that event has a row of its own).
                    let read_at = match is_thread {
                        true => None,
                        false => session.fully_read_at(room_id.clone()).await.ok().flatten(),
                    };
                    while let Some(diffs) = updates.next().await {
                        entries
                            .with_mut(|list| diffs.into_iter().for_each(|diff| diff.apply(list)));
                        if let Some(mut last_own) = last_own {
                            let newest =
                                entries.read().iter().rev().find_map(|entry| match entry {
                                    TimelineEntry::Message(m) if m.is_own && m.can_edit => {
                                        Some((**m).clone())
                                    }
                                    _ => None,
                                });
                            if last_own.peek().0 != newest {
                                last_own.set(LastOwn(newest));
                            }
                        }

                        // Capture the divider position as soon as the SDK has
                        // placed its read marker, and never move it afterwards.
                        if new_marker.peek().is_none()
                            && !caught_up()
                            && let Some(marker) = first_unread_after_marker(&entries.read())
                                .or_else(|| {
                                    read_at.and_then(|at| first_unread_since(&entries.read(), at))
                                })
                        {
                            new_marker.set(Some(marker));
                        }

                        // Sending a message means the user is caught up: drop the
                        // divider and advance the fully-read marker for next time.
                        if !caught_up()
                            && entries
                                .read()
                                .iter()
                                .any(|entry| matches!(entry, TimelineEntry::Message(m) if m.is_own && m.is_local_echo))
                        {
                            caught_up.set(true);
                            new_marker.set(None);
                            drop(opened.mark_as_fully_read());
                        }

                        // The SDK only sends a receipt when the latest event moved
                        // forward. Thread timelines are skipped: the server rejects
                        // threaded receipts for events outside the thread. A room
                        // marked unread while open stays so (a receipt clears the
                        // flag) until the next visit.
                        if !is_thread && !kept_unread(&room_id) {
                            let receipt = opened.clone();
                            spawn(async move {
                                if let Err(err) = receipt.send_read_receipt().await {
                                    report("read receipt", &err);
                                }
                            });
                        }
                    }
                }
                Err(err) => {
                    report("opening timeline", &err);
                    load_error.set(Some(err.to_string()));
                }
            }
        }
    });

    let mut window = use_signal(|| INITIAL_WINDOW);
    // Newest rows left out of the page (only far back in a long room).
    let mut skip = use_signal(|| 0usize);
    // Bumped when scrolling makes rows come or go below the view: the page
    // then puts the view back where it was (`hotkeys.rs`; WebKit does not
    // anchor the scroll position itself). Not bumped by "Jump to present".
    let mut anchor = use_signal(|| 0u32);
    // A check that the view really is at the bottom is pending (see on_scroll).
    let mut bottom_check = use_signal(|| false);

    let mut load_older = move || {
        // Reveal already-loaded rows before fetching more history. Hidden
        // entries (profile changes) do not count: they never become rows.
        let shown = entries
            .read()
            .iter()
            .filter(|entry| !matches!(entry, TimelineEntry::Message(m) if m.kind.is_hidden()))
            .count();
        if skip() + window() < shown {
            let wider = window() + WINDOW_STEP;
            if wider > MAX_ROWS {
                // Older rows in at the top, as many newest ones out below.
                skip += wider - MAX_ROWS;
                window.set(MAX_ROWS);
                anchor += 1;
            } else {
                window.set(wider);
            }
            return;
        }
        let Some(active) = handle() else { return };
        if paginating() || reached_start() {
            return;
        }
        paginating.set(true);
        spawn(async move {
            match active.paginate_backwards(PAGE_SIZE).await {
                Ok(hit_start) => reached_start.set(hit_start),
                Err(err) => load_error.set(Some(err.to_string())),
            }
            paginating.set(false);
        });
    };

    // The scroll container, for pinning to the bottom and the jump button.
    let mut container = use_signal(|| None::<Rc<MountedData>>);
    // True once the user has scrolled away from the newest message.
    let mut scrolled_up = use_signal(|| false);

    // Jump to a message (a search result): reveal it, paging back through
    // history if it is not loaded yet, then scroll to it and highlight it.
    let jump = try_use_context::<super::shell::JumpTo>();
    use_effect(move || {
        let Some(mut jump) = jump else { return };
        let Some(target) = jump().0 else { return };
        if !is_main_timeline {
            return;
        }
        spawn(async move {
            for _ in 0..JUMP_MAX_PAGES {
                // Rows from the newest, as rendered (hidden entries skipped).
                let from_newest = entries
                    .peek()
                    .iter()
                    .rev()
                    .filter(|entry| !matches!(entry, TimelineEntry::Message(m) if m.kind.is_hidden()))
                    .position(|entry| matches!(entry, TimelineEntry::Message(m) if matches!(&m.id, MessageId::Event(id) if id.as_str() == target)));
                if let Some(from_newest) = from_newest {
                    let (first, last) = (*skip.peek(), *skip.peek() + *window.peek());
                    if from_newest < first || from_newest + JUMP_MARGIN >= last {
                        if from_newest + JUMP_MARGIN < MAX_ROWS {
                            skip.set(0);
                            window.set((from_newest + JUMP_MARGIN + 1).max(INITIAL_WINDOW));
                        } else {
                            // Far back: the page holds the rows around it.
                            skip.set(from_newest.saturating_sub(MAX_ROWS / 2));
                            window.set(MAX_ROWS);
                        }
                    }
                    break;
                }
                let Some(active) = handle.peek().clone() else {
                    break;
                };
                if *reached_start.peek() {
                    break;
                }
                match active.paginate_backwards(PAGE_SIZE).await {
                    Ok(hit_start) => reached_start.set(hit_start),
                    Err(err) => {
                        load_error.set(Some(err.to_string()));
                        break;
                    }
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            // Let the rows render, then bring the message into view.
            tokio::time::sleep(Duration::from_millis(80)).await;
            let id = serde_json::to_string(&target).unwrap_or_default();
            document::eval(&format!(
                "const m = document.querySelector('.timeline [data-event-id=' + JSON.stringify({id}) + ']'); \
                 if (m) {{ m.scrollIntoView({{ block: 'center' }}); m.classList.add('jump-target'); \
                 setTimeout(() => m.classList.remove('jump-target'), {JUMP_HIGHLIGHT_MS}); }}"
            ));
            jump.set(super::shell::JumpTarget(None));
        });
    });

    // `column-reverse` makes scrollTop run from 0 (bottom) to negative (top).
    let on_scroll = move |evt: Event<ScrollData>| {
        let distance_to_top =
            f64::from(evt.scroll_height()) - f64::from(evt.client_height()) + evt.scroll_top();
        if distance_to_top < PAGINATE_THRESHOLD_PX {
            load_older();
        }
        // Near the bottom with newer rows left out: bring them back (the
        // oldest go at the top), if the view is still there a moment later.
        // While rows change (older ones coming in, a message updating),
        // WebKit reports the view at the bottom for a moment; taken at its
        // word, that dropped the rows being read and jumped the view.
        if skip() > 0 && -evt.scroll_top() < PAGINATE_THRESHOLD_PX && !*bottom_check.peek() {
            bottom_check.set(true);
            spawn(async move {
                tokio::time::sleep(Duration::from_millis(BOTTOM_RECHECK_MS)).await;
                let offset = match container.peek().clone() {
                    Some(el) => el.get_scroll_offset().await.ok(),
                    None => None,
                };
                if let Some(offset) = offset
                    && *skip.peek() > 0
                    && -offset.y < PAGINATE_THRESHOLD_PX
                {
                    let fewer = skip.peek().saturating_sub(WINDOW_STEP);
                    skip.set(fewer);
                    anchor += 1;
                }
                bottom_check.set(false);
            });
        }
        let away = -evt.scroll_top() > JUMP_THRESHOLD_PX || skip() > 0;
        if away != scrolled_up() {
            scrolled_up.set(away);
        }
    };

    // Keep the newest message in view as content arrives unless the user has
    // scrolled up. `column-reverse` normally does this by itself, but WebKit
    // leaves the view wherever it was when rows are added to an empty
    // container, which showed older messages after switching rooms.
    let mut pin_to_bottom = move |behavior: ScrollBehavior| {
        // The newest rows back in the page first.
        if *skip.peek() > 0 {
            skip.set(0);
            window.set(INITIAL_WINDOW);
        }
        if let Some(el) = container() {
            spawn(async move {
                let _ = el
                    .scroll(
                        dioxus::html::geometry::PixelsVector2D::new(0.0, 0.0),
                        behavior,
                    )
                    .await;
            });
        }
    };
    use_effect(move || {
        let _ = entries.read().len();
        if !*scrolled_up.peek() {
            pin_to_bottom(ScrollBehavior::Instant);
        }
    });
    // Auto-fill: when what is shown does not fill the column (a short first
    // page, or one that is all hidden profile changes) there is nothing to
    // scroll, so fetch older history until it does or the room starts. Waits
    // for updates to settle first: paginating while the room subscription's
    // first response lands can reset the timeline. Capped per room open.
    let mut auto_pages = use_signal(|| 0u32);
    let mut settle = use_signal(|| 0u64);
    use_effect(move || {
        let _ = (entries.read().len(), paginating());
        let generation = *settle.peek() + 1;
        settle.set(generation);
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(AUTO_FILL_SETTLE_MS)).await;
            let at_start = *reached_start.peek()
                || entries
                    .peek()
                    .iter()
                    .any(|entry| matches!(entry, TimelineEntry::TimelineStart));
            if *settle.peek() != generation
                || *paginating.peek()
                || at_start
                || entries.peek().is_empty()
                || *auto_pages.peek() >= AUTO_FILL_MAX_PAGES
            {
                return;
            }
            let Some(el) = container.peek().clone() else {
                return;
            };
            let (Ok(scroll), Ok(rect)) = (el.get_scroll_size().await, el.get_client_rect().await)
            else {
                return;
            };
            if scroll.height <= rect.height() + PAGINATE_THRESHOLD_PX {
                auto_pages += 1;
                load_older();
            }
        });
    });
    use_effect(move || {
        if hotkey() == Some(Hotkey::JumpToBottom) && is_main_timeline {
            hotkey.set(None);
            // Instant: a smooth scroll started right after another one (Esc
            // twice) can be dropped by WebKit, leaving the view where it was.
            pin_to_bottom(ScrollBehavior::Instant);
            if let Some(active) = handle.peek().clone() {
                drop(active.mark_as_fully_read());
                caught_up.set(true);
                new_marker.set(None);
            }
        }
    });

    let at_start = reached_start()
        || entries
            .read()
            .iter()
            .any(|entry| matches!(entry, TimelineEntry::TimelineStart));
    let entries_now = entries.read();
    let marker = new_marker();
    // Messages deleted a moment ago, still undoable, are left out.
    let hidden = try_use_context::<super::toast::Hidden>();
    let shown: Option<Vector<TimelineEntry>> = hidden
        .filter(|hidden| !hidden.read().0.is_empty())
        .map(|hidden| {
            let hidden = hidden.read();
            entries_now
                .iter()
                .filter(|entry| {
                    !matches!(entry, TimelineEntry::Message(m) if hidden.0.contains(&message_key(&m.id)))
                })
                .cloned()
                .collect()
        });
    let rows = group_rows(shown.as_ref().unwrap_or(&entries_now), marker.as_deref());
    // Nothing from the SDK yet (not even the start marker): show placeholders
    // instead of an empty column.
    let loading = entries_now.is_empty() && load_error().is_none();
    // Messages this device cannot read: say how to unlock them (main chat only).
    let show_key_banner = is_main_timeline
        && device_keys.is_some_and(|keys| keys().locked)
        && entries_now.iter().any(|entry| {
            matches!(entry, TimelineEntry::Message(m) if matches!(m.kind, MessageKind::UnableToDecrypt))
        });

    rsx! {
        if show_key_banner {
            KeyBanner {}
        }
        // Rendered newest-first; CSS flips it and keeps the view pinned to the bottom.
        section {
            class: "timeline",
            // Which rows are in the page, for tests: newest left out, how many shown.
            "data-skip": "{skip}",
            "data-window": "{window}",
            "data-anchor": "{anchor}",
            onscroll: on_scroll,
            onmounted: move |e| container.set(Some(e.data())),
            if loading {
                SkeletonRows {}
            }
            for row in rows.into_iter().rev().skip(skip()).take(window()) {
                match row {
                    Row::Message { message, show_header } => rsx! {
                        MessageRow { key: "{message.unique_id}", message: message.clone(), show_header }
                    },
                    Row::Date(label, at) => rsx! {
                        div { key: "date-{at}", class: "date-divider", span { "{label}" } }
                    },
                    Row::Start => rsx! {
                        div { class: "timeline-start", "This is the beginning of the room." }
                    },
                    Row::New => {
                        let key = "new-divider";
                        rsx! {
                            div { key: "{key}", class: "new-divider", span { "NEW" } }
                        }
                    }
                }
            }

            if let Some(message) = load_error() {
                p { class: "error", "{message}" }
            }
            if paginating() {
                p { class: "muted loading", "Loading older messages…" }
            } else if !at_start && handle().is_some() {
                button { class: "ghost load-older", onclick: move |_| load_older(), "Load older messages" }
            }
        }
        if scrolled_up() {
            button {
                class: "jump-to-bottom",
                title: "Jump to the newest message",
                onclick: move |_| pin_to_bottom(ScrollBehavior::Instant),
                "Jump to present ↓"
            }
        }
    }
}

/// Grey placeholder rows shown while a timeline has nothing to show yet.
#[component]
fn SkeletonRows() -> Element {
    // Widths vary so the column reads as messages, not a grid.
    const LINES: [(u8, u8); 6] = [(45, 70), (30, 0), (60, 40), (25, 55), (50, 0), (35, 65)];
    rsx! {
        for (index, (first, second)) in LINES.iter().enumerate() {
            div { key: "{index}", class: "message with-header skeleton-row", "aria-hidden": "true",
                div { class: "avatar skeleton" }
                div { class: "message-main",
                    div { class: "skeleton skeleton-line", style: "width: {first}%" }
                    if *second > 0 {
                        div { class: "skeleton skeleton-line", style: "width: {second}%" }
                    }
                }
            }
        }
    }
}

enum Row<'a> {
    Message {
        message: &'a Message,
        show_header: bool,
    },
    /// The label, and the timestamp that keys it (a label can repeat).
    Date(String, u64),
    Start,
    /// The Discord-style "new messages" divider.
    New,
}

/// Decide which messages get a sender header and turn dividers into labels.
/// `marker` is the `unique_id` of the message the "NEW" divider belongs above,
/// if any is still showing.
fn group_rows<'a>(entries: &'a Vector<TimelineEntry>, marker: Option<&str>) -> Vec<Row<'a>> {
    let mut rows = Vec::with_capacity(entries.len());
    let mut previous: Option<&Message> = None;
    // A divider meant for a hidden row goes above the next visible one.
    let mut marker_pending = false;

    for entry in entries {
        match entry {
            // Name and avatar changes stay out of the chat, as in Discord, and
            // so do calls' joins, leaves and keys (the sidebar shows calls).
            TimelineEntry::Message(message) if message.kind.is_hidden() => {
                marker_pending |= marker == Some(message.unique_id.as_str());
            }
            TimelineEntry::Message(message) => {
                if std::mem::take(&mut marker_pending) || marker == Some(message.unique_id.as_str())
                {
                    rows.push(Row::New);
                    // Always show a header right after the divider, like Discord does.
                    previous = None;
                }
                let continues = previous.is_some_and(|prev| {
                    prev.sender == message.sender
                        && message.timestamp_ms.saturating_sub(prev.timestamp_ms) < GROUP_WINDOW_MS
                        && !matches!(prev.kind, MessageKind::State(_))
                        && !matches!(message.kind, MessageKind::State(_))
                });
                rows.push(Row::Message {
                    message,
                    show_header: !continues,
                });
                previous = Some(message);
            }
            TimelineEntry::DateDivider { timestamp_ms } => {
                rows.push(Row::Date(calendar_date(*timestamp_ms), *timestamp_ms));
                previous = None;
            }
            TimelineEntry::TimelineStart => {
                rows.push(Row::Start);
                previous = None;
            }
            TimelineEntry::ReadMarker => {}
        }
    }
    rows
}

/// The `unique_id` of the first message after the SDK's read-marker virtual
/// item, i.e. the first message that was still unread when the room opened.
fn first_unread_after_marker(entries: &Vector<TimelineEntry>) -> Option<String> {
    let marker_idx = entries
        .iter()
        .position(|entry| matches!(entry, TimelineEntry::ReadMarker))?;
    entries
        .iter()
        .skip(marker_idx + 1)
        .find_map(|entry| match entry {
            TimelineEntry::Message(message) => Some(message.unique_id.clone()),
            _ => None,
        })
}

/// The first message from someone else sent after `read_at` (ms): where the
/// divider goes when the read marker sits on an event without a row.
fn first_unread_since(entries: &Vector<TimelineEntry>, read_at: u64) -> Option<String> {
    entries.iter().find_map(|entry| match entry {
        TimelineEntry::Message(message) if !message.is_own && message.timestamp_ms > read_at => {
            Some(message.unique_id.clone())
        }
        _ => None,
    })
}

#[component]
pub(crate) fn MessageRow(message: Message, show_header: bool) -> Element {
    let timeline = use_context::<ActiveTimeline>();
    let mut mode = use_context::<Mode>();
    let mut draft = use_context::<Draft>();
    let mut active_thread = use_context::<ActiveThread>();
    let time = clock_time(message.timestamp_ms);
    // Your nickname for the sender, if you gave them one.
    let shown_sender = try_use_context::<super::nicknames::Nicknames>()
        .map(|map| {
            super::nicknames::shown_name(&map(), message.sender.as_str(), &message.sender_name)
        })
        .unwrap_or_else(|| message.sender_name.clone());

    let open_thread = {
        let message = message.clone();
        move |_| active_thread.set(Some(message.clone()))
    };
    let open_thread_chip = {
        let message = message.clone();
        move |()| active_thread.set(Some(message.clone()))
    };

    let start_reply = {
        let message = message.clone();
        move |_| {
            if let MessageId::Event(event_id) = &message.id {
                mode.set(Some(ComposerMode::Reply {
                    event_id: event_id.clone(),
                    sender_name: message.sender_name.clone(),
                    summary: message.kind.summary(),
                }));
            }
        }
    };
    let start_edit = {
        let message = message.clone();
        move |_| {
            let original = message.kind.plain_body().unwrap_or_default().to_owned();
            draft.set(original.clone());
            mode.set(Some(ComposerMode::Edit {
                id: message.id.clone(),
                original,
            }));
        }
    };
    let delete = {
        let id = message.id.clone();
        move |_| {
            let Some(active) = timeline() else { return };
            delete_with_undo(active, id.clone());
        }
    };
    let can_reply = matches!(message.id, MessageId::Event(_)) && !message.is_local_echo;

    if let MessageKind::State(text) = &message.kind {
        return rsx! {
            div { class: "message system",
                span { class: "time", "{time}" }
                span { class: "system-text", "{text}" }
            }
        };
    }

    let react = {
        let id = message.id.clone();
        move |key: &str| {
            let Some(active) = timeline() else { return };
            let (id, key) = (id.clone(), key.to_owned());
            spawn(async move {
                if let Err(err) = active.toggle_reaction(id, key).await {
                    report("reaction", &err);
                }
            });
        }
    };

    // Right-click: the same actions as the hover bar, plus copy.
    let on_context = {
        let message = message.clone();
        let react = react.clone();
        let selected = use_context::<Selected>();
        move |e: Event<MouseData>| {
            let mut items = Vec::new();
            if can_reply {
                items.push(MenuItem::new("Reply", {
                    let message = message.clone();
                    move || {
                        let mut mode = mode;
                        if let MessageId::Event(event_id) = &message.id {
                            mode.set(Some(ComposerMode::Reply {
                                event_id: event_id.clone(),
                                sender_name: message.sender_name.clone(),
                                summary: message.kind.summary(),
                            }));
                        }
                    }
                }));
                items.push(MenuItem::new("Reply in thread", {
                    let message = message.clone();
                    move || {
                        let mut active_thread = active_thread;
                        active_thread.set(Some(message.clone()));
                    }
                }));
            }
            if message.can_edit {
                items.push(MenuItem::new("Edit message", {
                    let message = message.clone();
                    move || {
                        let (mut draft, mut mode) = (draft, mode);
                        let original = message.kind.plain_body().unwrap_or_default().to_owned();
                        draft.set(original.clone());
                        mode.set(Some(ComposerMode::Edit {
                            id: message.id.clone(),
                            original,
                        }));
                    }
                }));
            }
            // Everything below needs a sent event in a known room.
            let sent = match (&message.id, selected.peek().as_ref()) {
                (MessageId::Event(event_id), Some(room_id)) if !message.is_local_echo => {
                    Some((event_id.clone(), room_id.clone()))
                }
                _ => None,
            };
            if sent.is_some() {
                // A picker of common emoji; the quick row above has the top three.
                let grid = EMOJI_PICKER
                    .iter()
                    .map(|key| {
                        MenuItem::new(*key, {
                            let react = react.clone();
                            move || react(key)
                        })
                    })
                    .collect();
                items.push(MenuItem::grid("Add reaction", grid));
                if !message.reactions.is_empty() {
                    items.push(MenuItem::new("View reactions", {
                        let message = message.clone();
                        move || {
                            consume_context::<OpenMessageDialog>()
                                .set(MessageDialog::Reactions(Box::new(message.clone())));
                        }
                    }));
                }
            }
            if let Some((event_id, room_id)) = &sent {
                let session = consume_context::<Session>();
                // Forward to any other joined room.
                let targets: Vec<MenuItem> = consume_context::<Rooms>()
                    .read()
                    .iter()
                    .filter(|room| !room.is_space && &room.id != room_id)
                    .map(|room| {
                        MenuItem::new(
                            if room.is_dm {
                                format!("@{}", room.name)
                            } else {
                                format!("#{}", room.name)
                            },
                            {
                                let session = session.clone();
                                let (from, event_id, to) =
                                    (room_id.clone(), event_id.clone(), room.id.clone());
                                move || {
                                    let future =
                                        session.forward(from.clone(), event_id.clone(), to.clone());
                                    spawn(async move {
                                        if let Err(err) = future.await {
                                            report("forward", &err);
                                        }
                                    });
                                }
                            },
                        )
                        .icon(if room.is_dm {
                            crate::icons::AT_SIGN
                        } else {
                            crate::icons::HASH
                        })
                    })
                    .collect();
                if !targets.is_empty() {
                    items.push(MenuItem::submenu("Forward to", targets));
                }

                let pinned = consume_context::<Pinned>().peek().0.contains(event_id);
                items.push(MenuItem::new(
                    if pinned {
                        "Unpin message"
                    } else {
                        "Pin message"
                    },
                    {
                        let session = session.clone();
                        let (room_id, event_id) = (room_id.clone(), event_id.clone());
                        move || {
                            let future = session.pin(room_id.clone(), event_id.clone(), !pinned);
                            let mut refresh = consume_context::<PinnedRefresh>();
                            spawn(async move {
                                if let Err(err) = future.await {
                                    report("pin", &err);
                                }
                                refresh += 1;
                            });
                        }
                    },
                ));
                items.push(MenuItem::new("Mark unread", {
                    let session = session.clone();
                    let room_id = room_id.clone();
                    move || {
                        keep_unread(room_id.clone(), true);
                        let future = session.mark_unread(room_id.clone(), true);
                        spawn(async move {
                            if let Err(err) = future.await {
                                report("mark unread", &err);
                            }
                        });
                    }
                }));
            }
            if let Some(text) = message.kind.plain_body().filter(|t| !t.is_empty()) {
                let text = text.to_owned();
                // Holding opens this menu on a phone, so selecting needs a way in.
                if cfg!(any(target_os = "android", target_os = "ios")) {
                    let text = text.clone();
                    items.push(MenuItem::new("Select text", move || {
                        consume_context::<OpenMessageDialog>()
                            .set(MessageDialog::SelectText(text.clone()));
                    }));
                }
                items.push(MenuItem::new("Copy text", move || copy_to_clipboard(&text)));
            }
            if let Some((event_id, room_id)) = &sent {
                let link = format!("https://matrix.to/#/{room_id}/{event_id}");
                items.push(MenuItem::new("Copy message link", move || {
                    copy_to_clipboard(&link)
                }));
            }
            // Only our own reactions can be taken back.
            let mine: Vec<&str> = message
                .reactions
                .iter()
                .filter(|r| r.mine)
                .map(|r| r.key.as_str())
                .collect();
            if !mine.is_empty() {
                let each: Vec<MenuItem> = mine
                    .iter()
                    .map(|key| {
                        MenuItem::new(*key, {
                            let (react, key) = (react.clone(), (*key).to_owned());
                            move || react(&key)
                        })
                    })
                    .collect();
                items.push(MenuItem::submenu("Remove reactions", each));
                items.push(MenuItem::new("Remove all my reactions", {
                    let react = react.clone();
                    let keys: Vec<String> = mine.iter().map(|k| (*k).to_owned()).collect();
                    move || keys.iter().for_each(|key| react(key))
                }));
            }
            if let Some((event_id, _)) = &sent
                && !message.is_own
            {
                let event_id = event_id.clone();
                items.push(MenuItem::new("Report message", move || {
                    consume_context::<OpenMessageDialog>()
                        .set(MessageDialog::Report(event_id.clone()));
                }));
            }
            if message.is_own && can_reply {
                items.push(
                    MenuItem::new("Delete message", {
                        let id = message.id.clone();
                        move || {
                            let Some(active) = timeline() else { return };
                            delete_with_undo(active, id.clone());
                        }
                    })
                    .danger(),
                );
            }
            if let Some((event_id, _)) = &sent {
                let id = event_id.to_string();
                items.push(MenuItem::new("Copy message ID", move || {
                    copy_to_clipboard(&id)
                }));
            }
            let quick: Rc<dyn Fn(&'static str)> = Rc::new({
                let react = react.clone();
                move |key| react(key)
            });
            open_menu(&e, items, (!message.is_local_echo).then_some(quick));
        }
    };

    // Right-click on who sent it: the member menu, not the message menu.
    let sender_menu = {
        let session = use_context::<Session>();
        let selected = use_context::<Selected>();
        let (sender, name) = (message.sender.clone(), message.sender_name.clone());
        move |e: Event<MouseData>| {
            let Some(room_id) = selected.peek().clone() else {
                return;
            };
            open_sender_menu(&session, room_id, sender.clone(), name.clone(), &e);
        }
    };

    // Someone you ignore: folded away, shown on request.
    let ignored = try_use_context::<super::marks::Marks>()
        .is_some_and(|marks| marks.ignored.read().contains_key(message.sender.as_str()));
    let mut revealed = use_signal(|| false);
    if ignored && !revealed() {
        return rsx! {
            div {
                class: "message ignored",
                "data-event-id": match &message.id {
                    MessageId::Event(id) => id.to_string(),
                    MessageId::Transaction(_) => String::new(),
                },
                button {
                    class: "ignored-fold",
                    onclick: move |_| revealed.set(true),
                    "Ignored message · Show"
                }
            }
        };
    }

    rsx! {
        div {
            class: "message",
            "data-event-id": match &message.id {
                MessageId::Event(id) => id.to_string(),
                MessageId::Transaction(_) => String::new(),
            },
            oncontextmenu: on_context,
            class: if show_header { "with-header" },
            class: if message.is_local_echo { "pending" },
            class: if message.is_own { "own" },
            if show_header {
                button {
                    class: "avatar-button",
                    title: "{message.sender}",
                    oncontextmenu: sender_menu.clone(),
                    onclick: {
                        let sender = message.sender.clone();
                        move |e| open_profile_at(sender.clone(), &e)
                    },
                    Avatar { media: message.sender_avatar.clone(), fallback: message.sender_name.clone(), size: 40 }
                }
                div { class: "message-main",
                    div { class: "message-meta",
                        button {
                            class: "sender",
                            title: "{message.sender}",
                            oncontextmenu: sender_menu.clone(),
                            onclick: {
                                let sender = message.sender.clone();
                                move |e| open_profile_at(sender.clone(), &e)
                            },
                            "{shown_sender}"
                        }
                        span { class: "time", "{time}" }
                        MsgShield { shield: message.shield.clone() }
                    }
                    ReplyQuote { reply: message.in_reply_to.clone() }
                    MessageBody { kind: message.kind.clone(), edited: message.is_edited }
                    Reactions { reactions: message.reactions.clone(), on_toggle: react.clone() }
                    ThreadChip { thread: message.thread.clone(), on_open: open_thread_chip.clone() }
                }
            } else {
                span { class: "time gutter", "{time}" }
                div { class: "message-main",
                    MsgShield { shield: message.shield.clone() }
                    ReplyQuote { reply: message.in_reply_to.clone() }
                    MessageBody { kind: message.kind.clone(), edited: message.is_edited }
                    Reactions { reactions: message.reactions.clone(), on_toggle: react.clone() }
                    ThreadChip { thread: message.thread.clone(), on_open: open_thread_chip.clone() }
                }
            }
            if !message.is_local_echo {
                div { class: "actions",
                    for key in QUICK_REACTIONS {
                        button {
                            class: "action",
                            title: "React with {key}",
                            onclick: {
                                let react = react.clone();
                                move |_| react(key)
                            },
                            "{key}"
                        }
                    }
                    if can_reply {
                        button { class: "action reply", title: "Reply", onclick: start_reply, crate::icons::Icon { glyph: crate::icons::REPLY } }
                        button { class: "action thread", title: "Reply in thread", onclick: open_thread, crate::icons::Icon { glyph: crate::icons::MESSAGES_SQUARE } }
                    }
                    if message.can_edit {
                        button { class: "action edit", title: "Edit", onclick: start_edit, crate::icons::Icon { glyph: crate::icons::PENCIL } }
                    }
                    if message.is_own && can_reply {
                        button { class: "action delete", title: "Delete", onclick: delete, crate::icons::Icon { glyph: crate::icons::TRASH_2 } }
                    }
                }
            }
        }
    }
}

#[component]
fn Reactions(reactions: Vec<Reaction>, on_toggle: Callback<&'static str>) -> Element {
    if reactions.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { class: "reactions",
            for reaction in reactions {
                button {
                    key: "{reaction.key}",
                    class: "reaction",
                    class: if reaction.mine { "mine" },
                    onclick: {
                        // Keys outlive the click handler; leaking the few distinct emoji is fine.
                        let key: &'static str = Box::leak(reaction.key.clone().into_boxed_str());
                        move |_| on_toggle(key)
                    },
                    span { class: "reaction-key", "{reaction.key}" }
                    span { class: "reaction-count", "{reaction.count}" }
                }
            }
        }
    }
}

/// Chip under a thread root summarising its replies; opens the thread panel.
#[component]
fn ThreadChip(thread: Option<ThreadSummary>, on_open: Callback<()>) -> Element {
    let Some(thread) = thread else { return rsx! {} };
    let count_label = if thread.reply_count == 1 {
        "1 reply".to_owned()
    } else {
        format!("{} replies", thread.reply_count)
    };
    rsx! {
        button { class: "thread-chip", onclick: move |_| on_open(()),
            span { class: "thread-chip-icon", crate::icons::Icon { glyph: crate::icons::MESSAGES_SQUARE, size: 14 } }
            span { class: "thread-chip-count", "{count_label}" }
            if let Some(name) = &thread.latest_sender_name {
                span { class: "thread-chip-latest", "· last from {name}" }
            }
        }
    }
}

#[component]
fn MessageBody(kind: MessageKind, edited: bool) -> Element {
    let edited_tag = if edited { " (edited)" } else { "" };
    let locked = try_use_context::<DeviceKeys>().is_some_and(|keys| keys().locked);
    match kind {
        MessageKind::Text { body, html } => {
            rsx! { RichText { class: "body", body, html, edited_tag } }
        }
        MessageKind::Notice { body, html } => {
            rsx! { RichText { class: "body notice", body, html, edited_tag } }
        }
        MessageKind::Emote { body, html } => {
            rsx! { RichText { class: "body emote", body, html, edited_tag } }
        }
        MessageKind::Image {
            body,
            media,
            width,
            height,
            blurhash,
        } => rsx! { ImageBody { body, media, width, height, blurhash } },
        MessageKind::File {
            body,
            media,
            size,
            mime,
        } => rsx! { FileBody { body, media, size, mime } },
        MessageKind::Video {
            body,
            media,
            thumbnail,
            width,
            height,
            duration,
        } => rsx! { VideoBody { body, media, thumbnail, width, height, duration } },
        MessageKind::Audio {
            body,
            media,
            size,
            mime,
            duration,
        } => rsx! { AudioBody { body, media, size, mime, duration } },
        MessageKind::Redacted => rsx! { p { class: "body muted", "Message deleted" } },
        // On a device that can read encrypted history, keys usually arrive
        // shortly (backup, other device): a loading shimmer. On one that
        // cannot (not verified, no recovery key) they never will until the
        // user acts, so say so instead of looking like it is loading.
        MessageKind::UnableToDecrypt if locked => rsx! {
            p {
                class: "body muted undecrypted locked",
                title: "Verify this device or enter your recovery key to read it",
                crate::icons::Icon { glyph: crate::icons::LOCK, size: 14 }
                " Encrypted message — this device has no key for it yet"
            }
        },
        MessageKind::UnableToDecrypt => rsx! {
            div {
                class: "body skeleton skeleton-line undecrypted",
                title: "Waiting for the keys to decrypt this message",
                "aria-label": "Encrypted message, waiting for keys",
            }
        },
        MessageKind::State(text) => rsx! { p { class: "body muted", "{text}" } },
        // Never rendered: `group_rows` drops them.
        MessageKind::ProfileChange | MessageKind::CallPlumbing => rsx! {},
        MessageKind::Unsupported(what) => rsx! { p { class: "body muted", "Unsupported: {what}" } },
    }
}

/// Sanitised HTML when the sender provided it, plain text otherwise.
#[component]
fn RichText(
    class: &'static str,
    body: String,
    html: Option<String>,
    edited_tag: &'static str,
) -> Element {
    match html {
        Some(html) => rsx! {
            div { class: "{class} rich",
                div { dangerous_inner_html: "{html}" }
                span { class: "edited", "{edited_tag}" }
            }
        },
        // Bare URLs become links; the shell hands clicks to the browser.
        None => rsx! {
            p { class: "{class}",
                for (index, segment) in crate::linkify::segments(&body).into_iter().enumerate() {
                    match segment {
                        crate::linkify::Segment::Text(text) => rsx! { span { key: "{index}", "{text}" } },
                        crate::linkify::Segment::Link { text, url } => rsx! {
                            a { key: "{index}", class: "link", href: "{url}", rel: "noopener", "{text}" }
                        },
                    }
                }
                span { class: "edited", "{edited_tag}" }
            }
        },
    }
}

#[component]
fn ImageBody(
    body: String,
    media: MediaRef,
    width: Option<u32>,
    height: Option<u32>,
    blurhash: Option<String>,
) -> Element {
    let mut lightbox_index = use_context::<LightboxIndex>();
    let gallery = use_context::<LightboxImages>();
    let url = use_thumbnail(Some(media.clone()), 800, 600);
    // A blurred preview behind the picture until it has loaded (and in its
    // place before then), when the sender's app gave one.
    let blur = use_memo(use_reactive!(|blurhash| blurhash
        .as_deref()
        .and_then(morpheus_core::blur_placeholder)));
    let aspect = match (width, height) {
        (Some(w), Some(h)) if h > 0 => format!("aspect-ratio: {w} / {h};"),
        _ => String::new(),
    };
    let aspect = match blur() {
        Some(blur) => format!("{aspect} background-image: url(\"{blur}\");"),
        None => aspect,
    };

    let open = move |_| {
        let key = media.key().to_owned();
        let index = gallery.read().iter().position(|img| img.media.key() == key);
        lightbox_index.set(index);
    };

    rsx! {
        match url() {
            Some(src) => rsx! {
                img {
                    class: "image",
                    style: "{aspect}",
                    src: "{src}",
                    alt: "{body}",
                    title: "{body}",
                    onclick: open,
                }
            },
            None => rsx! { div { class: "image placeholder", style: "{aspect}", "{body}" } },
        }
    }
}

/// What a download/upload-less attachment action is doing right now.
#[derive(Clone, PartialEq)]
enum DownloadStatus {
    Idle,
    Downloading,
    Saved(PathBuf),
    Failed(String),
}

/// "Download" button shown on file and audio cards. Downloads into the
/// user's Downloads directory and reports where it landed.
#[component]
fn DownloadButton(media: MediaRef, suggested_name: String) -> Element {
    let session = use_context::<Session>();
    let mut status = use_signal(|| DownloadStatus::Idle);

    let start = move |_| {
        let session = session.clone();
        let media = media.clone();
        let suggested_name = suggested_name.clone();
        status.set(DownloadStatus::Downloading);
        spawn(async move {
            status.set(match session.download(media, suggested_name).await {
                Ok(path) => DownloadStatus::Saved(path),
                Err(err) => DownloadStatus::Failed(err.to_string()),
            });
        });
    };

    match status() {
        DownloadStatus::Idle => rsx! {
            button { class: "ghost attachment-download", onclick: start, "Download" }
        },
        DownloadStatus::Downloading => rsx! {
            span { class: "attachment-status muted", "Downloading…" }
        },
        DownloadStatus::Saved(path) => rsx! {
            span { class: "attachment-status", title: "{path.display()}", "Saved to {path.display()}" }
        },
        DownloadStatus::Failed(message) => rsx! {
            span { class: "attachment-status error", "{message}" }
        },
    }
}

/// File attachment: icon, name, human-readable size, and a download action.
#[component]
fn FileBody(body: String, media: MediaRef, size: Option<u64>, mime: Option<String>) -> Element {
    let _ = mime;
    let size_label = size.map(human_size);

    rsx! {
        div { class: "attachment-card",
            span { class: "attachment-icon", "📎" }
            div { class: "attachment-info",
                span { class: "attachment-name", title: "{body}", "{body}" }
                if let Some(size_label) = size_label {
                    span { class: "attachment-size muted", "{size_label}" }
                }
            }
            DownloadButton { media, suggested_name: body.clone() }
        }
    }
}

/// Audio attachment: a simple row with duration/size and a download action.
#[component]
fn AudioBody(
    body: String,
    media: MediaRef,
    size: Option<u64>,
    mime: Option<String>,
    duration: Option<Duration>,
) -> Element {
    let _ = mime;
    let meta = [duration.map(human_duration), size.map(human_size)]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");

    rsx! {
        div { class: "attachment-card audio",
            span { class: "attachment-icon", "🎵" }
            div { class: "attachment-info",
                span { class: "attachment-name", title: "{body}", "{body}" }
                if !meta.is_empty() {
                    span { class: "attachment-size muted", "{meta}" }
                }
            }
            DownloadButton { media, suggested_name: body.clone() }
        }
    }
}

/// "Play" button shown on video cards: downloads the file (if needed) and
/// hands it to the OS's default player via its registered file handler.
#[component]
fn PlayButton(media: MediaRef, suggested_name: String) -> Element {
    let session = use_context::<Session>();
    let mut status = use_signal(|| DownloadStatus::Idle);
    let working = matches!(status(), DownloadStatus::Downloading);

    let start = move |_| {
        let session = session.clone();
        let media = media.clone();
        let suggested_name = suggested_name.clone();
        status.set(DownloadStatus::Downloading);
        spawn(async move {
            let outcome = match session.download(media, suggested_name).await {
                Ok(path) => match open_with_system(&path) {
                    Ok(()) => DownloadStatus::Saved(path),
                    Err(err) => DownloadStatus::Failed(err),
                },
                Err(err) => DownloadStatus::Failed(err.to_string()),
            };
            status.set(outcome);
        });
    };

    rsx! {
        button {
            class: "play-button",
            title: "Play",
            disabled: working,
            onclick: start,
            if working { "…" } else { "▶" }
        }
        if let DownloadStatus::Failed(message) = status() {
            span { class: "attachment-status error", "{message}" }
        }
    }
}

/// Video attachment: its thumbnail with a play button overlaid, and the
/// clip's duration.
#[component]
fn VideoBody(
    body: String,
    media: MediaRef,
    thumbnail: Option<MediaRef>,
    width: Option<u32>,
    height: Option<u32>,
    duration: Option<Duration>,
) -> Element {
    let thumb_url = use_thumbnail(thumbnail.or_else(|| Some(media.clone())), 480, 320);
    let aspect = match (width, height) {
        (Some(w), Some(h)) if h > 0 => format!("aspect-ratio: {w} / {h};"),
        _ => String::new(),
    };
    let duration_label = duration.map(human_duration);

    rsx! {
        div { class: "video-card", style: "{aspect}",
            match thumb_url() {
                Some(src) => rsx! { img { class: "video-thumb", src: "{src}", alt: "{body}", title: "{body}" } },
                None => rsx! { div { class: "image placeholder", "{body}" } },
            }
            div { class: "video-overlay",
                PlayButton { media, suggested_name: body.clone() }
                if let Some(duration_label) = duration_label {
                    span { class: "video-duration", "{duration_label}" }
                }
            }
        }
    }
}

#[component]
fn ReplyQuote(reply: Option<ReplyPreview>) -> Element {
    let Some(reply) = reply else { return rsx! {} };
    rsx! {
        div { class: "reply-quote", title: "{reply.event_id}",
            span { "↩" }
            b { "{reply.sender_name.as_deref().unwrap_or(\"…\")}" }
            match reply.summary.as_deref() {
                Some(summary) => rsx! { span { "{summary}" } },
                // The message replied to is still on its way.
                None => rsx! { span { class: "skeleton skeleton-inline", "aria-hidden": "true" } },
            }
        }
    }
}

/// The newest message of ours that can still be edited, kept by the room's
/// timeline for the composer's "Up to edit". Provided per room by the shell.
/// A newtype: Dioxus context is keyed by type, and `Signal<Option<Message>>`
/// is already `ActiveThread` (sharing it made the thread chip set this).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LastOwn(pub Option<Message>);
pub type LastOwnMessage = Signal<LastOwn>;

/// A shield on a message whose sender couldn't be confirmed (encrypted rooms):
/// red for real problems, grey otherwise; the tooltip says why.
#[component]
fn MsgShield(shield: Option<morpheus_core::Shield>) -> Element {
    let Some(shield) = shield else {
        return rsx! {};
    };
    rsx! {
        span {
            class: "msg-shield",
            class: if shield.red { "red" } else { "grey" },
            title: "{shield.reason}",
            "aria-label": "{shield.reason}",
            crate::icons::Icon { glyph: crate::icons::SHIELD, size: 14 }
        }
    }
}

/// A message's key in [`super::toast::Hidden`].
fn message_key(id: &MessageId) -> String {
    match id {
        MessageId::Event(id) => id.to_string(),
        MessageId::Transaction(id) => id.to_string(),
    }
}

/// Delete a message, undoably: it goes now, for good once the toast has.
fn delete_with_undo(active: morpheus_core::TimelineHandle, id: MessageId) {
    super::toast::hide_with_undo(message_key(&id), "Message deleted", "delete", move || {
        active.redact(id)
    });
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn message(id: &str, kind: MessageKind) -> TimelineEntry {
        TimelineEntry::Message(Box::new(Message {
            unique_id: id.to_owned(),
            id: MessageId::Event(OwnedEventId::try_from(format!("${id}")).expect("event id")),
            sender: morpheus_core::OwnedUserId::try_from("@bob:localhost").expect("user id"),
            sender_name: "bob".into(),
            sender_avatar: None,
            timestamp_ms: 0,
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

    #[test]
    fn profile_changes_are_hidden_and_the_new_divider_moves_past_them() {
        let entries: Vector<TimelineEntry> = [
            message("a", MessageKind::State("bob joined".into())),
            message("b", MessageKind::ProfileChange),
            message("c", MessageKind::ProfileChange),
            message("d", MessageKind::State("bob left".into())),
        ]
        .into_iter()
        .collect();
        let rows = group_rows(&entries, Some("b"));
        let shape: Vec<&str> = rows
            .iter()
            .map(|row| match row {
                Row::Message { message, .. } => message.unique_id.as_str(),
                Row::New => "NEW",
                Row::Date(..) => "date",
                Row::Start => "start",
            })
            .collect();
        assert_eq!(shape, ["a", "NEW", "d"]);
    }
}
