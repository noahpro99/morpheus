//! Selecting text in messages with the mouse (#273). gpui's text is not
//! selectable, so each paragraph of a message registers its shaped layout while
//! it paints, and one host element at the start of the window's paint listens for
//! the mouse: a press puts the caret, word or paragraph (by click count; Shift
//! extends) where the layout's `index_for_position` says, a drag moves the other
//! end, a press on nothing selectable clears it. The model (positions, words,
//! paragraphs, the text) is view-model's `text_selection`; the selection lives
//! here, outside any element, so it stays through every redraw.

use std::cell::RefCell;

use gpui::{
    App, Bounds, DispatchPhase, Element, ElementId, GlobalElementId, Hitbox, HitboxBehavior,
    InspectorElementId, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, SharedString, StyledText, TextLayout, Window, canvas, point, prelude::*, size,
};

use crate::theme;
use crate::view_model::text_selection::{Pos, Selection};

/// A paragraph painted this frame.
struct Entry {
    scope: SharedString,
    text: SharedString,
    layout: TextLayout,
    hitbox: Hitbox,
}

struct Active {
    group: SharedString,
    selection: Selection,
    /// The group's paragraphs when the selection last changed, and the scope (message) of each.
    texts: Vec<String>,
    scopes: Vec<SharedString>,
}

#[derive(Default)]
struct State {
    entries: Vec<Entry>,
    active: Option<Active>,
    dragging: bool,
    focused_root: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn refs(texts: &[String]) -> Vec<&str> {
    texts.iter().map(String::as_str).collect()
}

impl Active {
    fn non_empty(&self) -> bool {
        self.selection.is_empty(&refs(&self.texts)) == false
    }
}

/// The selected text per message, in order: (scope, text), for a non-empty selection.
pub fn selected_pieces() -> Option<Vec<(String, String)>> {
    STATE.with(|s| {
        let s = s.borrow();
        let active = s.active.as_ref().filter(|a| a.non_empty())?;
        let mut out: Vec<(String, Vec<String>)> = Vec::new();
        for (para, text) in active.selection.slices(&refs(&active.texts)) {
            let scope = active.scopes.get(para).map(|s| s.to_string()).unwrap_or_default();
            match out.last_mut() {
                Some((last, lines)) if *last == scope => lines.push(text),
                _ => out.push((scope, vec![text])),
            }
        }
        Some(out.into_iter().map(|(scope, lines)| (scope, lines.join("\n"))).collect())
    })
}

/// The selected text, if any (the messages' pieces one line apart when it spans several).
pub fn selected_text() -> Option<String> {
    let pieces = selected_pieces()?;
    Some(pieces.into_iter().map(|(_, text)| text).collect::<Vec<_>>().join("\n"))
}

/// The selected text when it is in `scope` (a message's key) and in it alone.
pub fn selected_text_in(scope: &str) -> Option<String> {
    let mut pieces = selected_pieces()?;
    (pieces.len() == 1 && pieces[0].0 == scope).then(|| pieces.remove(0).1)
}

/// Whether the selection takes in `scope` (a message's key) among others, or alone.
pub fn selection_includes(scope: &str) -> bool {
    selected_pieces().is_some_and(|p| p.iter().any(|(s, _)| s == scope))
}

/// Selections do not cross between these: each of the full-screen views and the notes is its
/// own; every message of a timeline is one.
fn group_of(scope: &str) -> &str {
    match scope {
        "long" | "help-notes" | "whats-new" => scope,
        _ => "messages",
    }
}

/// Lets go of the selection.
pub fn clear() {
    STATE.with(|s| s.borrow_mut().active = None);
}

/// The paragraphs of `group` this frame, in paint order, with their entry indices.
fn scope_entries<'a>(entries: &'a [Entry], group: &str) -> Vec<(usize, &'a Entry)> {
    entries.iter().enumerate().filter(|(_, e)| group_of(&e.scope) == group).collect()
}

