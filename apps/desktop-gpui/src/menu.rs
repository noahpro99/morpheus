//! The right-click menu (`crates/ui/src/components/context_menu.rs`): a
//! styled panel at the pointer over a backdrop that closes it, quick
//! reactions on top for a message, rows with an icon, flyouts opened by
//! hovering a row, Escape and a click outside to close. What it offers is
//! `view_model::menu`; this draws it and hands the chosen action to the
//! shell (`Shell::run_menu_action`).

use gpui::{
    AnyElement, Context, FocusHandle, FontWeight, IntoElement, MouseButton, Pixels, Point,
    ScrollHandle, Window, anchored, deferred, div, prelude::*, px, relative, rgb, rgba,
};

use crate::{
    e2e::{E2eExt as _, HandleProbe},
    icons::icon,
    shell::Shell,
    theme,
    view_model::menu::{MenuAction, MenuItem, MenuTarget, QUICK_REACTIONS},
};

/// The open menu.
pub struct OpenMenu {
    pub at: Point<Pixels>,
    pub items: Vec<MenuItem>,
    /// Quick reactions on top (messages that were sent).
    pub react: bool,
    pub target: MenuTarget,
    /// The row whose flyout is shown.
    pub open: Option<usize>,
    /// The row under the pointer: (row, row in its flyout).
    pub hover: Option<(usize, Option<usize>)>,
    /// Focus the menu on the next frame, so Escape reaches it.
    pub take_focus: bool,
    /// A long flyout scrolls.
    pub flyout_scroll: ScrollHandle,
}

impl OpenMenu {
    pub fn new(at: Point<Pixels>, items: Vec<MenuItem>, react: bool, target: MenuTarget) -> Self {
        Self {
            at,
            items,
            react,
            target,
            open: None,
            hover: None,
            take_focus: true,
            flyout_scroll: ScrollHandle::new(),
        }
    }

    /// The item a path (row, row in its flyout) names.
    pub fn item(&self, (row, sub): (usize, Option<usize>)) -> Option<&MenuItem> {
        let item = self.items.get(row)?;
        match sub {
            None => Some(item),
            Some(sub) => item.children.get(sub),
        }
    }
}

impl Shell {
    pub fn open_menu(&mut self, menu: OpenMenu, cx: &mut Context<Self>) {
        self.menu = Some(menu);
        cx.notify();
    }

    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            cx.notify();
        }
    }

    /// A row was chosen: an item with an action runs it and closes the menu;
    /// a row with a flyout opens it.
    fn menu_pick(&mut self, path: (usize, Option<usize>), cx: &mut Context<Self>) {
        let Some(menu) = &mut self.menu else { return };
        let Some(item) = menu.item(path) else { return };
        match item.action.clone() {
            Some(action) if !item.has_submenu() => {
                let menu = self.menu.take().expect("open");
                cx.notify();
                self.run_menu_action(action, menu.target, cx);
            }
            _ => {
                menu.open = Some(path.0);
                cx.notify();
            }
        }
    }

    fn menu_react(&mut self, key: &'static str, cx: &mut Context<Self>) {
        if let Some(menu) = self.menu.take() {
            cx.notify();
            self.run_menu_action(MenuAction::React(key.to_owned()), menu.target, cx);
        }
    }

    fn menu_hover(&mut self, path: (usize, Option<usize>), cx: &mut Context<Self>) {
        let Some(menu) = &mut self.menu else { return };
        if menu.hover == Some(path) {
            return;
        }
        menu.hover = Some(path);
        if path.1.is_none() {
            // Another row of the menu: its flyout (if any) replaces the last.
            menu.open = menu
                .items
                .get(path.0)
                .filter(|item| item.has_submenu())
                .map(|_| path.0);
        }
        cx.notify();
    }
}

