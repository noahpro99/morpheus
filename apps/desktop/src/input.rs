//! A text box: single-line (the sign-in fields, with a password mask) or
//! multi-line and growing (the message box). Typing, deleting, the arrow
//! keys with Shift to select (Ctrl for words, Home and End), Ctrl+A, copy,
//! cut and paste through the clipboard, and the mouse: click to place the
//! caret, drag to select, double click for a word, triple click for a line.
//! No IME composition.
//!
//! What the text and selection are lives in [`TextBuffer`] (no toolkit in
//! it); this draws it and maps keys and the mouse to it. The text is drawn
//! by gpui's `StyledText`, whose layout answers where the caret goes and
//! which index a click is at.

use crate::view_model::i18n::t;
use std::{
    cell::{Cell, RefCell},
    ops::Range,
    rc::Rc,
};

use gpui::{
    App, Bounds, ClipboardEntry, ClipboardItem, Context, CursorStyle, DispatchPhase, Element,
    ElementId, Entity, EventEmitter, FocusHandle, Focusable, GlobalElementId, HighlightStyle,
    Hitbox, HitboxBehavior, InspectorElementId, IntoElement, KeyDownEvent, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Render, ScrollHandle, SharedString,
    StyledText, TextLayout, Window, div, fill, point, prelude::*, px, rgb, rgba, size,
};

use crate::{
    e2e::E2eExt as _,
    theme,
    view_model::{
        text_buffer::TextBuffer,
        vim::{Key as VimKey, Mode as VimMode, Vim},
    },
};

/// Enter was pressed in the input (a plain Enter in the message box).
pub struct Submit;

impl EventEmitter<Submit> for TextInput {}

/// The text changed by typing, deleting, cutting or pasting (not by `set_value`).
pub struct Changed;

impl EventEmitter<Changed> for TextInput {}

/// A key the owner may want instead of the input: sent when the input is
/// told to capture navigation (a list of suggestions is open), and for
/// Escape, and for Up in an empty message box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavKey {
    Up,
    Down,
    Enter,
    Tab,
    Escape,
}

pub struct Nav(pub NavKey);

impl EventEmitter<Nav> for TextInput {}

