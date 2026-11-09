//! "Are you sure?" before something hard to take back (signing out,
//! deleting a message): a title, what will happen, Cancel and the action.

use std::{cell::RefCell, rc::Rc};

use dioxus::prelude::*;

/// What is being asked. Provided by the shell.
#[derive(Clone)]
pub struct ConfirmRequest {
    pub title: String,
    pub body: String,
    /// The button that goes ahead ("Sign out", "Delete").
    pub confirm: String,
    pub action: Rc<RefCell<dyn FnMut()>>,
}

impl PartialEq for ConfirmRequest {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.action, &other.action)
    }
}

pub type Confirm = Signal<Option<ConfirmRequest>>;

/// Ask before running `action`.
pub fn ask(
    title: impl Into<String>,
    body: impl Into<String>,
    confirm: impl Into<String>,
    action: impl FnMut() + 'static,
) {
    if let Some(mut asking) = try_consume_context::<Confirm>() {
        asking.set(Some(ConfirmRequest {
            title: title.into(),
            body: body.into(),
            confirm: confirm.into(),
            action: Rc::new(RefCell::new(action)),
        }));
    }
}

#[component]
pub fn ConfirmDialog() -> Element {
    let mut asking = use_context::<Confirm>();
    let Some(request) = asking() else {
        return rsx! {};
    };
    let go = {
        let action = request.action.clone();
        move || {
            asking.set(None);
            (action.borrow_mut())();
        }
    };
    rsx! {
        div { class: "overlay", onclick: move |_| asking.set(None),
            div {
                class: "message-dialog confirm-dialog",
                role: "alertdialog",
                onclick: move |e| e.stop_propagation(),
                onkeydown: {
                    let mut go = go.clone();
                    move |e: KeyboardEvent| {
                        if e.key() == Key::Enter {
                            e.prevent_default();
                            go();
                        }
                    }
                },
                h2 { "{request.title}" }
                p { class: "muted", "{request.body}" }
                div { class: "settings-actions confirm-actions",
                    button { class: "secondary", onclick: move |_| asking.set(None), "Cancel" }
                    button {
                        class: "danger",
                        onmounted: move |e| {
                            spawn(async move {
                                let _ = e.data().set_focus(true).await;
                            });
                        },
                        onclick: {
                            let mut go = go.clone();
                            move |_| go()
                        },
                        "{request.confirm}"
                    }
                }
            }
        }
    }
}
