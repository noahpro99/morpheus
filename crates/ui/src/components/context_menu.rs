//! Right-click menus, Discord-style: one styled menu for messages, rooms and
//! members, replacing the webview's native menu everywhere in the shell.
//!
//! A target builds its items at right-click time as plain closures (no
//! scope-owned callbacks, so the menu outlives any row that re-renders) and
//! hands them to [`open_menu`]; [`ContextMenuLayer`] draws whatever is open.

use std::{cell::RefCell, rc::Rc};

use dioxus::prelude::*;

use super::timeline::QUICK_REACTIONS;

type Action = Rc<RefCell<dyn FnMut()>>;

pub struct MenuItem {
    label: String,
    danger: bool,
    /// Shown with a check mark (the current choice in a submenu).
    checked: bool,
    run: Option<Action>,
    /// A flyout opened on hover; the item itself does nothing when clicked.
    children: Vec<Rc<MenuItem>>,
    /// Lay the flyout out as a grid (emoji pickers).
    grid: bool,
}

impl MenuItem {
    pub fn new(label: impl Into<String>, run: impl FnMut() + 'static) -> Self {
        Self {
            label: label.into(),
            danger: false,
            checked: false,
            run: Some(Rc::new(RefCell::new(run))),
            children: Vec::new(),
            grid: false,
        }
    }

    /// An item that opens `children` in a flyout.
    pub fn submenu(label: impl Into<String>, children: Vec<MenuItem>) -> Self {
        Self {
            label: label.into(),
            danger: false,
            checked: false,
            run: None,
            children: children.into_iter().map(Rc::new).collect(),
            grid: false,
        }
    }

    /// A submenu whose children are laid out as a grid of short labels.
    pub fn grid(label: impl Into<String>, children: Vec<MenuItem>) -> Self {
        let mut item = Self::submenu(label, children);
        item.grid = true;
        item
    }

    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }
}

pub struct Menu {
    x: f64,
    y: f64,
    items: Vec<Rc<MenuItem>>,
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
    open_menu_at(point.x, point.y, items, react);
}

/// Open a menu at viewport coordinates, for menus built after an async
/// lookup (the event is long gone by then; call `prevent_default` first).
pub fn open_menu_at(
    x: f64,
    y: f64,
    items: Vec<MenuItem>,
    react: Option<Rc<dyn Fn(&'static str)>>,
) {
    let mut menu = consume_context::<ContextMenu>();
    menu.set(MenuState(Some(Rc::new(Menu {
        x,
        y,
        items: items.into_iter().map(Rc::new).collect(),
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
    let menu = use_context::<ContextMenu>();
    let Some(open) = menu().0 else {
        return rsx! {};
    };
    let close = move || {
        let mut menu = menu;
        menu.set(MenuState::default());
    };

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
                {entries(&open.items, close)}
            }
        }
    }
}

/// Rows of a menu; a row with children renders its flyout inline (CSS shows
/// it on hover).
fn entries(items: &[Rc<MenuItem>], close: impl FnMut() + Copy + 'static) -> Element {
    rsx! {
        for item in items.iter().cloned() {
            if item.children.is_empty() {
                button {
                    class: "context-item",
                    class: if item.danger { "danger" },
                    class: if item.checked { "checked" },
                    role: "menuitem",
                    onclick: {
                        let item = item.clone();
                        move |_| {
                            let mut close = close;
                            if let Some(run) = &item.run {
                                (run.borrow_mut())();
                            }
                            close();
                        }
                    },
                    "{item.label}"
                }
            } else {
                div { class: "context-item has-submenu", role: "menuitem", "aria-haspopup": "true",
                    span { "{item.label}" }
                    span { class: "submenu-arrow", "›" }
                    div { class: "context-menu context-submenu", class: if item.grid { "grid" }, role: "menu",
                        {entries(&item.children, close)}
                    }
                }
            }
        }
    }
}
