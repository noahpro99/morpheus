//! The right-click menu (`crates/ui/src/components/context_menu.rs`): a
//! styled panel at the pointer over a backdrop that closes it, quick
//! reactions on top for a message, rows with an icon, flyouts opened by
//! hovering a row, Escape and a click outside to close. What it offers is
//! `view_model::menu`; this draws it and hands the chosen action to the
//! shell (`Shell::run_menu_action`).

use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyElement, ClickEvent, Context, FocusHandle, FontWeight, IntoElement, MouseButton, Pixels, Point,
    ScrollHandle, Window, anchored, canvas, deferred, div, prelude::*, px, relative, rgb, rgba,
};

use crate::{
    e2e::{E2eExt as _, HandleProbe},
    icons::icon,
    shell::Shell,
    theme,
    view_model::menu::{Frame, MenuAction, MenuItem, MenuTarget, QUICK_REACTIONS, place_flyout},
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
    /// Where the open row was drawn last frame, and the size its flyout had (with
    /// the row it belongs to), so the flyout can be placed inside the window.
    pub row_frame: Rc<Cell<gpui::Bounds<Pixels>>>,
    pub flyout_size: Rc<Cell<Option<(usize, gpui::Size<Pixels>)>>>,
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
            row_frame: Rc::default(),
            flyout_size: Rc::default(),
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
    fn menu_pick(&mut self, path: (usize, Option<usize>), shift: bool, cx: &mut Context<Self>) {
        let Some(menu) = &mut self.menu else { return };
        let Some(item) = menu.item(path) else { return };
        match item.action.clone() {
            Some(action) if !item.has_submenu() => {
                let menu = self.menu.take().expect("open");
                self.menu_at = menu.at;
                cx.notify();
                match (action, menu.target) {
                    // Shift held: delete without asking, as the bin does.
                    (MenuAction::Delete, MenuTarget::Message { message, .. }) if shift => {
                        self.delete_message(*message, true, cx)
                    }
                    (action, target) => self.run_menu_action(action, target, cx),
                }
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
        .bg(rgb(theme::bg_rail()))
        .border_1()
        .border_color(rgba(theme::edge()))
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
                .border_color(rgb(theme::bg_elevated()))
                .children(QUICK_REACTIONS.into_iter().map(|key| {
                    div()
                        .id(gpui::ElementId::from(gpui::SharedString::from(format!("react-{key}"))))
                        .px(px(8.0))
                        .py(px(4.0))
                        .rounded(px(theme::RADIUS_SM))
                        .text_size(px(1.1 * theme::REM))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgba(theme::bg_selected())))
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
    if let Some(slider) = &item.slider {
        return menu_slider_el(row, &item.label, slider, cx);
    }
    let path = (row, sub);
    let hovered = menu.hover == Some(path);
    let glyph = item.glyph();
    let colour = if item.danger {
        theme::danger()
    } else {
        theme::text_muted()
    };
    let in_grid = sub.is_some() && menu.items.get(row).is_some_and(|parent| parent.grid);
    let lit = hovered || (sub.is_none() && menu.open == Some(row));
    let highlight = if item.danger {
        theme::danger()
    } else {
        theme::accent_strong()
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
        .when(sub.is_none(), |d| d.w_full())
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
        .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
            this.menu_pick(path, event.modifiers().shift, cx)
        }));
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
            theme::text_faint()
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
        el = el.child(div().text_color(rgb(theme::accent())).child("✓"));
    }
    if item.has_submenu() {
        el = el.child(div().ml(px(12.0)).child("›").text_color(rgb(if lit {
            0xffffff
        } else {
            theme::text_faint()
        })));
        if menu.open == Some(row) {
            let cell = menu.row_frame.clone();
            el = el.child(
                canvas(
                    move |bounds, window, _| {
                        if cell.get() != bounds {
                            cell.set(bounds);
                            window.refresh();
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            );
            el = el.child(flyout(menu, row, item, width, height, cx));
        }
    }
    el.e2e(if item.has_submenu() { "div" } else { "button" }, &classes)
        .attr("role", "menuitem")
        .attr("data-icon", glyph.unwrap_or(""))
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
    // Placed from where the row and the flyout were last drawn (a guess until then).
    let guess = gpui::size(
        px(if item.grid { 236.0 } else { 188.0 }),
        px((item.children.len() as f32 * 35.0 + 14.0).min(0.6 * height)),
    );
    let measured = menu.flyout_size.get().filter(|(r, _)| *r == row).map(|(_, s)| s);
    let size = measured.unwrap_or(guess);
    let frame = menu.row_frame.get();
    let (x, y) = place_flyout(
        Frame {
            x: f32::from(frame.origin.x),
            y: f32::from(frame.origin.y),
            w: f32::from(frame.size.width),
            h: f32::from(frame.size.height),
        },
        (f32::from(size.width), f32::from(size.height)),
        (width, height),
        8.0,
    );
    let (dx, dy) = (x - f32::from(frame.origin.x), y - f32::from(frame.origin.y));
    let size_cell = menu.flyout_size.clone();
    let mut list = div()
        .id("context-flyout")
        .absolute()
        .left(px(dx))
        .top(px(dy))
        .flex()
        .gap(px(2.0))
        .min_w(px(188.0))
        .max_h(px(0.6 * height))
        .p(px(6.0))
        .rounded(px(theme::RADIUS_LG))
        .bg(rgb(theme::bg_rail()))
        .border_1()
        .border_color(rgba(theme::edge()))
        .shadow_lg()
        .overflow_y_scroll()
        .track_scroll(&menu.flyout_scroll)
        .occlude()
        .font_weight(FontWeight::NORMAL);
    // Hidden until measured, then placed on the next frame.
    if measured.is_none() {
        list = list.opacity(0.0);
    }
    list = list.child(
        canvas(
            move |bounds, window, _| {
                let now = Some((row, bounds.size));
                if size_cell.get().is_none_or(|(r, s)| r != row || (s.width - bounds.size.width).abs() > px(0.5) || (s.height - bounds.size.height).abs() > px(0.5)) {
                    size_cell.set(now);
                    window.refresh();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0(),
    );
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

/// An interactive volume slider row in a menu.
fn menu_slider_el(
    row: usize,
    label: &str,
    slider: &crate::view_model::menu::MenuSlider,
    cx: &mut Context<Shell>,
) -> AnyElement {
    use std::rc::Rc;
    let max = morpheus_call::MAX_VOLUME;
    let volume = slider.value();
    let percent = (volume * 100.0).round() as u32;
    let bar: Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>> = Rc::default();
    let set_at = {
        let bar = bar.clone();
        move |x: gpui::Pixels| -> Option<f32> {
            let bounds = bar.get();
            let width = f32::from(bounds.size.width);
            (width > 0.0).then(|| (f32::from(x - bounds.origin.x) / width).clamp(0.0, 1.0) * max)
        }
    };
    let (down_set, move_set) = (set_at.clone(), set_at);
    let down_kind = slider.clone();
    let move_kind = slider.clone();

    let path = (row, None);
    let id_prefix = match slider {
        crate::view_model::menu::MenuSlider::InputVolume(_) => "input-volume",
        crate::view_model::menu::MenuSlider::OutputVolume(_) => "output-volume",
        crate::view_model::menu::MenuSlider::UserVolume(..) => "user-volume",
    };

    let label_str = label.to_owned();
    div()
        .id(gpui::ElementId::from(gpui::SharedString::from(format!("ctx-slider-{row}"))))
        .relative()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .w_full()
        .px(px(10.0))
        .py(px(6.0))
        .rounded(px(theme::RADIUS_SM))
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            if *hovered {
                this.menu_hover(path, cx);
            }
        }))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .text_size(px(0.8 * theme::REM))
                .text_color(rgb(theme::text_muted()))
                .child(label_str)
                .child(div().child(format!("{percent}%")).e2e("span", "slider-percent")),
        )
        .child(
            div()
                .id(gpui::ElementId::from(gpui::SharedString::from(format!("ctx-bar-{row}"))))
                .relative()
                .w_full()
                .h(px(14.0))
                .flex()
                .items_center()
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        if let Some(vol) = (if event.click_count >= 2 { Some(1.0) } else { down_set(event.position.x) }) {
                            match &down_kind {
                                crate::view_model::menu::MenuSlider::InputVolume(_) => {
                                    this.set_input_volume(vol, cx);
                                }
                                crate::view_model::menu::MenuSlider::OutputVolume(_) => {
                                    this.set_output_volume(vol, cx);
                                }
                                crate::view_model::menu::MenuSlider::UserVolume(user, _) => {
                                    this.set_user_volume(user, vol, cx);
                                }
                            }
                        }
                    }),
                )
                .on_mouse_move(cx.listener(move |this, event: &gpui::MouseMoveEvent, _, cx| {
                    if event.pressed_button == Some(MouseButton::Left)
                        && let Some(vol) = move_set(event.position.x)
                    {
                        match &move_kind {
                            crate::view_model::menu::MenuSlider::InputVolume(_) => {
                                this.set_input_volume(vol, cx);
                            }
                            crate::view_model::menu::MenuSlider::OutputVolume(_) => {
                                this.set_output_volume(vol, cx);
                            }
                            crate::view_model::menu::MenuSlider::UserVolume(user, _) => {
                                this.set_user_volume(user, vol, cx);
                            }
                        }
                    }
                }))
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(canvas(move |bounds, _, _| bar.set(bounds), |_, _, _, _| {}).absolute().inset_0())
                .child(
                    div()
                        .relative()
                        .w_full()
                        .h(px(4.0))
                        .rounded_full()
                        .bg(rgba(theme::bg_selected()))
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .h_full()
                                .w(gpui::relative((volume / max).clamp(0.0, 1.0)))
                                .rounded_full()
                                .bg(rgb(theme::accent())),
                        )
                        .child(slider_handle(volume / max, 4.0)),
                )
                .e2e("div", &format!("menu-slider {id_prefix}"))
                .attr("data-value", percent.to_string()),
        )
        .e2e("div", &format!("context-slider {id_prefix}"))
        .into_any_element()
}

/// The round knob on a volume slider's track, `fraction` of the way along;
/// `top` lifts it so it is centred on the track.
pub(crate) fn slider_handle(fraction: f32, top: f32) -> impl IntoElement {
    div()
        .absolute()
        .top(px(-top))
        .left(gpui::relative(fraction.clamp(0.0, 1.0)))
        .ml(px(-6.0))
        .size(px(12.0))
        .rounded_full()
        .bg(rgb(0xffffff))
        .border_2()
        .border_color(rgb(theme::accent()))
        .e2e("div", "slider-handle")
}