/// A picture on the clipboard when pasting.
#[derive(Clone, Debug)]
pub struct PastedFile {
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

pub struct Pasted(pub Vec<PastedFile>);

impl EventEmitter<Pasted> for TextInput {}

/// The mask drawn for each character of a password.
const MASK: char = '\u{2022}';
const LINE_HEIGHT: f32 = theme::REM * theme::LINE_HEIGHT;

pub struct TextInput {
    focus: FocusHandle,
    name: &'static str,
    tag: &'static str,
    class: &'static str,
    placeholder: SharedString,
    password: bool,
    /// Most lines it grows to before it scrolls; 1 for a single-line field.
    max_lines: usize,
    /// No box of its own (the message box sits in the composer's).
    bare: bool,
    /// A dialog's field: an always-visible elevated border, no focus ring.
    framed: bool,
    /// The settings search box: smaller, no ring.
    compact: bool,
    /// A bare box in a bar (the header's search): little padding.
    slim: bool,
    disabled: bool,
    buf: TextBuffer,
    /// Navigation keys go to the owner (suggestions are open).
    capture_nav: bool,
    selecting: bool,
    /// The wanted x of Up and Down, kept while moving by lines.
    goal_x: Option<Pixels>,
    layout: Rc<RefCell<Option<TextLayout>>>,
    /// The colour the text was last laid out in (inherited from the
    /// parents), as `rrggbb`, for the test bridge.
    drawn_colour: Rc<Cell<Option<u32>>>,
    scroll: ScrollHandle,
    /// The caret when the view last followed it, to follow only a moved one.
    followed: Rc<RefCell<Option<(usize, usize)>>>,
    /// A message box that vim mode (when on) makes modal; its engine and
    /// undo history (the history serves every box).
    vim_capable: bool,
    vim: Vim,
    was_focused: bool,
    /// A message box: misspelled words get a wavy underline and a menu.
    spell: bool,
    /// The misspelled words of the text, for the text and the spell check's
    /// state they were found with.
    spell_cache: (String, u64, Vec<Range<usize>>),
    spell_menu: Option<SpellMenu>,
    spell_polling: bool,
}

/// The right-click menu of a message box.
struct SpellMenu {
    at: gpui::Point<Pixels>,
    /// The misspelled word under the click, and its corrections.
    word: Option<(Range<usize>, Vec<String>)>,
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
            tag: "input",
            class: "",
            placeholder: placeholder.to_owned().into(),
            password,
            max_lines: 1,
            bare: false,
            framed: false,
            compact: false,
            slim: false,
            disabled: false,
            buf: TextBuffer::new(value),
            capture_nav: false,
            selecting: false,
            goal_x: None,
            layout: Rc::default(),
            drawn_colour: Rc::default(),
            scroll: ScrollHandle::new(),
            followed: Rc::default(),
            vim_capable: false,
            vim: Vim::new(),
            was_focused: false,
            spell: false,
            spell_cache: (String::new(), u64::MAX, Vec::new()),
            spell_menu: None,
            spell_polling: false,
        }
    }

    /// Grows with its lines up to `max_lines`; Shift+Enter adds a line.
    pub fn multiline(mut self, max_lines: usize) -> Self {
        self.max_lines = max_lines.max(2);
        self
    }

    /// The test bridge's classes for the field (`settings-search`).
    pub fn with_class(mut self, class: &'static str) -> Self {
        self.class = class;
        self
    }

    /// The settings search box (`.settings-search`): small padding, no ring.
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    /// A dialog's field (`.message-dialog input`): bordered, no focus colour.
    pub fn framed(mut self) -> Self {
        self.framed = true;
        self
    }

    /// Drawn without its own box, as `tag` with `class` for the test bridge.
    pub fn bare(mut self, tag: &'static str, class: &'static str) -> Self {
        self.bare = true;
        self.tag = tag;
        self.class = class;
        self
    }

    /// A bare box that is one bar line high.
    pub fn slim(mut self) -> Self {
        self.slim = true;
        self
    }

    /// A message box: vim mode, when it is on, makes it modal.
    pub fn vim(mut self) -> Self {
        self.vim_capable = true;
        self
    }

    /// A message box: misspelled words are underlined (a setting turns it off).
    pub fn spellcheck(mut self) -> Self {
        self.spell = true;
        self
    }

    /// Whether vim mode rules this box now.
    fn vim_on(&self) -> bool {
        self.vim_capable
            && self.max_lines > 1
            && !self.password
            && !self.disabled
            && crate::vim::enabled()
    }

    /// The mode, when vim mode rules this box.
    pub fn vim_mode(&self) -> Option<VimMode> {
        self.vim_on().then(|| self.vim.mode())
    }

    /// "-- NORMAL --" and what is typed so far of a command, when vim mode
    /// rules this box.
    pub fn vim_label(&self) -> Option<String> {
        let mode = self.vim_mode()?;
        let name = match mode {
            VimMode::Insert => "INSERT",
            VimMode::Normal => "NORMAL",
            VimMode::Visual => "VISUAL",
        };
        let pending = self.vim.pending();
        Some(if pending.is_empty() {
            format!("-- {name} --")
        } else {
            format!("-- {name} -- {pending}")
        })
    }

    /// Whether Escape is this box's to take: vim mode is on and the box is
    /// in insert or visual mode, or has a command half typed (the shell's
    /// Escape handling leaves it alone).
    pub fn wants_escape(&self, window: &Window) -> bool {
        self.vim_on()
            && self.focus.is_focused(window)
            && (self.vim.mode() != VimMode::Normal || !self.vim.pending().is_empty())
    }

    /// Whether Ctrl+R is redo here, not the app's reload: vim mode is on and
    /// the box is in normal or visual mode.
    pub fn wants_ctrl_r(&self, window: &Window) -> bool {
        self.vim_on() && self.focus.is_focused(window) && self.vim.mode() != VimMode::Insert
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus.is_focused(window) && !self.disabled
    }

    pub fn value(&self) -> &str {
        self.buf.text()
    }

    pub fn focus(&self, window: &mut Window) {
        window.focus(&self.focus);
    }

    /// Replaces the text (the caret goes to the end); no `Changed`.
    pub fn set_value(&mut self, text: &str, cx: &mut Context<Self>) {
        self.buf.set_text(text);
        self.goal_x = None;
        self.vim.history.clear();
        self.vim.settle(&mut self.buf);
        cx.notify();
    }

    pub fn set_placeholder(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.placeholder.as_ref() != text {
            self.placeholder = text.to_owned().into();
            cx.notify();
        }
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled != disabled {
            self.disabled = disabled;
            cx.notify();
        }
    }

    /// Tab stays with this box: the message composer (and its suggestions)
    /// use it themselves and never pass the focus on.
    fn keeps_tab(&self) -> bool {
        self.capture_nav || (self.vim_capable && self.max_lines > 1)
    }

    /// Whether Up, Down, Enter and Tab go to the owner (as [`Nav`]).
    pub fn set_capture_nav(&mut self, capture: bool) {
        self.capture_nav = capture;
    }

    /// The text as drawn, and whether that is the placeholder.
    fn shown(&self) -> (SharedString, bool) {
        if self.buf.is_empty() {
            return (self.placeholder.clone(), true);
        }
        if self.password {
            let count = self.buf.text().chars().count();
            return (MASK.to_string().repeat(count).into(), false);
        }
        (self.buf.text().to_owned().into(), false)
    }

    /// A byte offset of the text as the drawn text's.
    fn to_shown(&self, offset: usize) -> usize {
        if self.password {
            self.buf.text()[..offset].chars().count() * MASK.len_utf8()
        } else {
            offset
        }
    }

    /// An offset of the drawn text as the text's.
    fn from_shown(&self, shown: usize) -> usize {
        if !self.password {
            return shown.min(self.buf.text().len());
        }
        let chars = shown / MASK.len_utf8();
        self.buf
            .text()
            .char_indices()
            .nth(chars)
            .map_or(self.buf.text().len(), |(i, _)| i)
    }

    /// What the text is at the point, for a click: the nearer character edge.
    fn index_at(&self, position: gpui::Point<Pixels>) -> Option<usize> {
        let layout = self.layout.borrow().clone()?;
        if self.buf.is_empty() {
            return Some(0);
        }
        let shown = layout
            .index_for_position(position)
            .unwrap_or_else(|nearest| nearest);
        let real = self.from_shown(shown);
        // The index is of the character under the point: past its middle,
        // the caret belongs after it.
        let next = self.buf.next_boundary(real);
        if next != real
            && let (Some(a), Some(b)) = (
                layout.position_for_index(self.to_shown(real)),
                layout.position_for_index(self.to_shown(next)),
            )
            && (a.y - b.y).abs() < px(1.0)
            && position.x > (a.x + b.x) / 2.0
        {
            return Some(next);
        }
        Some(real)
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        window.focus(&self.focus);
        let Some(index) = self.index_at(event.position) else {
            return;
        };
        self.goal_x = None;
        match event.click_count {
            0 | 1 => self.buf.move_to(index, event.modifiers.shift),
            2 => {
                let word = self.buf.word_at(index);
                self.buf.select_range(word);
            }
            _ => {
                let line = self.buf.line_start(index)..self.buf.line_end(index);
                self.buf.select_range(line);
            }
        }
        self.selecting = event.click_count <= 1;
        self.vim.history.break_run();
        if self.vim_on() && self.vim.mode() == VimMode::Normal {
            self.vim.settle(&mut self.buf);
        }
        cx.notify();
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.selecting {
            return;
        }
        if !event.dragging() {
            self.selecting = false;
            return;
        }
        if let Some(index) = self.index_at(event.position) {
            self.buf.move_to(index, true);
            cx.notify();
        }
    }

    /// Up or Down by a visual line, keeping the column.
    fn move_vertically(&mut self, up: bool, extend: bool) {
        let layout = self.layout.borrow().clone();
        let Some(layout) = layout else { return };
        let shown_head = self.to_shown(self.buf.head());
        let Some(at) = layout.position_for_index(shown_head) else {
            return;
        };
        let goal = *self.goal_x.get_or_insert(at.x);
        let line = layout.line_height();
        let target = point(
            goal,
            if up {
                at.y - line / 2.0
            } else {
                at.y + line * 1.5
            },
        );
        let first_row = at.y < layout.bounds().top() + line;
        let moved = if up && first_row {
            Some(0)
        } else {
            match layout.index_for_position(target) {
                Ok(i) => Some(i),
                // Below the last row: the end.
                Err(_) if !up => Some(self.to_shown(self.buf.text().len())),
                Err(i) => Some(i),
            }
        };
        if let Some(shown) = moved {
            let real = self.from_shown(shown);
            self.buf.move_to(real, extend);
        }
    }

    fn copy(&self, cx: &mut Context<Self>) {
        if !self.buf.has_selection() || self.password {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(
            self.buf.selected_text().to_owned(),
        ));
    }

    /// Ctrl+V with nothing focused: the box takes the caret and the
    /// clipboard (text, or pictures handed to the owner), as a paste in it.
    pub fn paste_focused(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus);
        let before = self.buf.clone();
        let was = self.buf.text().to_owned();
        self.paste(cx);
        self.goal_x = None;
        cx.notify();
        if self.buf.text() != was {
            self.vim.history.record(&before);
            cx.emit(Changed);
        }
    }

    fn paste(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(item) = cx.read_from_clipboard() else {
            return false;
        };
        let text = item.text().filter(|t| !t.is_empty());
        if text.is_none() {
            let files: Vec<PastedFile> = item
                .entries()
                .iter()
                .filter_map(|entry| match entry {
                    ClipboardEntry::Image(image) => {
                        let mime = image.format.mime_type();
                        Some(PastedFile {
                            name: format!("image.{}", mime.rsplit('/').next().unwrap_or("png")),
                            mime: mime.to_owned(),
                            bytes: image.bytes.clone(),
                        })
                    }
                    ClipboardEntry::String(_) => None,
                })
                .collect();
            if !files.is_empty() {
                cx.emit(Pasted(files));
            }
            return false;
        }
        let text = text.unwrap_or_default();
        self.insert_text(&text);
        true
    }

    /// Typed or pasted text: a single-line box takes its lines as spaces.
    fn insert_text(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if self.max_lines > 1 {
            self.buf.insert(&text);
        } else {
            self.buf.insert(&text.replace('\n', " "));
        }
        self.goal_x = None;
    }

    /// A menu item or shortcut of the macOS Edit menu (`mac_menu`): the same
    /// as pressing the shortcut key, Ctrl or Cmd with `key`.
    fn shortcut(&mut self, key: &str, cx: &mut Context<Self>) {
        let keystroke = gpui::Keystroke {
            modifiers: gpui::Modifiers::secondary_key(),
            key: key.to_owned(),
            key_char: None,
        };
        self.key_down(
            &KeyDownEvent {
                keystroke,
                is_held: false,
            },
            cx,
        );
    }

    /// A key in vim mode: whether the engine took it (otherwise the box
    /// types or edits as usual: insert mode, and Ctrl combos such as copy).
    fn vim_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        let cmd = m.control || m.platform;
        let mode = self.vim.mode();
        let typed = typed_text(keystroke)
            .filter(|t| t.chars().count() == 1 && !t.chars().any(char::is_control))
            .and_then(|t| t.chars().next());
        let key = match keystroke.key.as_str() {
            "escape" => Some(VimKey::Esc),
            "enter" if !m.shift && !cmd && !m.alt => Some(VimKey::Enter),
            "backspace" if !m.shift && !cmd && !m.alt => Some(VimKey::Backspace),
            "left" if mode != VimMode::Insert && !cmd => Some(VimKey::Char('h')),
            "right" if mode != VimMode::Insert && !cmd => Some(VimKey::Char('l')),
            "up" if mode != VimMode::Insert && !cmd => Some(VimKey::Char('k')),
            "down" if mode != VimMode::Insert && !cmd => Some(VimKey::Char('j')),
            "home" if mode != VimMode::Insert && !cmd => Some(VimKey::Char('0')),
            "end" if mode != VimMode::Insert && !cmd => Some(VimKey::Char('$')),
            k if cmd && !m.alt && k.chars().count() == 1 => k.chars().next().map(VimKey::Ctrl),
            _ if !cmd && !m.alt => typed.map(VimKey::Char),
            _ => None,
        };
        let Some(key) = key else {
            // In normal and visual mode an unmapped key types nothing (the
            // Ctrl combos, copy and paste, are the box's).
            return mode != VimMode::Insert && !cmd;
        };
        // Escape with suggestions open closes them first (the owner's).
        if mode == VimMode::Insert && key == VimKey::Esc && self.capture_nav {
            return false;
        }
        if mode != VimMode::Insert && matches!(key, VimKey::Char('p' | 'P')) {
            let clip = cx.read_from_clipboard().and_then(|item| item.text());
            self.vim.set_clipboard(clip.as_deref());
        }
        let out = self.vim.key(&mut self.buf, key);
        if !out.handled {
            return false;
        }
        self.goal_x = None;
        if let Some(text) = self.vim.take_yank() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        cx.notify();
        if out.changed {
            cx.emit(Changed);
        }
        if out.submit {
            cx.emit(Submit);
        }
        if out.escape {
            cx.emit(Nav(NavKey::Escape));
        }
        true
    }

    fn key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if self.spell_menu.take().is_some() {
            cx.notify();
            if event.keystroke.key == "escape" {
                return;
            }
        }
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        let multiline = self.max_lines > 1;
        let key = keystroke.key.as_str();
        // The shortcut key: Ctrl, or Cmd on a Mac.
        let cmd = m.control || m.platform;
        let word = m.control || m.alt;
        let was = (self.buf.text().to_owned(), self.buf.selection());
        let before = self.buf.clone();
        let mut typing = false;
        let mut undid = false;

        if self.vim_on() && self.vim_key(event, cx) {
            return;
        }

        // Keys the owner takes over.
        let nav = match key {
            "escape" => Some(NavKey::Escape),
            "up" if self.capture_nav => Some(NavKey::Up),
            "down" if self.capture_nav => Some(NavKey::Down),
            "enter" if self.capture_nav => Some(NavKey::Enter),
            "tab" if self.capture_nav => Some(NavKey::Tab),
            "up" if multiline && self.buf.is_empty() && !m.shift && !cmd && !m.alt => {
                Some(NavKey::Up)
            }
            _ => None,
        };
        if let Some(nav) = nav {
            cx.emit(Nav(nav));
            return;
        }

        if cmd && !m.alt {
            match key {
                "a" => self.buf.select_all(),
                "c" => {
                    self.copy(cx);
                    return;
                }
                "x" => {
                    if self.buf.has_selection() && !self.password {
                        self.copy(cx);
                        self.buf.take_selection();
                    }
                }
                "v" => {
                    self.paste(cx);
                }
                "z" => {
                    undid = if m.shift {
                        self.vim.history.redo(&mut self.buf)
                    } else {
                        self.vim.history.undo(&mut self.buf)
                    };
                }
                "home" => self.buf.home(m.shift, true),
                "end" => self.buf.end(m.shift, true),
                "left" => self.buf.left(m.shift, true),
                "right" => self.buf.right(m.shift, true),
                "backspace" => {
                    self.buf.backspace(true);
                }
                "delete" => {
                    self.buf.delete(true);
                }
                "enter" if multiline => {
                    cx.emit(Submit);
                    return;
                }
                _ => return,
            }
        } else {
            match key {
                "enter" => {
                    if multiline && m.shift {
                        self.insert_text("\n");
                    } else {
                        cx.emit(Submit);
                        return;
                    }
                }
                "backspace" => {
                    typing = true;
                    self.vim.backspaced();
                    self.buf.backspace(m.alt);
                }
                "delete" => {
                    typing = true;
                    self.buf.delete(m.alt);
                }
                "left" => self.buf.left(m.shift, word),
                "right" => self.buf.right(m.shift, word),
                "up" if multiline => self.move_vertically(true, m.shift),
                "down" if multiline => self.move_vertically(false, m.shift),
                "up" => self.buf.home(m.shift, true),
                "down" => self.buf.end(m.shift, true),
                "home" => self.buf.home(m.shift, false),
                "end" => self.buf.end(m.shift, false),
                _ => {
                    if (m.control || m.alt || m.platform) && !types_with_modifiers(keystroke) {
                        return;
                    }
                    match typed_text(keystroke) {
                        Some(text) if !text.chars().any(char::is_control) => {
                            typing = true;
                            self.vim.typed(&text);
                            self.insert_text(&text)
                        }
                        _ => return,
                    }
                }
            }
        }
        if !matches!(key, "up" | "down") {
            self.goal_x = None;
        }
        if self.vim_on() {
            self.vim.settle(&mut self.buf);
        }
        cx.notify();
        if self.buf.text() != was.0 {
            if !undid && !self.password {
                if typing {
                    self.vim.history.record_typing(&before);
                } else {
                    self.vim.history.record(&before);
                }
            }
            cx.emit(Changed);
        } else {
            self.vim.history.break_run();
        }
    }

    /// Scrolls the box so the caret shows, once per caret move.
    fn follow_caret(&self, caret: gpui::Point<Pixels>, window: &mut Window) {
        let key = (self.buf.head(), self.buf.text().len());
        if *self.followed.borrow() == Some(key) {
            return;
        }
        *self.followed.borrow_mut() = Some(key);
        let view = self.scroll.bounds();
        if view.size.height <= px(0.0) {
            return;
        }
        let mut offset = self.scroll.offset();
        let line = px(LINE_HEIGHT);
        if caret.y < view.top() {
            offset.y += view.top() - caret.y;
        } else if caret.y + line > view.bottom() {
            offset.y -= caret.y + line - view.bottom();
        } else {
            return;
        }
        self.scroll.set_offset(offset);
        window.refresh();
    }
}