/// Called as a paragraph paints: it takes part in selection, and its highlight
/// is what this returns (the selected byte range, if any).
fn register(scope: &SharedString, text: &SharedString, layout: &TextLayout, hitbox: Hitbox) -> Option<std::ops::Range<usize>> {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let group = group_of(scope);
        let within = s.entries.iter().filter(|e| &e.scope == scope).count();
        s.entries.push(Entry {
            scope: scope.clone(),
            text: text.clone(),
            layout: layout.clone(),
            hitbox,
        });
        let active = s.active.as_ref().filter(|a| a.group == group)?;
        // This paragraph is the `within`-th of its message: found by that, not by its place in the
        // frame, which moves as the list scrolls.
        let index = active.scopes.iter().enumerate().filter(|(_, s)| *s == scope).nth(within)?.0;
        active.selection.range_in(&refs(&active.texts), index)
    })
}

/// The selection's tint under the text.
fn tint() -> gpui::Hsla {
    crate::message::hsla_alpha(theme::alpha(theme::accent(), 0x59))
}

/// Paints the highlight of `range` of the paragraph laid out in `layout`.
fn paint_highlight(window: &mut Window, layout: &TextLayout, range: std::ops::Range<usize>) {
    let (Some(start), Some(end)) = (
        layout.position_for_index(range.start),
        layout.position_for_index(range.end),
    ) else {
        return;
    };
    let line = layout.line_height();
    let bounds = layout.bounds();
    let rows = (((end.y - start.y) / line).round().max(0.0)) as usize;
    for row in 0..=rows {
        let left = if row == 0 { start.x } else { bounds.left() };
        let right = if row == rows { end.x } else { bounds.right() };
        if right <= left {
            continue;
        }
        window.paint_quad(gpui::fill(
            Bounds::new(
                point(left, start.y + line * row as f32),
                size(right - left, line),
            ),
            tint(),
        ));
    }
}

/// What a paragraph does as it prepaints and paints: a hitbox over its text
/// first, then, with the layout known, the registration and the highlight.
pub struct Part {
    scope: SharedString,
    text: SharedString,
    hitbox: Option<Hitbox>,
}

impl Part {
    pub fn new(scope: &str, text: &SharedString) -> Self {
        Self {
            scope: SharedString::from(scope.to_owned()),
            text: text.clone(),
            hitbox: None,
        }
    }

    pub fn prepaint(&mut self, bounds: Bounds<Pixels>, window: &mut Window) {
        self.hitbox = Some(window.insert_hitbox(bounds, HitboxBehavior::Normal));
    }

    /// Before the text paints (the highlight goes under it).
    pub fn paint(&mut self, layout: &TextLayout, window: &mut Window) {
        let Some(hitbox) = self.hitbox.take() else {
            return;
        };
        if let Some(range) = register(&self.scope, &self.text, layout, hitbox) {
            paint_highlight(window, layout, range);
        }
    }
}

/// A plain paragraph: gpui's text with a [`Part`].
pub struct SelectableText {
    text: StyledText,
    part: Part,
}

impl SelectableText {
    pub fn new(scope: &str, source: &SharedString, text: StyledText) -> Self {
        Self {
            part: Part::new(scope, source),
            text,
        }
    }
}

impl IntoElement for SelectableText {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for SelectableText {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        self.text.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.text.prepaint(id, inspector, bounds, state, window, cx);
        self.part.prepaint(bounds, window);
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.part.paint(&self.text.layout().clone(), window);
        self.text.paint(id, inspector, bounds, state, &mut (), window, cx);
    }
}

fn root_focus(window: &mut Window, cx: &App) {
    if let Some(root) = cx.try_global::<crate::shell::RootFocus>().map(|r| r.0.clone()) {
        window.focus(&root);
    }
}

