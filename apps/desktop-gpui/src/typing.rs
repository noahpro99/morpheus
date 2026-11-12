//! The line under the messages that says who is typing
//! (`TypingLine` in `crates/ui/src/components/shell.rs`). The text is in
//! `view_model::typing`; this entity follows the room's typing notices.

use gpui::{Context, IntoElement, Render, Window, div, prelude::*, px, rgb};
use morpheus_core::{OwnedRoomId, Session};

use crate::{e2e::E2eExt as _, theme, view_model::typing};

pub struct TypingView {
    names: Vec<String>,
}

impl TypingView {
    pub fn new(session: Option<Session>, room: OwnedRoomId, cx: &mut Context<Self>) -> Self {
        if let Some(session) = session {
            let subscription = session.typing(room);
            cx.spawn(async move |this, cx| {
                let mut updates = match subscription.await {
                    Ok(updates) => updates,
                    Err(err) => {
                        eprintln!("morpheus-gpui: typing subscription: {err}");
                        return;
                    }
                };
                while let Some(names) = updates.next().await {
                    if this
                        .update(cx, |this, cx| {
                            if this.names != names {
                                this.names = names;
                                cx.notify();
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        Self { names: Vec::new() }
    }
}

impl Render for TypingView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        match typing::line(&self.names) {
            None => div().h(px(8.0)).flex_none().e2e("div", "typing empty"),
            Some(text) => div()
                .h(px(20.0))
                .flex_none()
                .px(px(20.0))
                .text_size(px(0.8 * theme::REM))
                .text_color(rgb(theme::text_muted()))
                .child(text.clone())
                .e2e("div", "typing")
                .text(text),
        }
    }
}
