//! Password locks on rooms and DMs, on this device (`view_model::locks` has
//! the rules). A locked room shows a lock screen where its messages would be
//! and no timeline is made for it, so nothing of it is held or drawn; the
//! notifier, the inbox and the room list treat it as hidden through
//! `is_locked`.

use std::{
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use gpui::{AnyElement, Context, FontWeight, IntoElement, div, prelude::*, px, rgb};
use morpheus_core::{OwnedRoomId, RoomSummary};

use crate::{
    e2e::E2eExt as _,
    icons::icon,
    input::{Submit, TextInput},
    media::avatar,
    shell::Shell,
    theme,
    view_model::locks::{self as vm, Locks},
};

/// How often the clock of open rooms is looked at.
const TICK: Duration = Duration::from_millis(500);

static LOCKS: OnceLock<Mutex<Locks>> = OnceLock::new();

fn locks() -> &'static Mutex<Locks> {
    LOCKS.get_or_init(|| {
        let mut locks = Locks::load(&crate::data_dir());
        // Tests shorten the timeout (a number of seconds).
        if std::env::var_os("MORPHEUS_E2E_PORT").is_some()
            && let Some(secs) = std::env::var("MORPHEUS_E2E_LOCK_SECS").ok().and_then(|s| s.parse::<f64>().ok())
        {
            locks.override_timeout(Duration::from_secs_f64(secs));
        }
        Mutex::new(locks)
    })
}

/// Run `f` on the locks, and keep them if it says it changed them.
fn with<R>(f: impl FnOnce(&mut Locks) -> R) -> R {
    match locks().lock() {
        Ok(mut locks) => f(&mut locks),
        Err(poisoned) => f(&mut poisoned.into_inner()),
    }
}

fn save() {
    let dir = crate::data_dir();
    if let Err(err) = with(|locks| locks.save(&dir)) {
        eprintln!("morpheus-desktop: locks not kept: {err}");
    }
}

/// Whether the room has a lock that is closed now: its content stays hidden.
pub fn is_locked(room: &OwnedRoomId) -> bool {
    with(|locks| locks.is_locked(room.as_str()))
}

pub fn has_lock(room: &OwnedRoomId) -> bool {
    with(|locks| locks.has_lock(room.as_str()))
}

pub fn hash_of(room: &OwnedRoomId) -> Option<String> {
    with(|locks| locks.hash_of(room.as_str()).map(str::to_owned))
}

/// The rooms that are not locked (what counts and lists may use).
pub fn unlocked(rooms: &[RoomSummary]) -> Vec<RoomSummary> {
    with(|locks| rooms.iter().filter(|r| !locks.is_locked(r.id.as_str())).cloned().collect())
}

/// Signing out: no lock outlives the account it was made in.
pub fn clear_all() {
    with(Locks::clear);
    save();
}

/// Settings: how long an open room that is not looked at stays open.
pub fn timeout_secs() -> u64 {
    with(|locks| locks.timeout_secs())
}

pub fn set_timeout_secs(secs: u64) {
    with(|locks| locks.set_timeout_secs(secs));
    save();
}

