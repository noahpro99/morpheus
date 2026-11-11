//! A single-line text input: focus, typing, backspace and delete, the arrow
//! keys, Home and End, Ctrl+A (select all; the next character replaces it),
//! a password mask and Enter. No IME composition and no clipboard yet.

use gpui::{
    Context, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent, MouseButton, Render,
    SharedString, Window, div, prelude::*, px, rgb,
};

use crate::e2e::E2eExt as _;
use crate::theme;

/// Enter was pressed in the input.
pub struct Submit;

impl EventEmitter<Submit> for TextInput {}

/// The text changed (typed, deleted or set).
pub struct Changed;

impl EventEmitter<Changed> for TextInput {}

pub struct TextInput {
    focus: FocusHandle,
    name: &'static str,
    placeholder: SharedString,
    password: bool,
    value: String,
    /// Byte offset of the caret in `value`.
    caret: usize,
    all_selected: bool,
}

impl TextInput {
    pub fn new(
        name: &'static str,
        placeholder: &str,
        password: bool,
        value: &str,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus: cx.focus_handle(),
            name,
            placeholder: placeholder.to_owned().into(),
            password,
            value: value.to_owned(),
            caret: value.len(),
            all_selected: false,
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn focus(&self, window: &mut Window) {
        window.focus(&self.focus);
    }

    fn prev(&self) -> usize {
        self.value[..self.caret]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    fn next(&self) -> usize {
        self.value[self.caret..]
            .chars()
            .next()
            .map_or(self.caret, |c| self.caret + c.len_utf8())
    }

    fn insert(&mut self, text: &str) {
        if self.all_selected {
            self.value.clear();
            self.caret = 0;
            self.all_selected = false;
        }
        self.value.insert_str(self.caret, text);
        self.caret += text.len();
    }

    fn key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        let before = self.value.clone();
        if m.control && !m.alt && keystroke.key == "a" {
            self.all_selected = true;
            self.caret = self.value.len();
            cx.notify();
            return;
        }
        match keystroke.key.as_str() {
            "enter" => {
                cx.emit(Submit);
                return;
            }
            "backspace" => {
                if self.all_selected {
                    self.value.clear();
                    self.caret = 0;
                    self.all_selected = false;
                } else if self.caret > 0 {
                    let start = self.prev();
                    self.value.replace_range(start..self.caret, "");
                    self.caret = start;
                }
            }
            "delete" => {
                if self.all_selected {
                    self.value.clear();
                    self.caret = 0;
                    self.all_selected = false;
                } else if self.caret < self.value.len() {
                    let end = self.next();
                    self.value.replace_range(self.caret..end, "");
                }
            }
            "left" => {
                self.caret = if self.all_selected { 0 } else { self.prev() };
                self.all_selected = false;
            }
            "right" => {
                self.caret = if self.all_selected {
                    self.value.len()
                } else {
                    self.next()
                };
                self.all_selected = false;
            }
            "home" => {
                self.caret = 0;
                self.all_selected = false;
            }
            "end" => {
                self.caret = self.value.len();
                self.all_selected = false;
            }
            _ => {
                if m.control || m.alt || m.platform {
                    return;
                }
                match &keystroke.key_char {
                    Some(text) if !text.chars().any(char::is_control) => self.insert(text),
                    _ => return,
                }
            }
        }
        cx.notify();
        if self.value != before {
            cx.emit(Changed);
        }
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.is_focused(window);
        let shown = |text: &str| -> String {
            if self.password {
                "\u{2022}".repeat(text.chars().count())
            } else {
                text.to_owned()
            }
        };
        let caret = || div().w(px(1.0)).h(px(16.0)).bg(rgb(theme::TEXT));
        let mut text = div().flex().flex_row().items_center().overflow_hidden();
        if self.value.is_empty() {
            if focused {
                text = text.child(caret());
            }
            text = text.child(
                div()
                    .text_color(rgb(theme::TEXT_FAINT))
                    .child(self.placeholder.clone()),
            );
        } else if self.all_selected {
            text = text.child(
                div()
                    .bg(rgb(theme::ACCENT_STRONG))
                    .child(shown(&self.value)),
            );
        } else {
            text = text.child(shown(&self.value[..self.caret]));
            if focused {
                text = text.child(caret());
            }
            text = text.child(shown(&self.value[self.caret..]));
        }
        let entity = cx.entity();
        let focus = self.focus.clone();
        div()
            .id(self.name)
            .track_focus(&self.focus)
            .on_key_down(move |event, _, cx| {
                entity.update(cx, |input, cx| input.key_down(event, cx));
            })
            .on_mouse_down(MouseButton::Left, move |_, window, _| window.focus(&focus))
            .px(px(12.0))
            .py(px(10.0))
            .rounded(px(theme::RADIUS))
            .border_1()
            .border_color(if focused {
                rgb(theme::ACCENT)
            } else {
                rgb(theme::BG_INPUT)
            })
            .bg(rgb(theme::BG_INPUT))
            .child(text)
            .e2e("input", "")
            .attr("name", self.name)
            .attr("type", if self.password { "password" } else { "text" })
            .attr("value", self.value.clone())
            .focusable(&self.focus)
    }
}
