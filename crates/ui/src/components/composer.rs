use dioxus::prelude::*;
use morpheus_core::TimelineHandle;

/// Shared between the timeline (which owns the handle) and the composer.
pub type ActiveTimeline = Signal<Option<TimelineHandle>>;

#[component]
pub fn Composer(room_name: String) -> Element {
    let timeline = use_context::<ActiveTimeline>();
    let mut draft = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);

    let mut send = move || {
        let body = draft().trim().to_owned();
        let Some(handle) = timeline() else { return };
        if body.is_empty() {
            return;
        }
        draft.set(String::new());
        spawn(async move {
            match handle.send_text(body).await {
                Ok(()) => error.set(None),
                Err(err) => error.set(Some(err.to_string())),
            }
        });
    };

    rsx! {
        footer { class: "composer",
            if let Some(message) = error() {
                p { class: "error", "{message}" }
            }
            input {
                r#type: "text",
                placeholder: "Message #{room_name}",
                value: "{draft}",
                disabled: timeline().is_none(),
                oninput: move |e| draft.set(e.value()),
                onkeydown: move |e| {
                    if e.key() == Key::Enter && !e.modifiers().shift() {
                        e.prevent_default();
                        send();
                    }
                },
            }
        }
    }
}
