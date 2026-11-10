//! The Rust half of push notifications. The Kotlin `MorpheusPushService`
//! (apps/mobile/android) writes the UnifiedPush endpoint to
//! `push_endpoint` in the data directory; this registers it with the
//! homeserver as a pusher (and drops a replaced one), and keeps
//! `room_names.json` current so notifications can name the room. Plain
//! files keep the two sides free of JNI calls.

use std::{
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::Duration,
};

use morpheus_core::Session;

/// How often the endpoint file and the room names are looked at.
const INTERVAL: Duration = Duration::from_secs(15);

/// The signed-in session the bridge serves; swapped on sign-in.
static ACTIVE: OnceLock<Mutex<Option<Session>>> = OnceLock::new();

/// Called for every session that becomes active. Starts the bridge thread
/// the first time.
pub fn on_session(session: Session, data_dir: PathBuf) {
    write_notices(session.clone(), data_dir.clone());
    let first = ACTIVE.get().is_none();
    let slot = ACTIVE.get_or_init(|| Mutex::new(None));
    if let Ok(mut active) = slot.lock() {
        *active = Some(session);
    }
    if !first {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("morpheus-push".into())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                tracing::warn!("push bridge runtime failed");
                return;
            };
            runtime.block_on(async move {
                let mut empty_passes = 0;
                loop {
                    let session = slot.lock().ok().and_then(|active| active.clone());
                    if let Some(session) = session {
                        sync(&session, &data_dir, &mut empty_passes).await;
                    }
                    tokio::time::sleep(INTERVAL).await;
                }
            });
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "push bridge thread failed");
    }
}

fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

/// Passes with an empty room list before registering anyway (an account
/// with no rooms): about a minute.
const EMPTY_PASSES_BEFORE_PUSH: u32 = 4;

/// One pass. `empty_passes` counts passes that found no rooms yet.
async fn sync(session: &Session, dir: &Path, empty_passes: &mut u32) {
    let mut have_rooms = false;
    if let Ok(names) = session.room_names().await {
        have_rooms = !names.is_empty();
        let map: serde_json::Map<String, serde_json::Value> = names
            .into_iter()
            .map(|(id, name)| (id, serde_json::Value::String(name)))
            .collect();
        let _ = std::fs::write(
            dir.join("room_names.json"),
            serde_json::Value::Object(map).to_string(),
        );
    }

    // Right after sign-in the room list has not synced yet: register once it
    // has, so the first notification can already name its room.
    if !have_rooms && *empty_passes < EMPTY_PASSES_BEFORE_PUSH {
        *empty_passes += 1;
        return;
    }

    let endpoint = read(&dir.join("push_endpoint"));
    // One record per account: each account on the device registers the
    // phone's endpoint for itself (switching accounts keeps the others').
    let user = session
        .user_id()
        .map(|id| id.to_string())
        .unwrap_or_default()
        .replace([':', '@'], "_");
    let registered_path = dir.join(format!("push_registered_{user}"));
    let registered = read(&registered_path);
    if endpoint == registered {
        return;
    }
    if let Some(old) = &registered
        && let Err(err) = session.unregister_push(old.clone()).await
    {
        tracing::warn!(%err, "removing the old pusher failed");
    }
    match endpoint {
        Some(endpoint) => match session
            .register_push(endpoint.clone(), "Morpheus on Android".into())
            .await
        {
            Ok(gateway) => {
                tracing::info!(%gateway, "push notifications registered");
                let _ = std::fs::write(&registered_path, &endpoint);
                let _ =
                    std::fs::write(dir.join("push_status"), format!("registered via {gateway}"));
            }
            Err(err) => {
                tracing::warn!(%err, "registering for push failed");
                let _ = std::fs::write(dir.join("push_status"), format!("failed: {err}"));
            }
        },
        None => {
            let _ = std::fs::remove_file(&registered_path);
        }
    }
}

/// How often to look for a room a tapped notification asked for.
const OPEN_ROOM_POLL: Duration = Duration::from_millis(400);

/// The notification trampoline (`OpenRoomActivity`) leaves the room id in
/// `open_room`; hand it to the shell, which shows that room.
pub fn watch_open_room(data_dir: PathBuf, commands: morpheus_ui::AppCommands) {
    let path = data_dir.join("open_room");
    // A Matrix link opened elsewhere (`OpenLinkActivity`).
    let link = data_dir.join("open_link");
    // Shared from another app (`ShareActivity`).
    let share = data_dir.join("share.json");
    // Hang up, from the phone's ongoing call notification (`CallService`).
    let hang_up = data_dir.join("hang_up_call");
    let spawned = std::thread::Builder::new()
        .name("morpheus-open-room".into())
        .spawn(move || {
            loop {
                if let Some(room) = read(&path) {
                    let _ = std::fs::remove_file(&path);
                    commands.send(morpheus_ui::AppCommand::OpenRoom(room));
                }
                if let Some(url) = read(&link) {
                    let _ = std::fs::remove_file(&link);
                    morpheus_ui::external_links::open(url);
                }
                if let Some(note) = read(&share) {
                    let _ = std::fs::remove_file(&share);
                    match morpheus_ui::shared::from_json(&note) {
                        Some(shared) => morpheus_ui::shared::offer(shared),
                        None => tracing::warn!("an empty share; ignored"),
                    }
                }
                if hang_up.exists() {
                    let _ = std::fs::remove_file(&hang_up);
                    commands.send(morpheus_ui::AppCommand::HangUp);
                }
                std::thread::sleep(OPEN_ROOM_POLL);
            }
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "open-room watcher failed");
    }
}

