//! Right-click menus, Discord-style: one styled menu for messages, rooms and
//! members, replacing the webview's native menu everywhere in the shell.
//!
//! A target builds its items at right-click time as plain closures (no
//! scope-owned callbacks, so the menu outlives any row that re-renders) and
//! hands them to [`open_menu`]; [`ContextMenuLayer`] draws whatever is open.

use std::{cell::RefCell, rc::Rc};

use dioxus::prelude::*;

use super::timeline::QUICK_REACTIONS;
use crate::icons::Icon;
use crate::media::Avatar;
use morpheus_core::MediaRef;

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
    /// Glyph in front of the label; falls back to [`default_icon`].
    icon: Option<&'static str>,
    /// A picture in front of the label instead of a glyph (rooms, servers).
    avatar: Option<(Option<MediaRef>, String)>,
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
            icon: None,
            avatar: None,
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
            icon: None,
            avatar: None,
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

    pub fn icon(mut self, glyph: &'static str) -> Self {
        self.icon = Some(glyph);
        self
    }

    /// Show this picture (or the initial of `fallback`) in place of an icon.
    pub fn avatar(mut self, media: Option<MediaRef>, fallback: impl Into<String>) -> Self {
        self.avatar = Some((media, fallback.into()));
        self
    }

    fn glyph(&self) -> Option<&'static str> {
        self.icon.or_else(|| default_icon(&self.label))
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

