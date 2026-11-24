//! The sidebar (`crates/ui/src/components/room_list.rs`): a header with what
//! the rail selected, then the rooms of that scope. Inside a server its
//! channels follow the server's own order, nested servers become headed
//! categories, and the channels you are not in come last.
//! Not ported yet: invites, the Friends row, call participants, pins and
//! closed DMs, the room and server menus, drag to reorder, the create-room
//! dialog.

use crate::view_model::rail_drag;
use crate::view_model::i18n::t;
use std::collections::HashSet;

use gpui::{
    Animation, AnimationExt as _, AnyElement, Context, FontWeight, IntoElement, MouseButton, Stateful, Div, div,
    prelude::*, px, rgb,
};
use morpheus_core::{OwnedRoomId, RoomSummary, SpaceRoom};

pub use morpheus_view_model::scopes::Sections;

use crate::tooltip::TipExt as _;
use crate::{
    e2e::E2eExt as _,
    icons::icon,
    media::avatar,
    model::{Scope, header_title, rooms_in_scope},
    shell::Shell,
    theme,
    view_model::marks::Marks,
    view_model::room_dialogs::RoomDialogKind,
};

/// What decides which rooms the lists show besides the scope.
pub struct Filters<'a> {
    pub hide_muted: bool,
    pub open: Option<&'a OwnedRoomId>,
    pub marks: &'a Marks,
    /// Rooms just left, still undoable.
    pub hidden: &'a HashSet<String>,
}

pub fn sections(
    rooms: &[RoomSummary],
    scope: &Scope,
    directory: &[SpaceRoom],
    filters: &Filters,
) -> Sections {
    let open = filters.open;
    let (dms, channels): (Vec<_>, Vec<_>) = rooms_in_scope(rooms, scope)
        .into_iter()
        // "Hide muted channels" keeps the open room visible.
        .filter(|r| !filters.hide_muted || !r.muted || Some(&r.id) == open)
        .filter(|r| !filters.hidden.contains(r.id.as_str()))
        .partition(|r| r.is_dm);
    // Closed DMs stay out until a new message; pinned ones come first.
    let dms = filters.marks.visible_dms(dms, open, false);
    morpheus_view_model::scopes::categorise(rooms, scope, dms, channels, directory)
}

pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement {
    let scope = &shell.scope;
    let found = sections(
        &shell.rooms,
        scope,
        &shell.directory,
        &Filters {
            hide_muted: shell.prefs.hide_muted,
            open: shell.selected.as_ref(),
            marks: &shell.marks,
            hidden: &shell.hidden_rooms,
        },
    );
    // Who may reorder a server's channels can drag them (onto another one, or
    // into and out of its categories), and its categories by their headers.
    let reorder: Option<OwnedRoomId> = match scope {
        Scope::Space(space)
            if shell
                .rooms
                .iter()
                .any(|r| &r.id == space && r.can_edit_children) =>
        {
            Some(space.clone())
        }
        _ => None,
    };
    let in_space = matches!(scope, Scope::Space(_));
    // Inside a space (and favorites) both kinds can appear: label the groups.
    let grouped = in_space || *scope == Scope::Favorites;
    let title = header_title(&shell.rooms, scope);

    // The name is a button of its own size (not the header's), brightening on hover.
    let name_colour = if shell.bar_hover == Some("server-name") {
        theme::text()
    } else {
        dimmed(theme::text())
    };
    let mut name = div()
        .id("sidebar-title")
        .flex()
        .flex_none()
        .min_w_0()
        .h(px(28.0))
        .ml(px(8.0))
        .px(px(8.0))
        .items_center()
        .gap(px(4.0))
        .rounded(px(theme::RADIUS_SM))
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(name_colour))
        .child(div().min_w_0().truncate().child(title.clone()));
    // Clicking the server's name (or right-clicking it) opens the server menu.
    if let Scope::Space(space_id) = scope {
        let space_id = space_id.clone();
        name = name
            .cursor_pointer()
            .on_hover(crate::chat::bar_hover(cx, "server-name"))
            .on_click(cx.listener({
                let space_id = space_id.clone();
                move |this, event: &gpui::ClickEvent, _, cx| {
                    this.open_space_menu(&space_id, event.position(), cx)
                }
            }));
        name = name.on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                this.open_space_menu(&space_id, event.position, cx)
            }),
        );
    }
    if in_space {
        name = name.child(icon("chevron-down", 14.0).text_color(rgb(name_colour)));
    }
    let header = div()
        .w_full()
        .h(px(48.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .border_b_1()
        .border_color(gpui::rgba(0x0000004d))
        // The DMs have no heading: the command palette button is there (as
        // Discord's "Find or start a conversation").
        .when(*scope == Scope::Dms, |h| h.px(px(10.0)))
        .child(if *scope == Scope::Dms {
            palette_button(cx).into_any_element()
        } else {
            name.e2e("h2", if in_space { "server-name" } else { "" })
                .attr("data-color", format!("{name_colour:06x}"))
                .attr("data-bg", "")
                .text(title)
                .into_any_element()
        });
    let mut list = div()
        .id("room-list")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&shell.room_scroll)
        .p(px(8.0))
        .pb(px(4.0))
        .flex()
        .flex_col()
        .gap(px(2.0));
    if found.dms.is_empty()
        && found.channels.is_empty()
        && found.categories.iter().all(|c| c.2.is_empty())
    {
        let text = if *scope == Scope::Dms {
            "No direct messages yet."
        } else {
            "No rooms here."
        };
        list = list.child(
            div()
                .px(px(16.0))
                .py(px(12.0))
                .text_color(rgb(theme::text_muted()))
                .child(text)
                .e2e("p", "muted pad")
                .text(text),
        );
    }
    // Invites show at the top of the DMs list only (and in the inbox).
    let pending = if *scope == Scope::Dms {
        crate::view_model::invites::pending_invites(&shell.rooms)
    } else {
        Vec::new()
    };
    if !pending.is_empty() {
        list = list.child(section("Invites", "invites"));
        for room in &pending {
            list = list.child(crate::invites::invite_row(shell, cx, room));
        }
    }
    // The Friends page, at the top of the DMs (as in Discord).
    if *scope == Scope::Dms {
        list = list.child(friends_row(shell, cx));
    }
    if !found.dms.is_empty() {
        if grouped {
            list = list.child(section("Direct messages", ""));
        }
        for room in &found.dms {
            list = list.child(row(shell, cx, room, None, None));
        }
    }
    // The Rooms heading carries the create-room plus (#369), so it shows in every
    // scope but the DMs, with no rooms yet and above categories too.
    if *scope != Scope::Dms {
        list = list.child(rooms_heading(shell, cx));
    }
    if !found.channels.is_empty() {
        for (i, room) in found.channels.iter().enumerate() {
            let next = found.channels.get(i + 1).map(|r| &r.id);
            list = list.child(row(shell, cx, room, reorder.as_ref(), next));
        }
    }
    let shown: Vec<&OwnedRoomId> = found
        .categories
        .iter()
        .filter(|(_, _, members)| !members.is_empty() || reorder.is_some())
        .map(|(id, _, _)| id)
        .collect();
    for (id, name, members) in &found.categories {
        // A category you can edit shows when empty too: a channel can be dropped in.
        let can_edit = reorder.is_some()
            && shell.rooms.iter().any(|r| &r.id == id && r.can_edit_children);
        if members.is_empty() && reorder.is_none() {
            continue;
        }
        let next_header = shown.iter().position(|s| *s == id).and_then(|at| shown.get(at + 1)).copied();
        list = list.child(category_header(shell, cx, id, name, reorder.as_ref(), next_header));
        for (i, room) in members.iter().enumerate() {
            let next = members.get(i + 1).map(|r| &r.id);
            list = list.child(row(shell, cx, room, can_edit.then_some(id), next));
        }
    }
    if !found.unjoined.is_empty() {
        list = list.child(section("More channels", "more-channels"));
        for room in &found.unjoined {
            list = list.child(unjoined_row(shell, cx, room));
        }
    }

    let list = scroll_on_drag::<DraggedRoom>(list, cx);
    let list = scroll_on_drag::<DraggedCategory>(list, cx);

    div()
        .flex()
        .flex_col()
        // The grid's column: as wide as the CSS has it whatever the names inside.
        .w(px(shell.sidebar_shown))
        .flex_none()
        .min_h_0()
        .h_full()
        .rounded_tl(px(theme::RADIUS_LG))
        .bg(theme::fill_sidebar())
        // See-through, the notch outside the curve would show the wallpaper; it is filled
        // with the rail's own fill (same alpha), only outside the curve so nothing stacks.
        .when(theme::glass_percent().is_some(), |d| d.child(corner_notch()))
        .child(header.e2e("header", "sidebar-header"))
        .child(
            list.e2e("div", "room-list")
                .scrollable(std::rc::Rc::new(crate::e2e::HandleProbe(
                    shell.room_scroll.clone(),
                ))),
        )
        // The right edge: dragged to resize the list, a double click resets it.
        .child({
            let view = cx.entity();
            div()
                .id("sidebar-resize")
                .absolute()
                .top_0()
                .bottom_0()
                .right_0()
                .w(px(6.0))
                .cursor_col_resize()
                .hover(|s| s.bg(gpui::rgba(theme::bg_hover())))
                .on_mouse_down(gpui::MouseButton::Left, move |event, _, cx| {
                    cx.stop_propagation();
                    let x = f32::from(event.position.x);
                    let reset = event.click_count >= 2;
                    view.update(cx, |this, cx| {
                        if reset {
                            this.sidebar_w = theme::SIDEBAR_W;
                            this.sidebar_drag = None;
                            this.save_sidebar_width();
                        } else {
                            this.sidebar_drag = Some((x, this.sidebar_shown));
                        }
                        cx.notify();
                    });
                })
                .e2e("div", "sidebar-resize")
                .attr("title", "Drag to resize")
        })
        .relative()
        .e2e("aside", "sidebar")
        .attr("data-bg", format!("{:06x}", theme::bg_sidebar()))
        .attr("data-alpha", theme::alpha_of(theme::fill_sidebar()).to_string())
        .attr("data-radius-tl", theme::RADIUS_LG.to_string())
}

/// The square's corner outside the sidebar's rounded top-left curve, in the rail's fill.
fn corner_notch() -> impl IntoElement {
    let r = theme::RADIUS_LG;
    div()
        .absolute()
        .top_0()
        .left_0()
        .size(px(r))
        .child(
            gpui::canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let o = bounds.origin;
                    let mut path = gpui::PathBuilder::fill();
                    path.move_to(o);
                    path.line_to(gpui::point(o.x + px(r), o.y));
                    path.arc_to(
                        gpui::point(px(r), px(r)),
                        px(0.0),
                        false,
                        false,
                        gpui::point(o.x, o.y + px(r)),
                    );
                    path.close();
                    if let Ok(path) = path.build() {
                        window.paint_path(path, theme::fill_rail());
                    }
                },
            )
            .size_full(),
        )
        .e2e("div", "sidebar-notch")
        .attr("data-bg", format!("{:06x}", theme::bg_rail()))
        .attr("data-alpha", theme::alpha_of(theme::fill_rail()).to_string())
}