impl Shell {
    /// Starts the clock that locks open rooms again.
    pub fn start_lock_clock(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                if this.update(cx, |this, cx| this.lock_tick(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// Time passes: the room in front (its window active) is the one being
    /// looked at; open rooms not looked at for the timeout close.
    pub fn lock_tick(&mut self, cx: &mut Context<Self>) {
        let active = cx
            .windows()
            .first()
            .and_then(|w| w.update(cx, |_, window, _| window.is_window_active()).ok())
            .unwrap_or(false);
        let watching = self.selected.clone().filter(|_| active).map(|id| id.to_string());
        let closed = with(|locks| locks.tick(Instant::now(), watching.as_deref()));
        for room in closed {
            if let Ok(room) = OwnedRoomId::try_from(room) {
                self.apply_lock(&room, cx);
            }
        }
    }

    /// A room just closed (a lock put on, "Lock now", the clock): nothing of
    /// it stays in the cache or on screen.
    pub fn apply_lock(&mut self, room: &OwnedRoomId, cx: &mut Context<Self>) {
        drop(self.timeline_cache.take(room));
        if self.selected.as_ref() == Some(room) {
            self.timeline = None;
            self.composer = None;
            self.typing = None;
            self.timeline_handle = None;
            self.members = None;
            self.thread = None;
            self.search = None;
            self.pinned.clear();
            self.show_lock_screen(room, cx);
        }
        cx.notify();
    }

    /// The password box of a locked room that is open.
    pub fn show_lock_screen(&mut self, room: &OwnedRoomId, cx: &mut Context<Self>) {
        let input = cx.new(|cx| TextInput::new("password", "Password", true, "", cx).framed());
        cx.subscribe(&input, |this, _, _: &Submit, cx| this.try_unlock(cx)).detach();
        self.lock_input = Some((room.clone(), input));
        self.lock_error = None;
        self.lock_busy = false;
        self.lock_take_focus = true;
    }

    /// A lock was put on `room` with the password whose hash this is.
    pub fn lock_set(&mut self, room: &OwnedRoomId, hash: String, cx: &mut Context<Self>) {
        with(|locks| locks.add(room.as_str(), hash));
        save();
        self.dialog_close(cx);
        self.apply_lock(room, cx);
    }

    /// The lock was taken off `room`.
    pub fn lock_removed(&mut self, room: &OwnedRoomId, cx: &mut Context<Self>) {
        with(|locks| locks.remove(room.as_str()));
        save();
        self.dialog_close(cx);
        self.lock_input = None;
        if self.selected.as_ref() == Some(room) {
            self.open_room_now(room.clone(), cx);
        }
        cx.notify();
    }

    /// "Lock now" in an open room's menu.
    pub fn lock_now(&mut self, room: &OwnedRoomId, cx: &mut Context<Self>) {
        with(|locks| locks.lock(room.as_str()));
        self.apply_lock(room, cx);
    }

    /// Unlock was pressed: check the password (off the UI thread; hashing is
    /// slow on purpose).
    pub fn try_unlock(&mut self, cx: &mut Context<Self>) {
        let Some((room, input)) = self.lock_input.clone() else {
            return;
        };
        if self.lock_busy {
            return;
        }
        let password = input.read(cx).value().to_owned();
        let Some(hash) = hash_of(&room) else {
            return;
        };
        self.lock_busy = true;
        cx.notify();
        let checking = cx.background_executor().spawn(async move { vm::verify_password(&hash, &password) });
        cx.spawn(async move |this, cx| {
            let right = checking.await;
            this.update(cx, |this, cx| {
                this.lock_busy = false;
                if right {
                    with(|locks| locks.unlock(room.as_str()));
                    this.lock_input = None;
                    this.lock_error = None;
                    this.open_room_now(room, cx);
                } else {
                    this.lock_error = Some("That is not the password.".into());
                    input.update(cx, |input, cx| input.set_value("", cx));
                    this.lock_take_focus = true;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

/// The page of a locked room: its name in a bar, and the password box.
pub fn lock_screen(shell: &Shell, cx: &mut Context<Shell>, room: &RoomSummary) -> AnyElement {
    let title = crate::streamer::room_title(&shell.nicknames, room);
    let anchor = shell.title_bounds.clone();
    let menu_room = room.clone();
    let icon_el = if room.is_dm {
        avatar(shell.picture(&room.avatar, 24), &title, 24.0, None).into_any_element()
    } else {
        div().font_weight(FontWeight::SEMIBOLD).child("#").e2e("span", "hash").text("#").into_any_element()
    };
    let header = div()
        .h(px(48.0))
        .flex_none()
        .flex()
        .items_center()
        .pl(px(6.0))
        .pr(px(16.0))
        .border_b_1()
        .border_color(gpui::rgba(0x0000004d))
        .child(
            div()
                .id("room-head")
                .h(px(32.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .px(px(10.0))
                .rounded(px(theme::RADIUS_SM))
                .cursor_pointer()
                .text_color(rgb(theme::text_muted()))
                .child(icon_el)
                .child(
                    div()
                        .relative()
                        .font_weight(FontWeight::BOLD)
                        .child(title.clone())
                        .child(
                            gpui::canvas(
                                {
                                    let anchor = anchor.clone();
                                    move |bounds, _, _| anchor.set(bounds)
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_full(),
                        )
                        .e2e("h2", "room-title")
                        .text(title.clone()),
                )
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    let b = anchor.get();
                    let at = gpui::point(b.origin.x, b.origin.y + b.size.height + px(2.0));
                    this.open_room_menu(menu_room.clone(), at, cx)
                }))
                .e2e("button", "ghost room-head"),
        );
    let mut card = div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(12.0))
        .w(px(320.0))
        .child(icon("lock", 40.0).text_color(rgb(theme::text_muted())))
        .child(
            div()
                .font_weight(FontWeight::BOLD)
                .text_size(px(1.2 * theme::REM))
                .child(vm::LOCKED_TITLE)
                .e2e("h3", "lock-title")
                .text(vm::LOCKED_TITLE),
        );
    if let Some((id, input)) = &shell.lock_input
        && id == &room.id
    {
        card = card.child(div().w_full().child(input.clone()));
    }
    if let Some(error) = &shell.lock_error {
        card = card.child(
            div()
                .text_color(rgb(theme::danger()))
                .child(error.clone())
                .e2e("p", "error")
                .text(error.clone()),
        );
    }
    card = card.child(crate::room_dialogs::button(
        "lock-unlock",
        Some("lock-open"),
        "Unlock",
        "primary unlock",
        false,
        shell.lock_busy,
        cx.listener(|this, _, _, cx| this.try_unlock(cx)),
    ));
    card = card.child(
        div()
            .text_size(px(0.85 * theme::REM))
            .text_color(rgb(theme::text_faint()))
            .text_center()
            .child(vm::FORGOT)
            .e2e("p", "muted lock-hint")
            .text(vm::FORGOT),
    );
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .child(header.e2e("header", "chat-header"))
        .child(
            div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .child(card.e2e("div", "lock-card"))
                .e2e("div", "lock-screen"),
        )
        .into_any_element()
}
