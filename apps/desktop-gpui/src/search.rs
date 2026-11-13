//! The room search's results panel (`crates/ui/src/components/search.rs`), in
//! the side column over the member list, opened by Enter in the header's
//! search box (`Shell::search_input`) and closed when the box is cleared.
//! Finds messages this device has loaded; picking one jumps to it in the
//! chat. What it shows for each outcome is in `view_model::search`.

use gpui::{
    Context, EventEmitter, FontWeight, IntoElement, Render, ScrollHandle, Window, div,
    prelude::*, px, rgb,
};
use morpheus_core::{OwnedRoomId, SearchHit, Session};

use crate::{
    e2e::E2eExt as _,
    theme,
    view_model::search::{self, Results},
};

/// What the panel asks of the shell.
pub enum SearchEvent {
    /// A result was picked: show this event in the chat.
    Jump(String),
}

impl EventEmitter<SearchEvent> for SearchPanel {}

pub struct SearchPanel {
    session: Option<Session>,
    room_id: OwnedRoomId,
    /// The query last searched, and what came back (`None` while waiting).
    settled: String,
    outcome: Option<Result<Vec<SearchHit>, String>>,
    scroll: ScrollHandle,
}

impl SearchPanel {
    pub fn new(session: Option<Session>, room_id: OwnedRoomId) -> Self {
        Self {
            session,
            room_id,
            settled: String::new(),
            outcome: None,
            scroll: ScrollHandle::new(),
        }
    }

    /// Searches the room for `text` (the box's, after Enter).
    pub fn run(&mut self, text: String, cx: &mut Context<Self>) {
        self.settled = text.clone();
        self.outcome = None;
        if !search::is_query(&text) {
            self.outcome = Some(Ok(Vec::new()));
            cx.notify();
            return;
        }
        let Some(session) = self.session.clone() else {
            return;
        };
        let searching = session.search_room(self.room_id.clone(), text.clone(), search::LIMIT);
        cx.spawn(async move |this, cx| {
            let found = searching.await.map_err(|err| err.to_string());
            this.update(cx, |this, cx| {
                // An answer to an older question is dropped.
                if this.settled == text {
                    this.outcome = Some(found);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
}

impl Render for SearchPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut panel = div()
            .id("search-panel")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap(px(8.0))
            .p(px(12.0))
            .overflow_hidden()
            .line_height(gpui::relative(theme::LINE_HEIGHT))
            .child(
                div()
                    .text_size(px(0.8 * theme::REM))
                    .text_color(rgb(theme::text_muted()))
                    .child(search::NOTE)
                    .e2e("p", "muted search-note")
                    .text(search::NOTE),
            );
        match search::results(&self.settled, self.outcome.as_ref()) {
            Results::Idle => {}
            Results::Error(err) => {
                panel = panel.child(
                    div()
                        .text_color(rgb(theme::danger()))
                        .child(err.clone())
                        .e2e("p", "error")
                        .text(err),
                );
            }
            Results::Empty => {
                panel = panel.child(
                    div()
                        .text_color(rgb(theme::text_muted()))
                        .child(search::NONE_FOUND)
                        .e2e("p", "muted")
                        .text(search::NONE_FOUND),
                );
            }
            Results::Hits(rows) => {
                let mut list = div()
                    .id("search-results")
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll);
                for (ix, row) in rows.into_iter().enumerate() {
                    let id = row.event_id.clone();
                    list = list.child(
                        div()
                            .id(("search-hit", ix))
                            .flex_none()
                            .px(px(10.0))
                            .py(px(8.0))
                            .rounded(px(theme::RADIUS))
                            .bg(rgb(theme::bg_chat()))
                            .cursor_pointer()
                            .hover(|s| s.bg(gpui::rgba(theme::bg_hover())))
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(SearchEvent::Jump(id.clone()))
                            }))
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .gap(px(8.0))
                                    .text_size(px(0.8 * theme::REM))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(row.sender.clone())
                                            .e2e("span", "search-sender")
                                            .text(row.sender.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_color(rgb(theme::text_faint()))
                                            .child(row.when.clone())
                                            .e2e("span", "search-time")
                                            .text(row.when.clone()),
                                    )
                                    .e2e("div", "search-meta"),
                            )
                            .child(
                                div()
                                    .mt(px(4.0))
                                    .text_size(px(0.9 * theme::REM))
                                    .text_color(rgb(theme::text_muted()))
                                    .child(row.body.clone())
                                    .e2e("p", "search-body")
                                    .text(row.body.clone()),
                            )
                            .e2e("li", "search-hit"),
                    );
                }
                panel = panel.child(list.e2e("ul", "search-results"));
            }
        }
        panel.e2e("div", "search-panel")
    }
}