/// The rooms of `scope` in the order the sidebar draws them (the Friends
/// page, which sits above the DMs, is not among them).
pub fn sidebar_order(shell: &Shell, scope: &Scope) -> Vec<RoomSummary> {
    let found = sections(
        &shell.rooms,
        scope,
        &[],
        &Filters {
            hide_muted: shell.prefs.hide_muted,
            open: shell.selected.as_ref(),
            marks: &shell.marks,
            hidden: &shell.hidden_rooms,
        },
    );
    let mut order = found.dms;
    order.extend(found.channels);
    for (_, _, members) in found.categories {
        order.extend(members);
    }
    order
}

/// The row that opens the Friends page.
fn friends_row(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement {
    let selected = shell.friends_open;
    div()
        .id("friends-row")
        .flex()
        .items_center()
        .gap(px(12.0))
        .px(px(8.0))
        .py(px(5.0))
        .mb(px(6.0))
        .min_h(px(42.0))
        .rounded(px(theme::RADIUS_SM))
        .cursor_pointer()
        .text_color(rgb(if selected {
            theme::text()
        } else {
            theme::text_muted()
        }))
        .when(selected, |d| d.bg(gpui::rgba(theme::bg_selected())))
        .when(!selected, |d| {
            d.hover(|s| {
                s.bg(gpui::rgba(theme::bg_hover()))
                    .text_color(rgb(theme::text()))
            })
        })
        .on_click(cx.listener(|this, _, _, cx| this.open_friends(cx)))
        .child(div().w(px(32.0)).flex_none().flex().items_center().justify_center().child(
            icon("users", 22.0).text_color(rgb(if selected {
                theme::text()
            } else {
                theme::text_muted()
            })),
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(t("Friends"))
                .e2e("span", "room-name")
                .text(t("Friends")),
        )
        .e2e(
            "button",
            if selected {
                "room friends-row selected"
            } else {
                "room friends-row"
            },
        )
}

/// A small upper-case heading.
fn section(text: &str, class: &str) -> impl IntoElement {
    div()
        .mx(px(8.0))
        .mt(px(12.0))
        .mb(px(4.0))
        .text_size(px(0.7 * theme::REM))
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(theme::text_faint()))
        .child(text.to_uppercase())
        .e2e("h4", &format!("section {class}"))
        .text(text)
}

/// The "Rooms" heading with the small create-room plus to its right (#369).
fn rooms_heading(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement {
    let plus_colour = crate::chat::bar_colour(shell, "create-room", theme::text_muted());
    div()
        .mx(px(8.0))
        .mt(px(12.0))
        .mb(px(4.0))
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_size(px(0.7 * theme::REM))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(theme::text_faint()))
                .child("ROOMS")
                .e2e("h4", "section")
                .text("Rooms"),
        )
        .child(
            div()
                .id("create-room")
                .size(px(18.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(theme::RADIUS_SM))
                .text_color(rgb(plus_colour))
                .on_hover(crate::chat::bar_hover(cx, "create-room"))
                .cursor_pointer()
                .child(icon("plus", 12.0).text_color(rgb(plus_colour)))
                .on_click(
                    cx.listener(|this, _, _, cx| this.open_room_dialog(RoomDialogKind::Create, cx)),
                )
                .tip(t("Create room"))
                .e2e("button", "ghost create-room-button")
                .attr("data-color", format!("{plus_colour:06x}"))
                .attr("data-bg", "")
                .attr("title", "Create room"),
        )
        .e2e("div", "section-row")
}

fn row(
    shell: &Shell,
    cx: &mut Context<Shell>,
    room: &RoomSummary,
    reorder: Option<&OwnedRoomId>,
    next: Option<&OwnedRoomId>,
) -> impl IntoElement {
    let call = crate::call_ui::room_call(shell, cx, room);
    let aside = shell.channel_drop.as_ref().filter(|(r, _)| *r == room.id && cx.has_active_drag()).map(|(_, z)| *z);
    let mut wrap = div()
        .relative()
        .flex()
        .flex_col()
        .child(room_button(shell, cx, room, reorder))
        .children(call)
        .when(aside == Some(rail_drag::Zone::Before), |d| d.child(drop_line(true, &room.id)))
        .when(aside == Some(rail_drag::Zone::After), |d| d.child(drop_line(false, &room.id)));
    // The drop target is the whole row with the gap it opens, so the pointer
    // stays on it as the row moves aside.
    if let Some(container) = reorder {
        let (target, container, view, next) = (room.id.clone(), container.clone(), cx.entity(), next.cloned());
        wrap = wrap
            .on_drag_move::<DraggedRoom>({
                let (over, view) = (target.clone(), view.clone());
                move |event, _, cx| {
                    if event.drag(cx).id != over {
                        hover_edge(&view, &over, next.as_ref(), event.bounds, event.event.position, cx);
                    }
                }
            })
            .on_drag_move::<DraggedCategory>(move |event, _, cx| {
                // A category held over a channel: no line here.
                if event.bounds.contains(&event.event.position) {
                    view.update(cx, |shell, cx| {
                        if shell.channel_drop.take().is_some() {
                            cx.notify();
                        }
                    });
                }
            })
            .on_drop::<DraggedRoom>(cx.listener(move |this, dragged: &DraggedRoom, _, cx| {
                // The slot the line showed: "after this row" is "before the next".
                let (target, after) = this.channel_drop.take().map(|(r, z)| (r, z == rail_drag::Zone::After)).unwrap_or((target.clone(), false));
                let drop = ChannelDrop::Beside { target, after };
                this.drop_channel(dragged.id.clone(), drop, container.clone(), cx)
            }));
    }
    open_gap(wrap, aside, &room.id.to_string())
}

/// Rows move aside to show where the dragged one will land: the row under the
/// pointer opens a gap on the edge the line is on, easing open.
fn open_gap(wrap: Div, at: Option<rail_drag::Zone>, key: &str) -> AnyElement {
    match at {
        Some(zone @ (rail_drag::Zone::Before | rail_drag::Zone::After)) => wrap
            .with_animation(
                gpui::SharedString::from(format!("gap-{key}-{zone:?}")),
                Animation::new(std::time::Duration::from_millis(120)),
                move |el, t| {
                    let gap = px(DROP_GAP * t);
                    if zone == rail_drag::Zone::Before { el.pt(gap) } else { el.pb(gap) }
                },
            )
            .into_any_element(),
        _ => wrap.into_any_element(),
    }
}

/// The room a dragged row opens between its neighbours (px).
const DROP_GAP: f32 = 10.0;

/// The rail's white drop line, in the gap on one edge of a row or header.
fn drop_line(at_top: bool, owner: &OwnedRoomId) -> impl IntoElement {
    let base = div()
        .absolute()
        .left(px(4.0))
        .right(px(4.0))
        .h(px(2.0))
        .rounded(px(1.0))
        .bg(rgb(0xffffff)); // fixed: the drop line, as on the rail
    (if at_top { base.top(px(DROP_GAP / 2.0 - 1.0)) } else { base.bottom(px(DROP_GAP / 2.0 - 1.0)) })
        .e2e("div", "channel-drop-line")
        .attr("data-slot", format!("{owner} {}", if at_top { "before" } else { "after" }))
        .attr("data-bg", "#ffffff")
}

/// A drag over `id` (spanning `bounds`) with the pointer at `y`: the edge it is nearer, kept in the shell.
fn hover_edge(
    view: &gpui::Entity<Shell>,
    id: &OwnedRoomId,
    next: Option<&OwnedRoomId>,
    bounds: gpui::Bounds<gpui::Pixels>,
    at: gpui::Point<gpui::Pixels>,
    cx: &mut gpui::App,
) {
    if !bounds.contains(&at) {
        return;
    }
    let (top, bottom) = (f32::from(bounds.origin.y), f32::from(bounds.bottom()));
    let y = f32::from(at.y);
    let (id, next) = (id.clone(), next.cloned());
    view.update(cx, |shell, cx| {
        let slot = Some(rail_drag::slot(&id, next.as_ref(), y, top, bottom, shell.channel_drop.as_ref()));
        if shell.channel_drop != slot {
            shell.channel_drop = slot;
            cx.notify();
        }
    });
}

/// A drag held near the list's top or bottom edge scrolls it.
fn scroll_on_drag<T: 'static>(list: Stateful<Div>, cx: &mut Context<Shell>) -> Stateful<Div> {
    let view = cx.entity();
    list.on_drag_move::<T>(move |event, _, cx| {
        let (p, b) = (event.event.position, event.bounds);
        let step = if p.x >= b.left() && p.x <= b.right() {
            rail_drag::auto_scroll(f32::from(p.y), f32::from(b.origin.y), f32::from(b.bottom()))
        } else {
            0.0
        };
        view.update(cx, |shell, cx| shell.scroll_while_dragging(step, cx));
    })
}

