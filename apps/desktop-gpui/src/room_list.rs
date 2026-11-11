//! The room list after sign-in. A stub until the shell (task 4): one `.room`
//! per joined room, showing its name.

use gpui::{Context, IntoElement, Render, Window, div, prelude::*, px, rgb};
use morpheus_core::{RoomSummary, Session};

use crate::e2e::E2eExt as _;
use crate::theme;

pub struct RoomList {
    rooms: Vec<RoomSummary>,
}

impl RoomList {
    pub fn new(session: Session, cx: &mut Context<Self>) -> Self {
        let subscribed = session.room_list();
        cx.spawn(async move |this, cx| {
            let Ok(mut updates) = subscribed.await else {
                return;
            };
            while let Some(diffs) = updates.next().await {
                let alive = this
                    .update(cx, |this, cx| {
                        for diff in diffs {
                            apply(&mut this.rooms, diff);
                        }
                        cx.notify();
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();
        Self { rooms: Vec::new() }
    }
}

/// One batch change of the core's list, applied to a plain vector.
fn apply(rooms: &mut Vec<RoomSummary>, diff: morpheus_core::VectorDiff<RoomSummary>) {
    use morpheus_core::VectorDiff::*;
    match diff {
        Append { values } => rooms.extend(values),
        Clear => rooms.clear(),
        PushFront { value } => rooms.insert(0, value),
        PushBack { value } => rooms.push(value),
        PopFront => {
            if !rooms.is_empty() {
                rooms.remove(0);
            }
        }
        PopBack => {
            rooms.pop();
        }
        Insert { index, value } => rooms.insert(index.min(rooms.len()), value),
        Set { index, value } => {
            if let Some(slot) = rooms.get_mut(index) {
                *slot = value;
            }
        }
        Remove { index } => {
            if index < rooms.len() {
                rooms.remove(index);
            }
        }
        Truncate { length } => rooms.truncate(length),
        Reset { values } => *rooms = values.into_iter().collect(),
    }
}

impl Render for RoomList {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = div()
            .size_full()
            .p(px(8.0))
            .flex()
            .flex_col()
            .bg(rgb(theme::BG_SIDEBAR))
            .text_color(rgb(theme::TEXT_MUTED));
        for room in self.rooms.iter().filter(|r| !r.invited && !r.is_space) {
            list = list.child(
                div()
                    .px(px(8.0))
                    .py(px(6.0))
                    .child(room.name.clone())
                    .e2e("div", "room")
                    .text(room.name.clone()),
            );
        }
        list.e2e("div", "room-list")
    }
}
