//! Link previews: the cards under messages with a link. What the homeserver
//! says each page is (`morpheus_core::UrlPreview`) and the pictures are kept
//! here, shared by every timeline; the settings and the cards dismissed are
//! `view_model::link_previews`, kept in the data directory.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, LazyLock, Mutex},
};

use gpui::{Context, Image};
use morpheus_core::{Session, UrlPreview};

use crate::{
    media,
    view_model::link_previews::{self as vm, Dismissed, Settings},
};

struct State {
    dir: PathBuf,
    settings: Settings,
    dismissed: Dismissed,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
    Mutex::new(State {
        dir: PathBuf::new(),
        settings: Settings::default(),
        dismissed: Dismissed::default(),
    })
});
/// What the homeserver said about each link (`None`: nothing to show).
static CARDS: LazyLock<Mutex<HashMap<String, Option<UrlPreview>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static PENDING: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));
static PICTURES: LazyLock<Mutex<HashMap<String, Arc<Image>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}

/// Reads what this device kept.
pub fn init(dir: &std::path::Path) {
    let (settings, dismissed) = vm::load(dir);
    let mut state = state();
    state.dir = dir.to_owned();
    state.settings = settings;
    state.dismissed = dismissed;
}

fn save(state: &State) {
    if let Err(err) = vm::save(&state.dir, &state.settings, &state.dismissed) {
        eprintln!("morpheus-desktop: saving link preview settings: {err}");
    }
}

pub fn settings() -> Settings {
    state().settings.clone()
}

pub fn set_enabled(on: bool) {
    let mut state = state();
    state.settings.enabled = on;
    save(&state);
}

pub fn set_in_encrypted(on: bool) {
    let mut state = state();
    state.settings.in_encrypted = on;
    save(&state);
}

pub fn is_dismissed(message: &str) -> bool {
    state().dismissed.contains(message)
}

pub fn dismiss(message: &str) {
    let mut state = state();
    state.dismissed.add(message);
    save(&state);
}

/// The card for `url`, once the homeserver has answered with something.
pub fn card(url: &str) -> Option<UrlPreview> {
    CARDS.lock().ok()?.get(url).cloned().flatten()
}

/// The card's picture (by `mxc://` URI), once fetched.
pub fn picture(key: &str) -> Option<Arc<Image>> {
    PICTURES.lock().ok()?.get(key).cloned()
}

/// Asks the homeserver about `url` (once) and fetches the card's picture;
/// `arrived` runs on the view when either came, to lay the row out again.
pub fn want<T: 'static>(
    session: &Session,
    url: &str,
    cx: &mut Context<T>,
    arrived: impl Fn(&mut T, &mut Context<T>) + Clone + 'static,
) {
    let known = CARDS.lock().is_ok_and(|c| c.contains_key(url));
    if known {
        if let Some(card) = card(url)
            && let Some(image) = card.image
        {
            want_picture(session, image, cx, arrived);
        }
        return;
    }
    if !PENDING.lock().is_ok_and(|mut p| p.insert(url.to_owned())) {
        return;
    }
    let asking = session.url_preview(url.to_owned());
    let (session, url) = (session.clone(), url.to_owned());
    cx.spawn(async move |this, cx| {
        let card = asking.await.ok().flatten();
        if let Ok(mut cards) = CARDS.lock() {
            cards.insert(url.clone(), card);
        }
        if let Ok(mut pending) = PENDING.lock() {
            pending.remove(&url);
        }
        this.update(cx, |this, cx| {
            arrived(this, cx);
            want_known(&session, &url, cx, arrived.clone());
            cx.notify();
        })
        .ok();
    })
    .detach();
}

fn want_known<T: 'static>(
    session: &Session,
    url: &str,
    cx: &mut Context<T>,
    arrived: impl Fn(&mut T, &mut Context<T>) + Clone + 'static,
) {
    if let Some(card) = card(url)
        && let Some(image) = card.image
    {
        want_picture(session, image, cx, arrived);
    }
}

fn want_picture<T: 'static>(
    session: &Session,
    image: morpheus_core::MediaRef,
    cx: &mut Context<T>,
    arrived: impl Fn(&mut T, &mut Context<T>) + 'static,
) {
    let key = image.key().to_owned();
    if picture(&key).is_some() || !PENDING.lock().is_ok_and(|mut p| p.insert(key.clone())) {
        return;
    }
    let fetching = session.thumbnail(image, 640, 360);
    cx.spawn(async move |this, cx| {
        let decoded = media::fetched(cx, fetching, (640, 360), false).await;
        if let Some(decoded) = decoded
            && let Ok(mut pictures) = PICTURES.lock()
        {
            if pictures.len() >= 60 {
                pictures.clear();
            }
            pictures.insert(key.clone(), decoded);
        }
        if let Ok(mut pending) = PENDING.lock() {
            pending.remove(&key);
        }
        this.update(cx, |this, cx| {
            arrived(this, cx);
            cx.notify();
        })
        .ok();
    })
    .detach();
}