/// A category's header: the section title, dragged to reorder the categories,
/// and a drop target for channels (into the category).
fn category_header(
    shell: &Shell,
    cx: &mut Context<Shell>,
    id: &OwnedRoomId,
    name: &str,
    root: Option<&OwnedRoomId>,
    next: Option<&OwnedRoomId>,
) -> AnyElement {
    let drop_at = shell.channel_drop.clone().filter(|_| cx.has_active_drag());
    let here = |zone| drop_at == Some((id.clone(), zone));
    let mut header = div()
        .id(gpui::SharedString::from(format!("category-{id}")))
        .rounded(px(theme::RADIUS_SM))
        .mx(px(8.0))
        .text_size(px(0.7 * theme::REM))
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(theme::text_faint()))
        .when(here(rail_drag::Zone::Onto), |d| d.bg(gpui::rgba(theme::bg_hover())))
        .child(name.to_uppercase());
    let mut wrap = div()
        .relative()
        .mt(px(12.0))
        .mb(px(4.0))
        .when(here(rail_drag::Zone::Before), |d| d.child(drop_line(true, id)))
        .when(here(rail_drag::Zone::After), |d| d.child(drop_line(false, id)));
    if let Some(root) = root {
        let view = cx.entity();
        let dragged = DraggedCategory {
            id: id.clone(),
            name: name.to_owned(),
            width: (shell.sidebar_shown - 16.0).max(80.0),
        };
        let (over, target, root, next) = (id.clone(), id.clone(), root.clone(), next.cloned());
        header = header.on_drag(dragged, |dragged, _, _, cx| cx.new(|_| DragGhost::Category(dragged.clone())));
        wrap = wrap
            .on_drag_move::<DraggedCategory>({
                let (view, over) = (view.clone(), over.clone());
                move |event, _, cx| {
                    if event.drag(cx).id != over {
                        hover_edge(&view, &over, next.as_ref(), event.bounds, event.event.position, cx);
                    }
                }
            })
            .on_drop::<DraggedCategory>(cx.listener({
                let (target, root) = (target.clone(), root.clone());
                move |this, dragged: &DraggedCategory, _, cx| {
                    let (target, after) = this.channel_drop.take().map(|(r, z)| (r, z == rail_drag::Zone::After)).unwrap_or((target.clone(), false));
                    this.drop_category(dragged.id.clone(), target, after, root.clone(), cx)
                }
            }));
        {
            // Whoever may edit the server drops channels in; the homeserver has the last word.
            wrap = wrap
                .on_drag_move::<DraggedRoom>(move |event, _, cx| {
                    if !event.bounds.contains(&event.event.position) {
                        return;
                    }
                    let next = Some((over.clone(), rail_drag::Zone::Onto));
                    view.update(cx, |shell, cx| {
                        if shell.channel_drop != next {
                            shell.channel_drop = next;
                            cx.notify();
                        }
                    });
                })
                .on_drop::<DraggedRoom>(cx.listener(move |this, dragged: &DraggedRoom, _, cx| {
                    this.channel_drop = None;
                    this.drop_channel(dragged.id.clone(), ChannelDrop::Into, target.clone(), cx)
                }));
        }
    }
    let gap = drop_at.as_ref().filter(|(r, _)| r == id).map(|(_, z)| *z);
    let wrap = wrap.child(header.e2e("h4", "section category").attr("data-room", id.to_string()).text(name));
    open_gap(wrap, gap.filter(|z| *z != rail_drag::Zone::Onto), &format!("header-{id}"))
}