impl MenuState {
    /// Where the open menu sits, for things that pop up in its place.
    pub fn position(&self) -> Option<(f64, f64)> {
        self.0.as_ref().map(|m| (m.x, m.y))
    }
}

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
pub fn open_menu_at(x: f64, y: f64, items: Vec<MenuItem>, react: Option<Rc<dyn Fn(&'static str)>>) {
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
    let sheet = try_use_context::<super::shell::NarrowWindow>().is_some_and(|narrow| narrow().0);
    // The submenu shown in place inside a sheet; reset whenever a menu opens.
    let mut sub = use_signal(|| None::<usize>);
    use_effect(move || {
        let _ = menu.read();
        sub.set(None);
    });
    let Some(open) = menu().0 else {
        return rsx! {};
    };
    let close = move || {
        let mut menu = menu;
        menu.set(MenuState::default());
    };

    // Phone layout: a full-width sheet on the bottom edge instead of a
    // pop-up by the pointer, and submenus open in place (no hover on touch).
    if sheet {
        let shown = sub().and_then(|index| open.items.get(index).cloned());
        return rsx! {
            div {
                class: "context-backdrop sheet-backdrop",
                onclick: move |_| close(),
                oncontextmenu: move |e| {
                    e.prevent_default();
                    close();
                },
                div {
                    class: "context-menu sheet",
                    role: "menu",
                    onclick: move |e| e.stop_propagation(),
                    div { class: "sheet-handle", "aria-hidden": "true" }
                    if let Some(item) = shown {
                        button {
                            class: "context-item sheet-back",
                            onclick: move |_| sub.set(None),
                            Icon { glyph: crate::icons::CHEVRON_LEFT, size: 18, class: "menu-icon" }
                            span { class: "menu-label", "{item.label}" }
                        }
                        div { class: if item.grid { "sheet-items grid" } else { "sheet-items" },
                            {entries(&item.children, close)}
                        }
                    } else {
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
                        div { class: "sheet-items",
                            for (index, item) in open.items.iter().cloned().enumerate() {
                                if item.children.is_empty() {
                                    {entries(std::slice::from_ref(&item), close)}
                                } else {
                                    button {
                                        class: "context-item has-submenu",
                                        role: "menuitem",
                                        onclick: move |_| sub.set(Some(index)),
                                        if let Some(glyph) = item.glyph() {
                                            Icon { glyph, size: 16, class: "menu-icon" }
                                        }
                                        span { class: "menu-label", "{item.label}" }
                                        span { class: "submenu-arrow", "›" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        };
    }

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
                    document::eval(SUBMENU_JS);
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
                    class: if item.glyph().is_none() && item.avatar.is_none() { "no-icon" },
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
                    if let Some((media, fallback)) = item.avatar.clone() {
                        span { class: "menu-avatar", Avatar { media, fallback, size: 20 } }
                    } else if let Some(glyph) = item.glyph() {
                        Icon { glyph, size: 16, class: "menu-icon" }
                    }
                    span { class: "menu-label", "{item.label}" }
                }
            } else {
                div {
                    class: "context-item has-submenu",
                    role: "menuitem",
                    "aria-haspopup": "true",
                    if let Some(glyph) = item.glyph() {
                        Icon { glyph, size: 16, class: "menu-icon" }
                    }
                    span { class: "menu-label", "{item.label}" }
                    span { class: "submenu-arrow", "›" }
                    div { class: "context-menu context-submenu", class: if item.grid { "grid" }, role: "menu",
                        {entries(&item.children, close)}
                    }
                }
            }
        }
    }
}

/// Icons by label, so every menu gets glyphs without each builder naming
/// them. Prefix matches cover labels with names in them ("Kick carol").
fn default_icon(label: &str) -> Option<&'static str> {
    use crate::icons::*;
    let table: &[(&str, &str)] = &[
        ("Add reaction", SMILE_PLUS),
        ("View reactions", EYE),
        ("Remove reactions", CIRCLE_MINUS),
        ("Remove all my reactions", CIRCLE_MINUS),
        ("Reply in thread", MESSAGES_SQUARE),
        ("Reply", REPLY),
        ("Edit message", PENCIL),
        ("Edit room", PENCIL),
        ("Edit profile", PENCIL),
        ("Copy text", COPY),
        ("Copy message link", LINK),
        ("Copy link", LINK),
        ("Copy message ID", HASH),
        ("Copy room ID", HASH),
        ("Copy user ID", HASH),
        ("Forward to", FORWARD),
        ("Unpin message", PIN_OFF),
        ("Pin message", PIN),
        ("Mark unread", MAIL),
        ("Mark as read", CHECK_CHECK),
        ("Report message", FLAG),
        ("Delete message", TRASH_2),
        ("View profile", USER),
        ("Profile", USER),
        ("Mention", AT_SIGN),
        ("Set nickname…", PENCIL),
        ("Add friend", USER_PLUS),
        ("Remove friend", USER_X),
        ("Message", MESSAGE_CIRCLE),
        ("Invite to room", USER_PLUS),
        ("Invite people", USER_PLUS),
        ("Unblock", BAN),
        ("Block", BAN),
        ("Roles", SHIELD),
        ("Kick", USER_X),
        ("Ban", GAVEL),
        ("Notifications", BELL),
        ("Create room", PLUS),
        ("Leave room", LOG_OUT),
        ("Set status", SMILE),
        ("Switch account", USERS),
        ("Sign out", LOG_OUT),
        ("All messages", BELL),
        ("Only mentions", AT_SIGN),
        ("Mute", BELL_OFF),
        ("Member", USER),
        ("Moderator", SHIELD),
        ("Admin", SHIELD_CHECK),
    ];
    table
        .iter()
        .find(|(prefix, _)| label.starts_with(prefix))
        .map(|(_, glyph)| *glyph)
}

/// Opens submenus as the pointer reaches their row, and keeps one open
/// while the pointer heads for it: crossing the rows below on the way (the
/// "safe triangle", from where the pointer was to the flyout's near edge)
/// waits a moment before switching, instead of closing it at once.
/// Installed once; flyouts are kept on screen as they open.
const SUBMENU_JS: &str = r#"
(() => {
  if (window.__morpheusSubmenus) return;
  window.__morpheusSubmenus = true;
  let last = null, prev = null, timer = null;
  const openRow = () => document.querySelector('.context-item.has-submenu.open');
  const clamp = row => {
    const sub = row.querySelector(':scope > .context-submenu'); if (!sub) return;
    sub.style.top = ''; sub.style.bottom = ''; sub.style.left = ''; sub.style.right = '';
    const r = sub.getBoundingClientRect();
    if (r.bottom > innerHeight - 8) { sub.style.top = 'auto'; sub.style.bottom = '-6px'; }
    if (r.right > innerWidth - 8) { sub.style.left = 'auto'; sub.style.right = '100%'; sub.style.marginRight = '2px'; }
  };
  const open = row => {
    const current = openRow();
    if (current === row) return;
    current?.classList.remove('open');
    row.classList.add('open');
    clamp(row);
  };
  const close = () => openRow()?.classList.remove('open');
  // Is `p` inside the triangle from `from` to the flyout's near edge?
  const heading = (from, p, sub) => {
    if (!from || !sub) return false;
    const r = sub.getBoundingClientRect();
    const x = r.left >= from.x ? r.left : r.right;
    const b = { x, y: r.top - 16 }, c = { x, y: r.bottom + 16 };
    const side = (p1, p2, p3) => (p1.x - p3.x) * (p2.y - p3.y) - (p2.x - p3.x) * (p1.y - p3.y);
    const d1 = side(p, from, b), d2 = side(p, b, c), d3 = side(p, c, from);
    return !((d1 < 0 || d2 < 0 || d3 < 0) && (d1 > 0 || d2 > 0 || d3 > 0));
  };
  document.addEventListener('mousemove', e => {
    const p = { x: e.clientX, y: e.clientY };
    if (!last || last.x !== p.x || last.y !== p.y) { prev = last; last = p; }
  }, true);
  document.addEventListener('mouseover', e => {
    const item = e.target.closest && e.target.closest('.context-menu .context-item');
    if (!item) return;
    const p = { x: e.clientX, y: e.clientY };
    const from = last && last.x === p.x && last.y === p.y ? prev : last;
    const current = openRow();
    clearTimeout(timer);
    // In the open row or its flyout: nothing changes.
    if (current && current.contains(item)) return;
    const next = item.classList.contains('has-submenu') ? item : null;
    const switchTo = () => { if (next) open(next); else if (current && item.parentElement === current.parentElement) close(); };
    if (current && item.parentElement === current.parentElement
        && heading(from, p, current.querySelector(':scope > .context-submenu'))) {
      timer = setTimeout(switchTo, 300);
      return;
    }
    switchTo();
  }, true);
})();
"#;
