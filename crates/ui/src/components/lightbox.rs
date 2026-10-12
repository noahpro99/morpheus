//! Full-size image viewer. A room provides the current gallery (the list of
//! image messages in its timeline) and the open index; `ImageBody` in
//! `timeline.rs` sets the index on click, and this overlay renders on top.

use dioxus::prelude::*;
use morpheus_core::MediaRef;

use crate::media::{use_full_media, use_thumbnail};

/// One image available in the current room's gallery.
#[derive(Clone, PartialEq)]
pub struct LightboxImage {
    pub media: MediaRef,
    pub body: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// The current room's images, in timeline order. Kept in sync by `TimelineView`.
pub type LightboxImages = Signal<Vec<LightboxImage>>;
/// Index into `LightboxImages` of the open image, or `None` when closed.
pub type LightboxIndex = Signal<Option<usize>>;

/// Install once per open room, alongside the timeline it describes.
pub fn provide_lightbox() {
    use_context_provider(LightboxImages::default);
    use_context_provider(LightboxIndex::default);
}

#[component]
pub fn Lightbox() -> Element {
    let images = use_context::<LightboxImages>();
    let mut index = use_context::<LightboxIndex>();
    // Tell the shell when we are open, and close on the Escape it routes here.
    let mut shown = use_context::<LightboxOpen>();
    let escapes = use_context::<EscapeTick>();
    use_effect(move || {
        let open = index().is_some();
        if shown.peek().0 != open {
            shown.set(LightboxShown(open));
        }
    });
    let mut seen = use_signal(|| escapes.peek().0);
    use_effect(move || {
        let tick = escapes().0;
        if tick != *seen.peek() {
            seen.set(tick);
            if index.peek().is_some() {
                index.set(None);
            }
        }
    });

    let Some(current) = index() else {
        return rsx! {};
    };
    let Some(image) = images.read().get(current).cloned() else {
        // The gallery changed under us (e.g. a redaction); just close.
        index.set(None);
        return rsx! {};
    };
    let count = images.read().len();

    let thumb_url = use_thumbnail(Some(image.media.clone()), 800, 600);
    let full_url = use_full_media(Some(image.media.clone()));
    let full_now = full_url();
    let loading = full_now.is_none();
    // Not actually redundant: `Memo`'s call sugar derefs to an unsized `dyn
    // Fn`, which `Option::or_else` cannot take directly.
    #[allow(clippy::redundant_closure)]
    let display_url = full_now.or_else(|| thumb_url());

    let dims = match (image.width, image.height) {
        (Some(w), Some(h)) => format!("{w} × {h}"),
        _ => String::new(),
    };

    let mut close = move || index.set(None);
    let mut go = move |delta: i32| {
        let Some(cur) = index() else { return };
        let len = images.read().len() as i32;
        if len == 0 {
            return;
        }
        index.set(Some((cur as i32 + delta).clamp(0, len - 1) as usize));
    };

    rsx! {
        div {
            class: "overlay lightbox-overlay",
            tabindex: "-1",
            autofocus: true,
            onclick: move |_| close(),
            onkeydown: move |e| match e.key() {
                Key::ArrowLeft => go(-1),
                Key::ArrowRight => go(1),
                _ => {}
            },
            div { class: "lightbox", onclick: move |e| e.stop_propagation(),
                button { class: "lightbox-close", title: "Close", onclick: move |_| close(), "✕" }
                if count > 1 && current > 0 {
                    button {
                        class: "lightbox-nav prev",
                        title: "Previous image",
                        onclick: move |_| go(-1),
                        "‹"
                    }
                }
                div { class: "lightbox-image-wrap",
                    match &display_url {
                        Some(src) => rsx! {
                            img { class: "lightbox-image", src: "{src}", alt: "{image.body}" }
                        },
                        None => rsx! { div { class: "lightbox-loading", "Loading…" } },
                    }
                    if loading && display_url.is_some() {
                        div { class: "lightbox-loading-badge", "Loading full size…" }
                    }
                }
                if count > 1 && current + 1 < count {
                    button {
                        class: "lightbox-nav next",
                        title: "Next image",
                        onclick: move |_| go(1),
                        "›"
                    }
                }
                div { class: "lightbox-caption",
                    span { class: "lightbox-filename", "{image.body}" }
                    if !dims.is_empty() {
                        span { class: "lightbox-dims", "{dims}" }
                    }
                    if count > 1 {
                        span { class: "lightbox-count", "{current + 1} / {count}" }
                    }
                }
            }
        }
    }
}

/// Whether a lightbox is open anywhere, so the shell's Escape handling can
/// hand that press to the lightbox instead of the next modal down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LightboxShown(pub bool);
pub type LightboxOpen = Signal<LightboxShown>;

/// Bumped by the shell for every Escape it routes to the lightbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Escapes(pub u32);
pub type EscapeTick = Signal<Escapes>;
