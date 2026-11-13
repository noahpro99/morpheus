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
    view_model::text_buffer::TextBuffer,
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
        let shown = layout.index_for_position(position).unwrap_or_else(|nearest| nearest);
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
        let target = point(goal, if up { at.y - line / 2.0 } else { at.y + line * 1.5 });
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
        self.key_down(&KeyDownEvent { keystroke, is_held: false }, cx);
    }

    fn key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        let multiline = self.max_lines > 1;
        let key = keystroke.key.as_str();
        // The shortcut key: Ctrl, or Cmd on a Mac.
        let cmd = m.control || m.platform;
        let word = m.control || m.alt;
        let was = (self.buf.text().to_owned(), self.buf.selection());

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
                    self.buf.backspace(m.alt);
                }
                "delete" => {
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
                    if m.control || m.alt || m.platform {
                        return;
                    }
                    match &keystroke.key_char {
                        Some(text) if !text.chars().any(char::is_control) => {
                            self.insert_text(text)
                        }
                        _ => return,
                    }
                }
            }
        }
        if !matches!(key, "up" | "down") {
            self.goal_x = None;
        }
        cx.notify();
        if self.buf.text() != was.0 {
            cx.emit(Changed);
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
        self.drawn_colour
            .set(Some(byte(colour.r) << 16 | byte(colour.g) << 8 | byte(colour.b)));
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

        // The selection, a box per run of characters on one row.
        if self.selection.is_some() {
            let mut run: Option<(gpui::Point<Pixels>, Pixels)> = None;
            let mut boxes = Vec::new();
            for &(start, end, newline) in &self.selected_chars {
                let (Some(a), Some(b)) =
                    (layout.position_for_index(start), layout.position_for_index(end))
                else {
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
                window.paint_quad(fill(
                    Bounds::new(origin, size(width, line)),
                    rgba(theme::alpha(theme::accent_strong(), 0x73)),
                ));
            }
        }

        self.text
            .paint(id, inspector_id, bounds, state, &mut (), window, cx);

        if let Some(at) = self.caret.and_then(|i| layout.position_for_index(i)) {
            window.paint_quad(fill(
                Bounds::new(at, size(px(1.5), line)),
                rgb(theme::text()),
            ));
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
        let (shown, is_placeholder) = self.shown();
        let mut text = StyledText::new(shown.clone());
        if is_placeholder && !shown.is_empty() {
            text = text.with_highlights([(
                0..shown.len(),
                HighlightStyle {
                    color: Some(rgb(theme::text_faint()).into()),
                    ..HighlightStyle::default()
                },
            )]);
        }
        let selection = self.buf.has_selection().then(|| {
            let range = self.buf.selection();
            self.to_shown(range.start)..self.to_shown(range.end)
        });
        let selected_chars: Vec<(usize, usize, bool)> = if selection.is_some() {
            let range = self.buf.selection();
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
            .flex_1()
            .min_w_0();
        body = if multiline {
            body.max_h(px(self.max_lines as f32 * LINE_HEIGHT))
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
        } else {
            body.overflow_hidden().whitespace_nowrap()
        };
        let body = body.child(editor);
        let mut outer = div()
            .id(self.name)
            .track_focus(&self.focus)
            .key_context(crate::mac_menu::TEXT_INPUT)
            .on_action({
                let entity = entity.clone();
                move |_: &crate::mac_menu::Cut, _, cx| entity.update(cx, |input, cx| input.shortcut("x", cx))
            })
            .on_action({
                let entity = entity.clone();
                move |_: &crate::mac_menu::Copy, _, cx| entity.update(cx, |input, cx| input.shortcut("c", cx))
            })
            .on_action({
                let entity = entity.clone();
                move |_: &crate::mac_menu::Paste, _, cx| entity.update(cx, |input, cx| input.shortcut("v", cx))
            })
            .on_action({
                let entity = entity.clone();
                move |_: &crate::mac_menu::SelectAll, _, cx| entity.update(cx, |input, cx| input.shortcut("a", cx))
            })
            .on_key_down(move |event, _, cx| {
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
                .rounded(px(if self.compact { theme::RADIUS_SM } else { theme::RADIUS }))
                .border_1()
                .border_color(if self.framed || self.compact {
                    rgb(if self.compact { theme::bg_input() } else { theme::bg_elevated() })
                } else if focused {
                    rgb(theme::accent())
                } else {
                    rgb(theme::bg_input())
                })
                .bg(rgb(theme::bg_input()));
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
                self.buf.text().lines().count().clamp(1, self.max_lines).to_string(),
            );
        }
        if self.disabled {
            tagged = tagged.attr("disabled", "");
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
        assert_eq!(value(&input, cx), "one\ntwo", "Enter leaves the text to its owner");
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
        assert_eq!(cx.read_from_clipboard().and_then(|c| c.text()).as_deref(), Some("alpha beta"));
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
            cx.subscribe(&input, move |_, event: &Nav, _| sink.borrow_mut().push(event.0))
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
