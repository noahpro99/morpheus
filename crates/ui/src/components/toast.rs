//! Toasts: short notices stacked in the bottom-right corner, for actions
//! that failed in the background. Every failure is also logged.

use dioxus::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub id: u32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToastList {
    pub items: Vec<Toast>,
    next_id: u32,
}
pub type Toasts = Signal<ToastList>;

/// How long a toast stays unless dismissed.
const TOAST_SECS: u64 = 6;

/// Show `text` as a toast. Outside the shell (no toast context) it only logs.
pub fn toast(text: impl Into<String>) {
    let text = text.into();
    let Some(mut toasts) = try_consume_context::<Toasts>() else {
        tracing::warn!(%text, "toast without a shell");
        return;
    };
    let id = toasts.with_mut(|list| {
        let id = list.next_id;
        list.next_id += 1;
        list.items.push(Toast { id, text });
        id
    });
    spawn(async move {
        tokio_sleep(std::time::Duration::from_secs(TOAST_SECS)).await;
        toasts.with_mut(|list| list.items.retain(|t| t.id != id));
    });
}

/// Report a failed action: a warning in the log and a toast for the user.
pub fn report(what: &str, err: &impl std::fmt::Display) {
    tracing::warn!(%err, what, "action failed");
    let mut what = what.chars();
    let label: String = match what.next() {
        Some(first) => first.to_uppercase().chain(what).collect(),
        None => String::new(),
    };
    toast(format!("{label} failed: {err}"));
}

/// Dioxus desktop and mobile both run on a Tokio runtime, so its timer
/// serves here.
async fn tokio_sleep(duration: std::time::Duration) {
    tokio::time::sleep(duration).await;
}

/// Renders the stack; clicking a toast dismisses it.
#[component]
pub fn ToastLayer() -> Element {
    let mut toasts = use_context::<Toasts>();
    rsx! {
        div { class: "toast-layer", "aria-live": "polite",
            for toast in toasts().items.iter().cloned() {
                div {
                    key: "{toast.id}",
                    class: "toast",
                    role: "status",
                    onclick: move |_| toasts.with_mut(|list| list.items.retain(|t| t.id != toast.id)),
                    "{toast.text}"
                }
            }
        }
    }
}
