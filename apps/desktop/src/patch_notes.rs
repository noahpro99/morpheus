//! Patch notes (#172): the running version's GitHub release notes, fetched
//! once in the background, shown in Settings > Help & Feedback, and on the
//! first launch after an update in a "What's new" popup (once).

use std::{rc::Rc, sync::Mutex};

use gpui::{AnyElement, Context, IntoElement, Window, div, prelude::*, px, rgb};
use morpheus_core::Session;

use crate::{
    dialogs::{actions, dialog_box, dialog_button, heading, overlay},
    e2e::E2eExt as _,
    message::{Base, LinkHandler, blocks_el},
    shell::{Shell, open_external},
    theme,
    view_model::{
        about::VERSION,
        release_notes as vm,
        rich::{self, Block},
    },
};

enum State {
    Loading,
    Ready { blocks: Vec<Block>, url: Option<String> },
    Failed,
}

static NOTES: Mutex<State> = Mutex::new(State::Loading);

fn page() -> String {
    let url = match NOTES.lock().as_deref() {
        Ok(State::Ready { url: Some(url), .. }) => Some(url.clone()),
        _ => None,
    };
    url.unwrap_or_else(|| vm::release_page(VERSION))
}

fn links() -> LinkHandler {
    Rc::new(|url: &str, cx: &mut gpui::App| open_external(url, cx))
}

/// The notes (or what stands in for them) as an element.
fn notes_element(key: &str) -> AnyElement {
    let base = Base { color: theme::text(), italic: false };
    let muted = |text: &str| {
        div()
            .text_color(rgb(theme::text_muted()))
            .child(text.to_owned())
            .e2e("p", "muted")
            .text(text)
            .into_any_element()
    };
    match NOTES.lock().as_deref() {
        Ok(State::Ready { blocks, .. }) => blocks_el(blocks, key, base, &links())
            .e2e("div", "rich")
            .text(rich::text_of(blocks))
            .into_any_element(),
        Ok(State::Failed) => muted(vm::UNAVAILABLE),
        _ => muted("Loading the patch notes…"),
    }
}

/// The Help page's Patch notes text (its link is built beside it).
pub fn help_notes() -> AnyElement {
    div()
        .child(notes_element("help-notes"))
        .e2e("div", "patch-notes")
        .text(match NOTES.lock().as_deref() {
            Ok(State::Ready { blocks, .. }) => rich::text_of(blocks),
            Ok(State::Failed) => vm::UNAVAILABLE.to_owned(),
            _ => "Loading the patch notes…".to_owned(),
        })
        .into_any_element()
}

pub fn release_page() -> String {
    page()
}

impl Shell {
    /// Fetch this version's notes in the background; after an update, show
    /// them once. A first install only records the version.
    pub fn start_patch_notes(&mut self, session: &Session, cx: &mut Context<Self>) {
        let dir = crate::data_dir();
        let seen = vm::seen(&dir);
        if seen.is_none() {
            let _ = vm::mark_seen(&dir, VERSION);
        }
        let due = vm::popup_due(seen.as_deref(), VERSION);
        let fetching = session.release_notes(VERSION);
        cx.spawn(async move |this, cx| {
            let found = fetching.await;
            let ready = match found {
                Ok(notes) => {
                    let blocks = vm::blocks(&notes.body);
                    if let Ok(mut state) = NOTES.lock() {
                        *state = State::Ready { blocks, url: notes.url };
                    }
                    true
                }
                Err(err) => {
                    eprintln!("morpheus-desktop: patch notes: {err}");
                    if let Ok(mut state) = NOTES.lock() {
                        *state = State::Failed;
                    }
                    false
                }
            };
            this.update(cx, |this, cx| {
                if ready && due {
                    this.whats_new = true;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn dismiss_whats_new(&mut self, cx: &mut Context<Self>) {
        if !self.whats_new {
            return;
        }
        self.whats_new = false;
        if let Err(err) = vm::mark_seen(&crate::data_dir(), VERSION) {
            eprintln!("morpheus-desktop: saving the shown version: {err}");
        }
        cx.notify();
    }

    /// The "What's new in X" popup, while it is due.
    pub fn whats_new_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.whats_new {
            return None;
        }
        let text = match NOTES.lock().as_deref() {
            Ok(State::Ready { blocks, .. }) => rich::text_of(blocks),
            _ => return None,
        };
        let title = vm::popup_title(VERSION);
        let more = dialog_button("whats-new-more", "View release on GitHub", false)
            .text_color(rgb(theme::text()))
            .on_click(cx.listener(|_, _, _, cx| open_external(&page(), cx)))
            .e2e("button", "secondary whats-new-more")
            .text("View release on GitHub")
            .into_any_element();
        let close = dialog_button("whats-new-close", "Close", false)
            .text_color(rgb(theme::text()))
            .on_click(cx.listener(|this, _, _, cx| this.dismiss_whats_new(cx)))
            .e2e("button", "secondary whats-new-close")
            .text("Close")
            .into_any_element();
        let body = div()
            .id("whats-new-scroll")
            .max_h(px(360.0))
            .overflow_y_scroll()
            .child(notes_element("whats-new"))
            .into_any_element();
        let boxed = dialog_box()
            .child(heading(&title))
            .child(div().child(body).e2e("div", "whats-new-body").text(text))
            .child(actions(vec![close, more]))
            .e2e("div", "message-dialog whats-new");
        let this = cx.weak_entity();
        Some(overlay(window, cx, boxed.into_any_element(), move |_, cx| {
            this.update(cx, |this, cx| this.dismiss_whats_new(cx)).ok();
        }))
    }
}
