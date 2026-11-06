//! Toasts: short notices stacked in the bottom-right corner, for actions
//! that failed in the background. Every failure is also logged.

use dioxus::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub id: u32,
    pub text: String,
    /// Offers Undo (see [`toast_with_undo`]).
    pub undo: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToastList {
    pub items: Vec<Toast>,
    next_id: u32,
}
pub type Toasts = Signal<ToastList>;

/// How long a toast stays unless dismissed.
const TOAST_SECS: u64 = 6;

thread_local! {
    /// The shell's toasts, for tasks that run outside any component
    /// (`spawn_forever`), where context lookups find nothing.
    // Already const; clippy on the Android target misreads the macro.
    #[allow(clippy::missing_const_for_thread_local)]
    static SHELL_TOASTS: std::cell::Cell<Option<Toasts>> = const { std::cell::Cell::new(None) };
}

/// Called by the shell as it provides its toasts.
pub fn register_shell_toasts(toasts: Toasts) {
    SHELL_TOASTS.with(|slot| slot.set(Some(toasts)));
}

/// Show `text` as a toast. Outside the shell (no toast context) it only logs.
pub fn toast(text: impl Into<String>) {
    let text = text.into();
    let toasts = try_consume_context::<Toasts>()
        .or_else(|| SHELL_TOASTS.with(std::cell::Cell::get))
        .filter(|toasts| toasts.try_peek().is_ok());
    let Some(mut toasts) = toasts else {
        tracing::warn!(%text, "toast without a shell");
        return;
    };
    let id = toasts.with_mut(|list| {
        let id = list.next_id;
        list.next_id += 1;
        list.items.push(Toast {
            id,
            text,
            undo: false,
        });
        id
    });
    dioxus::core::spawn_forever(async move {
        tokio_sleep(std::time::Duration::from_secs(TOAST_SECS)).await;
        if toasts.try_peek().is_ok() {
            toasts.with_mut(|list| list.items.retain(|t| t.id != id));
        }
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
                    span { class: "toast-text", "{toast.text}" }
                    if toast.undo {
                        button {
                            class: "toast-undo",
                            onclick: move |e| {
                                e.stop_propagation();
                                undo(toast.id);
                                toasts.with_mut(|list| list.items.retain(|t| t.id != toast.id));
                            },
                            "Undo"
                        }
                    }
                }
            }
        }
    }
}

/// How long an undoable action waits before it is done for good.
pub const UNDO_SECS: u64 = 5;

type Pending = std::collections::HashMap<u32, (Box<dyn FnOnce()>, Box<dyn FnOnce()>)>;

thread_local! {
    /// Undoable actions waiting out their toast: (undo, do it) by toast id.
    #[allow(clippy::missing_const_for_thread_local)]
    static PENDING: std::cell::RefCell<Pending> = std::cell::RefCell::new(Pending::new());
}

/// Instead of asking "are you sure": show `text` with an Undo button and
/// do `commit` once the toast has gone; Undo runs `revert` instead. The
/// caller has already made the change look done.
pub fn toast_with_undo(
    text: impl Into<String>,
    revert: impl FnOnce() + 'static,
    commit: impl FnOnce() + 'static,
) {
    let text = text.into();
    let toasts = try_consume_context::<Toasts>()
        .or_else(|| SHELL_TOASTS.with(std::cell::Cell::get))
        .filter(|toasts| toasts.try_peek().is_ok());
    let Some(mut toasts) = toasts else {
        commit();
        return;
    };
    let id = toasts.with_mut(|list| {
        let id = list.next_id;
        list.next_id += 1;
        list.items.push(Toast {
            id,
            text,
            undo: true,
        });
        id
    });
    PENDING.with(|pending| {
        pending
            .borrow_mut()
            .insert(id, (Box::new(revert), Box::new(commit)));
    });
    dioxus::core::spawn_forever(async move {
        tokio_sleep(std::time::Duration::from_secs(UNDO_SECS)).await;
        if let Some((_, commit)) = PENDING.with(|pending| pending.borrow_mut().remove(&id)) {
            commit();
        }
        if toasts.try_peek().is_ok() {
            toasts.with_mut(|list| list.items.retain(|t| t.id != id));
        }
    });
}

/// Undo the action behind toast `id`, if it is still waiting.
fn undo(id: u32) {
    if let Some((revert, _)) = PENDING.with(|pending| pending.borrow_mut().remove(&id)) {
        revert();
    }
}

/// What an undoable action has made look done while its toast is up:
/// messages (by event ID) and rooms (by room ID), hidden until the action
/// is undone or done.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HiddenSet(pub std::collections::HashSet<String>);
pub type Hidden = Signal<HiddenSet>;

/// Hide `key` now; undoable for [`UNDO_SECS`] from a toast saying `text`,
/// after which `commit` runs (`what` names it if it fails), and `key`
/// shows again once it is done, as what it became.
pub fn hide_with_undo(
    key: String,
    text: impl Into<String>,
    what: &'static str,
    commit: impl FnOnce() -> morpheus_core::CoreFuture<()> + 'static,
) {
    let Some(mut hidden) = try_consume_context::<Hidden>() else {
        let future = commit();
        spawn(async move {
            if let Err(err) = future.await {
                report(what, &err);
            }
        });
        return;
    };
    hidden.with_mut(|set| set.0.insert(key.clone()));
    let shown_again = key.clone();
    toast_with_undo(
        text,
        move || {
            hidden.with_mut(|set| set.0.remove(&shown_again));
        },
        move || {
            let future = commit();
            dioxus::core::spawn_forever(async move {
                if let Err(err) = future.await {
                    report(what, &err);
                }
                hidden.with_mut(|set| set.0.remove(&key));
            });
        },
    );
}