/// The byte position the pointer is at in the paragraphs of a group (every message of the timeline): in the one
/// it is over, else the nearest above or below.
fn position_in(entries: &[(usize, &Entry)], at: gpui::Point<Pixels>) -> Option<Pos> {
    let (first, last) = (entries.first()?, entries.last()?);
    let top = |e: &Entry| e.layout.bounds().top();
    let bottom = |e: &Entry| e.layout.bounds().bottom();
    if at.y < top(first.1) {
        return Some(Pos::new(0, 0));
    }
    if at.y > bottom(last.1) {
        return Some(Pos::new(entries.len() - 1, last.1.text.len()));
    }
    let n = entries.iter().position(|(_, e)| at.y <= bottom(e)).unwrap_or(entries.len() - 1);
    let e = entries[n].1;
    if at.y < top(e) {
        return Some(Pos::new(n, 0));
    }
    Some(Pos::new(n, e.layout.index_for_position(at).unwrap_or_else(|x| x)))
}

fn press(event: &MouseDownEvent, window: &mut Window, cx: &mut App) {
    if event.button != MouseButton::Left {
        return;
    }
    let changed = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.dragging = false;
        let Some(hit) = s.entries.iter().rposition(|e| e.hitbox.is_hovered(window)) else {
            return s.active.take().is_some();
        };
        let group = SharedString::from(group_of(&s.entries[hit].scope).to_owned());
        let entries = scope_entries(&s.entries, &group);
        let index = entries.iter().position(|(i, _)| *i == hit).unwrap_or(0);
        let offset = s.entries[hit]
            .layout
            .index_for_position(event.position)
            .unwrap_or_else(|x| x);
        let at = Pos::new(index, offset);
        let texts: Vec<String> = entries.iter().map(|(_, e)| e.text.to_string()).collect();
        let parts = refs(&texts);
        let existing = s.active.as_ref().filter(|a| a.group == group).map(|a| a.selection);
        let selection = match existing {
            Some(mut selection) if event.modifiers.shift => {
                selection.extend_to(at);
                selection
            }
            _ => match event.click_count {
                0 | 1 => Selection::caret(at),
                2 => Selection::word(&parts, at),
                _ => Selection::paragraph(&parts, at),
            },
        };
        let scopes = entries.iter().map(|(_, e)| e.scope.clone()).collect();
        s.active = Some(Active { group, selection, texts, scopes });
        s.dragging = true;
        true
    });
    if changed {
        if STATE.with(|s| s.borrow().active.as_ref().is_some_and(Active::non_empty)) {
            root_focus(window, cx);
        }
        window.refresh();
    }
}

fn drag(event: &MouseMoveEvent, window: &mut Window, cx: &mut App) {
    let moved = STATE.with(|s| {
        let mut s = s.borrow_mut();
        if !s.dragging {
            return None;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            s.dragging = false;
            return None;
        }
        let group = s.active.as_ref()?.group.clone();
        let at = position_in(&scope_entries(&s.entries, &group), event.position)?;
        let active = s.active.as_mut()?;
        active.selection.extend_to(at);
        Some(active.non_empty())
    });
    if let Some(non_empty) = moved {
        if non_empty {
            root_focus(window, cx);
        }
        window.refresh();
    }
}

/// The element that listens for the mouse: first in the window's tree, so the
/// registry is cleared before the paragraphs paint into it.
pub fn host() -> impl IntoElement {
    canvas(
        |_, _, _| (),
        |_, _, window, _| {
            STATE.with(|s| s.borrow_mut().entries.clear());
            window.on_mouse_event(|event: &MouseDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    press(event, window, cx);
                }
            });
            window.on_mouse_event(|event: &MouseMoveEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    drag(event, window, cx);
                }
            });
            window.on_mouse_event(|_: &MouseUpEvent, phase, _, _| {
                if phase == DispatchPhase::Capture {
                    STATE.with(|s| s.borrow_mut().dragging = false);
                }
            });
        },
    )
    .size_0()
    .absolute()
}

