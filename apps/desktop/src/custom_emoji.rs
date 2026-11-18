//! Custom emoji (MSC2545 packs, `morpheus_core::EmotePack`): their pictures,
//! shared by every place one is drawn (messages, reactions, the picker, the
//! autocomplete), and the packs the open room has in reach (`Shell`).

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, LazyLock, Mutex},
};

use gpui::{Context, Image};
use morpheus_core::{Emote, EmotePack, MediaRef, OwnedRoomId, Session};

use crate::{media, shell::Shell};

/// Pictures fetched so far, by `mxc://` URI, and those on their way.
static IMAGES: LazyLock<Mutex<HashMap<String, Arc<Image>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static PENDING: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// How many pictures are kept before the oldest go (they are fetched again).
const KEPT: usize = 400;

/// The picture at `url`, if fetched ([`want`] starts it).
pub fn image(url: &str) -> Option<Arc<Image>> {
    IMAGES.lock().ok()?.get(url).cloned()
}

/// Starts fetching the picture at `url` if it is not here or on its way;
/// `cx`'s view redraws when it arrives.
pub fn want<T: 'static>(session: &Session, url: &str, cx: &mut Context<T>) {
    if !url.starts_with("mxc://") || image(url).is_some() {
        return;
    }
    let Ok(mut pending) = PENDING.lock() else {
        return;
    };
    if !pending.insert(url.to_owned()) {
        return;
    }
    drop(pending);
    let media = MediaRef::from_mxc_uri(url);
    let fetching = session.thumbnail(media, 96, 96);
    let url = url.to_owned();
    cx.spawn(async move |this, cx| {
        let picture = media::fetched(cx, fetching, (96, 96), false).await;
        if let Some(picture) = picture
            && let Ok(mut images) = IMAGES.lock()
        {
            if images.len() >= KEPT {
                images.clear();
            }
            images.insert(url.clone(), picture);
        }
        if let Ok(mut pending) = PENDING.lock() {
            pending.remove(&url);
        }
        this.update(cx, |_, cx| cx.notify()).ok();
    })
    .detach();
}

/// Every emoji of `packs`, the first of each shortcode winning.
pub fn all(packs: &[EmotePack]) -> Vec<Emote> {
    let mut seen = HashSet::new();
    packs
        .iter()
        .flat_map(|p| p.emotes.iter())
        .filter(|e| seen.insert(e.shortcode.clone()))
        .cloned()
        .collect()
}

/// `(shortcode, uri)` pairs of [`all`], what sending takes.
pub fn pairs(packs: &[EmotePack]) -> Vec<(String, String)> {
    all(packs)
        .into_iter()
        .map(|e| (e.shortcode, e.url))
        .collect()
}

impl Shell {
    /// Reads the packs in reach of the open room (and so what the message
    /// box suggests and the picker lists), now and on every call.
    pub fn refresh_emotes(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let room: Option<OwnedRoomId> = self.selected.clone();
        let reading = session.emote_packs(room.clone());
        cx.spawn(async move |this, cx| {
            let Ok(packs) = reading.await else {
                return;
            };
            this.update(cx, |this, cx| {
                // Another room opened meanwhile: its own read follows.
                if this.selected != room {
                    return;
                }
                this.emote_packs = packs;
                if let Some((_, composer)) = &this.composer {
                    let emotes = all(&this.emote_packs);
                    composer.update(cx, |composer, cx| composer.set_emotes(emotes, cx));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
