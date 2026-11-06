//! Dialogs reached from a message's right-click menu: who reacted, report a
//! message, select its text on a touch screen, and the room's pinned
//! messages (also behind the header pin).

use super::toast::report;
use dioxus::prelude::*;
use morpheus_core::{Message, OwnedEventId, OwnedRoomId, Session};

use super::shell::Selected;

/// Which message dialog is open. Provided by the shell.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum MessageDialog {
    #[default]
    None,
    Reactions(Box<Message>),
    Report(OwnedEventId),
    /// Touch screens: a message shown where holding selects text.
    SelectText(String),
    Pinned,
}
pub type OpenMessageDialog = Signal<MessageDialog>;

/// Pinned event IDs of the open room, refetched when the room or
/// [`PinnedRefresh`] changes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PinnedIds(pub Vec<OwnedEventId>);
pub type Pinned = Signal<PinnedIds>;
pub type PinnedRefresh = Signal<u32>;

/// Install the pinned-ids fetcher. Call once in the shell.
pub fn use_pinned_fetcher(
    session: Session,
    selected: Selected,
    refresh: PinnedRefresh,
    mut pinned: Pinned,
) {
    use_effect(move || {
        let Some(room_id) = selected() else {
            pinned.set(PinnedIds::default());
            return;
        };
        let _ = refresh();
        let session = session.clone();
        spawn(async move {
            match session.pinned_event_ids(room_id).await {
                Ok(ids) => pinned.set(PinnedIds(ids)),
                Err(err) => report("pinned events", &err),
            }
        });
    });
}

#[component]
pub fn ReactionsModal(message: Message) -> Element {
    let mut dialog = use_context::<OpenMessageDialog>();
    let mut close = move || dialog.set(MessageDialog::None);
    rsx! {
        div { class: "overlay", onclick: move |_| close(),
            div { class: "message-dialog reactions-dialog", tabindex: "-1", autofocus: true, onclick: move |e| e.stop_propagation(),
                h2 { "Reactions" }
                if message.reactions.is_empty() {
                    p { class: "muted", "No reactions yet." }
                }
                for reaction in message.reactions.iter() {
                    div { class: "reaction-row",
                        span { class: "reaction-key", "{reaction.key}" }
                        span { class: "reaction-senders",
                            for (index, sender) in reaction.senders.iter().enumerate() {
                                if index > 0 { ", " }
                                span { title: "{sender}", "{sender.localpart()}" }
                            }
                        }
                    }
                }
                div { class: "settings-actions",
                    button { class: "secondary", onclick: move |_| close(), "Close" }
                }
            }
        }
    }
}

