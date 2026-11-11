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

#[cfg(feature = "e2e")]
pub use bridge::start;
#[cfg(feature = "e2e")]
#[allow(unused_imports)]
pub use tag::{E2eExt, Tagged, frame};

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
pub use off::{E2eExt, Tagged, frame};
