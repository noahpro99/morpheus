//! System notifications (the desktop notifier of `apps/desktop/src/main.rs`):
//! a message announced while the window is in the background or hidden in
//! the tray, or someone calling. Test runs never notify (they would pile
//! real pop-ups on the developer's desktop): they write what would show to
//! `shown-notifications.jsonl`.

use crate::view_model::i18n::ts;
use std::path::PathBuf;

use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

use gpui::Context;
use morpheus_core::{Notification, OwnedRoomId, Session};

use crate::{
    shell::Shell,
    view_model::{notices, settings},
};

/// The system's sound for an incoming call (freedesktop sound theme name).
pub const RING_SOUND: &str = "phone-incoming-call";

/// Raise an OS notification, the sender's picture as its icon and a photo
/// sent shown in it. Off the UI thread, since the D-Bus round trip can stall
/// when no notification daemon answers.
pub fn show(
    title: String,
    body: String,
    icon: Option<PathBuf>,
    image: Option<PathBuf>,
    sound: Option<&'static str>,
) {
    show_for(title, body, icon, image, sound, None);
}

/// As [`show`], a click on it opening `launch` (a `win_toast::launch_arg`): the
/// room, or the message, it is about. Only Windows toasts carry one; a click
/// with none brings the window to the front.
pub fn show_for(
    title: String,
    body: String,
    icon: Option<PathBuf>,
    image: Option<PathBuf>,
    sound: Option<&'static str>,
    launch: Option<String>,
) {
    eprintln!("morpheus-desktop: notification: {title}");
    if std::env::var_os("MORPHEUS_E2E_PORT").is_some() {
        let line = serde_json::json!({
            "title": title,
            "body": body,
            "icon": icon,
            "image": image,
            "sound": sound,
            "launch": launch,
        });
        let written = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(crate::data_dir().join("shown-notifications.jsonl"))
            .and_then(|mut file| {
                std::io::Write::write_all(&mut file, format!("{line}\n").as_bytes())
            });
        if let Err(err) = written {
            eprintln!("morpheus-desktop: test notification log not written: {err}");
        }
        return;
    }
    std::thread::spawn(move || {
        #[cfg(target_os = "windows")]
        {
            let launch = launch
                .unwrap_or_else(|| format!("{}:", crate::view_model::win_toast::PROTOCOL));
            let toast = crate::view_model::win_toast::Toast {
                title: &title,
                body: &body,
                icon: icon.as_deref(),
                image: image.as_deref(),
                launch: Some(&launch),
                ringing: sound.is_some(),
            };
            if let Err(err) = crate::win_notify::show(&toast) {
                eprintln!("morpheus-desktop: desktop notification failed: {err}");
            }
            return;
        }
        #[cfg(not(target_os = "windows"))]
        let _ = &launch;
        let mut notification = notify_rust::Notification::new();
        notification.appname("Morpheus").summary(&title).body(&body);
        if let Some(sound) = sound {
            notification.sound_name(sound);
        }
        if let Some(icon) = &icon {
            notification.icon(&icon.to_string_lossy());
        }
        if let Some(image) = &image {
            notification.image_path(&image.to_string_lossy());
        }
        if let Err(err) = notification.show() {
            eprintln!("morpheus-desktop: desktop notification failed: {err}");
        }
    });
}

/// What a click on the toast of `room` (and its message) opens.
fn launch_for(room: &OwnedRoomId, event: Option<&str>) -> String {
    crate::view_model::win_toast::launch_arg(room.as_str(), event)
}

/// Run `future` on a small runtime of its own, as the threads here need one
/// (the core's sleeps are tokio's).
fn block_on<F: std::future::Future>(future: F) -> Option<F::Output> {
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => Some(runtime.block_on(future)),
        Err(err) => {
            eprintln!("morpheus-desktop: notifier runtime failed: {err}");
            None
        }
    }
}

// ---- the process-wide notifier ----------------------------------------------
//
// One thread for the whole run (as the Dioxus app's `notifier`): the window
// can be closed to the tray, and messages must go on being announced then.