impl TextInput {
    /// Whether spell check marks this box now.
    fn spell_on(&self) -> bool {
        self.spell && !self.password && !self.disabled && crate::spell::enabled()
    }

    /// The misspelled words now (as byte ranges): found again only when the
    /// text or the spell check's state changed, and leaving out the word the
    /// caret is at the end of, which is still being typed.
    fn marks(&mut self, cx: &mut Context<Self>) -> Vec<Range<usize>> {
        if !self.spell_on() {
            return Vec::new();
        }
        let generation = crate::spell::generation();
        if self.spell_cache.0 != self.buf.text() || self.spell_cache.1 != generation {
            self.spell_cache = (
                self.buf.text().to_owned(),
                generation,
                crate::spell::misspellings(self.buf.text()),
            );
        }
        // The dictionary builds in the background: look again until it is there.
        if !crate::spell::ready() && !self.spell_polling {
            self.spell_polling = true;
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(100))
                        .await;
                    let done = this
                        .update(cx, |this, cx| {
                            let ready = crate::spell::ready();
                            if ready {
                                this.spell_polling = false;
                                cx.notify();
                            }
                            ready
                        })
                        .unwrap_or(true);
                    if done {
                        break;
                    }
                }
            })
            .detach();
        }
        let typing_at = (!self.buf.has_selection()).then_some(self.buf.head());
        self.spell_cache
            .2
            .iter()
            .filter(|r| {
                !typing_at.is_some_and(|c| {
                    crate::view_model::spellcheck::being_typed(r.end, c)
                })
            })
            .cloned()
            .collect()
    }

    /// A right click in a message box: the menu, with corrections when the
    /// click is on a misspelled word.
    fn open_spell_menu(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.spell || self.disabled || self.password {
            return;
        }
        window.focus(&self.focus);
        let marks = self.marks(cx);
        let word = self.index_at(event.position).and_then(|at| {
            marks
                .into_iter()
                .find(|r| r.start <= at && at <= r.end)
                .map(|r| {
                    let suggestions = crate::spell::suggest(&self.buf.text()[r.clone()]);
                    (r, suggestions)
                })
        });
        // Kept inside the window: rows (35 px each, a rule and padding) that would
        // run off the bottom make the menu open upwards.
        let rows = 3 + word.as_ref().map_or(0, |(_, s)| s.len().max(1) + 2);
        let (width, height) = (200.0, rows as f32 * 35.0 + 20.0);
        let view = window.viewport_size();
        let at = point(
            px(f32::from(event.position.x).min(f32::from(view.width) - width - 8.0).max(8.0)),
            px(f32::from(event.position.y).min(f32::from(view.height) - height - 8.0).max(8.0)),
        );
        self.spell_menu = Some(SpellMenu { at, word });
        cx.notify();
    }

    fn spell_pick(&mut self, choice: SpellChoice, cx: &mut Context<Self>) {
        let Some(menu) = self.spell_menu.take() else {
            return;
        };
        cx.notify();
        match choice {
            SpellChoice::Replace(text) => {
                if let Some((range, _)) = menu.word {
                    let before = self.buf.clone();
                    self.buf.select_range(range);
                    self.insert_text(&text);
                    self.vim.history.record(&before);
                    cx.emit(Changed);
                }
            }
            SpellChoice::Add => {
                if let Some((range, _)) = menu.word {
                    crate::spell::add_word(&crate::data_dir(), &self.buf.text()[range]);
                }
            }
            SpellChoice::Ignore => {
                if let Some((range, _)) = menu.word {
                    crate::spell::ignore(&self.buf.text()[range]);
                }
            }
            SpellChoice::Cut => self.shortcut("x", cx),
            SpellChoice::Copy => self.shortcut("c", cx),
            SpellChoice::Paste => self.shortcut("v", cx),
        }
    }

    /// The right-click menu, over a backdrop that closes it.
    fn spell_menu_el(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        use gpui::{AnyElement, ClickEvent};
        let menu = self.spell_menu.as_ref()?;
        let has_selection = self.buf.has_selection();
        let has_word = menu.word.is_some();
        let row = |id: String, label: String, choice: Option<SpellChoice>, cx: &mut Context<Self>| -> AnyElement {
            let live = choice.is_some();
            let mut el = crate::menu::item_base(gpui::ElementId::from(SharedString::from(id)))
                .px(px(10.0))
                .text_color(rgb(if live {
                    theme::text_muted()
                } else {
                    theme::text_faint()
                }))
                .child(
                    div()
                        .child(label.clone())
                        .e2e("span", "menu-label")
                        .text(label),
                );
            if let Some(choice) = choice {
                el = el
                    .cursor_pointer()
                    .hover(|s| {
                        s.bg(rgb(theme::accent_strong()))
                            .text_color(rgb(theme::on(theme::accent_strong())))
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.spell_pick(choice.clone(), cx)
                    }));
            }
            el.e2e("button", if live { "context-item" } else { "context-item disabled" })
                .attr("role", "menuitem")
                .into_any_element()
        };
        let mut rows: Vec<AnyElement> = Vec::new();
        if let Some((_, suggestions)) = &menu.word {
            for (n, s) in suggestions.iter().enumerate() {
                rows.push(row(
                    format!("spell-suggest-{n}"),
                    s.clone(),
                    Some(SpellChoice::Replace(s.clone())),
                    cx,
                ));
            }
            if suggestions.is_empty() {
                rows.push(row("spell-none".into(), t("No suggestions"), None, cx));
            }
            rows.push(row("spell-add".into(), t("Add to dictionary"), Some(SpellChoice::Add), cx));
            rows.push(row("spell-ignore".into(), t("Ignore"), Some(SpellChoice::Ignore), cx));
            rows.push(div().h(px(1.0)).my(px(4.0)).bg(theme::fill_elevated()).into_any_element());
        }
        let _ = has_word;
        rows.push(row("spell-cut".into(), t("Cut"), has_selection.then_some(SpellChoice::Cut), cx));
        rows.push(row("spell-copy".into(), t("Copy"), has_selection.then_some(SpellChoice::Copy), cx));
        rows.push(row("spell-paste".into(), t("Paste"), Some(SpellChoice::Paste), cx));
        let panel = div()
            .id("spell-menu")
            .absolute()
            .left(menu.at.x)
            .top(menu.at.y)
            .occlude()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .min_w(px(188.0))
            .p(px(6.0))
            .rounded(px(theme::RADIUS_LG))
            .bg(theme::solid_rail())
            .border_1()
            .border_color(rgba(theme::edge()))
            .shadow_lg()
            .text_size(px(0.9 * theme::REM))
            .line_height(gpui::relative(theme::LINE_HEIGHT))
            .text_color(rgb(theme::text_muted()))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .children(rows)
            .e2e("div", "context-menu")
            .attr("role", "menu");
        let close = |this: &mut Self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>| {
            this.spell_menu = None;
            cx.notify();
        };
        // The backdrop is the window's size, placed at its corner whatever the
        // box's own place is.
        let backdrop = div()
            .id("spell-menu-backdrop")
            .size_full()
            .occlude()
            .on_mouse_down(MouseButton::Left, cx.listener(close))
            .on_mouse_down(MouseButton::Right, cx.listener(close))
            .child(panel);
        Some(
            gpui::deferred(
                gpui::anchored()
                    .position(point(px(0.0), px(0.0)))
                    .anchor(gpui::Corner::TopLeft)
                    .child(backdrop.w(px(100_000.0)).h(px(100_000.0))),
            )
            .with_priority(30)
            .into_any_element(),
        )
    }
}

