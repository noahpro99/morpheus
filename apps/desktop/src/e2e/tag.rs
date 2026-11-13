//! Element tags: a wrapper element that records its tag, bounds and parent
//! into the frame's tree while painting.

use std::{cell::RefCell, collections::HashMap, ops::Range, rc::Rc};

use gpui::{
    App, Bounds, Element, ElementId, FocusHandle, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, TextLayout, Window,
};

use super::tree::{Node, Rect, Tree};

/// The tree being built or last built, on the main thread (the one that
/// paints and the one the bridge answers on).
#[derive(Default)]
pub struct State {
    pub tree: Tree,
    /// Open elements while painting: the innermost is the next one's parent.
    stack: Vec<usize>,
    /// Counts frames, so the bridge can wait for the effect of an input.
    pub frame: u64,
    /// Focus handles of tagged elements, by node index.
    pub focus: HashMap<usize, FocusHandle>,
    /// Scroll probes of tagged elements, by node index.
    pub scroll: HashMap<usize, Rc<dyn super::Scrollable>>,
}

/// Whether elements record themselves. A test turns it off to time frames
/// without the cost of tagging (the tree then stays as it last was).
pub static TAGGING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

fn tagging() -> bool {
    TAGGING.load(std::sync::atomic::Ordering::Relaxed)
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|state| f(&mut state.borrow_mut()))
}

#[derive(Default)]
struct Meta {
    tag: String,
    classes: String,
    attrs: Vec<(String, String)>,
    text: String,
    focus: Option<FocusHandle>,
    scroll: Option<Rc<dyn super::Scrollable>>,
    root: bool,
}

/// An element with an e2e tag.
pub struct Tagged<E> {
    inner: E,
    meta: Meta,
}

#[allow(dead_code)]
impl<E> Tagged<E> {
    pub fn attr(mut self, name: &str, value: impl Into<String>) -> Self {
        if !tagging() {
            return self;
        }
        self.meta.attrs.push((name.to_owned(), value.into()));
        self
    }

    pub fn text(mut self, text: impl Into<String>) -> Self {
        if !tagging() {
            return self;
        }
        self.meta.text = text.into();
        self
    }

    pub fn focusable(mut self, handle: &FocusHandle) -> Self {
        self.meta.focus = Some(handle.clone());
        self
    }

    /// Lets the bridge read and set this element's scroll position.
    pub fn scrollable(mut self, probe: Rc<dyn super::Scrollable>) -> Self {
        self.meta.scroll = Some(probe);
        self
    }
}

pub trait E2eExt: IntoElement + Sized {
    /// Tag this element: `tag` is the element name selectors match
    /// (`button`), `classes` a space-separated class list.
    fn e2e(self, tag: &str, classes: &str) -> Tagged<Self> {
        if !tagging() {
            return Tagged { inner: self, meta: Meta::default() };
        }
        Tagged {
            inner: self,
            meta: Meta {
                tag: tag.to_ascii_lowercase(),
                classes: classes.to_owned(),
                ..Meta::default()
            },
        }
    }
}
impl<T: IntoElement> E2eExt for T {}

/// The root of a window's tree (`body`): starts a new tree each frame.
pub fn frame<E: IntoElement>(element: E) -> Tagged<E> {
    let mut tagged = element.e2e("body", "");
    tagged.meta.root = true;
    tagged
}

impl<E: IntoElement> IntoElement for Tagged<E> {
    type Element = TaggedElement<E::Element>;

    fn into_element(self) -> Self::Element {
        TaggedElement {
            inner: self.inner.into_element(),
            meta: Some(self.meta),
        }
    }
}

pub struct TaggedElement<E> {
    inner: E,
    meta: Option<Meta>,
}