fn room_button(
    shell: &Shell,
    cx: &mut Context<Shell>,
    room: &RoomSummary,
    reorder: Option<&OwnedRoomId>,
) -> impl IntoElement {
    let selected = shell.selected.as_ref() == Some(&room.id) && !shell.friends_open;
    let locked = crate::locks::is_locked(&room.id);
    let unread = room.unread_messages > 0 || room.marked_unread;
    let pinned = shell.marks.is_pinned(room);
    let mut classes = String::from("room");
    for (on, class) in [
        (room.is_dm, "dm"),
        (room.is_voice, "voice"),
        (shell.calls.in_call(&room.id), "in-call"),
        (pinned, "pinned"),
        (shell.marks.is_muted(room), "muted"),
        (selected, "selected"),
        (locked, "locked"),
        (unread, "unread"),
    ] {
        if on {
            classes.push(' ');
            classes.push_str(class);
        }
    }
    let lit = selected || unread;
    let id = room.id.clone();
    let view = cx.entity();
    let chat_shown = room.is_voice && !room.is_dm && shell.hovered_voice.as_ref() == Some(&room.id);
    let mut button = div()
        .id(gpui::SharedString::from(format!("room-{}", room.id)))
        .flex()
        .items_center()
        .gap(px(if room.is_dm { 12.0 } else { 8.0 }))
        .px(px(8.0))
        .py(px(if room.is_dm { 5.0 } else { 6.0 }))
        .when(room.is_dm, |d| d.min_h(px(42.0)))
        .rounded(px(theme::RADIUS_SM))
        .cursor_pointer()
        .text_color(rgb(if lit {
            theme::text()
        } else {
            theme::text_muted()
        }))
        .when(unread, |d| d.font_weight(FontWeight::SEMIBOLD))
        .when(selected, |d| d.bg(gpui::rgba(theme::bg_selected())))
        .when(!selected, |d| {
            d.hover(|s| {
                s.bg(gpui::rgba(theme::bg_hover()))
                    .text_color(rgb(theme::text()))
            })
        })
        .on_click({
            let (voice, name) = (room.is_voice, room.name.clone());
            move |_, _, cx| {
                view.update(cx, |s, cx| {
                    // A server's voice channel, as in Discord: a click joins its
                    // call and leaves you where you are; in the call already, it
                    // opens (as a call page).
                    if voice && !s.calls.in_call(&id) {
                        s.join_call(id.clone(), name.clone(), cx)
                    } else {
                        s.open_room(id.clone(), cx)
                    }
                })
            }
        })
        .on_mouse_down(MouseButton::Right, {
            let room = room.clone();
            cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                this.open_room_menu(room.clone(), event.position, cx)
            })
        });
    if room.is_voice && !room.is_dm {
        let id = room.id.clone();
        button = button.on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            let now = this.hovered_voice.clone();
            if *hovered {
                if now.as_ref() != Some(&id) {
                    this.hovered_voice = Some(id.clone());
                    cx.notify();
                }
            } else if now.as_ref() == Some(&id) {
                this.hovered_voice = None;
                cx.notify();
            }
        }));
    }
    if reorder.is_some() {
        let dragged = DraggedRoom {
            id: room.id.clone(),
            name: room.name.clone(),
            voice: room.is_voice,
            width: (shell.sidebar_shown - 16.0).max(80.0),
        };
        button = button
            .on_drag(dragged, |dragged, _, _, cx| cx.new(|_| DragGhost::Room(dragged.clone())));
    }
    let title = crate::streamer::room_title(&shell.nicknames, room);
    button = if room.is_dm {
        button.child(avatar(shell.picture(&room.avatar, 32), &title, 32.0, None))
    } else {
        button.child(channel_glyph(room.is_voice))
    };
    button = button.child(
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .child(title.clone())
            .e2e("span", "room-name")
            .text(title.clone()),
    );
    let timer = if room.is_voice && !room.is_dm {
        shell.call_timer(room)
    } else {
        None
    };
    // Like Discord's: the channel's text chat, without joining the call.
    let chat_button = chat_shown.then(|| {
        let id = room.id.clone();
        div()
            .id(gpui::SharedString::from(format!("room-chat-{}", room.id)))
            .size(px(20.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(theme::RADIUS_SM))
            .hover(|s| s.bg(gpui::rgba(theme::bg_selected())))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.open_room(id.clone(), cx);
                this.calls.voice_chat = true;
                cx.notify();
            }))
            .child(icon("messages-square", 14.0).text_color(rgb(theme::text())))
            .tip(t("Open chat"))
            .e2e("button", "room-chat-button")
            .attr("title", "Open chat")
    });
    match (timer, chat_button) {
        // On hover the button takes the timer's place: the timer stays in the
        // layout, unseen, so the row does not shift.
        (Some(timer), Some(chat)) => {
            button = button.child(
                div()
                    .relative()
                    .flex_none()
                    .child(
                        div()
                            .text_size(px(0.75 * theme::REM))
                            .opacity(0.0)
                            .tabular_nums()
                            .child(timer),
                    )
                    .child(div().absolute().right_0().top_0().bottom_0().flex().items_center().child(chat)),
            );
        }
        (Some(timer), None) => {
            button = button.child(
                div()
                    .flex_none()
                    .text_size(px(0.75 * theme::REM))
                    .text_color(rgb(theme::text_faint()))
                    .tabular_nums()
                    .child(timer.clone())
                    .e2e("span", "call-timer")
                    .text(timer),
            );
        }
        (None, Some(chat)) => button = button.child(chat),
        (None, None) => {}
    }
    if pinned {
        button = button.child(icon("pin", 12.0).text_color(rgb(theme::text_faint())));
    }
    if locked {
        // A lock hides how many: a dot says there is something.
        button = button.child(
            icon("lock", 12.0)
                .text_color(rgb(theme::text_faint()))
                .e2e("svg", "lock-icon"),
        );
        if unread || room.highlight_count > 0 {
            button = button.child(
                div()
                    .size(px(8.0))
                    .flex_none()
                    .rounded_full()
                    .bg(rgb(theme::danger()))
                    .e2e("span", "lock-dot"),
            );
        }
    } else if room.highlight_count > 0 {
        let count = room.highlight_count.to_string();
        button = button.child(
            div()
                .min_w(px(18.0))
                .h(px(18.0))
                .px(px(6.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(9.0))
                .bg(rgb(theme::danger()))
                .text_color(rgb(theme::on_danger()))
                .text_size(px(0.75 * theme::REM))
                .line_height(px(18.0))
                .font_weight(FontWeight::BOLD)
                .child(count.clone())
                .e2e("span", "badge")
                .text(count),
        );
    }
    let mut tagged = button
        .e2e("button", &classes)
        .attr("data-room", room.id.to_string());
    if reorder.is_some() {
        tagged = tagged.attr("draggable", "true");
    }
    tagged
}