/// Where notices for the push service go, one file per message.
const NOTICES_DIR: &str = "notices";
/// Notices older than this are cleared away.
const NOTICE_TTL: Duration = Duration::from_secs(600);

/// File name for a message's notice: its event id, made filename-safe (the
/// Kotlin service does the same).
fn notice_file(event_id: &str) -> String {
    event_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// While the app runs it syncs, so it has every new message decrypted: write
/// who said what for each one that should notify, for the push service
/// (`MorpheusPushService`) to show when the matching push arrives. A push
/// alone carries only ids.
fn write_notices(session: Session, data_dir: PathBuf) {
    let dir = data_dir.join(NOTICES_DIR);
    let spawned = std::thread::Builder::new()
        .name("morpheus-notices".into())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            runtime.block_on(async move {
                tokio::spawn(write_ring_notices(session.clone(), dir.clone()));
                let Ok(mut updates) = session.notifications().await else {
                    return;
                };
                let _ = std::fs::create_dir_all(&dir);
                while let Some(notice) = updates.next().await {
                    let file = dir.join(notice_file(&notice.event_id));
                    let room_id = notice.room_id.to_string();
                    // Worded and pictured as this device's setting says.
                    let ready = morpheus_ui::notices::ready(&session, notice, &data_dir).await;
                    let body = serde_json::json!({
                        "room_id": room_id,
                        "title": ready.text.title,
                        "body": ready.text.body,
                        "icon": ready.icon,
                        "image": ready.image,
                    });
                    let _ = std::fs::write(file, body.to_string());
                    prune(&dir);
                }
            });
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "notice writer failed");
    }
}

/// Rings get notices too, so a push for one (app in the background) says
/// who is calling.
async fn write_ring_notices(session: Session, dir: PathBuf) {
    let Ok(mut rings) = session.ring_updates().await else {
        return;
    };
    while let Some(ring) = rings.next().await {
        let morpheus_core::Ringing::Incoming(call) = ring else {
            continue;
        };
        let name = match session.profile(call.caller.clone()).await {
            Ok(profile) => profile.name,
            Err(_) => call.caller.localpart().to_owned(),
        };
        // Hidden on this device: not who is calling either.
        let hidden = dir.parent().is_some_and(|data_dir| {
            morpheus_ui::device_prefs::notification_content(data_dir)
                == morpheus_ui::device_prefs::NotificationContent::Hidden
        });
        let name = if hidden { "Someone".to_owned() } else { name };
        let body = serde_json::json!({
            "room_id": call.room_id.to_string(),
            "room": name,
            "sender": name,
            "body": "Incoming call",
            "call": true,
        });
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(
            dir.join(notice_file(call.notification.as_str())),
            body.to_string(),
        );
    }
}

fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > NOTICE_TTL);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Called by `MorpheusPushService` (Kotlin) when a push arrives while the app
/// is closed: decrypt that one message for the notification, without
/// starting the app. Returns JSON `{title, body, sender, icon, image, call}`, or null when
/// there is nothing to show.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_morpheus_mobile_MorpheusPushService_previewNative<'local>(
    mut env: jni::JNIEnv<'local>,
    _class: jni::objects::JClass<'local>,
    data_dir: jni::objects::JString<'local>,
    room_id: jni::objects::JString<'local>,
    event_id: jni::objects::JString<'local>,
) -> jni::sys::jstring {
    let mut text = |value: &jni::objects::JString<'local>| -> Option<String> {
        env.get_string(value).ok().map(Into::into)
    };
    let (Some(dir), Some(room), Some(event)) = (text(&data_dir), text(&room_id), text(&event_id))
    else {
        return std::ptr::null_mut();
    };
    let preview = preview(PathBuf::from(dir), room, event);
    match preview.and_then(|json| env.new_string(json).ok()) {
        Some(value) => value.into_raw(),
        None => std::ptr::null_mut(),
    }
}

fn preview(dir: PathBuf, room_id: String, event_id: String) -> Option<String> {
    let core = morpheus_core::Core::new(dir.clone()).ok()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    let found = runtime.block_on(core.preview_notification(room_id, event_id));
    match found {
        Ok(Some(preview)) => {
            let content = morpheus_ui::device_prefs::notification_content(&dir);
            let text = morpheus_ui::notices::notice_text(
                content,
                &preview.sender,
                &preview.room,
                &preview.body,
                preview.is_dm,
                preview.is_mention,
            );
            let hidden = content == morpheus_ui::device_prefs::NotificationContent::Hidden;
            let keep = |media: Option<morpheus_core::Media>, allowed: bool| {
                media
                    .filter(|_| allowed)
                    .and_then(|media| morpheus_ui::notices::keep_picture(&dir, &media))
            };
            Some(
                serde_json::json!({
                    "title": text.title,
                    "body": text.body,
                    "sender": if hidden { "Someone".to_owned() } else { preview.sender },
                    "icon": keep(preview.sender_avatar, text.sender_picture),
                    "image": keep(preview.image, text.photo),
                    "call": preview.is_call,
                })
                .to_string(),
            )
        }
        Ok(None) => None,
        Err(err) => {
            tracing::warn!(%err, "previewing a pushed message failed");
            None
        }
    }
}