/// The menu over the window, if one is open.
pub fn render(shell: &mut Shell, window: &mut Window, cx: &mut Context<Shell>) -> Option<AnyElement> {
    shell.menu.as_ref()?;
    let focus = shell.menu_focus(cx);
    let viewport = window.viewport_size();
    let menu = shell.menu.as_mut()?;
    if std::mem::take(&mut menu.take_focus) {
        window.focus(&focus);
    }
    let at = menu.at;
    let panel = panel(menu, &focus, f32::from(viewport.width), f32::from(viewport.height), cx, false);
    let backdrop = div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_menu(cx)))
        .on_mouse_down(MouseButton::Right, cx.listener(|this, _, _, cx| this.close_menu(cx)))
        .child(
            anchored()
                .position(at)
                .snap_to_window_with_margin(px(8.0))
                .child(panel),
        )
        .e2e("div", "context-backdrop");
    Some(deferred(backdrop).with_priority(30).into_any_element())
}

/// The panel: the quick-reaction row and the items (or, for a flyout, its items).
fn panel(
    menu: &OpenMenu,
    focus: &FocusHandle,
    width: f32,
    height: f32,
    cx: &mut Context<Shell>,
    _flyout: bool,
) -> AnyElement {
    let mut column = div()
        .id("context-menu")
        .track_focus(focus)
        .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
            if event.keystroke.key == "escape" {
                this.close_menu(cx);
            }
        }))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
        .occlude()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .min_w(px(188.0))
        .p(px(6.0))
        .rounded(px(theme::RADIUS_LG))
        .bg(rgb(theme::BG_RAIL))
        .border_1()
        .border_color(rgba(0xffffff0f))
        .shadow_lg()
        .text_size(px(0.9 * theme::REM))
        .line_height(relative(theme::LINE_HEIGHT));
    if menu.react {
        column = column.child(
            div()
                .flex()
                .gap(px(4.0))
                .px(px(4.0))
                .pt(px(2.0))
                .pb(px(6.0))
                .mb(px(4.0))
                .border_b_1()
                .border_color(rgb(theme::BG_ELEVATED))
                .children(QUICK_REACTIONS.into_iter().map(|key| {
                    div()
                        .id(gpui::ElementId::from(gpui::SharedString::from(format!("react-{key}"))))
                        .px(px(8.0))
                        .py(px(4.0))
                        .rounded(px(theme::RADIUS_SM))
                        .text_size(px(1.1 * theme::REM))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgba(theme::BG_SELECTED)))
                        .child(key)
                        .on_click(cx.listener(move |this, _, _, cx| this.menu_react(key, cx)))
                        .e2e("button", "context-reaction")
                        .attr("title", format!("React with {key}"))
                        .text(key)
                }))
                .e2e("div", "context-reactions"),
        );
    }
    for (row, item) in menu.items.iter().enumerate() {
        column = column.child(item_el(menu, row, None, item, width, height, cx));
    }
    column
        .e2e("div", "context-menu")
        .attr("role", "menu")
        .focusable(focus)
        .into_any_element()
}