/// What a row of the right-click menu does.
#[derive(Clone)]
enum SpellChoice {
    Replace(String),
    Add,
    Ignore,
    Cut,
    Copy,
    Paste,
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The drawn text with the selection behind it and the caret over it.
struct EditorText {
    input: Entity<TextInput>,
    text: StyledText,
    caret: Option<usize>,
    /// A block cursor (vim's normal mode): where it ends, as a drawn offset.
    block: Option<usize>,
    selection: Option<Range<usize>>,
    /// Where each selected character of the real text is, as drawn offsets.
    selected_chars: Vec<(usize, usize, bool)>,
    layout: Rc<RefCell<Option<TextLayout>>>,
    drawn_colour: Rc<Cell<Option<u32>>>,
}

impl IntoElement for EditorText {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for EditorText {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let colour = gpui::Rgba::from(window.text_style().color);
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
        let drawn = Some(byte(colour.r) << 16 | byte(colour.g) << 8 | byte(colour.b));
        if self.drawn_colour.get() != drawn {
            // The tag (built when the box renders, before this) reads it: draw once more
            // when the colour changed (the theme did), so it is not a frame behind.
            self.drawn_colour.set(drawn);
            self.input.update(cx, |_, cx| cx.notify());
        }
        // The text in a node that grows to the input's width (the body is a
        // flex row): sized by percentages, the body and the text came out 0 px
        // wide in an input stretched by a flex column (the login form's), and
        // the input clipped all of the text away.
        let (text, ()) = self.text.request_layout(id, inspector_id, window, cx);
        let mut style = gpui::Style::default();
        style.flex_grow = 1.0;
        style.min_size.width = px(0.0).into();
        (window.request_layout(style, [text], cx), ())
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Hitbox {
        self.text
            .prepaint(id, inspector_id, bounds, state, window, cx);
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut (),
        hitbox: &mut Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        let layout = self.text.layout().clone();
        *self.layout.borrow_mut() = Some(layout.clone());
        let line = layout.line_height();
        let scale = window.scale_factor();

        // The selection, a box per run of characters on one row.
        if self.selection.is_some() {
            let mut run: Option<(gpui::Point<Pixels>, Pixels)> = None;
            let mut boxes = Vec::new();
            for &(start, end, newline) in &self.selected_chars {
                let (Some(a), Some(b)) = (
                    layout.position_for_index(start),
                    layout.position_for_index(end),
                ) else {
                    continue;
                };
                let width = if newline || (b.y - a.y).abs() >= px(1.0) {
                    px(6.0)
                } else {
                    b.x - a.x
                };
                match &mut run {
                    Some((origin, w)) if (origin.y - a.y).abs() < px(1.0) => {
                        *w = (a.x + width) - origin.x;
                    }
                    _ => {
                        if let Some(done) = run.take() {
                            boxes.push(done);
                        }
                        run = Some((a, width));
                    }
                }
            }
            boxes.extend(run);
            for (origin, width) in boxes {
                let (x, width) = snap::span(f32::from(origin.x), f32::from(width), scale);
                let origin = gpui::point(px(x), origin.y);
                window.paint_quad(fill(
                    Bounds::new(origin, size(px(width), line)),
                    rgba(theme::alpha(theme::accent_strong(), 0x73)),
                ));
            }
        }

        // The block cursor, under the character it stands on.
        if let (Some(from), Some(to)) = (self.caret, self.block)
            && let Some(a) = layout.position_for_index(from)
        {
            let width = layout
                .position_for_index(to)
                .filter(|b| (b.y - a.y).abs() < px(1.0) && b.x > a.x)
                .map_or(px(8.0), |b| b.x - a.x);
            let (x, width) = snap::span(f32::from(a.x), f32::from(width), scale);
            window.paint_quad(fill(
                Bounds::new(gpui::point(px(x), a.y), size(px(width), line)),
                rgba(theme::alpha(theme::accent(), 0xcc)),
            ));
        }

        self.text
            .paint(id, inspector_id, bounds, state, &mut (), window, cx);

        if let Some(at) = self.caret.and_then(|i| layout.position_for_index(i)) {
            if self.block.is_none() {
                let (x, width) = snap::caret(f32::from(at.x), 1.5, scale);
                window.paint_quad(fill(
                    Bounds::new(gpui::point(px(x), at.y), size(px(width), line)),
                    rgb(theme::text()),
                ));
            }
            let input = self.input.clone();
            input.update(cx, |input, _| input.follow_caret(at, window));
        }

        window.set_cursor_style(CursorStyle::IBeam, hitbox);
        let (input, hit) = (self.input.clone(), hitbox.clone());
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble
                && event.button == MouseButton::Left
                && hit.is_hovered(window)
            {
                input.update(cx, |input, cx| input.mouse_down(event, window, cx));
            }
        });
        let (input, hit) = (self.input.clone(), hitbox.clone());
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble
                && event.button == MouseButton::Right
                && hit.is_hovered(window)
            {
                input.update(cx, |input, cx| input.open_spell_menu(event, window, cx));
            }
        });
        let input = self.input.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble {
                input.update(cx, |input, cx| input.mouse_move(event, cx));
            }
        });
        let input = self.input.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                input.update(cx, |input, _| input.selecting = false);
            }
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.is_focused(window) && !self.disabled;
        // A box gaining the focus starts in insert mode; switched off, vim
        // leaves nothing behind.
        if focused != self.was_focused {
            self.was_focused = focused;
            if focused {
                self.vim.enter_insert();
            }
        }
        if !self.vim_on() && self.vim.mode() != VimMode::Insert {
            self.vim.enter_insert();
        }
        let marks = self.marks(cx);
        let (shown, is_placeholder) = self.shown();
        // The placeholder is not part of the measured text: drawn beside it, cut off plainly (no
        // ellipsis: #372), it cannot hold the box wider than its row (#363: a narrow panel's box was
        // as wide as the placeholder, the buttons beside it pushed out).
        let placeholder = (is_placeholder && !shown.is_empty()).then(|| shown.clone());
        let mut text = StyledText::new(if placeholder.is_some() {
            SharedString::default()
        } else {
            shown.clone()
        });
        if placeholder.is_none() && !marks.is_empty() {
            let wavy = HighlightStyle {
                underline: Some(gpui::UnderlineStyle {
                    thickness: px(1.0),
                    color: Some(rgb(theme::danger()).into()),
                    wavy: true,
                }),
                ..HighlightStyle::default()
            };
            text = text.with_highlights(marks.iter().map(|r| (r.clone(), wavy)));
        }
        let chosen = self
            .vim_on()
            .then(|| self.vim.visual_range(&self.buf))
            .flatten()
            .or_else(|| self.buf.has_selection().then(|| self.buf.selection()));
        let selection = chosen
            .clone()
            .map(|range| self.to_shown(range.start)..self.to_shown(range.end));
        let selected_chars: Vec<(usize, usize, bool)> = if let Some(range) = chosen {
            self.buf.text()[range.clone()]
                .char_indices()
                .map(|(i, c)| {
                    let start = range.start + i;
                    let end = start + c.len_utf8();
                    (self.to_shown(start), self.to_shown(end), c == '\n')
                })
                .collect()
        } else {
            Vec::new()
        };
        let editor = EditorText {
            input: cx.entity(),
            text,
            caret: focused.then(|| self.to_shown(self.buf.head())),
            block: (focused && self.vim_on() && self.vim.mode() != VimMode::Insert).then(|| {
                let head = self.buf.head();
                let next = self.buf.next_boundary(head);
                let at_line_end = self.buf.text()[head..next].contains('\n');
                self.to_shown(if at_line_end { head } else { next })
            }),
            selection,
            selected_chars,
            layout: self.layout.clone(),
            drawn_colour: self.drawn_colour.clone(),
        };
        let entity = cx.entity();
        let multiline = self.max_lines > 1;
        let mut body = div()
            .id(ElementId::Name(format!("{}-text", self.name).into()))
            .flex()
            .relative()
            .flex_1()
            .min_w_0();
        body = if multiline {
            body.max_h(px(self.max_lines as f32 * LINE_HEIGHT))
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
        } else {
            body.overflow_hidden().whitespace_nowrap()
        };
        let body = body.child(editor).children(placeholder.map(|text| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_color(rgb(theme::text_faint()))
                .child(text)
                .e2e("div", "input-placeholder")
                .attr("data-overflow", "clip")
        }));
        let mut outer = div()
            .id(self.name)
            .track_focus(
                &self
                    .focus
                    .clone()
                    .tab_stop(!self.keeps_tab() && !self.disabled),
            )
            .key_context(crate::mac_menu::TEXT_INPUT)
            .on_action({
                let entity = entity.clone();
                move |_: &crate::mac_menu::Cut, _, cx| {
                    entity.update(cx, |input, cx| input.shortcut("x", cx))
                }
            })
            .on_action({
                let entity = entity.clone();
                move |_: &crate::mac_menu::Copy, _, cx| {
                    entity.update(cx, |input, cx| input.shortcut("c", cx))
                }
            })
            .on_action({
                let entity = entity.clone();
                move |_: &crate::mac_menu::Paste, _, cx| {
                    entity.update(cx, |input, cx| input.shortcut("v", cx))
                }
            })
            .on_action({
                let entity = entity.clone();
                move |_: &crate::mac_menu::SelectAll, _, cx| {
                    entity.update(cx, |input, cx| input.shortcut("a", cx))
                }
            })
            .on_key_down(move |event, _, cx| {
                // A Tab nobody takes goes up to the form, to move the focus.
                if event.keystroke.key == "tab" && entity.read(cx).keeps_tab() {
                    cx.stop_propagation();
                }
                entity.update(cx, |input, cx| input.key_down(event, cx));
            })
            .flex()
            .min_w_0()
            .child(body);
        if self.bare {
            outer = if self.slim {
                outer.flex_1().py(px(4.0))
            } else {
                outer.flex_1().py(px(12.0)).pl(px(4.0)).pr(px(8.0))
            };
        } else {
            outer = outer
                .px(px(if self.compact { 10.0 } else { 12.0 }))
                .py(px(if self.compact { 7.0 } else { 10.0 }))
                .when(self.compact, |d| d.text_size(px(0.9 * theme::REM)))
                .rounded(px(if self.compact {
                    theme::RADIUS_SM
                } else {
                    theme::RADIUS
                }))
                .border_1()
                .border_color(if self.framed || self.compact {
                    rgb(if self.compact {
                        theme::bg_input()
                    } else {
                        theme::bg_elevated()
                    })
                } else if focused {
                    rgb(theme::accent())
                } else {
                    theme::fill_input()
                })
                .bg(theme::fill_input());
        }
        if let Some(menu) = self.spell_menu_el(cx) {
            outer = outer.child(menu);
        }
        let mut tagged = outer
            .e2e(self.tag, self.class)
            .attr("name", self.name)
            .attr("type", if self.password { "password" } else { "text" })
            .attr("value", self.buf.text().to_owned())
            .attr("placeholder", self.placeholder.to_string())
            .focusable(&self.focus);
        if let Some(colour) = self.drawn_colour.get() {
            tagged = tagged.attr("data-color", format!("{colour:06x}"));
        }
        if !self.bare {
            tagged = tagged.attr("data-bg", format!("{:06x}", theme::bg_input()));
        }
        if multiline {
            tagged = tagged.attr(
                "rows",
                self.buf
                    .text()
                    .lines()
                    .count()
                    .clamp(1, self.max_lines)
                    .to_string(),
            );
        }
        if self.disabled {
            tagged = tagged.attr("disabled", "");
        }
        if self.spell {
            let text = self.buf.text();
            let words: Vec<&str> = marks.iter().map(|r| &text[r.clone()]).collect();
            tagged = tagged.attr("data-misspelled", words.join(","));
        }
        if focused {
            // Which field has the keyboard, for the test bridge.
            tagged = tagged.attr("data-focused", "");
        }
        tagged
    }
}

