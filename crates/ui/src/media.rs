//! Media loading for the webview: thumbnails become `data:` URLs.
//!
//! The SDK caches bytes on disk; this layer caches the encoded URL in memory
//! so re-renders and repeated senders do not re-encode.

use std::{collections::HashMap, rc::Rc};

use base64::{Engine, engine::general_purpose::STANDARD};
use dioxus::prelude::*;
use morpheus_core::{MediaRef, Session};

/// `(media key, width, height)` → data URL.
type Cache = Signal<HashMap<(String, u32, u32), Rc<str>>>;

/// Install once near the root of the logged-in shell.
pub fn provide_media_cache() {
    use_context_provider(|| Cache::new(HashMap::new()));
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
            if let Some(url) = cache.peek().get(&cache_key) {
                return Some(url.clone());
            }
            let fetched = session
                .thumbnail(media, width, height)
                .await
                .ok()
                .flatten()?;
            let url: Rc<str> = format!(
                "data:{};base64,{}",
                fetched.mime(),
                STANDARD.encode(&fetched.bytes)
            )
            .into();
            cache.write().insert(cache_key, url.clone());
            Some(url)
        }
    }));

    use_memo(move || {
        if let Some(key) = &key
            && let Some(url) = cache.read().get(key)
        {
            return Some(url.clone());
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