/// One row; a row with children draws its flyout beside it while open.
fn item_el(
    menu: &OpenMenu,
    row: usize,
    sub: Option<usize>,
    item: &MenuItem,
    width: f32,
    height: f32,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let path = (row, sub);
    let hovered = menu.hover == Some(path);
    let glyph = item.glyph();
    let colour = if item.danger {
        theme::DANGER
    } else {
        theme::TEXT_MUTED
    };
    let in_grid = sub.is_some() && menu.items.get(row).is_some_and(|parent| parent.grid);
    let lit = hovered || (sub.is_none() && menu.open == Some(row));
    let highlight = if item.danger {
        theme::DANGER
    } else {
        theme::ACCENT_STRONG
    };
    let label = item.label.clone();
    let mut classes = String::from("context-item");
    if item.danger {
        classes.push_str(" danger");
    }
    if item.checked {
        classes.push_str(" checked");
    }
    if item.has_submenu() {
        classes.push_str(" has-submenu");
    }
    let iconless = glyph.is_none() && item.avatar.is_none();
    if iconless {
        classes.push_str(" no-icon");
    }
    let id = gpui::ElementId::from(gpui::SharedString::from(format!(
        "ctx-{row}-{}",
        sub.map_or(-1, |s| s as i64)
    )));
    let mut el = div()
        .id(id)
        .relative()
        .flex()
        .items_center()
        .gap(px(10.0))
        .w_full()
        .py(px(7.0))
        .rounded(px(theme::RADIUS_SM))
        .cursor_pointer()
        .text_color(rgb(if lit { 0xffffff } else { colour }))
        .when(lit, |d| d.bg(rgb(highlight)))
        .hover(|s| s.bg(rgb(highlight)).text_color(rgb(0xffffff)))
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            if *hovered {
                this.menu_hover(path, cx);
            }
        }))
        .on_click(cx.listener(move |this, _, _, cx| this.menu_pick(path, cx)));
    el = if in_grid {
        // Emoji in a grid: a square cell, centred.
        el.w(px(36.0)).px(px(0.0)).py(px(6.0)).justify_center().text_size(px(1.1 * theme::REM))
    } else if iconless {
        el.pl(px(36.0)).pr(px(10.0))
    } else {
        el.px(px(10.0))
    };
    if let Some(name) = &item.avatar {
        // A server's or room's picture (its letter, until pictures are fetched here).
        el = el.child(
            div()
                .size(px(20.0))
                .flex_none()
                .child(crate::media::avatar(None, name, 20.0, None).text(String::new()))
                .e2e("span", "menu-avatar"),
        );
    }
    if let Some(glyph) = glyph {
        el = el.child(icon(glyph, 16.0).text_color(rgb(if lit || hovered {
            0xffffff
        } else {
            theme::TEXT_FAINT
        })));
    }
    let text = div()
        .child(label.clone())
        .e2e("span", "menu-label")
        .text(label);
    el = if in_grid {
        el.child(text)
    } else {
        el.child(div().flex_1().child(text))
    };
    if item.checked {
        el = el.child(div().text_color(rgb(theme::ACCENT)).child("✓"));
    }
    if item.has_submenu() {
        el = el.child(div().ml(px(12.0)).child("›").text_color(rgb(if lit {
            0xffffff
        } else {
            theme::TEXT_FAINT
        })));
        if menu.open == Some(row) {
            el = el.child(flyout(menu, row, item, width, height, cx));
        }
    }
    el.e2e(if item.has_submenu() { "div" } else { "button" }, &classes)
        .attr("role", "menuitem")
        .into_any_element()
}

/// The flyout of row `row`, beside it (on the other side near the window's edge).
fn flyout(
    menu: &OpenMenu,
    row: usize,
    item: &MenuItem,
    width: f32,
    height: f32,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let flyout_width = if item.grid { 236.0 } else { 188.0 };
    let flip = f32::from(menu.at.x) + 188.0 + flyout_width + 12.0 > width;
    let mut list = div()
        .id("context-flyout")
        .absolute()
        .top(px(-6.0))
        .flex()
        .gap(px(2.0))
        .min_w(px(188.0))
        .max_h(px(0.6 * height))
        .p(px(6.0))
        .rounded(px(theme::RADIUS_LG))
        .bg(rgb(theme::BG_RAIL))
        .border_1()
        .border_color(rgba(0xffffff0f))
        .shadow_lg()
        .overflow_y_scroll()
        .track_scroll(&menu.flyout_scroll)
        .occlude()
        .font_weight(FontWeight::NORMAL);
    list = if flip {
        list.right(relative(1.0)).mr(px(2.0))
    } else {
        list.left(relative(1.0)).ml(px(2.0))
    };
    list = if item.grid {
        list.flex_row().flex_wrap().w(px(236.0))
    } else {
        list.flex_col()
    };
    for (sub, child) in item.children.iter().enumerate() {
        list = list.child(item_el(menu, row, Some(sub), child, width, height, cx));
    }
    list.e2e("div", "context-menu context-submenu")
        .attr("role", "menu")
        .scrollable(std::rc::Rc::new(HandleProbe(menu.flyout_scroll.clone())))
        .into_any_element()
}