#[cfg(all(test, feature = "e2e"))]
mod tests {
    use super::*;
    use crate::e2e::{self, query};
    use gpui::{Modifiers, TestAppContext, VisualTestContext};

    struct Harness(Entity<TextInput>);

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            e2e::frame(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .font_family("Inter")
                    .text_size(px(theme::REM))
                    .line_height(gpui::relative(theme::LINE_HEIGHT))
                    .child(div().w(px(300.0)).child(self.0.clone())),
            )
        }
    }

    /// A multi-line box in a window, focused.
    fn open(cx: &mut TestAppContext) -> (Entity<TextInput>, &mut VisualTestContext) {
        let (harness, cx) = cx.add_window_view(|_, cx| {
            Harness(cx.new(|cx| {
                TextInput::new("composer", "Message #room", false, "", cx)
                    .multiline(8)
                    .bare("textarea", "composer-input")
            }))
        });
        let input = harness.read_with(cx, |h, _| h.0.clone());
        input.update_in(cx, |input, window, _| input.focus(window));
        cx.run_until_parked();
        cx.draw(
            point(px(0.0), px(0.0)),
            size(px(400.0), px(300.0)),
            |_, _| div(),
        );
        (input, cx)
    }

    fn value(input: &Entity<TextInput>, cx: &VisualTestContext) -> String {
        input.read_with(cx, |i, _| i.value().to_owned())
    }

    fn redraw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.draw(
            point(px(0.0), px(0.0)),
            size(px(400.0), px(300.0)),
            |_, _| div(),
        );
    }

    #[gpui::test]
    fn typing_inserts_and_backspace_removes(cx: &mut TestAppContext) {
        let (input, cx) = open(cx);
        cx.simulate_input("héllo");
        assert_eq!(value(&input, cx), "héllo");
        cx.simulate_keystrokes("backspace backspace");
        assert_eq!(value(&input, cx), "hél");
        cx.simulate_keystrokes("left delete");
        assert_eq!(value(&input, cx), "hé");
    }

    #[gpui::test]
    fn shift_enter_adds_a_line_and_enter_submits(cx: &mut TestAppContext) {
        let (input, cx) = open(cx);
        let submits = Rc::new(RefCell::new(0));
        let counted = submits.clone();
        cx.update(|_, cx| {
            cx.subscribe(&input, move |_, _: &Submit, _| *counted.borrow_mut() += 1)
                .detach();
        });
        cx.simulate_input("one");
        cx.simulate_keystrokes("shift-enter");
        cx.simulate_input("two");
        assert_eq!(value(&input, cx), "one\ntwo");
        assert_eq!(*submits.borrow(), 0);
        redraw(cx);
        assert_eq!(query("textarea.composer-input[rows=\"2\"]").len(), 1);
        cx.simulate_keystrokes("enter");
        assert_eq!(*submits.borrow(), 1);
        assert_eq!(
            value(&input, cx),
            "one\ntwo",
            "Enter leaves the text to its owner"
        );
    }

    #[gpui::test]
    fn it_grows_with_its_lines_and_stops_at_the_most(cx: &mut TestAppContext) {
        let (_input, cx) = open(cx);
        redraw(cx);
        let height = |cx: &mut VisualTestContext| {
            redraw(cx);
            e2e::query_bounds("textarea.composer-input").map(|b| b.h)
        };
        let h1 = height(cx).expect("height");
        cx.simulate_keystrokes("shift-enter");
        cx.simulate_input("x");
        let h2 = height(cx).expect("height");
        assert!(h2 > h1, "a second line grows it: {h1} then {h2}");
        for _ in 0..20 {
            cx.simulate_keystrokes("shift-enter");
        }
        let h_many = height(cx).expect("height");
        // 8 lines of 21 px and the box's own 24 px of padding.
        assert!(
            h_many <= 8.0 * LINE_HEIGHT + 24.0 + 1.0,
            "it stops growing at 8 lines: {h_many}"
        );
        assert!(h_many > h2);
    }

    #[gpui::test]
    fn shift_arrows_select_and_typing_replaces(cx: &mut TestAppContext) {
        let (input, cx) = open(cx);
        cx.simulate_input("hello world");
        cx.simulate_keystrokes("shift-left shift-left shift-left shift-left shift-left");
        input.read_with(cx, |i, _| assert_eq!(i.buf.selected_text(), "world"));
        cx.simulate_input("there");
        assert_eq!(value(&input, cx), "hello there");
        cx.simulate_keystrokes("ctrl-shift-left");
        input.read_with(cx, |i, _| assert_eq!(i.buf.selected_text(), "there"));
        cx.simulate_keystrokes("ctrl-a");
        input.read_with(cx, |i, _| assert_eq!(i.buf.selected_text(), "hello there"));
        cx.simulate_keystrokes("backspace");
        assert_eq!(value(&input, cx), "");
    }

    /// The macOS Edit menu (and its shortcuts) send actions to the box that has the focus.
    #[gpui::test]
    fn the_edit_menus_actions_act_on_the_focused_box(cx: &mut TestAppContext) {
        use crate::mac_menu::{Copy, Cut, Paste, SelectAll};
        let (input, cx) = open(cx);
        cx.simulate_input("alpha beta");
        cx.dispatch_action(SelectAll);
        cx.dispatch_action(Copy);
        assert_eq!(
            cx.read_from_clipboard().and_then(|c| c.text()).as_deref(),
            Some("alpha beta")
        );
        cx.dispatch_action(Cut);
        assert_eq!(value(&input, cx), "");
        cx.dispatch_action(Paste);
        cx.dispatch_action(Paste);
        assert_eq!(value(&input, cx), "alpha betaalpha beta");
    }

    #[gpui::test]
    fn copy_cut_and_paste_go_through_the_clipboard(cx: &mut TestAppContext) {
        let (input, cx) = open(cx);
        cx.simulate_input("alpha beta");
        cx.simulate_keystrokes("ctrl-a ctrl-c");
        assert_eq!(
            cx.read_from_clipboard().and_then(|c| c.text()).as_deref(),
            Some("alpha beta")
        );
        cx.simulate_keystrokes("right ctrl-v ctrl-v");
        assert_eq!(value(&input, cx), "alpha betaalpha betaalpha beta");
        cx.simulate_keystrokes("ctrl-a ctrl-x");
        assert_eq!(value(&input, cx), "");
        assert_eq!(
            cx.read_from_clipboard().and_then(|c| c.text()).as_deref(),
            Some("alpha betaalpha betaalpha beta")
        );
        cx.write_to_clipboard(ClipboardItem::new_string("one\r\ntwo".into()));
        cx.simulate_keystrokes("ctrl-v");
        assert_eq!(value(&input, cx), "one\ntwo", "pasted lines stay lines");
    }

    #[gpui::test]
    fn a_pasted_picture_is_handed_to_the_owner(cx: &mut TestAppContext) {
        let (input, cx) = open(cx);
        let got = Rc::new(RefCell::new(Vec::<PastedFile>::new()));
        let sink = got.clone();
        cx.update(|_, cx| {
            cx.subscribe(&input, move |_, event: &Pasted, _| {
                sink.borrow_mut().extend(event.0.clone())
            })
            .detach();
        });
        let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, vec![1, 2, 3]);
        cx.write_to_clipboard(ClipboardItem::new_image(&image));
        cx.simulate_keystrokes("ctrl-v");
        assert_eq!(value(&input, cx), "");
        let files = got.borrow();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].mime, "image/png");
        assert_eq!(files[0].bytes, vec![1, 2, 3]);
    }

    #[gpui::test]
    fn up_and_down_move_between_lines(cx: &mut TestAppContext) {
        let (input, cx) = open(cx);
        cx.simulate_input("abc");
        cx.simulate_keystrokes("shift-enter");
        cx.simulate_input("de");
        redraw(cx);
        cx.simulate_keystrokes("up");
        input.read_with(cx, |i, _| assert!(i.buf.head() <= 3, "{}", i.buf.head()));
        redraw(cx);
        cx.simulate_keystrokes("down");
        input.read_with(cx, |i, _| assert!(i.buf.head() >= 4));
    }

    #[gpui::test]
    fn the_mouse_places_the_caret_and_drags_a_selection(cx: &mut TestAppContext) {
        let (input, cx) = open(cx);
        cx.simulate_input("hello world");
        redraw(cx);
        let (_, (x, y)) = query("textarea.composer-input")
            .into_iter()
            .next()
            .expect("box");
        // A click at the far left puts the caret at the start.
        let left = e2e::query_bounds("textarea.composer-input").expect("bounds");
        cx.simulate_click(point(px(left.x + 5.0), px(y)), Modifiers::none());
        input.read_with(cx, |i, _| assert_eq!(i.buf.head(), 0));
        // Dragging to the right selects.
        cx.simulate_mouse_down(
            point(px(left.x + 5.0), px(y)),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.simulate_mouse_move(
            point(px(x), px(y)),
            Some(MouseButton::Left),
            Modifiers::none(),
        );
        cx.simulate_mouse_up(point(px(x), px(y)), MouseButton::Left, Modifiers::none());
        input.read_with(cx, |i, _| {
            assert!(i.buf.has_selection());
            assert!(i.buf.selected_text().starts_with("hel"));
        });
        // A double click takes the word.
        cx.simulate_click(point(px(left.x + 8.0), px(y)), Modifiers::none());
        cx.simulate_event(MouseDownEvent {
            position: point(px(left.x + 8.0), px(y)),
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 2,
            first_mouse: false,
        });
        input.read_with(cx, |i, _| assert_eq!(i.buf.selected_text(), "hello"));
    }

    #[gpui::test]
    fn a_disabled_box_ignores_keys_and_says_so(cx: &mut TestAppContext) {
        let (input, cx) = open(cx);
        input.update(cx, |i, cx| i.set_disabled(true, cx));
        redraw(cx);
        assert_eq!(query("textarea.composer-input[disabled]").len(), 1);
        cx.simulate_input("nope");
        assert_eq!(value(&input, cx), "");
    }

    #[gpui::test]
    fn the_owner_can_take_the_navigation_keys(cx: &mut TestAppContext) {
        let (input, cx) = open(cx);
        let keys = Rc::new(RefCell::new(Vec::new()));
        let sink = keys.clone();
        cx.update(|_, cx| {
            cx.subscribe(&input, move |_, event: &Nav, _| {
                sink.borrow_mut().push(event.0)
            })
            .detach();
        });
        // Up in an empty box, and Escape, always.
        cx.simulate_keystrokes("up escape");
        assert_eq!(*keys.borrow(), [NavKey::Up, NavKey::Escape]);
        keys.borrow_mut().clear();
        cx.simulate_input("x");
        cx.simulate_keystrokes("up");
        assert!(keys.borrow().is_empty(), "Up with text moves the caret");
        input.update(cx, |i, _| i.set_capture_nav(true));
        cx.simulate_keystrokes("up down enter tab");
        assert_eq!(
            *keys.borrow(),
            [NavKey::Up, NavKey::Down, NavKey::Enter, NavKey::Tab]
        );
        assert_eq!(value(&input, cx), "x");
    }

    #[gpui::test]
    fn a_password_shows_dots_but_keeps_the_text(cx: &mut TestAppContext) {
        let (harness, cx) = cx.add_window_view(|_, cx| {
            Harness(cx.new(|cx| TextInput::new("password", "", true, "", cx)))
        });
        let input = harness.read_with(cx, |h, _| h.0.clone());
        input.update_in(cx, |input, window, _| input.focus(window));
        cx.simulate_input("sécret");
        assert_eq!(value(&input, cx), "sécret");
        input.read_with(cx, |i, _| {
            assert_eq!(i.shown().0.as_ref(), "\u{2022}".repeat(6));
            assert_eq!(i.to_shown(i.buf.head()), 18);
            assert_eq!(i.from_shown(6), 3, "two bytes for the é");
        });
    }
}

