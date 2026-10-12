//! Right-click menus, Discord-style: one styled menu for messages, rooms and
//! members, replacing the webview's native menu everywhere in the shell.
//!
//! A target builds its items at right-click time as plain closures (no
//! scope-owned callbacks, so the menu outlives any row that re-renders) and
//! hands them to [`open_menu`]; [`ContextMenuLayer`] draws whatever is open.

use std::rc::Rc;

use dioxus::prelude::*;

use super::timeline::QUICK_REACTIONS;

pub struct MenuItem {
    pub label: String,
    pub danger: bool,
    pub run: Rc<dyn Fn()>,
}

impl MenuItem {
    pub fn new(label: impl Into<String>, run: impl Fn() + 'static) -> Self {
        Self {
            label: label.into(),
            danger: false,
            run: Rc::new(run),
        }
    }

    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
}

pub struct Menu {
    x: f64,
    y: f64,
    items: Vec<MenuItem>,
    /// Quick-reaction row at the top (messages only).
    react: Option<Rc<dyn Fn(&'static str)>>,
}

/// The open menu, if any. Provided by the shell.
#[derive(Clone, Default)]
pub struct MenuState(pub Option<Rc<Menu>>);
pub type ContextMenu = Signal<MenuState>;

/// Open a menu at the pointer for a right-click event. Swallows the event so
/// the shell's catch-all handler does not close it again.
pub fn open_menu(
    e: &Event<MouseData>,
    items: Vec<MenuItem>,
    react: Option<Rc<dyn Fn(&'static str)>>,
) {
    e.prevent_default();
    e.stop_propagation();
    let point = e.client_coordinates();
    let mut menu = consume_context::<ContextMenu>();
    menu.set(MenuState(Some(Rc::new(Menu {
        x: point.x,
        y: point.y,
        items,
        react,
    }))));
}

/// Put `text` on the system clipboard through the webview.
pub fn copy_to_clipboard(text: &str) {
    let js = format!(
        "navigator.clipboard.writeText({});",
        serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into())
    );
    document::eval(&js);
}

/// Keeps the menu inside the window when opened near an edge.
const CLAMP_JS: &str = "const m = document.querySelector('.context-menu'); if (!m) return; \
    const r = m.getBoundingClientRect(); \
    if (r.right > innerWidth - 8) m.style.left = Math.max(8, innerWidth - r.width - 8) + 'px'; \
    if (r.bottom > innerHeight - 8) m.style.top = Math.max(8, innerHeight - r.height - 8) + 'px';";

#[component]
pub fn ContextMenuLayer() -> Element {
    let mut menu = use_context::<ContextMenu>();
    let Some(open) = menu().0 else {
        return rsx! {};
    };
    let mut close = move || menu.set(MenuState::default());

    rsx! {
        div {
            class: "context-backdrop",
            onclick: move |_| close(),
            oncontextmenu: move |e| {
                e.prevent_default();
                close();
            },
            div {
                class: "context-menu",
                role: "menu",
                style: "left: {open.x}px; top: {open.y}px",
                onclick: move |e| e.stop_propagation(),
                onmounted: move |_| {
                    document::eval(CLAMP_JS);
                },
                if let Some(react) = open.react.clone() {
                    div { class: "context-reactions",
                        for key in QUICK_REACTIONS {
                            button {
                                class: "context-reaction",
                                title: "React with {key}",
                                onclick: {
                                    let react = react.clone();
                                    move |_| {
                                        react(key);
                                        close();
                                    }
                                },
                                "{key}"
                            }
                        }
                    }
                }
                for item in open.items.iter() {
                    button {
                        class: "context-item",
                        class: if item.danger { "danger" },
                        role: "menuitem",
                        onclick: {
                            let run = item.run.clone();
                            move |_| {
                                run();
                                close();
                            }
                        },
                        "{item.label}"
                    }
                }
            }
        }
    }
}
