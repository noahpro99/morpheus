//! End-to-end test support. With the `e2e` feature, views tag the elements
//! tests touch ([`E2eExt::e2e`]); each frame's tagged tree answers the
//! selector requests of the bridge (`MORPHEUS_E2E_PORT`), which also injects
//! clicks and keys. Without the feature, [`Tagged`] is a transparent
//! wrapper the compiler removes: no tree, no bridge, no code.
//!
//! Tagging an element:
//!
//! ```ignore
//! div().child("Sign in").e2e("button", "primary").attr("type", "submit").text("Sign in")
//! ```
//!
//! - Wrap an element as the last step of its builder chain (a `Tagged` is an
//!   element, but no longer a `Div`).
//! - `.text(..)` is what the element draws itself (a string child); an
//!   element's text for a test is its own plus its tagged descendants'.
//! - `.focusable(&handle)` lets `type` and `press` focus it directly.
//! - The window's root view wraps its output in [`frame`], which starts a
//!   new tree each frame (the `body` element).
//! - `submit` clicks the first `[type=submit]` element inside the target.
//! - Elements painted later through `deferred` (menus, popovers) are not
//!   attached to their logical parent yet: they have no parent in the tree.

#[cfg(feature = "e2e")]
mod bridge;
#[cfg(feature = "e2e")]
mod selector;
#[cfg(feature = "e2e")]
mod tag;
#[cfg(feature = "e2e")]
mod tree;

/// How far a scrolling element is scrolled, in pixels: `from_bottom` (0 at
/// the newest end), the `range` it can scroll and its `viewport` height.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollState {
    pub from_bottom: f32,
    pub range: f32,
    pub viewport: f32,
}

/// What a scrolling element offers the bridge (`scroll_state`, `scroll`).
pub trait Scrollable {
    fn scroll_state(&self) -> ScrollState;
    /// Scrolls to `from_bottom` pixels from the bottom (clamped to the range).
    fn scroll_to(&self, from_bottom: f32);
}

/// A scroll container's own handle as a probe (offset 0 at the top).
pub struct HandleProbe(pub gpui::ScrollHandle);

impl Scrollable for HandleProbe {
    fn scroll_state(&self) -> ScrollState {
        let max = f32::from(self.0.max_offset().height);
        let offset = f32::from(self.0.offset().y);
        ScrollState {
            from_bottom: (max + offset).max(0.0),
            range: max,
            viewport: f32::from(self.0.bounds().size.height),
        }
    }

    fn scroll_to(&self, from_bottom: f32) {
        let max = f32::from(self.0.max_offset().height);
        let mut offset = self.0.offset();
        offset.y = gpui::px(from_bottom.clamp(0.0, max) - max);
        self.0.set_offset(offset);
    }
}

#[cfg(feature = "e2e")]
pub use bridge::start;
#[cfg(feature = "e2e")]
#[allow(unused_imports)]
pub use tag::{E2eExt, Tagged, frame, text_links};

#[cfg(not(feature = "e2e"))]
mod off {
    use gpui::{FocusHandle, IntoElement};

    /// An element with an e2e tag (feature off: the element itself).
    pub struct Tagged<E>(E);

    impl<E> Tagged<E> {
        #[inline(always)]
        pub fn attr(self, _name: &str, _value: impl Into<String>) -> Self {
            self
        }
        #[inline(always)]
        pub fn text(self, _text: impl Into<String>) -> Self {
            self
        }
        #[inline(always)]
        pub fn focusable(self, _handle: &FocusHandle) -> Self {
            self
        }
        #[inline(always)]
        pub fn scrollable(self, _probe: std::rc::Rc<dyn super::Scrollable>) -> Self {
            self
        }
    }

    /// Text with links in it (feature off: the element itself).
    #[inline(always)]
    pub fn text_links<E: IntoElement>(
        inner: E,
        _layout: &gpui::TextLayout,
        _links: Vec<(std::ops::Range<usize>, String)>,
    ) -> E {
        inner
    }

    impl<E: IntoElement> IntoElement for Tagged<E> {
        type Element = E::Element;
        #[inline(always)]
        fn into_element(self) -> Self::Element {
            self.0.into_element()
        }
    }

    pub trait E2eExt: IntoElement + Sized {
        #[inline(always)]
        fn e2e(self, _tag: &str, _classes: &str) -> Tagged<Self> {
            Tagged(self)
        }
    }
    impl<T: IntoElement> E2eExt for T {}

    #[inline(always)]
    pub fn frame<E: IntoElement>(element: E) -> Tagged<E> {
        Tagged(element)
    }
}
#[cfg(not(feature = "e2e"))]
#[allow(unused_imports)]
pub use off::{E2eExt, Tagged, frame, text_links};

/// What `selector` matches in the last frame, as `(text, centre)`: for
/// widget tests, which click at the centre and read the text.
#[cfg(all(feature = "e2e", test))]
pub fn query(selector: &str) -> Vec<(String, (f32, f32))> {
    let selector = selector::Selector::parse(selector).expect("selector");
    tag::with_state(|state| {
        selector
            .select(&state.tree)
            .into_iter()
            .map(|i| (state.tree.text(i), state.tree.nodes[i].bounds.centre()))
            .collect()
    })
}

/// The bounds of the first match in the last frame, for widget tests.
#[cfg(all(feature = "e2e", test))]
pub fn query_bounds(selector: &str) -> Option<tree::Rect> {
    let selector = selector::Selector::parse(selector).expect("selector");
    tag::with_state(|state| {
        selector
            .select(&state.tree)
            .first()
            .map(|&i| state.tree.nodes[i].bounds)
    })
}

/// An attribute of the first match in the last frame, for widget tests.
#[cfg(all(feature = "e2e", test))]
pub fn query_attr(selector: &str, name: &str) -> Option<String> {
    let selector = selector::Selector::parse(selector).expect("selector");
    tag::with_state(|state| {
        selector
            .select(&state.tree)
            .first()
            .and_then(|&i| state.tree.nodes[i].attr(name))
    })
}

/// The composer the bridge's `attach_file` hands files to (the open room's).
#[cfg(feature = "e2e")]
pub fn on_attach(composer: gpui::WeakEntity<crate::composer::Composer>) {
    bridge::set_attach_target(composer);
}