static LOG: Mutex<Vec<Notification>> = Mutex::new(Vec::new());
/// Whether the window is shown and focused, and the room it has open.
static ACTIVE: AtomicBool = AtomicBool::new(false);
static OPEN_ROOM: Mutex<Option<OwnedRoomId>> = Mutex::new(None);

/// Sounds made by the notifier, for the shell to note in its log (`.sound-log`).
static SOUNDS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
/// When each room last made a message sound.
static LAST_SOUND: Mutex<Vec<(OwnedRoomId, std::time::Instant)>> = Mutex::new(Vec::new());
/// One message sound per room in this time: a burst is one ping.
const SOUND_GAP: std::time::Duration = std::time::Duration::from_secs(2);

/// The short sound of a new message that notifies you (the notifier has
/// already left out your own, muted rooms and the room you are looking at),
/// unless this device has them off or `room` just made one.
fn message_sound(room: &OwnedRoomId) {
    let dir = crate::data_dir();
    if !settings::message_sounds_enabled(&dir) {
        return;
    }
    let now = std::time::Instant::now();
    if let Ok(mut last) = LAST_SOUND.lock() {
        last.retain(|(_, at)| now.duration_since(*at) < SOUND_GAP);
        if last.iter().any(|(r, _)| r == room) {
            return;
        }
        last.push((room.clone(), now));
    }
    let log = std::env::var_os("MORPHEUS_E2E_PORT").map(|_| dir.join("played-sounds.log"));
    crate::sounds::play(crate::view_model::calls::Sound::Message, log.as_ref());
    if let Ok(mut sounds) = SOUNDS.lock() {
        sounds.push(crate::view_model::calls::Sound::Message.key());
    }
}

/// The announcements since launch (the inbox's Mentions, the test log).
pub fn log() -> Vec<Notification> {
    LOG.lock().map(|log| log.clone()).unwrap_or_default()
}

/// The window's focus, and which room it shows, as the notifier needs them.
pub fn set_looking(active: bool, room: Option<OwnedRoomId>) {
    ACTIVE.store(active, Ordering::Relaxed);
    if let Ok(mut open) = OPEN_ROOM.lock() {
        *open = room;
    }
}

/// Whether a new notice flashes the taskbar button: not while the window is
/// in front (the person sees it), not with notifications off on this device.
/// Notices of muted rooms never reach the notifier, so they do not flash.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn should_flash(window_in_front: bool, notifications_enabled: bool) -> bool {
    !window_in_front && notifications_enabled
}

/// Start announcing `session`'s new messages. `redraw` is called after each
/// is logged, so an open window shows it.
pub fn spawn_notifier(session: Session, redraw: impl Fn() + Send + 'static) {
    std::thread::Builder::new()
        .name("morpheus-notifier".into())
        .spawn(move || {
            block_on(async move {
                let Ok(mut updates) = session.notifications().await else {
                    eprintln!("morpheus-desktop: notification subscription failed");
                    return;
                };
                while let Some(notice) = updates.next().await {
                    let looking = ACTIVE.load(Ordering::Relaxed)
                        && OPEN_ROOM
                            .lock()
                            .ok()
                            .is_some_and(|room| room.as_ref() == Some(&notice.room_id));
                    if let Ok(mut log) = LOG.lock() {
                        notices::log_notice(&mut log, notice.clone());
                    }
                    if !looking {
                        message_sound(&notice.room_id);
                    }
                    redraw();
                    if looking {
                        continue;
                    }
                    let dir = crate::data_dir();
                    #[cfg(target_os = "windows")]
                    if should_flash(
                        ACTIVE.load(Ordering::Relaxed),
                        settings::notifications_enabled(&dir),
                    ) {
                        crate::win_sys::flash_taskbar();
                    }
                    if !settings::notifications_enabled(&dir) {
                        eprintln!(
                            "morpheus-desktop: notifications are off on this device; not shown"
                        );
                        continue;
                    }
                    if crate::locks::is_locked(&notice.room_id) {
                        show_for(
                            "Morpheus".into(),
                            crate::view_model::locks::NOTIFICATION.into(),
                            None,
                            None,
                            None,
                            Some(launch_for(&notice.room_id, Some(&notice.event_id))),
                        );
                        continue;
                    }
                    if crate::streamer::notifications_hidden() {
                        show_for(
                            "Morpheus".into(),
                            ts("Someone sent a message").into(),
                            None,
                            None,
                            None,
                            Some(launch_for(&notice.room_id, Some(&notice.event_id))),
                        );
                        continue;
                    }
                    let launch = launch_for(&notice.room_id, Some(&notice.event_id));
                    let ready = notices::ready(&session, notice, &dir).await;
                    show_for(
                        ready.text.title,
                        ready.text.body,
                        ready.icon,
                        ready.image,
                        None,
                        Some(launch),
                    );
                }
            });
        })
        .map(drop)
        .unwrap_or_else(|err| eprintln!("morpheus-desktop: notifier thread failed: {err}"));
}

