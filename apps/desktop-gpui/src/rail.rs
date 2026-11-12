//! The rail (`crates/ui/src/components/rail.rs`): direct messages, rooms,
//! favorites, then a badge per server (or folder of servers). A bar beside
//! the scope you are in, a dot beside one with something unread.
//! Not ported yet: dragging badges, the server and folder menus, the
//! create-a-server dialog.

use std::rc::Rc;

use gpui::{
    Context, Div, IntoElement, MouseButton, Pixels, Point, Stateful, div, prelude::*, px, rgb, rgba, svg,
};
use morpheus_core::RoomSummary;

use crate::{
    view_model::room_dialogs::RoomDialogKind,
    e2e::E2eExt as _,
    icons::icon,
    media::avatar,
    model::{Scope, layout, scope_has_unread, top_spaces},
    prefs::RailEntry,
    shell::Shell,
    theme,
};

/// A badge: round-cornered square, the bar `pill` at its left edge.
struct Badge {
    id: String,
    title: String,
    classes: String,
    size: f32,
    selected: bool,
    unread: bool,
    extra: Vec<(&'static str, String)>,
    /// A right click, where.
    context: Option<Rc<dyn Fn(Point<Pixels>, &mut gpui::Window, &mut gpui::App)>>,
}

impl Badge {
    fn new(id: String, title: &str, base: &str, size: f32, selected: bool, unread: bool) -> Self {
        let mut classes = format!("rail-item {base}");
        if selected {
            classes.push_str(" selected");
        } else if unread {
            classes.push_str(" unread");
        }
        Self {
            id,
            title: title.to_owned(),
            classes,
            size,
            selected,
            unread: unread && !selected,
            extra: Vec::new(),
            context: None,
        }
    }

    /// The badge as a button holding `content`, which runs `on_click`.
    fn build(
        self,
        content: impl IntoElement,
        folder: bool,
        on_click: impl Fn(&mut gpui::Window, &mut gpui::App) + 'static,
    ) -> impl IntoElement {
        let pill_height = if self.selected {
            40.0
        } else if self.unread {
            8.0
        } else {
            0.0
        };
        let (bg, fg) = match (folder, self.selected) {
            (_, true) => (theme::accent_strong(), theme::text()),
            (true, false) => (theme::alpha(theme::accent(), 0x1f), theme::accent()),
            (false, false) => (theme::bg_elevated(), theme::text_muted()),
        };
        let mut button: Stateful<Div> = div()
            .id(gpui::SharedString::from(self.id.clone()))
            .relative()
            .size(px(self.size))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(theme::RADIUS_XL))
            .cursor_pointer()
            .text_size(px(1.1 * theme::REM))
            .font_weight(gpui::FontWeight::BOLD)
            .text_color(rgb(fg))
            .when(folder, |d| d.bg(rgba(bg)))
            .when(!folder, |d| d.bg(rgb(bg)))
            .hover(move |s| {
                if self.selected {
                    s
                } else if folder {
                    s.bg(rgba(theme::alpha(theme::accent(), 0x33)))
                } else {
                    s.bg(rgb(theme::accent_strong())).text_color(rgb(theme::text()))
                }
            })
            .on_click(move |_, window, cx| on_click(window, cx));
        if let Some(context) = self.context.clone() {
            button = button.on_mouse_down(MouseButton::Right, move |event, window, cx| {
                context(event.position, window, cx)
            });
        }
        button = button
            .child(
                div()
                    .absolute()
                    .left(px(-12.0))
                    .top(px((self.size - pill_height) / 2.0))
                    .w(px(4.0))
                    .h(px(pill_height))
                    .rounded_r(px(4.0))
                    .bg(rgb(0xffffff))
                    .e2e("span", "rail-pill")
                    .attr("data-bg", "#ffffff"),
            )
            .child(content);
        let mut tagged = button
            .e2e("button", &self.classes)
            .attr("title", &self.title);
        tagged = tagged.attr("data-radius", theme::RADIUS_XL.to_string());
        for (name, value) in self.extra {
            tagged = tagged.attr(name, value);
        }
        tagged
    }
}

pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement {
    let rooms = &shell.rooms;
    let current = &shell.scope;
    let unread_in = |scope: &Scope| scope_has_unread(rooms, scope);
    let unread_dms = rooms
        .iter()
        .filter(|r| r.invited || (r.is_dm && r.highlight_count > 0))
        .count();
    let has_favorites = rooms.iter().any(|r| r.favorite && !r.invited);
    let spaces = top_spaces(rooms);
    let by_id = |id: &morpheus_core::OwnedRoomId| spaces.iter().find(|s| &s.id == id).cloned();

    let mut rail = div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(8.0))
        .w(px(theme::RAIL_W))
        .flex_none()
        .h_full()
        .pt(px(12.0))
        .pb(px(8.0))
        .bg(rgb(theme::bg_rail()));

    // Direct messages: the logo; its count of unread conversations.
    let dms_selected = *current == Scope::Dms;
    let logo_color = if dms_selected { 0xffffff } else { theme::text() };
    let dms = Badge::new(
        "rail-dms".into(),
        "Direct messages",
        "rail-dms",
        48.0,
        dms_selected,
        !dms_selected && (unread_dms > 0 || unread_in(&Scope::Dms)),
    );
    let mut dms_content = div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(
            svg()
                .path("logo.svg")
                .size(px(40.0))
                .text_color(rgb(logo_color))
                .e2e("svg", "logo"),
        );
    if unread_dms > 0 {
        dms_content = dms_content.child(
            div()
                .absolute()
                .right(px(4.0))
                .bottom(px(4.0))
                .min_w(px(16.0))
                .h(px(16.0))
                .px(px(4.0))
                .rounded(px(8.0))
                .bg(rgb(theme::danger()))
                .text_color(rgb(0xffffff))
                .text_size(px(0.65 * theme::REM))
                .child(unread_dms.to_string())
                .e2e("span", "rail-badge")
                .text(unread_dms.to_string()),
        );
    }
    rail = rail.child(dms.build(dms_content, false, {
        let view = cx.entity();
        move |_, cx| view.update(cx, |s, cx| s.set_scope(Scope::Dms, cx))
    }));

    let rooms_selected = *current == Scope::Rooms;
    rail = rail.child(
        Badge::new(
            "rail-rooms".into(),
            "Rooms",
            "rail-rooms",
            48.0,
            rooms_selected,
            !rooms_selected && unread_in(&Scope::Rooms),
        )
        .build(div().child("#").e2e("span", "").text("#"), false, {
            let view = cx.entity();
            move |_, cx| view.update(cx, |s, cx| s.set_scope(Scope::Rooms, cx))
        }),
    );

    if has_favorites {
        let selected = *current == Scope::Favorites;
        rail = rail.child(
            Badge::new(
                "rail-favorites".into(),
                "Favorites",
                "rail-favorites",
                48.0,
                selected,
                !selected && unread_in(&Scope::Favorites),
            )
            .build(icon("star", 20.0), false, {
                let view = cx.entity();
                move |_, cx| view.update(cx, |s, cx| s.set_scope(Scope::Favorites, cx))
            }),
        );
    }

    rail = rail.child(
        div()
            .w(px(32.0))
            .h(px(2.0))
            .rounded(px(1.0))
            .bg(rgb(theme::bg_elevated()))
            .e2e("div", "rail-divider"),
    );

    for entry in layout(&shell.prefs, &spaces) {
        match entry {
            RailEntry::Space(id) => {
                if let Some(space) = by_id(&id) {
                    let selected = current.space_id() == Some(&id);
                    rail = rail.child(space_badge(
                        shell,
                        cx,
                        &space,
                        48.0,
                        selected,
                        unread_in(&Scope::Space(id)),
                    ));
                }
            }
            RailEntry::Folder {
                id,
                name,
                spaces: members,
            } => {
                let open = shell.open_folders.contains(&id);
                let members: Vec<RoomSummary> = members.iter().filter_map(by_id).collect();
                let folder_unread = members
                    .iter()
                    .any(|s| unread_in(&Scope::Space(s.id.clone())));
                let contains_current = members.iter().any(|s| current.space_id() == Some(&s.id));
                let mut classes = String::from("rail-folder");
                if open {
                    classes.push_str(" open");
                }
                if contains_current {
                    classes.push_str(" has-current");
                }
                let content = if open {
                    div().child(icon("folder", 20.0)).into_any_element()
                } else {
                    let mut grid = div().flex().flex_wrap().w(px(39.0)).gap(px(3.0));
                    for space in members.iter().take(4) {
                        let picture = shell.picture(&space.avatar, 18);
                        grid = grid.child(avatar(picture, &space.name, 18.0, None));
                    }
                    grid.e2e("div", "folder-grid").into_any_element()
                };
                let badge = Badge::new(
                    format!("folder-{id}"),
                    &name,
                    "folder-badge",
                    48.0,
                    contains_current && !open,
                    !contains_current && folder_unread,
                )
                .build(content, true, {
                    let (view, id) = (cx.entity(), id.clone());
                    move |_, cx| {
                        view.update(cx, |s, cx| {
                            if let Some(at) = s.open_folders.iter().position(|f| *f == id) {
                                s.open_folders.remove(at);
                            } else {
                                s.open_folders.push(id.clone());
                            }
                            cx.notify();
                        })
                    }
                });
                let mut folder = div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.0))
                    .rounded(px(theme::RADIUS_XL))
                    .when(open, |d| d.bg(rgba(theme::bg_hover())).pb(px(8.0)))
                    .child(badge);
                if open {
                    let mut items = div().flex().flex_col().gap(px(8.0));
                    for space in &members {
                        let selected = current.space_id() == Some(&space.id);
                        items = items.child(space_badge(
                            shell,
                            cx,
                            space,
                            40.0,
                            selected,
                            unread_in(&Scope::Space(space.id.clone())),
                        ));
                    }
                    folder = folder.child(items.e2e("div", "folder-items"));
                }
                rail = rail.child(folder.e2e("div", &classes).attr("data-folder", id));
            }
        }
    }

    rail.child(
        div()
            .id("rail-add")
            .size(px(48.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(theme::RADIUS_XL))
            .border_2()
            .border_dashed()
            .border_color(rgb(theme::bg_elevated()))
            .text_color(rgb(theme::text_faint()))
            .cursor_pointer()
            .hover(|s| {
                s.border_color(rgb(theme::accent_strong()))
                    .text_color(rgb(theme::accent()))
            })
            .child(icon("plus", 22.0).text_color(rgb(theme::text_faint())))
            .on_click(cx.listener(|this, _, _, cx| {
                this.open_room_dialog(RoomDialogKind::CreateSpace(None), cx)
            }))
            .e2e("button", "rail-item rail-add")
            .attr("title", "Create a server"),
    )
    .e2e("nav", "rail")
}

fn space_badge(
    shell: &Shell,
    cx: &mut Context<Shell>,
    space: &RoomSummary,
    size: f32,
    selected: bool,
    unread: bool,
) -> impl IntoElement {
    let picture = shell.picture(&space.avatar, if size > 40.0 { 48 } else { 40 });
    let mut badge = Badge::new(
        format!("space-{}", space.id),
        &space.name,
        "space-badge",
        size,
        selected,
        unread,
    );
    badge.extra.push(("data-space", space.id.to_string()));
    let id = space.id.clone();
    let view = cx.entity();
    // Right click: the server's menu.
    badge.context = Some({
        let (view, id) = (view.clone(), id.clone());
        Rc::new(move |at, _, cx| view.update(cx, |s, cx| s.open_space_menu(&id, at, cx)))
    });
    badge.build(
        avatar(picture, &space.name, size, Some(theme::RADIUS_XL)),
        false,
        move |_, cx| view.update(cx, |s, cx| s.set_scope(Scope::Space(id.clone()), cx)),
    )
}