impl<E: Element> IntoElement for TaggedElement<E> {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl<E: Element> Element for TaggedElement<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.inner.id()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.inner.source_location()
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.inner.request_layout(id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.inner
            .prepaint(id, inspector_id, bounds, request_layout, window, cx)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if !tagging() {
            self.inner.paint(id, inspector_id, bounds, request_layout, prepaint, window, cx);
            return;
        }
        if let Some(meta) = self.meta.take() {
            open(meta, bounds);
        }
        self.inner.paint(
            id,
            inspector_id,
            bounds,
            request_layout,
            prepaint,
            window,
            cx,
        );
        with_state(|state| {
            state.stack.pop();
        });
    }
}

fn open(meta: Meta, bounds: Bounds<Pixels>) {
    with_state(|state| {
        if meta.root {
            state.tree.clear();
            state.stack.clear();
            state.focus.clear();
            state.scroll.clear();
            state.frame += 1;
        }
        let node = Node {
            tag: meta.tag,
            classes: meta.classes.split_whitespace().map(Into::into).collect(),
            attrs: meta.attrs,
            text: meta.text,
            bounds: Rect {
                x: f32::from(bounds.origin.x),
                y: f32::from(bounds.origin.y),
                w: f32::from(bounds.size.width),
                h: f32::from(bounds.size.height),
            },
            ..Node::default()
        };
        let index = state.tree.push(node, state.stack.last().copied());
        if let Some(handle) = meta.focus {
            state.focus.insert(index, handle);
        }
        if let Some(probe) = meta.scroll {
            state.scroll.insert(index, probe);
        }
        state.stack.push(index);
    });
}

/// Wraps text that has links in it (an `InteractiveText`): once the text is
/// painted, each link's range is added to the tree as a detached `a[href]`
/// node (its text is its own, not part of its parent's), placed where the
/// text layout put it, so a test can find a link and click it.
pub fn text_links<E: IntoElement>(
    inner: E,
    layout: &TextLayout,
    links: Vec<(Range<usize>, String, Option<(u32, u32)>)>,
    hovered: Option<usize>,
) -> TextLinks<E::Element> {
    TextLinks {
        inner: inner.into_element(),
        layout: layout.clone(),
        links,
        hovered,
    }
}

pub struct TextLinks<E> {
    inner: E,
    layout: TextLayout,
    links: Vec<(Range<usize>, String, Option<(u32, u32)>)>,
    hovered: Option<usize>,
}

impl<E: Element> IntoElement for TextLinks<E> {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl<E: Element> Element for TextLinks<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.inner.id()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.inner.source_location()
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.inner.request_layout(id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.inner
            .prepaint(id, inspector_id, bounds, request_layout, window, cx)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner.paint(
            id,
            inspector_id,
            bounds,
            request_layout,
            prepaint,
            window,
            cx,
        );
        let line = f32::from(self.layout.line_height());
        let right = f32::from(bounds.right());
        let text = self.layout.text();
        with_state(|state| {
            let parent = state.stack.last().copied();
            for (n, (range, href, pill)) in self.links.iter().enumerate() {
                let Some(start) = self.layout.position_for_index(range.start) else {
                    continue;
                };
                let (x, y) = (f32::from(start.x), f32::from(start.y));
                let end = self.layout.position_for_index(range.end);
                let w = match end {
                    Some(end) if (f32::from(end.y) - y).abs() < 1.0 && f32::from(end.x) > x => {
                        f32::from(end.x) - x
                    }
                    _ => (right - x).max(8.0),
                };
                let node = Node {
                    tag: "a".into(),
                    attrs: {
                        let mut attrs = vec![("href".into(), href.clone())];
                        if self.hovered == Some(n) {
                            attrs.push(("data-hovered".into(), "true".into()));
                        }
                        if let Some((bg, colour)) = pill {
                            attrs.push(("data-pill".into(), "true".into()));
                            attrs.push(("data-bg".into(), format!("{bg:08x}")));
                            attrs.push(("data-color".into(), format!("{colour:06x}")));
                        }
                        attrs
                    },
                    text: text.get(range.clone()).unwrap_or_default().to_owned(),
                    bounds: Rect { x, y, w, h: line },
                    detached: true,
                    ..Node::default()
                };
                state.tree.push(node, parent);
            }
        });
    }
}