#[component]
pub fn ReportModal(event_id: OwnedEventId) -> Element {
    let session = use_context::<Session>();
    let selected = use_context::<Selected>();
    let mut dialog = use_context::<OpenMessageDialog>();
    let mut reason = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(|| None::<Result<&'static str, String>>);
    let mut close = move || dialog.set(MessageDialog::None);

    let submit = move |evt: FormEvent| {
        evt.prevent_default();
        let Some(room_id) = selected.peek().clone() else {
            return;
        };
        if busy() {
            return;
        }
        busy.set(true);
        let future = session.report(room_id, event_id.clone(), reason());
        spawn(async move {
            match future.await {
                Ok(()) => message.set(Some(Ok("Reported to the server admins."))),
                Err(err) => message.set(Some(Err(err.to_string()))),
            }
            busy.set(false);
        });
    };

    rsx! {
        div { class: "overlay", onclick: move |_| close(),
            form { class: "message-dialog report-dialog", tabindex: "-1", autofocus: true, onclick: move |e| e.stop_propagation(), onsubmit: submit,
                h2 { "Report message" }
                p { class: "muted", "Tell the homeserver admins what is wrong with this message." }
                label { "Reason"
                    input { name: "reason", placeholder: "Spam, abuse, …", value: "{reason}", oninput: move |e| reason.set(e.value()) }
                }
                match message() {
                    Some(Ok(text)) => rsx! { p { class: "ok", "{text}" } },
                    Some(Err(text)) => rsx! { p { class: "error", "{text}" } },
                    None => rsx! {},
                }
                div { class: "settings-actions",
                    if message().is_none_or(|m| m.is_err()) {
                        button { r#type: "submit", class: "danger", disabled: busy(), "Report" }
                    }
                    button { r#type: "button", class: "secondary", onclick: move |_| close(), "Close" }
                }
            }
        }
    }
}

/// The room's pinned messages, with unpin for each.
#[component]
pub fn PinnedPanel(room_id: OwnedRoomId) -> Element {
    let session = use_context::<Session>();
    let fetcher = session.clone();
    let mut dialog = use_context::<OpenMessageDialog>();
    let mut refresh = use_context::<PinnedRefresh>();
    let mut close = move || dialog.set(MessageDialog::None);

    let pinned = use_resource(use_reactive!(|room_id| {
        let session = fetcher.clone();
        let _ = refresh();
        async move {
            session
                .pinned_messages(room_id)
                .await
                .map_err(|e| e.to_string())
        }
    }));

    rsx! {
        div { class: "overlay", onclick: move |_| close(),
            div { class: "message-dialog pinned-dialog", tabindex: "-1", autofocus: true, onclick: move |e| e.stop_propagation(),
                h2 { "Pinned messages" }
                match &*pinned.read() {
                    None => rsx! { crate::media::Skeleton { rows: 3 } },
                    Some(Err(err)) => rsx! { p { class: "error", "{err}" } },
                    Some(Ok(list)) if list.is_empty() => rsx! { p { class: "muted", "Nothing pinned yet. Right-click a message to pin it." } },
                    Some(Ok(list)) => rsx! {
                        for item in list.iter().rev() {
                            div { key: "{item.event_id}", class: "pinned-item",
                                div { class: "pinned-main",
                                    span { class: "sender", "{item.sender.localpart()}" }
                                    p { class: "body", "{item.body}" }
                                }
                                button {
                                    class: "ghost",
                                    title: "Unpin",
                                    onclick: {
                                        let session = session.clone();
                                        let (room_id, event_id) = (room_id.clone(), item.event_id.clone());
                                        move |_| {
                                            let future = session.pin(room_id.clone(), event_id.clone(), false);
                                            spawn(async move {
                                                if let Err(err) = future.await {
                                                    report("unpin", &err);
                                                }
                                                refresh += 1;
                                            });
                                        }
                                    },
                                    "✕"
                                }
                            }
                        }
                    },
                }
                div { class: "settings-actions",
                    button { class: "secondary", onclick: move |_| close(), "Close" }
                }
            }
        }
    }
}

/// Touch screens hold for menus, so a message's text cannot be selected
/// where it is: here holding selects, with the system's handles and Copy.
#[component]
pub fn SelectTextModal(text: String) -> Element {
    let mut dialog = use_context::<OpenMessageDialog>();
    let mut close = move || dialog.set(MessageDialog::None);
    let all = text.clone();
    rsx! {
        div { class: "overlay", onclick: move |_| close(),
            div {
                class: "message-dialog select-text",
                tabindex: "-1",
                onclick: move |e| e.stop_propagation(),
                // The system's own press-and-hold: no app menu over it.
                oncontextmenu: move |e| e.stop_propagation(),
                h2 { "Select text" }
                p { class: "muted", "Press and hold to select." }
                div { class: "selectable", "{text}" }
                div { class: "settings-actions",
                    button {
                        class: "secondary",
                        onclick: move |_| {
                            super::context_menu::copy_to_clipboard(&all);
                            close();
                        },
                        "Copy all"
                    }
                    button { class: "secondary", onclick: move |_| close(), "Close" }
                }
            }
        }
    }
}