impl Shell {
    /// The log of announcements, as the notifier has it.
    pub fn sync_notices(&mut self, cx: &mut Context<Self>) {
        let sounds: Vec<&'static str> = SOUNDS
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default();
        if !sounds.is_empty() {
            self.calls.sounds.extend(sounds);
            cx.notify();
        }
        let log = log();
        if log != self.notices {
            self.notices = log;
            cx.notify();
        }
    }
}

/// Rings with the window closed to the tray: the Shell that rings them is
/// gone, so this thread does (the ringtone every other second, and the
/// system's notification once), until the call stops ringing or the window
/// is back (its own Shell rings from then on).
pub fn spawn_ring_watcher(session: Session) {
    std::thread::Builder::new()
        .name("morpheus-rings".into())
        .spawn(move || {
            block_on(async move {
                let Ok(mut rings) = session.ring_updates().await else {
                    eprintln!("morpheus-desktop: ring subscription failed");
                    return;
                };
                let declined = crate::view_model::ringing::Declined::default();
                while let Some(ring) = rings.next().await {
                    let call = match ring {
                        morpheus_core::Ringing::Incoming(call) => call,
                        morpheus_core::Ringing::Declined { notification, .. } => {
                            if let Ok(mut declined) = declined.lock() {
                                declined.insert(notification.to_string());
                            }
                            continue;
                        }
                    };
                    if !crate::headless() {
                        continue;
                    }
                    let (session, declined) = (session.clone(), declined.clone());
                    std::thread::spawn(move || {
                        block_on(async move {
                            let dir = crate::data_dir();
                            if settings::notifications_enabled(&dir) {
                                let name = match session.profile(call.caller.clone()).await {
                                    Ok(profile) => profile.name,
                                    Err(_) => call.caller.localpart().to_owned(),
                                };
                                let hidden = settings::notification_content(&dir)
                                    == settings::NotificationContent::Hidden
                                    || crate::streamer::notifications_hidden();
                                show_for(
                                    if crate::locks::is_locked(&call.room_id) {
                                        crate::locks::locked_call()
                                    } else {
                                        crate::view_model::i18n::tf(
                                            "%{name} is calling",
                                            &[("name", if hidden { ts("Someone") } else { &name })]
                                        )
                                    },
                                    ts("Open Morpheus to answer").into(),
                                    None,
                                    None,
                                    Some(RING_SOUND),
                                    Some(launch_for(&call.room_id, None)),
                                );
                            }
                            let log = std::env::var_os("MORPHEUS_E2E_PORT")
                                .is_some()
                                .then(|| dir.join("played-sounds.log"));
                            crate::view_model::ringing::while_ringing(
                                &session,
                                &call,
                                &declined,
                                || {
                                    if crate::headless() {
                                        crate::sounds::play(
                                            crate::view_model::calls::Sound::Ring,
                                            log.as_ref(),
                                        );
                                    }
                                },
                            )
                            .await;
                        });
                    });
                }
            });
        })
        .map(drop)
        .unwrap_or_else(|err| eprintln!("morpheus-desktop: ring thread failed: {err}"));
}

#[cfg(test)]
mod tests {
    use super::should_flash;

    #[test]
    fn the_taskbar_flashes_only_for_a_window_out_of_front_with_notifications_on() {
        assert!(should_flash(false, true));
        assert!(!should_flash(true, true));
        assert!(!should_flash(false, false));
        assert!(!should_flash(true, false));
    }
}