fn unjoined_row(shell: &Shell, cx: &mut Context<Shell>, room: &SpaceRoom) -> impl IntoElement {
    let selected = shell.selected.as_ref() == Some(&room.id);
    let view = cx.entity();
    let mut classes = String::from("room unjoined");
    if selected {
        classes.push_str(" selected");
    }
    div()
        .id(gpui::SharedString::from(format!("unjoined-{}", room.id)))
        .flex()
        .items_center()
        .gap(px(8.0))
        .px(px(8.0))
        .py(px(6.0))
        .rounded(px(theme::RADIUS_SM))
        .cursor_pointer()
        .text_color(rgb(if selected {
            theme::text()
        } else {
            theme::text_muted()
        }))
        .when(selected, |d| d.bg(gpui::rgba(theme::bg_selected())))
        .when(!selected, |d| {
            d.hover(|s| {
                s.bg(gpui::rgba(theme::bg_hover()))
                    .text_color(rgb(theme::text()))
            })
        })
        .on_click({
            let room = room.clone();
            move |_, _, cx| view.update(cx, |s, cx| s.open_unjoined(room.clone(), cx))
        })
        .child(channel_glyph(room.is_voice))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(room.name.clone())
                .e2e("span", "room-name")
                .text(room.name.clone()),
        )
        .e2e("button", &classes)
        .attr("data-room", room.id.to_string())
        .attr("title", "Not joined")
}

/// A channel being dragged to a new place in its server's list.
#[derive(Clone)]
pub struct DraggedRoom {
    id: OwnedRoomId,
    name: String,
    voice: bool,
    width: f32,
}

/// A category being dragged to a new place among its server's categories.
#[derive(Clone)]
pub struct DraggedCategory {
    id: OwnedRoomId,
    name: String,
    width: f32,
}

/// Where a dragged channel was dropped.
enum ChannelDrop {
    /// On this row's edge (`after`: its lower one).
    Beside { target: OwnedRoomId, after: bool },
    /// On a category's header: into it, last.
    Into,
}

/// What follows the pointer while a channel or category is dragged.
enum DragGhost {
    Room(DraggedRoom),
    Category(DraggedCategory),
}

impl gpui::Render for DragGhost {
    fn render(&mut self, _: &mut gpui::Window, _: &mut Context<Self>) -> impl IntoElement {
        // The row as it is listed: its glyph and name, at its size, lifted,
        // in the UI font (the ghost is drawn outside the app's own tree).
        let (width, glyph, name) = match self {
            Self::Room(r) => (r.width, Some(channel_glyph(r.voice)), r.name.clone()),
            Self::Category(c) => (c.width, None, c.name.to_uppercase()),
        };
        div()
            .w(px(width))
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(8.0))
            .py(px(6.0))
            .rounded(px(theme::RADIUS_SM))
            .bg(rgb(theme::bg_elevated()))
            .shadow_lg()
            .opacity(0.9)
            .font_family("Inter")
            .text_color(rgb(theme::text()))
            .children(glyph)
            .child(div().flex_1().min_w_0().truncate().child(name))
            .e2e("div", "channel-drag-ghost")
            .attr("data-font", "Inter")
    }
}

