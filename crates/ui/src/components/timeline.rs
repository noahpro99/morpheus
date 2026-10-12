use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Duration};

use dioxus::prelude::*;
use morpheus_core::{
    MediaRef, Message, MessageId, MessageKind, OpenTimeline, OwnedEventId, OwnedRoomId, Reaction,
    ReplyPreview, Session, ThreadSummary, TimelineEntry, TimelineHandle, Vector,
};
use tracing::{error, warn};

use super::{
    composer::{ActiveTimeline, ComposerMode, Draft, Mode},
    context_menu::{MenuItem, copy_to_clipboard, open_menu},
    message_dialogs::{MessageDialog, OpenMessageDialog, Pinned, PinnedRefresh},
    shell::{Rooms, Selected},
    lightbox::{LightboxImage, LightboxImages, LightboxIndex},
    profile::open_profile,
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
/// Rows rendered at first; the window grows as the user scrolls up, and only
/// once it covers everything loaded do we ask the server for more.
const INITIAL_WINDOW: usize = 120;
const WINDOW_STEP: usize = 100;
/// One-click reactions offered on hover.
pub(crate) const QUICK_REACTIONS: [&str; 3] = ["👍", "❤️", "😂"];
/// The "Add reaction" picker.
const EMOJI_PICKER: [&str; 24] = [
    "👍", "👎", "❤️", "😂", "😮", "😢", "😡", "🎉", "🔥", "👀", "🙏", "👏", "💯", "✅", "❌", "🤔", "😎",
    "🥳", "🚀", "⭐", "💀", "🫡", "🤝", "💡",
];

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
    // The message to draw the "NEW" divider above, captured once from the SDK's
    // read-marker position when the room is opened. It stays fixed for the rest
    // of this viewing session; it is not recomputed as later diffs arrive, so
    // the divider does not chase the user down the timeline.
    let mut new_marker = use_signal(|| None::<String>);
    let mut marker_captured = use_signal(|| false);
    let mut caught_up = use_signal(|| false);
    // Plain (non-signal) handle to the open timeline. Leaving the room (it
    // remounts on room switch) fires a fully-read receipt on cleanup, without
    // touching reactive state while the component is being torn down.
    let handle_cell = use_hook(|| Rc::new(RefCell::new(None::<TimelineHandle>)));
    use_drop({
        let handle_cell = handle_cell.clone();
        move || {
            if let Some(active) = handle_cell.borrow_mut().take() {
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
            let opened = match root_event_id {
                Some(root_event_id) => session.thread(room_id, root_event_id).await,
                None => session.timeline(room_id).await,
            };
            match opened {
                Ok(OpenTimeline {
                    handle: opened,
                    mut updates,
                }) => {
                    handle.set(Some(opened.clone()));
                    *handle_cell.borrow_mut() = Some(opened.clone());
                    while let Some(diffs) = updates.next().await {
                        entries
                            .with_mut(|list| diffs.into_iter().for_each(|diff| diff.apply(list)));

                        // The read-marker virtual item only reflects the fully-read
                        // position as of opening the room, so capture it once.
                        if !marker_captured() {
                            marker_captured.set(true);
                            new_marker.set(first_unread_after_marker(&entries.read()));
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

                        // The SDK only sends a receipt when the latest event moved forward.
                        let receipt = opened.clone();
                        spawn(async move {
                            if let Err(err) = receipt.send_read_receipt().await {
                                warn!(%err, "read receipt failed");
                            }
                        });
                    }
                }
                Err(err) => {
                    error!(%err, "opening timeline failed");
                    load_error.set(Some(err.to_string()));
                }
            }
        }
    });

    let mut window = use_signal(|| INITIAL_WINDOW);

    let mut load_older = move || {
        // Reveal already-loaded rows before fetching more history.
        if window() < entries.read().len() {
            window += WINDOW_STEP;
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

    // `column-reverse` makes scrollTop run from 0 (bottom) to negative (top).
    let on_scroll = move |evt: Event<ScrollData>| {
        let distance_to_top =
            f64::from(evt.scroll_height()) - f64::from(evt.client_height()) + evt.scroll_top();
        if distance_to_top < PAGINATE_THRESHOLD_PX {
            load_older();
        }
        let away = -evt.scroll_top() > JUMP_THRESHOLD_PX;
        if away != scrolled_up() {
            scrolled_up.set(away);
        }
    };

    // Keep the newest message in view as content arrives unless the user has
    // scrolled up. `column-reverse` normally does this by itself, but WebKit
    // leaves the view wherever it was when rows are added to an empty
    // container, which showed older messages after switching rooms.
    let pin_to_bottom = move |behavior: ScrollBehavior| {
        if let Some(el) = container() {
            spawn(async move {
                let _ = el.scroll(dioxus::html::geometry::PixelsVector2D::new(0.0, 0.0), behavior).await;
            });
        }
    };
    use_effect(move || {
        let _ = entries.read().len();
        if !*scrolled_up.peek() {
            pin_to_bottom(ScrollBehavior::Instant);
        }
    });

    let at_start = reached_start()
        || entries
            .read()
            .iter()
            .any(|entry| matches!(entry, TimelineEntry::TimelineStart));
    let entries_now = entries.read();
    let marker = new_marker();
    let rows = group_rows(&entries_now, marker.as_deref());
    // Nothing from the SDK yet (not even the start marker): show placeholders
    // instead of an empty column.
    let loading = entries_now.is_empty() && load_error().is_none();

    rsx! {
        // Rendered newest-first; CSS flips it and keeps the view pinned to the bottom.
        section {
            class: "timeline",
            onscroll: on_scroll,
            onmounted: move |e| container.set(Some(e.data())),
            if loading {
                SkeletonRows {}
            }
            for row in rows.into_iter().rev().take(window()) {
                match row {
                    Row::Message { message, show_header } => rsx! {
                        MessageRow { key: "{message.unique_id}", message: message.clone(), show_header }
                    },
                    Row::Date(label) => rsx! {
                        div { key: "date-{label}", class: "date-divider", span { "{label}" } }
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
                onclick: move |_| pin_to_bottom(ScrollBehavior::Smooth),
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
    Date(String),
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

    for entry in entries {
        match entry {
            TimelineEntry::Message(message) => {
                if marker == Some(message.unique_id.as_str()) {
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
                rows.push(Row::Date(calendar_date(*timestamp_ms)));
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

#[component]
pub(crate) fn MessageRow(message: Message, show_header: bool) -> Element {
    let timeline = use_context::<ActiveTimeline>();
    let mut mode = use_context::<Mode>();
    let mut draft = use_context::<Draft>();
    let mut active_thread = use_context::<ActiveThread>();
    let time = clock_time(message.timestamp_ms);

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
            let id = id.clone();
            spawn(async move {
                if let Err(err) = active.redact(id).await {
                    warn!(%err, "delete failed");
                }
            });
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
                    warn!(%err, "reaction failed");
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
                            if room.is_dm { format!("@{}", room.name) } else { format!("#{}", room.name) },
                            {
                                let session = session.clone();
                                let (from, event_id, to) = (room_id.clone(), event_id.clone(), room.id.clone());
                                move || {
                                    let future = session.forward(from.clone(), event_id.clone(), to.clone());
                                    spawn(async move {
                                        if let Err(err) = future.await {
                                            warn!(%err, "forward failed");
                                        }
                                    });
                                }
                            },
                        )
                    })
                    .collect();
                if !targets.is_empty() {
                    items.push(MenuItem::submenu("Forward to", targets));
                }

                let pinned = consume_context::<Pinned>().peek().0.contains(event_id);
                items.push(MenuItem::new(if pinned { "Unpin message" } else { "Pin message" }, {
                    let session = session.clone();
                    let (room_id, event_id) = (room_id.clone(), event_id.clone());
                    move || {
                        let future = session.pin(room_id.clone(), event_id.clone(), !pinned);
                        let mut refresh = consume_context::<PinnedRefresh>();
                        spawn(async move {
                            if let Err(err) = future.await {
                                warn!(%err, "pin failed");
                            }
                            refresh += 1;
                        });
                    }
                }));
                items.push(MenuItem::new("Mark unread", {
                    let session = session.clone();
                    let room_id = room_id.clone();
                    move || {
                        let future = session.mark_unread(room_id.clone(), true);
                        spawn(async move {
                            if let Err(err) = future.await {
                                warn!(%err, "mark unread failed");
                            }
                        });
                    }
                }));
            }
            if let Some(text) = message.kind.plain_body().filter(|t| !t.is_empty()) {
                let text = text.to_owned();
                items.push(MenuItem::new("Copy text", move || copy_to_clipboard(&text)));
            }
            if let Some((event_id, room_id)) = &sent {
                let link = format!("https://matrix.to/#/{room_id}/{event_id}");
                items.push(MenuItem::new("Copy message link", move || copy_to_clipboard(&link)));
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
                    consume_context::<OpenMessageDialog>().set(MessageDialog::Report(event_id.clone()));
                }));
            }
            if message.is_own && can_reply {
                items.push(
                    MenuItem::new("Delete message", {
                        let id = message.id.clone();
                        move || {
                            let Some(active) = timeline() else { return };
                            let id = id.clone();
                            spawn(async move {
                                if let Err(err) = active.redact(id).await {
                                    warn!(%err, "delete failed");
                                }
                            });
                        }
                    })
                    .danger(),
                );
            }
            if let Some((event_id, _)) = &sent {
                let id = event_id.to_string();
                items.push(MenuItem::new("Copy message ID", move || copy_to_clipboard(&id)));
            }
            let quick: Rc<dyn Fn(&'static str)> = Rc::new({
                let react = react.clone();
                move |key| react(key)
            });
            open_menu(&e, items, (!message.is_local_echo).then_some(quick));
        }
    };

    rsx! {
        div {
            class: "message",
            oncontextmenu: on_context,
            class: if show_header { "with-header" },
            class: if message.is_local_echo { "pending" },
            class: if message.is_own { "own" },
            if show_header {
                button {
                    class: "avatar-button",
                    title: "{message.sender}",
                    onclick: {
                        let sender = message.sender.clone();
                        move |_| open_profile(sender.clone())
                    },
                    Avatar { media: message.sender_avatar.clone(), fallback: message.sender_name.clone(), size: 40 }
                }
                div { class: "message-main",
                    div { class: "message-meta",
                        button {
                            class: "sender",
                            title: "{message.sender}",
                            onclick: {
                                let sender = message.sender.clone();
                                move |_| open_profile(sender.clone())
                            },
                            "{message.sender_name}"
                        }
                        span { class: "time", "{time}" }
                    }
                    ReplyQuote { reply: message.in_reply_to.clone() }
                    MessageBody { kind: message.kind.clone(), edited: message.is_edited }
                    Reactions { reactions: message.reactions.clone(), on_toggle: react.clone() }
                    ThreadChip { thread: message.thread.clone(), on_open: open_thread_chip.clone() }
                }
            } else {
                span { class: "time gutter", "{time}" }
                div { class: "message-main",
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
                        button { class: "action reply", title: "Reply", onclick: start_reply, "↩" }
                        button { class: "action thread", title: "Reply in thread", onclick: open_thread, "🧵" }
                    }
                    if message.can_edit {
                        button { class: "action edit", title: "Edit", onclick: start_edit, "✏️" }
                    }
                    if message.is_own && can_reply {
                        button { class: "action delete", title: "Delete", onclick: delete, "🗑" }
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
            span { class: "thread-chip-icon", "🧵" }
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
        } => rsx! { ImageBody { body, media, width, height } },
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
        // Keys usually arrive shortly (backup, other device), so this reads as
        // "still loading" rather than an error.
        MessageKind::UnableToDecrypt => rsx! {
            div {
                class: "body skeleton skeleton-line undecrypted",
                title: "Waiting for the keys to decrypt this message",
                "aria-label": "Encrypted message, waiting for keys",
            }
        },
        MessageKind::State(text) => rsx! { p { class: "body muted", "{text}" } },
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
        None => rsx! { p { class: "{class}", "{body}" span { class: "edited", "{edited_tag}" } } },
    }
}

#[component]
fn ImageBody(body: String, media: MediaRef, width: Option<u32>, height: Option<u32>) -> Element {
    let mut lightbox_index = use_context::<LightboxIndex>();
    let gallery = use_context::<LightboxImages>();
    let url = use_thumbnail(Some(media.clone()), 800, 600);
    let aspect = match (width, height) {
        (Some(w), Some(h)) if h > 0 => format!("aspect-ratio: {w} / {h};"),
        _ => String::new(),
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
            span { "{reply.summary.as_deref().unwrap_or(\"Loading…\")}" }
        }
    }
}
