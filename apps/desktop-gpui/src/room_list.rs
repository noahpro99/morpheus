//! The sidebar (`crates/ui/src/components/room_list.rs`): a header with what
//! the rail selected, then the rooms of that scope. Inside a server its
//! channels follow the server's own order, nested servers become headed
//! categories, and the channels you are not in come last.
//! Not ported yet: invites, the Friends row, call participants, pins and
//! closed DMs, the room and server menus, drag to reorder, the create-room
//! dialog.

use std::collections::HashSet;

use gpui::{Context, FontWeight, IntoElement, MouseButton, div, prelude::*, px, rgb};
use morpheus_core::{OwnedRoomId, RoomSummary, SpaceRoom};

use crate::{
    view_model::{nicknames::room_title, room_dialogs::RoomDialogKind},
    e2e::E2eExt as _,
    icons::icon,
    media::avatar,
    model::{Scope, header_title, rooms_in_scope},
    shell::Shell,
    theme,
    view_model::marks::Marks,
};

/// The sidebar's rows, in the order they are drawn.
#[derive(Debug, Default)]
pub struct Sections {
    pub dms: Vec<RoomSummary>,
    pub channels: Vec<RoomSummary>,
    /// Rooms under a nested server, by its name, in the server's order.
    pub categories: Vec<(OwnedRoomId, String, Vec<RoomSummary>)>,
    /// A server's channels you are not in.
    pub unjoined: Vec<SpaceRoom>,
}

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
    let mut categories: Vec<(OwnedRoomId, String)> = match scope {
        Scope::Space(space_id) => rooms
            .iter()
            .filter(|r| r.is_space && r.parent_spaces.contains(space_id))
            .map(|r| (r.id.clone(), r.name.clone()))
            .collect(),
        _ => Vec::new(),
    };
    let (mut channels, categorised): (Vec<_>, Vec<_>) = channels.into_iter().partition(|room| {
        !categories
            .iter()
            .any(|(id, _)| room.parent_spaces.contains(id))
    });
    let children_of = |id: &OwnedRoomId| -> Vec<OwnedRoomId> {
        rooms
            .iter()
            .find(|r| &r.id == id)
            .map(|r| r.space_children.clone())
            .unwrap_or_default()
    };
    let place = |order: &[OwnedRoomId], id: &OwnedRoomId| {
        order.iter().position(|c| c == id).unwrap_or(usize::MAX)
    };
    if let Scope::Space(space_id) = scope {
        let order = children_of(space_id);
        channels.sort_by_key(|r| place(&order, &r.id));
        categories.sort_by_key(|(id, _)| place(&order, id));
    }
    let categories = categories
        .into_iter()
        .map(|(id, name)| {
            let order = children_of(&id);
            let mut members: Vec<RoomSummary> = categorised
                .iter()
                .filter(|r| r.parent_spaces.contains(&id))
                .cloned()
                .collect();
            members.sort_by_key(|r| place(&order, &r.id));
            (id, name, members)
        })
        .collect();
    let unjoined = match scope {
        Scope::Space(_) => directory
            .iter()
            .filter(|r| !r.is_space)
            .filter(|r| !rooms.iter().any(|known| known.id == r.id && !known.invited))
            .cloned()
            .collect(),
        _ => Vec::new(),
    };
    Sections {
        dms,
        channels,
        categories,
        unjoined,
    }
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
    let in_space = matches!(scope, Scope::Space(_));
    // Inside a space (and favorites) both kinds can appear: label the groups.
    let grouped = in_space || *scope == Scope::Favorites;
    let title = header_title(&shell.rooms, scope);

    let mut name = div()
        .id("sidebar-title")
        .flex()
        .items_center()
        .gap(px(4.0))
        .font_weight(FontWeight::BOLD)
        .child(title.clone());
    // Clicking the server's name (or right-clicking it) opens the server menu.
    if let Scope::Space(space_id) = scope {
        let space_id = space_id.clone();
        name = name.cursor_pointer().on_click(cx.listener({
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
        name = name.child(icon("chevron-down", 14.0).text_color(rgb(theme::TEXT_FAINT)));
    }
    let mut header = div()
        .h(px(48.0))
        .flex_none()
        .px(px(16.0))
        .flex()
        .items_center()
        .justify_between()
        .border_b_1()
        .border_color(gpui::rgba(0x0000004d))
        .child(
            name.e2e("h2", if in_space { "server-name" } else { "" })
                .text(title),
        );
    if *scope != Scope::Dms {
        header = header.child(
            div()
                .id("create-room")
                .px(px(8.0))
                .py(px(4.0))
                .rounded(px(theme::RADIUS_SM))
                .text_color(rgb(theme::TEXT_MUTED))
                .hover(|s| {
                    s.bg(gpui::rgba(theme::BG_HOVER))
                        .text_color(rgb(theme::TEXT))
                })
                .cursor_pointer()
                .child(icon("plus", 18.0).text_color(rgb(theme::TEXT_MUTED)))
                .on_click(cx.listener(|this, _, _, cx| this.open_room_dialog(RoomDialogKind::Create, cx)))
                .e2e("button", "ghost create-room-button")
                .attr("title", "Create room"),
        );
    }

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
                .text_color(rgb(theme::TEXT_MUTED))
                .child(text)
                .e2e("p", "muted pad")
                .text(text),
        );
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
            list = list.child(row(shell, cx, room));
        }
    }
    if !found.channels.is_empty() {
        if grouped {
            list = list.child(section("Rooms", ""));
        }
        for room in &found.channels {
            list = list.child(row(shell, cx, room));
        }
    }
    for (_, name, members) in &found.categories {
        if members.is_empty() {
            continue;
        }
        list = list.child(section(name, "category"));
        for room in members {
            list = list.child(row(shell, cx, room));
        }
    }
    if !found.unjoined.is_empty() {
        list = list.child(section("More channels", "more-channels"));
        for room in &found.unjoined {
            list = list.child(unjoined_row(shell, cx, room));
        }
    }

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .h_full()
        .bg(rgb(theme::BG_SIDEBAR))
        .child(header.e2e("header", "sidebar-header"))
        .child(
            list.e2e("div", "room-list")
                .scrollable(std::rc::Rc::new(crate::e2e::HandleProbe(
                    shell.room_scroll.clone(),
                ))),
        )
        .e2e("aside", "sidebar")
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
        .text_color(rgb(if selected { theme::TEXT } else { theme::TEXT_MUTED }))
        .when(selected, |d| d.bg(gpui::rgba(theme::BG_SELECTED)))
        .when(!selected, |d| {
            d.hover(|s| {
                s.bg(gpui::rgba(theme::BG_HOVER))
                    .text_color(rgb(theme::TEXT))
            })
        })
        .on_click(cx.listener(|this, _, _, cx| this.open_friends(cx)))
        .child(
            div()
                .w(px(32.0))
                .flex_none()
                .flex()
                .items_center()
                .child(icon("users", 22.0).text_color(rgb(if selected {
                    theme::TEXT
                } else {
                    theme::TEXT_MUTED
                }))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child("Friends")
                .e2e("span", "room-name")
                .text("Friends"),
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
        .text_color(rgb(theme::TEXT_FAINT))
        .child(text.to_uppercase())
        .e2e("h4", &format!("section {class}"))
        .text(text)
}

fn row(shell: &Shell, cx: &mut Context<Shell>, room: &RoomSummary) -> impl IntoElement {
    let selected = shell.selected.as_ref() == Some(&room.id) && !shell.friends_open;
    let unread = room.unread_messages > 0 || room.marked_unread;
    let pinned = shell.marks.is_pinned(room);
    let mut classes = String::from("room");
    for (on, class) in [
        (room.is_dm, "dm"),
        (room.is_voice, "voice"),
        (pinned, "pinned"),
        (shell.marks.is_muted(room), "muted"),
        (selected, "selected"),
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
        .text_color(rgb(if lit { theme::TEXT } else { theme::TEXT_MUTED }))
        .when(unread, |d| d.font_weight(FontWeight::SEMIBOLD))
        .when(selected, |d| d.bg(gpui::rgba(theme::BG_SELECTED)))
        .when(!selected, |d| {
            d.hover(|s| {
                s.bg(gpui::rgba(theme::BG_HOVER))
                    .text_color(rgb(theme::TEXT))
            })
        })
        .on_click(move |_, _, cx| view.update(cx, |s, cx| s.open_room(id.clone(), cx)))
        .on_mouse_down(MouseButton::Right, {
            let room = room.clone();
            cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                this.open_room_menu(room.clone(), event.position, cx)
            })
        });
    let title = room_title(&shell.nicknames, room);
    button = if room.is_dm {
        button.child(avatar(
            shell.picture(&room.avatar, 32),
            &title,
            32.0,
            None,
        ))
    } else if room.is_voice {
        button.child(icon("volume-2", 18.0).text_color(rgb(theme::TEXT_FAINT)))
    } else {
        button.child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(theme::TEXT_FAINT))
                .child("#")
                .e2e("span", "hash")
                .text("#"),
        )
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
    if pinned {
        button = button.child(icon("pin", 12.0).text_color(rgb(theme::TEXT_FAINT)));
    }
    if room.highlight_count > 0 {
        let count = room.highlight_count.to_string();
        button = button.child(
            div()
                .min_w(px(18.0))
                .px(px(6.0))
                .rounded(px(9.0))
                .bg(rgb(theme::DANGER))
                .text_color(rgb(0xffffff))
                .text_size(px(0.75 * theme::REM))
                .font_weight(FontWeight::BOLD)
                .child(count.clone())
                .e2e("span", "badge")
                .text(count),
        );
    }
    button
        .e2e("button", &classes)
        .attr("data-room", room.id.to_string())
}

fn unjoined_row(shell: &Shell, cx: &mut Context<Shell>, room: &SpaceRoom) -> impl IntoElement {
    let selected = shell.selected.as_ref() == Some(&room.id);
    let id = room.id.clone();
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
            theme::TEXT
        } else {
            theme::TEXT_MUTED
        }))
        .when(selected, |d| d.bg(gpui::rgba(theme::BG_SELECTED)))
        .when(!selected, |d| {
            d.hover(|s| {
                s.bg(gpui::rgba(theme::BG_HOVER))
                    .text_color(rgb(theme::TEXT))
            })
        })
        .on_click(move |_, _, cx| view.update(cx, |s, cx| s.open_room(id.clone(), cx)))
        .child(if room.is_voice {
            icon("volume-2", 18.0)
                .text_color(rgb(theme::TEXT_FAINT))
                .into_any_element()
        } else {
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(theme::TEXT_FAINT))
                .child("#")
                .e2e("span", "hash")
                .text("#")
                .into_any_element()
        })
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
        let again = sections(&all, &Scope::Space(server.id), &[archive], &filters(false, None));
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