impl Shell {
    /// A channel was dropped: onto a row's edge or a category's header (`dest`
    /// is the server or category whose list that is). It is listed there, taken
    /// out of the server's other lists, in the order everyone sees.
    fn drop_channel(
        &mut self,
        from: OwnedRoomId,
        drop: ChannelDrop,
        dest: OwnedRoomId,
        cx: &mut Context<Self>,
    ) {
        let Scope::Space(root) = self.scope.clone() else {
            return;
        };
        let Some(session) = self.session.clone() else {
            return;
        };
        let found = self.channel_sections();
        let listed: Vec<OwnedRoomId> = if dest == root {
            found.channels.iter().map(|r| r.id.clone()).collect()
        } else {
            found
                .categories
                .iter()
                .find(|(id, _, _)| *id == dest)
                .map(|(_, _, members)| members.iter().map(|r| r.id.clone()).collect())
                .unwrap_or_default()
        };
        let order = self.children_of(&dest);
        let new_order = match drop {
            ChannelDrop::Beside { target, after } => {
                crate::view_model::reorder::dropped_beside(&order, &listed, &from, &target, after)
            }
            ChannelDrop::Into => Some(crate::view_model::reorder::dropped_last(&order, &listed, &from)),
        };
        let mut containers = vec![root];
        containers.extend(found.categories.iter().map(|(id, _, _)| id.clone()));
        let parents = self
            .rooms
            .iter()
            .find(|r| r.id == from)
            .map(|r| r.parent_spaces.clone())
            .unwrap_or_default();
        let leaving = crate::view_model::reorder::leaving(&parents, &containers, &dest);
        let Some(new_order) = new_order else {
            return;
        };
        let ordering = session.move_space_child(dest, new_order, Some(from), leaving);
        self.run_in_background("reorder channels", ordering, cx);
    }

    /// A category was dropped on the edge of another: it goes there among the
    /// server's children, for everyone.
    fn drop_category(
        &mut self,
        from: OwnedRoomId,
        target: OwnedRoomId,
        after: bool,
        root: OwnedRoomId,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let found = self.channel_sections();
        let mut listed: Vec<OwnedRoomId> = found.channels.iter().map(|r| r.id.clone()).collect();
        listed.extend(found.categories.iter().map(|(id, _, _)| id.clone()));
        let order = self.children_of(&root);
        if let Some(order) =
            crate::view_model::reorder::dropped_beside(&order, &listed, &from, &target, after)
        {
            let ordering = session.set_space_order(root, order);
            self.run_in_background("reorder categories", ordering, cx);
        }
    }

    fn children_of(&self, space: &OwnedRoomId) -> Vec<OwnedRoomId> {
        self.rooms
            .iter()
            .find(|r| &r.id == space)
            .map(|r| r.space_children.clone())
            .unwrap_or_default()
    }

    /// The channel list as it is drawn now.
    fn channel_sections(&self) -> Sections {
        sections(
            &self.rooms,
            &self.scope,
            &self.directory,
            &Filters {
                hide_muted: self.prefs.hide_muted,
                open: self.selected.as_ref(),
                marks: &self.marks,
                hidden: &self.hidden_rooms,
            },
        )
    }