/// The text a key types. gpui on Windows reports Space as the named key
/// "space" with no `key_char` (Linux and macOS give " "), so a box built on
/// `key_char` alone could not type a space there.
fn typed_text(keystroke: &gpui::Keystroke) -> Option<String> {
    keystroke
        .key_char
        .clone()
        .or_else(|| (keystroke.key == "space").then(|| " ".to_owned()))
}

/// Keys that type a character although a modifier is down: AltGr on Windows
/// (reported as Ctrl+Alt, e.g. @ on a German keyboard) and Option on macOS
/// (Option+L is @ on a German Mac). Shortcuts (Ctrl or Cmd alone) never type.
fn types_with_modifiers(keystroke: &gpui::Keystroke) -> bool {
    let m = keystroke.modifiers;
    let has_char = keystroke
        .key_char
        .as_ref()
        .is_some_and(|t| !t.chars().any(char::is_control));
    let altgr = cfg!(target_os = "windows") && m.control && m.alt && !m.platform;
    let option = cfg!(target_os = "macos") && m.alt && !m.control && !m.platform;
    has_char && (altgr || option)
}

#[cfg(test)]
mod typed_text_tests {
    use super::typed_text;

    #[test]
    fn space_types_a_space_even_without_a_key_char() {
        // As gpui's Windows backend reports it.
        let windows = gpui::Keystroke {
            modifiers: Default::default(),
            key: "space".into(),
            key_char: None,
        };
        assert_eq!(typed_text(&windows).as_deref(), Some(" "));
        // As Linux and macOS report it.
        let linux = gpui::Keystroke {
            modifiers: Default::default(),
            key: "space".into(),
            key_char: Some(" ".into()),
        };
        assert_eq!(typed_text(&linux).as_deref(), Some(" "));
        let a = gpui::Keystroke {
            modifiers: Default::default(),
            key: "a".into(),
            key_char: Some("a".into()),
        };
        assert_eq!(typed_text(&a).as_deref(), Some("a"));
        let f5 = gpui::Keystroke {
            modifiers: Default::default(),
            key: "f5".into(),
            key_char: None,
        };
        assert_eq!(typed_text(&f5), None);
    }
}

