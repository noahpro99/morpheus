//! Media loading for the webview: thumbnails become `data:` URLs.
//!
//! The SDK caches bytes on disk; this layer caches the encoded URL in memory
//! so re-renders and repeated senders do not re-encode.

use std::{collections::HashMap, rc::Rc};

use base64::{Engine, engine::general_purpose::STANDARD};
use dioxus::prelude::*;
use morpheus_core::{MediaRef, Session};

/// `(media key, width, height)` → data URL.
/// `None` remembers a failed fetch so dead media is not retried every render.
type Cache = Signal<HashMap<(String, u32, u32), Option<Rc<str>>>>;
/// media key → data URL, for full-resolution files (the lightbox).
type FullCache = Signal<HashMap<String, Option<Rc<str>>>>;

/// Install once near the root of the logged-in shell.
pub fn provide_media_cache() {
    use_context_provider(|| Cache::new(HashMap::new()));
    use_context_provider(|| FullCache::new(HashMap::new()));
}

/// Resolve a thumbnail to a data URL. `None` while loading or when unavailable.
pub fn use_thumbnail(media: Option<MediaRef>, width: u32, height: u32) -> Memo<Option<Rc<str>>> {
    let session = use_context::<Session>();
    let mut cache = use_context::<Cache>();
    let key = media.as_ref().map(|m| (m.key().to_owned(), width, height));

    let fetch = use_resource(use_reactive!(|media| {
        let session = session.clone();
        async move {
            let media = media?;
            let cache_key = (media.key().to_owned(), width, height);
            if let Some(entry) = cache.peek().get(&cache_key) {
                return entry.clone();
            }
            let Some(fetched) = session.thumbnail(media, width, height).await.ok().flatten() else {
                cache.write().insert(cache_key, None);
                return None;
            };
            let url: Rc<str> = format!(
                "data:{};base64,{}",
                fetched.mime(),
                STANDARD.encode(&fetched.bytes)
            )
            .into();
            cache.write().insert(cache_key, Some(url.clone()));
            Some(url)
        }
    }));

    use_memo(move || {
        let cached = cache.read();
        if let Some(entry) = key.as_ref().and_then(|key| cached.get(key)) {
            return entry.clone();
        }
        // The fetch follows the media as it changes (a profile that loads
        // after the picture's first render); `key` is the first render's.
        if let Some(url) = fetch.read().clone().flatten() {
            return Some(url);
        }
        let (media_key, ..) = key.as_ref()?;
        // Another size of the same picture is already here (a member's
        // picture in the list, before their card asks for a larger one):
        // shown until this size arrives.
        cached
            .iter()
            .filter(|((k, _, _), url)| k == media_key && url.is_some())
            .max_by_key(|((_, w, _), _)| *w)
            .and_then(|(_, url)| url.clone())
    })
}

/// Resolve a media's full-resolution file to a data URL. `None` while
/// loading or when unavailable. Used by the lightbox; thumbnails use
/// [`use_thumbnail`] instead.
pub fn use_full_media(media: Option<MediaRef>) -> Memo<Option<Rc<str>>> {
    let session = use_context::<Session>();
    let mut cache = use_context::<FullCache>();
    let key = media.as_ref().map(|m| m.key().to_owned());

    let fetch = use_resource(use_reactive!(|media| {
        let session = session.clone();
        async move {
            let media = media?;
            let cache_key = media.key().to_owned();
            if let Some(entry) = cache.peek().get(&cache_key) {
                return entry.clone();
            }
            let Some(fetched) = session.media_file(media).await.ok().flatten() else {
                cache.write().insert(cache_key, None);
                return None;
            };
            let url: Rc<str> = format!(
                "data:{};base64,{}",
                fetched.mime(),
                STANDARD.encode(&fetched.bytes)
            )
            .into();
            cache.write().insert(cache_key, Some(url.clone()));
            Some(url)
        }
    }));

    use_memo(move || {
        if let Some(key) = &key
            && let Some(entry) = cache.read().get(key)
        {
            return entry.clone();
        }
        fetch.read().clone().flatten()
    })
}

/// A circular avatar that falls back to a letter badge.
#[component]
pub fn Avatar(
    media: Option<MediaRef>,
    fallback: String,
    #[props(default = 32)] size: u32,
) -> Element {
    let url = use_thumbnail(media, size * 2, size * 2);
    let letter = fallback
        .chars()
        .find(|c| c.is_alphanumeric())
        .unwrap_or('?');
    let style = format!("width:{size}px;height:{size}px;font-size:{}px", size / 2);

    rsx! {
        match url() {
            Some(src) => rsx! { img { class: "avatar", style: "{style}", src: "{src}", alt: "{fallback}" } },
            None => rsx! { div { class: "avatar", style: "{style}", "{letter}" } },
        }
    }
}

/// What is coming, in outline, while it loads: `rows` lines, each after a
/// round picture when `pictures` (lists of people).
#[component]
pub fn Skeleton(rows: usize, #[props(default)] pictures: bool) -> Element {
    // Varied widths read as text rather than bars.
    const WIDTHS: [u32; 5] = [72, 54, 86, 61, 45];
    rsx! {
        div { class: "skeleton-block", "aria-hidden": "true",
            for index in 0..rows {
                div { key: "{index}", class: "skeleton-item",
                    if pictures {
                        div { class: "skeleton skeleton-picture" }
                    }
                    div { class: "skeleton skeleton-line", style: "width: {WIDTHS[index % WIDTHS.len()]}%" }
                }
            }
        }
    }
}