    /// A drag at `step` px per frame (negative up) near the list's edge keeps
    /// scrolling it while held, even with the pointer still.
    pub fn scroll_while_dragging(&mut self, step: f32, cx: &mut Context<Self>) {
        self.drag_scroll.0 = step;
        if step == 0.0 || self.drag_scroll.1 {
            return;
        }
        self.drag_scroll.1 = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(16)).await;
                let go = this.update(cx, |shell, cx| {
                    let step = shell.drag_scroll.0;
                    if step == 0.0 || !cx.has_active_drag() {
                        shell.drag_scroll = (0.0, false);
                        return false;
                    }
                    let (at, max) = (shell.room_scroll.offset(), shell.room_scroll.max_offset());
                    let y = (at.y - px(step)).clamp(-max.height, px(0.0));
                    shell.room_scroll.set_offset(gpui::point(at.x, y));
                    cx.notify();
                    true
                });
                if !go.unwrap_or(false) {
                    break;
                }
            }
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::room;

    fn names(rooms: &[RoomSummary]) -> Vec<&str> {
        rooms.iter().map(|r| r.name.as_str()).collect()
    }

    #[test]
    fn a_server_lists_its_channels_in_its_own_order() {
        let mut server = room("!s:x", "Projects");
        server.is_space = true;
        let (a, b) = (room("!a:x", "news"), room("!b:x", "general"));
        server.space_children = vec![b.id.clone(), a.id.clone()];
        let (mut a, mut b) = (a, b);
        a.parent_spaces = vec![server.id.clone()];
        b.parent_spaces = vec![server.id.clone()];
        let archive = SpaceRoom {
            id: OwnedRoomId::try_from("!c:x").expect("id"),
            name: "archive".into(),
            topic: None,
            avatar: None,
            member_count: 1,
            is_space: false,
            is_voice: false,
            via: Vec::new(),
        };
        let all = [server.clone(), a, b];
        let (marks, left) = (Marks::default(), HashSet::new());
        let filters = |hide_muted, open| Filters {
            hide_muted,
            open,
            marks: &marks,
            hidden: &left,
        };
        let found = sections(
            &all,
            &Scope::Space(server.id.clone()),
            &[archive.clone()],
            &filters(false, None),
        );
        assert_eq!(names(&found.channels), ["general", "news"]);
        assert_eq!(found.unjoined.len(), 1);
        // Joined ones are not listed twice.
        let again = sections(
            &all,
            &Scope::Space(server.id),
            &[archive],
            &filters(false, None),
        );
        assert_eq!(again.unjoined[0].name, "archive");
    }

    #[test]
    fn hiding_muted_keeps_the_open_room() {
        let mut quiet = room("!q:x", "quiet");
        quiet.muted = true;
        let loud = room("!l:x", "loud");
        let all = [quiet.clone(), loud];
        let (marks, left) = (Marks::default(), HashSet::new());
        let filters = |open| Filters {
            hide_muted: true,
            open,
            marks: &marks,
            hidden: &left,
        };
        let hidden = sections(&all, &Scope::Rooms, &[], &filters(None));
        assert_eq!(names(&hidden.channels), ["loud"]);
        let open = sections(&all, &Scope::Rooms, &[], &filters(Some(&quiet.id)));
        assert_eq!(names(&open.channels), ["quiet", "loud"]);
    }

    #[test]
    fn rooms_just_left_and_closed_dms_are_not_listed_and_pinned_ones_come_first() {
        let (a, b) = (room("!a:x", "a"), room("!b:x", "b"));
        let (mut d1, mut d2) = (room("!d1:x", "d1"), room("!d2:x", "d2"));
        d1.is_dm = true;
        d2.is_dm = true;
        let all = [a.clone(), b, d1.clone(), d2.clone()];
        let mut marks = Marks::default();
        marks.set(morpheus_core::marks::PINNED, "!d2:x", Some(5));
        let left: HashSet<String> = ["!a:x".to_owned()].into();
        let filters = Filters {
            hide_muted: false,
            open: None,
            marks: &marks,
            hidden: &left,
        };
        let found = sections(&all, &Scope::Dms, &[], &filters);
        assert_eq!(names(&found.dms), ["d2", "d1"]);
        let found = sections(&all, &Scope::Rooms, &[], &filters);
        assert_eq!(names(&found.channels), ["b"]);
        let mut closed = marks.clone();
        closed.set(morpheus_core::marks::CLOSED, "!d1:x", Some(i64::MAX));
        let filters = Filters {
            marks: &closed,
            ..filters
        };
        let found = sections(&all, &Scope::Dms, &[], &filters);
        assert_eq!(names(&found.dms), ["d2"]);
    }
}

/// The text colour a little below full brightness: what a button rests in
/// before hover brightens it to `colour`.
fn dimmed(colour: u32) -> u32 {
    let mix = |shift: u32| {
        let c = (colour >> shift) & 0xff;
        let m = (theme::text_muted() >> shift) & 0xff;
        (c * 7 + m * 3) / 10
    };
    (mix(16) << 16) | (mix(8) << 8) | mix(0)
}

/// The shortcut that opens the palette, as the platform writes it.
fn palette_shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘K"
    } else {
        "Ctrl+K"
    }
}

/// The top of the DMs: an inset, lighter button that opens the command palette.
fn palette_button(cx: &mut Context<Shell>) -> impl IntoElement {
    let label = crate::view_model::i18n::tf("Command palette %{keys}", &[("keys", &palette_shortcut().to_string())]);
    div()
        .id("palette-button")
        .flex()
        .items_center()
        .justify_between()
        .w_full()
        .px(px(10.0))
        .h(px(30.0))
        .rounded(px(theme::RADIUS_SM))
        .bg(theme::fill_elevated())
        .text_size(px(0.85 * theme::REM))
        .text_color(rgb(theme::text_muted()))
        .cursor_pointer()
        .hover(|s| s.text_color(rgb(theme::text())))
        .on_click(cx.listener(|this, _, _, cx| this.toggle_switcher(cx)))
        .child(t("Command palette"))
        .child(
            div()
                .text_size(px(0.75 * theme::REM))
                .text_color(rgb(theme::text_faint()))
                .child(palette_shortcut()),
        )
        .e2e("button", "palette-button")
        .attr("data-bg", format!("#{:06x}", theme::bg_elevated()))
        .text(label)
}

/// What a server's welcome screen lists: its channels you are in (text, then
/// voice, in the sidebar's order) and the ones you are not.
pub fn welcome_channels(shell: &Shell, space: &OwnedRoomId) -> (Vec<RoomSummary>, Vec<SpaceRoom>) {
    let scope = Scope::Space(space.clone());
    let found = sections(
        &shell.rooms,
        &scope,
        &shell.directory,
        &Filters {
            hide_muted: false,
            open: None,
            marks: &shell.marks,
            hidden: &shell.hidden_rooms,
        },
    );
    let mut joined = found.channels;
    for (_, _, members) in found.categories {
        joined.extend(members);
    }
    joined.retain(|r| !r.is_space && !r.invited);
    (joined, found.unjoined)
}

/// A channel's mark before its name: the speaker of a voice channel or the #
/// of a text one, in one slot as wide for both, so every name starts at the
/// same x and the mark is centred on the name's line.
pub fn channel_glyph(voice: bool) -> AnyElement {
    let slot = div()
        .w(px(20.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center();
    if voice {
        slot.child(
            icon("volume-2", 18.0)
                .text_color(rgb(theme::text_faint()))
                .e2e("svg", "voice-icon"),
        )
        .into_any_element()
    } else {
        slot.child(
            icon("hash", 18.0)
                .text_color(rgb(theme::text_faint()))
                .e2e("svg", "hash"),
        )
        .into_any_element()
    }
}
