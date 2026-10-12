use dioxus::prelude::*;
use morpheus_core::{MessageId, OwnedEventId, TimelineHandle};
use tracing::warn;

use crate::attach::pick_attachment;

/// Shared between the timeline (which owns the handle) and the composer.
pub type ActiveTimeline = Signal<Option<TimelineHandle>>;

/// What the next Enter in the composer does besides sending plain text.
#[derive(Debug, Clone, PartialEq)]
pub enum ComposerMode {
    Reply {
        event_id: OwnedEventId,
        sender_name: String,
        summary: String,
    },
    Edit {
        id: MessageId,
        original: String,
    },
}

/// Set by message hover actions; cleared by Esc, cancel, or send.
pub type Mode = Signal<Option<ComposerMode>>;
/// The draft text, exposed so "edit" can pre-fill it.
pub type Draft = Signal<String>;

#[component]
pub fn Composer(room_name: String, #[props(default)] placeholder: Option<String>) -> Element {
    let timeline = use_context::<ActiveTimeline>();
    let mut mode = use_context::<Mode>();
    let mut draft = use_context::<Draft>();
    let mut error = use_signal(|| None::<String>);
    // Focus the box as soon as the room is ready to send, so navigating to a
    // room (or opening a thread) lets you type straight away.
    let mut input_el = use_signal(|| None::<std::rc::Rc<MountedData>>);
    use_effect(move || {
        if timeline().is_some()
            && let Some(el) = input_el()
        {
            spawn(async move {
                let _ = el.set_focus(true).await;
            });
        }
    });

    let mut send = move || {
        let body = draft().trim().to_owned();
        let Some(handle) = timeline() else { return };
        if body.is_empty() {
            return;
        }
        draft.set(String::new());
        let action = mode.take();
        spawn(async move {
            let _ = handle.stop_typing().await;
            let result = match action {
                None => handle.send_text(body).await,
                Some(ComposerMode::Reply { event_id, .. }) => {
                    handle.send_reply(body, event_id).await
                }
                Some(ComposerMode::Edit { id, .. }) => handle.edit(id, body).await,
            };
            match result {
                Ok(()) => error.set(None),
                Err(err) => error.set(Some(err.to_string())),
            }
        });
    };

    let on_input = move |e: FormEvent| {
        draft.set(e.value());
        if let Some(handle) = timeline()
            && !e.value().is_empty()
        {
            spawn(async move {
                if let Err(err) = handle.renew_typing().await {
                    warn!(%err, "typing notice failed");
                }
            });
        }
    };

    let mut cancel = move || {
        if mode.take().is_some() {
            draft.set(String::new());
        }
    };

    let attach = move |_| {
        let Some(handle) = timeline() else { return };
        spawn(async move {
            let Some(path) = pick_attachment().await else {
                return;
            };
            match handle.send_attachment(path).await {
                Ok(()) => error.set(None),
                Err(err) => error.set(Some(err.to_string())),
            }
        });
    };

    let placeholder_text = placeholder.unwrap_or_else(|| format!("Message #{room_name}"));

    rsx! {
        footer { class: "composer",
            if let Some(message) = error() {
                p { class: "error", "{message}" }
            }
            if let Some(active) = mode() {
                div { class: "composer-banner",
                    match &active {
                        ComposerMode::Reply { sender_name, summary, .. } => rsx! {
                            span { class: "banner-label", "Replying to " b { "{sender_name}" } }
                            span { class: "banner-preview", "{summary}" }
                        },
                        ComposerMode::Edit { .. } => rsx! {
                            span { class: "banner-label", "Editing message" }
                        },
                    }
                    button { class: "ghost", title: "Cancel (Esc)", onclick: move |_| cancel(), "✕" }
                }
            }
            div { class: "composer-row",
                button {
                    class: "attach-button",
                    title: "Attach a file",
                    r#type: "button",
                    disabled: timeline().is_none(),
                    onclick: attach,
                    "📎"
                }
                input {
                    onmounted: move |e| input_el.set(Some(e.data())),
                    r#type: "text",
                    placeholder: "{placeholder_text}",
                    value: "{draft}",
                    disabled: timeline().is_none(),
                    oninput: on_input,
                    onkeydown: move |e| {
                        if e.key() == Key::Enter && !e.modifiers().shift() {
                            e.prevent_default();
                            send();
                        } else if e.key() == Key::Escape {
                            cancel();
                        }
                    },
                }
            }
        }
    }
}