/// Snapping to the device-pixel grid: a quad at a fractional device x is
/// antialiased over a varying number of pixels, so the caret looked thicker
/// or thinner as it moved. All in logical pixels at the window's `scale`.
mod snap {
    /// The caret's left edge and width: the edge on a whole device pixel,
    /// the width a whole number of device pixels (at least one).
    pub fn caret(x: f32, logical_width: f32, scale: f32) -> (f32, f32) {
        let width = (logical_width * scale).round().max(1.0);
        ((x * scale).round() / scale, width / scale)
    }

    /// A span from `x` to `x + width` with both edges on whole device
    /// pixels (at least one pixel wide): selections and the block cursor.
    pub fn span(x: f32, width: f32, scale: f32) -> (f32, f32) {
        let left = (x * scale).round();
        let right = ((x + width) * scale).round().max(left + 1.0);
        (left / scale, (right - left) / scale)
    }
}

#[cfg(test)]
mod snap_tests {
    use super::snap::{caret, span};

    const SCALES: [f32; 5] = [1.0, 1.2, 1.25, 1.5, 2.0];

    fn device(v: f32, scale: f32) -> f32 {
        v * scale
    }

    fn whole(v: f32) -> bool {
        (v - v.round()).abs() < 1e-3
    }

    #[test]
    fn the_caret_is_the_same_whole_number_of_device_pixels_wherever_it_is() {
        for scale in SCALES {
            let expected = (1.5 * scale).round().max(1.0);
            for i in 0..2000 {
                let x = i as f32 * 0.37 + 0.11;
                let (left, width) = caret(x, 1.5, scale);
                assert!(whole(device(left, scale)), "x {x} at {scale}: left {left}");
                assert!(
                    (device(width, scale) - expected).abs() < 1e-3,
                    "x {x} at {scale}: width {width}"
                );
                assert!((left - x).abs() <= 0.5 / scale + 1e-4, "stays near x");
            }
        }
    }

    #[test]
    fn spans_have_edges_on_whole_device_pixels() {
        for scale in SCALES {
            for i in 0..500 {
                let x = i as f32 * 0.53 + 0.07;
                for w in [0.4_f32, 3.7, 8.0, 41.3] {
                    let (left, width) = span(x, w, scale);
                    assert!(whole(device(left, scale)), "left at {scale}");
                    assert!(whole(device(left + width, scale)), "right at {scale}");
                    assert!(device(width, scale) >= 1.0 - 1e-3, "visible");
                }
            }
        }
    }
}
